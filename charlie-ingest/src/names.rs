// Concern: the import-time NAME EMISSION (CORE3 materialization) — turn each xlsx workbook/sheet DEFINED NAME into a charlie FS4 name entry on disk so the loader resolves it at load (HARD RULE 2: emit-names, not inline-resolve): a STATIC cell/range target becomes a POSIX SYMLINK (a bare `<ident>` to the cell, or canonical `<ident>.begin`/`<ident>.end` to the two corner cells, materializing a blank corner cell so every link resolves), a FORMULA/CONSTANT target becomes a REF-FILE holding `=<target>`, each placed in its SCOPE folder (workbook-scoped at the root, sheet-scoped in the tab folder); a name whose identifier parses as A1, or an unrepresentable target, is skipped so its refs stay a located `#NAME?` (never silently wrong) — the WRITER half of the representation SEAM (a Windows all-ref-file writer is the `#[cfg(not(unix))]` fallback, engine/reader unchanged) | Non-concern: READING the definedName metadata (xlsx_meta.rs) or the reader that collects them (reader.rs), resolving a name at LOAD (charlie-model `names`), TABLE/structured refs (resolve.rs keeps inline-resolving those), and the cell-file writing itself (lib.rs owns the tab-folder IO) | IO: (a `DefinedName` list + a workbook dest dir) -> symlinks + ref-files + materialized blank corner cells on disk, or a located `IngestError`
//! Import-time FS4 name emission: [`emit_names`] writes each [`DefinedName`] as a symlink (static
//! cell/range) or a ref-file (formula/constant) in its scope folder, so charlie-model resolves it at
//! load. The reader-union understands both forms; this writer emits the POSIX symlink form for statics.

use std::path::{Path, PathBuf};

use charlie_ast::a1::{format_cell, parse_a1};

use crate::error::{ErrorKind, IngestError};

/// One workbook/sheet defined name to emit: its identifier, scope (`None` = workbook, `Some(sheet)` =
/// sheet-local), and its already-Excel-A1 target formula text (`Sheet1!$B$5`, `$A$2:$A$4`, `Base*1.05`,
/// `3.14`). Built by the reader from the xlsx metadata; classified into a representation here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefinedName {
    pub name: String,
    pub scope: Option<String>,
    pub target: String,
}

/// A static target parsed to canonical (unanchored) A1 corners on a sheet.
enum Static {
    Cell {
        sheet: Option<String>,
        cell: String,
    },
    Range {
        sheet: Option<String>,
        begin: String,
        end: String,
    },
}

/// Emit every representable defined name under `dest` (the workbook root). A name whose identifier
/// parses as A1, or whose target is unrepresentable, is SKIPPED (its refs load as a located `#NAME?` —
/// never a silently-wrong entry). Any filesystem failure is a located [`IngestError`] (CORE2).
pub fn emit_names(dest: &Path, names: &[DefinedName]) -> Result<(), IngestError> {
    for n in names {
        if n.name.is_empty() || n.name.starts_with("_xlnm.") || parse_a1(&n.name).is_ok() {
            // A built-in, an empty name, or an identifier that IS an A1 address — not representable.
            continue;
        }
        match parse_static(&n.target) {
            Some(s) => emit_static(dest, n, &s)?,
            // A formula / constant / array / unrepresentable target -> the portable ref-file form.
            None => emit_ref_file(dest, n)?,
        }
    }
    Ok(())
}

/// The scope folder for a name: the workbook root, or its tab folder.
fn scope_dir(dest: &Path, scope: &Option<String>) -> PathBuf {
    match scope {
        Some(s) => dest.join(s),
        None => dest.to_path_buf(),
    }
}

/// The relative symlink target from a name's `scope` folder to `sheet/cell` under `dest`: same-folder
/// `cell` when the name is sheet-scoped on the target's own sheet, else `sheet/cell` (workbook root) or
/// `../sheet/cell` (a sheet-scoped name pointing at another sheet).
fn relative_target(scope: &Option<String>, sheet: &str, cell: &str) -> String {
    match scope {
        None => format!("{sheet}/{cell}"),
        Some(s) if s == sheet => cell.to_string(),
        Some(_) => format!("../{sheet}/{cell}"),
    }
}

/// Emit a static cell/range name as symlink(s) (the POSIX writer), materializing any blank corner cell
/// so every link resolves. The target sheet defaults to the name's own scope sheet when the target
/// carries no `Sheet!` qualifier; a workbook name with no target sheet, OR a target sheet that is not a
/// real tab ([`is_real_tab`] — so no phantom tab folder is fabricated), is unrepresentable as a symlink
/// and falls back to the ref-file form.
fn emit_static(dest: &Path, n: &DefinedName, s: &Static) -> Result<(), IngestError> {
    let dir = scope_dir(dest, &n.scope);
    create_dir(&dir)?;
    match s {
        Static::Cell { sheet, cell } => {
            let Some(ts) = sheet.clone().or_else(|| n.scope.clone()) else {
                return emit_ref_file(dest, n);
            };
            if !is_real_tab(dest, &ts) {
                return emit_ref_file(dest, n);
            }
            materialize_cell(dest, &ts, cell)?;
            make_link(&relative_target(&n.scope, &ts, cell), &dir.join(&n.name))?;
        }
        Static::Range { sheet, begin, end } => {
            let Some(ts) = sheet.clone().or_else(|| n.scope.clone()) else {
                return emit_ref_file(dest, n);
            };
            if !is_real_tab(dest, &ts) {
                return emit_ref_file(dest, n);
            }
            materialize_cell(dest, &ts, begin)?;
            materialize_cell(dest, &ts, end)?;
            make_link(
                &relative_target(&n.scope, &ts, begin),
                &dir.join(format!("{}.begin", n.name)),
            )?;
            make_link(
                &relative_target(&n.scope, &ts, end),
                &dir.join(format!("{}.end", n.name)),
            )?;
        }
    }
    Ok(())
}

/// Emit a formula / constant name as a ref-file holding `=<target>` in its scope folder (the portable
/// representation; also the only form for a computed name). Nested name tokens stay verbatim — the
/// loader resolves them recursively.
fn emit_ref_file(dest: &Path, n: &DefinedName) -> Result<(), IngestError> {
    let dir = scope_dir(dest, &n.scope);
    create_dir(&dir)?;
    let body = n.target.trim();
    let content = if body.starts_with('=') {
        body.to_string()
    } else {
        format!("={body}")
    };
    std::fs::write(dir.join(&n.name), content).map_err(|e| {
        IngestError::io(
            ErrorKind::DestIo,
            format!(
                "cannot write name file {:?}: {e}",
                dir.join(&n.name).display()
            ),
        )
    })
}

/// Parse a target into a static cell/range (canonical unanchored A1), or `None` for a
/// formula/constant/union/external/whole-column target (which becomes a ref-file).
///
/// The `,(){}`/space/`#` reject-set here is the WRITER half of the same "pure cell/range ref vs
/// expression" classification the READER performs across the crate firewall in charlie-model's
/// `names::is_pure_ref` / `classify_ref_file` (the reader cannot import this private writer fn, so the
/// knowledge is deliberately duplicated) — keep the two reject-sets in sync so a target this emits as a
/// symlink is one the reader also treats as a pure ref, and vice versa.
fn parse_static(target: &str) -> Option<Static> {
    let t = target.trim();
    // A formula / union / array / broken target carries one of these anywhere — a plain cell/range ref
    // never does (a `,` in the SHEET half of `Sheet1!A1,Sheet1!B1` is a union, caught here before the
    // sheet split would otherwise hide it in the sheet part).
    if t.contains(['{', '}', '(', ')', ',']) {
        return None;
    }
    let (sheet, addr) = match t.rsplit_once('!') {
        Some((s, a)) => (Some(unquote_sheet(s)), a),
        None => (None, t),
    };
    if addr.contains([' ', '#']) {
        return None;
    }
    match addr.split_once(':') {
        None => {
            let a = parse_a1(addr).ok()?;
            Some(Static::Cell {
                sheet,
                cell: format_cell(a.col, a.row),
            })
        }
        Some((l, r)) => {
            let la = parse_a1(l).ok()?;
            let ra = parse_a1(r).ok()?;
            // Normalize corner ordering so the emitted `.begin`/`.end` are canonical top-left/bottom-right.
            Some(Static::Range {
                sheet,
                begin: format_cell(la.col.min(ra.col), la.row.min(ra.row)),
                end: format_cell(la.col.max(ra.col), la.row.max(ra.row)),
            })
        }
    }
}

/// Strip a `'quoted'` sheet name's surrounding quotes and `''` escapes (bare names pass through).
fn unquote_sheet(s: &str) -> String {
    let s = s.trim();
    if let Some(inner) = s.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
        inner.replace("''", "'")
    } else {
        s.to_string()
    }
}

/// Whether `sheet` is a REAL tab folder under `dest`. `emit_names` runs after every sheet's tab folder
/// is written, so a target sheet that is not a directory here is one no worksheet defines — a name
/// pointing at it is unrepresentable as a symlink (materializing its corner would fabricate a phantom
/// tab), and the caller falls back to the ref-file form (fail-fast, mirroring the no-target-sheet path).
fn is_real_tab(dest: &Path, sheet: &str) -> bool {
    dest.join(sheet).is_dir()
}

/// Ensure a target cell file exists, creating an EMPTY (blank) one if absent — so a name symlink to a
/// blank cell still resolves (`cat`/`ls -L`/tar), FS4. Never clobbers a cell that already has content.
/// The caller has already confirmed `sheet` is a real tab ([`is_real_tab`]), so this never fabricates a
/// phantom tab folder — only a blank CORNER CELL inside an existing tab.
fn materialize_cell(dest: &Path, sheet: &str, cell: &str) -> Result<(), IngestError> {
    let dir = dest.join(sheet);
    create_dir(&dir)?;
    let path = dir.join(cell);
    if !path.exists() {
        std::fs::write(&path, "").map_err(|e| {
            IngestError::io(
                ErrorKind::DestIo,
                format!("cannot materialize blank corner {:?}: {e}", path.display()),
            )
        })?;
    }
    Ok(())
}

fn create_dir(dir: &Path) -> Result<(), IngestError> {
    std::fs::create_dir_all(dir).map_err(|e| {
        IngestError::io(
            ErrorKind::DestIo,
            format!("cannot create {:?}: {e}", dir.display()),
        )
    })
}

/// Create the name symlink (the POSIX writer). A pre-existing entry is removed first so a re-import is
/// idempotent.
#[cfg(unix)]
fn make_link(target: &str, link: &Path) -> Result<(), IngestError> {
    if link.exists() || std::fs::symlink_metadata(link).is_ok() {
        let _ = std::fs::remove_file(link);
    }
    std::os::unix::fs::symlink(target, link).map_err(|e| {
        IngestError::io(
            ErrorKind::DestIo,
            format!(
                "cannot create name symlink {:?} -> {target}: {e}",
                link.display()
            ),
        )
    })
}

/// The non-POSIX WRITER fallback of the representation seam: a static name is written as a ref-file
/// holding its target (the loader's reader-union reads it identically). The engine/reader are unchanged
/// — only the writer is platform-conditional.
#[cfg(not(unix))]
fn make_link(target: &str, link: &Path) -> Result<(), IngestError> {
    std::fs::write(link, format!("={target}")).map_err(|e| {
        IngestError::io(
            ErrorKind::DestIo,
            format!("cannot write name ref-file {:?}: {e}", link.display()),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_static_cells_ranges_and_formulas() {
        assert!(matches!(
            parse_static("Data!$H$1"),
            Some(Static::Cell { cell, .. }) if cell == "H1"
        ));
        assert!(matches!(
            parse_static("$B$2:$B$4"),
            Some(Static::Range { begin, end, .. }) if begin == "B2" && end == "B4"
        ));
        // A reversed range normalizes to top-left:bottom-right.
        assert!(matches!(
            parse_static("C4:A1"),
            Some(Static::Range { begin, end, .. }) if begin == "A1" && end == "C4"
        ));
        // Formula / constant / union targets are NOT static (become ref-files).
        assert!(parse_static("Base*1.05").is_none());
        assert!(parse_static("3.14").is_none());
        assert!(parse_static("{1,2,3}").is_none());
        assert!(parse_static("Sheet1!A1,Sheet1!B1").is_none());
    }

    #[test]
    fn relative_targets_span_scopes() {
        assert_eq!(relative_target(&None, "Data", "H1"), "Data/H1");
        assert_eq!(relative_target(&Some("Data".into()), "Data", "H1"), "H1");
        assert_eq!(
            relative_target(&Some("S1".into()), "Data", "H1"),
            "../Data/H1"
        );
    }

    #[test]
    fn writer_static_and_reader_pure_ref_classifications_agree_across_the_firewall() {
        // DRY across the crate firewall: the WRITER's `parse_static` reject-set and the READER's
        // `is_pure_ref`/`classify_ref_file` (charlie-model, which cannot import this private writer fn)
        // encode the SAME "pure cell/range ref vs expression" rule — duplicated deliberately. This pins
        // that they never drift: for a shared corpus, a target the WRITER emits as a symlink (static) is
        // EXACTLY one the READER classifies as a `Ref`, and a target the writer makes a ref-file
        // (formula/constant) is one the reader classifies as an `Expr`. A representation mismatch here is
        // the seam's silent-corruption failure mode (a `Ref` read as `Expr` or vice versa).
        //
        // The corpus avoids `/` and `!` so neither the reader's degraded-relative-path branch nor a
        // sheet-qualifier split is exercised — this isolates the pure-ref-vs-expr reject-set itself.
        let corpus = [
            "B5",
            "$A$1",
            "A1:B3",
            "$A$2:$A$4",
            "C4:A1", // pure refs (both: static / Ref)
            "Base*1.05",
            "3.14",
            "SUM(A1:A3)",
            "{1,2,3}",
            "A1,B1",
            "A1 B1",
            "#REF!", // expressions
        ];
        for t in corpus {
            let writer_static = parse_static(t).is_some();
            // Reader side: build a WORKBOOK-scoped ref-file holding the same text and read its target
            // kind through the public API (workbook scope => no scope-sheet qualification is injected).
            let (table, diags) =
                charlie_model::NameTable::build(vec![charlie_model::RawNameEntry {
                    scope: charlie_model::NameScope::Workbook,
                    entry_name: "N".to_string(),
                    form: charlie_model::NameRepr::RefFile {
                        content: t.to_string(),
                    },
                }]);
            assert!(diags.is_empty(), "{t:?}: {diags:?}");
            let reader_ref = matches!(table.names()[0].target, charlie_model::NameTarget::Ref(_));
            assert_eq!(
                writer_static, reader_ref,
                "firewall drift on {t:?}: writer static={writer_static}, reader Ref={reader_ref}"
            );
        }
    }

    #[test]
    fn a_range_name_over_a_blank_corner_materializes_the_missing_corner_cell() {
        // FS4 fitness: a range name whose CORNER cell is absent in the source (the xlsx serializer writes
        // no file for a blank cell) must still resolve — the writer MATERIALIZES a blank corner so the
        // corner symlink is never dangling. This exercises the materialize-if-absent branch the other
        // fixtures (populated corners) never hit; a regression that stopped materializing leaves a broken
        // link and would go uncaught without this pin.
        let dest = std::env::temp_dir().join(format!(
            "charlie-blank-corner-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        // A real tab with content only at A2 — the range B2:B4's corners (B2, B4) are BLANK/absent.
        std::fs::create_dir_all(dest.join("Sheet1")).unwrap();
        std::fs::write(dest.join("Sheet1").join("A2"), "10").unwrap();
        let names = vec![DefinedName {
            name: "Block".to_string(),
            scope: Some("Sheet1".to_string()),
            target: "Sheet1!$B$2:$B$4".to_string(),
        }];
        emit_names(&dest, &names).expect("emit ok");
        // Both corners were materialized as blank cell files (so the .begin/.end symlinks resolve).
        assert_eq!(
            std::fs::read_to_string(dest.join("Sheet1").join("B2")).unwrap(),
            "",
            "the blank .begin corner was materialized"
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("Sheet1").join("B4")).unwrap(),
            "",
            "the blank .end corner was materialized"
        );
        // The pre-existing A2 cell is never clobbered.
        assert_eq!(
            std::fs::read_to_string(dest.join("Sheet1").join("A2")).unwrap(),
            "10"
        );
        // On unix the corner entries are symlinks resolving onto the materialized cells.
        #[cfg(unix)]
        {
            let begin = dest.join("Sheet1").join("Block.begin");
            assert_eq!(std::fs::read_link(&begin).unwrap().to_str().unwrap(), "B2");
            assert!(
                begin.exists(),
                "the .begin symlink resolves (target exists)"
            );
        }
        std::fs::remove_dir_all(&dest).ok();
    }

    #[test]
    fn a_name_targeting_a_missing_tab_falls_back_to_a_ref_file_no_phantom_tab() {
        // A defined name pointing at a sheet that is NOT a real tab must not fabricate a phantom tab
        // folder holding a lone blank corner cell; it falls back to the portable ref-file form.
        let dest = std::env::temp_dir().join(format!(
            "charlie-emit-phantom-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(dest.join("Real")).unwrap(); // the only real tab
        let names = vec![DefinedName {
            name: "Ghosted".to_string(),
            scope: None, // workbook-scoped
            target: "Ghost!$A$1".to_string(),
        }];
        emit_names(&dest, &names).expect("emit ok");
        assert!(
            !dest.join("Ghost").exists(),
            "no phantom tab folder for the missing target sheet"
        );
        // The name is still emitted (fail-safe), as a ref-file at the root holding its target ref.
        assert_eq!(
            std::fs::read_to_string(dest.join("Ghosted")).unwrap(),
            "=Ghost!$A$1"
        );
        std::fs::remove_dir_all(&dest).ok();
    }
}
