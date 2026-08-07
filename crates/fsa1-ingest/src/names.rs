// Concern: writes each defined name into its scope folder | Non-concern: resolving a name at load, how an alias is stored (the format owns it) | IO: (dest, names) -> an alias per name

use std::path::{Path, PathBuf};

use fsa1_ast::a1::{format_cell, parse_a1};

use crate::error::{ErrorKind, IngestError};
use crate::warnings::UnpackWarning;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefinedName {
    pub name: String,
    pub scope: Option<String>,
    /// Excel-A1 already: `Sheet1!$B$5`, `$A$2:$A$4`, `Base*1.05`, `3.14`.
    pub target: String,
}

/// Corners are canonical and UNANCHORED — the `$` of the source target is gone.
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

pub fn emit_names(
    dest: &Path,
    names: &[DefinedName],
    warnings: &mut Vec<UnpackWarning>,
) -> Result<(), IngestError> {
    for n in names {
        let skip_reason = if n.name.is_empty() {
            Some("empty identifier")
        } else if n.name.starts_with("_xlnm.") {
            Some("a built-in name (print area / titles) FSA1 does not model")
        } else if parse_a1(&n.name).is_ok() {
            Some("identifier parses as an A1 address")
        } else {
            None
        };
        if let Some(reason) = skip_reason {
            warnings.push(UnpackWarning::NameSkipped {
                name: n.name.clone(),
                scope: n.scope.clone(),
                reason: reason.to_string(),
            });
            continue;
        }
        let emitted = match parse_static(&n.target) {
            Some(s) => emit_static(dest, n, &s)?,
            None => emit_ref_file(dest, n)?,
        };
        if !emitted {
            warnings.push(UnpackWarning::NameSkipped {
                name: n.name.clone(),
                scope: n.scope.clone(),
                reason:
                    "a sheet of the same name occupies this path; a tab folder (FS1) and a name \
                         entry (FS4) cannot share one path"
                        .to_string(),
            });
        }
    }
    Ok(())
}

fn scope_dir(dest: &Path, scope: &Option<String>) -> PathBuf {
    match scope {
        Some(s) => dest.join(s),
        None => dest.to_path_buf(),
    }
}

fn relative_target(scope: &Option<String>, sheet: &str, cell: &str) -> String {
    match scope {
        None => format!("{sheet}/{cell}"),
        Some(s) if s == sheet => cell.to_string(),
        Some(_) => format!("../{sheet}/{cell}"),
    }
}

/// Excel keeps sheet names and defined names in SEPARATE namespaces, but FSA1 projects both into
/// one POSIX directory, where a name cannot share a path with a tab folder.
fn blocked_by_tab(path: &Path) -> bool {
    path.is_dir()
}

/// `Ok(false)` when the destination is blocked; a target sheet that is absent or not a real tab, or a
/// corner with no file of its own to name, falls back to the ref-file form rather than fabricating a
/// phantom tab or laying down a link that resolves to nothing.
fn emit_static(dest: &Path, n: &DefinedName, s: &Static) -> Result<bool, IngestError> {
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
            if !linkable(&dest.join(&ts), cell) {
                return write_name_file(dest, n, &static_ref(&ts, s));
            }
            let link = dir.join(&n.name);
            if blocked_by_tab(&link) {
                return Ok(false);
            }
            materialize_cell(dest, &ts, cell)?;
            make_link(&relative_target(&n.scope, &ts, cell), &link)?;
        }
        Static::Range { sheet, begin, end } => {
            let Some(ts) = sheet.clone().or_else(|| n.scope.clone()) else {
                return emit_ref_file(dest, n);
            };
            if !is_real_tab(dest, &ts) {
                return emit_ref_file(dest, n);
            }
            let tab = dest.join(&ts);
            if !linkable(&tab, begin) || !linkable(&tab, end) {
                return write_name_file(dest, n, &static_ref(&ts, s));
            }
            let (lb, le) = (
                dir.join(format!("{}.begin", n.name)),
                dir.join(format!("{}.end", n.name)),
            );
            if blocked_by_tab(&lb) || blocked_by_tab(&le) {
                return Ok(false);
            }
            materialize_cell(dest, &ts, begin)?;
            materialize_cell(dest, &ts, end)?;
            make_link(&relative_target(&n.scope, &ts, begin), &lb)?;
            make_link(&relative_target(&n.scope, &ts, end), &le)?;
        }
    }
    Ok(true)
}

/// Nested name tokens stay verbatim — the loader resolves them recursively.
fn emit_ref_file(dest: &Path, n: &DefinedName) -> Result<bool, IngestError> {
    let body = n.target.trim();
    let content = if body.starts_with('=') {
        body.to_string()
    } else {
        format!("={body}")
    };
    write_name_file(dest, n, &content)
}

/// The ref a STATIC target spells once it can have no symlink: [`Static`]'s own canonical, unanchored
/// corners, qualified with the sheet through the very quoter the loader unquotes by — so the name
/// reads back as the `Sheet!A1` the symlink form would have resolved to, rather than the source's
/// `$`-anchored spelling.
fn static_ref(sheet: &str, s: &Static) -> String {
    let addr = match s {
        Static::Cell { cell, .. } => cell.clone(),
        Static::Range { begin, end, .. } => format!("{begin}:{end}"),
    };
    format!("={}!{addr}", fsa1_model::quote_sheet(sheet))
}

/// `Ok(false)` when a tab folder occupies the path.
fn write_name_file(dest: &Path, n: &DefinedName, content: &str) -> Result<bool, IngestError> {
    let dir = scope_dir(dest, &n.scope);
    create_dir(&dir)?;
    let path = dir.join(&n.name);
    if blocked_by_tab(&path) {
        return Ok(false);
    }
    std::fs::write(&path, content).map_err(|e| {
        IngestError::io(
            ErrorKind::DestIo,
            format!("cannot write name file {:?}: {e}", path.display()),
        )
    })?;
    Ok(true)
}

/// The reject-set below is the WRITER half of the pure-ref-vs-expression rule fsa1-model's
/// `names::is_pure_ref` performs on the read side; the two are duplicated across the crate firewall
/// and must agree.
fn parse_static(target: &str) -> Option<Static> {
    let t = target.trim();
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
            Some(Static::Range {
                sheet,
                begin: format_cell(la.col.min(ra.col), la.row.min(ra.row)),
                end: format_cell(la.col.max(ra.col), la.row.max(ra.row)),
            })
        }
    }
}

fn unquote_sheet(s: &str) -> String {
    let s = s.trim();
    if let Some(inner) = s.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
        inner.replace("''", "'")
    } else {
        s.to_string()
    }
}

/// Every tab folder is written before `emit_names` runs, so a non-directory here names no worksheet.
fn is_real_tab(dest: &Path, sheet: &str) -> bool {
    dest.join(sheet).is_dir()
}

/// Whether the coordinate can be the TARGET of a name symlink. A cell a wider range file covers has
/// no path of its own: a corner laid beside the range file is a second file over the same cell, which
/// the loader refuses as an overlap, and a link onto the range file names the whole range instead of
/// the one cell. The caller crosses such a name in its ref-file form.
fn linkable(dir: &Path, cell: &str) -> bool {
    dir.join(cell).exists() || !covered(dir, cell)
}

/// Creates an EMPTY cell file for a coordinate [`linkable`] has cleared, so a name symlink onto a
/// blank cell still resolves. Never clobbers a cell that already has content.
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

/// Whether any range file in the tab claims the coordinate — the cell's own file included.
fn covered(dir: &Path, cell: &str) -> bool {
    let Ok(at) = parse_a1(cell) else {
        return false;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .and_then(|name| fsa1_model::parse_filename(name).ok())
            .is_some_and(|file| file.region.contains(at.col, at.row))
    })
}

fn create_dir(dir: &Path) -> Result<(), IngestError> {
    std::fs::create_dir_all(dir).map_err(|e| {
        IngestError::io(
            ErrorKind::DestIo,
            format!("cannot create {:?}: {e}", dir.display()),
        )
    })
}

/// The host's say in how an alias is stored belongs to the format, not to ingest: this only turns
/// its failure into an `IngestError`.
fn make_link(target: &str, link: &Path) -> Result<(), IngestError> {
    fsa1_model::write_name_alias(target, link).map_err(|e| {
        IngestError::io(
            ErrorKind::DestIo,
            format!(
                "cannot create name alias {:?} -> {target}: {e}",
                link.display()
            ),
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
        assert!(
            matches!(
                parse_static("C4:A1"),
                Some(Static::Range { begin, end, .. }) if begin == "A1" && end == "C4"
            ),
            "a reversed range normalizes to top-left:bottom-right"
        );
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
        // No `/` or `!`, so neither the degraded-path branch nor a sheet split runs.
        let corpus = [
            "B5",
            "$A$1",
            "A1:B3",
            "$A$2:$A$4",
            "C4:A1",
            "Base*1.05",
            "3.14",
            "SUM(A1:A3)",
            "{1,2,3}",
            "A1,B1",
            "A1 B1",
            "#REF!",
        ];
        for t in corpus {
            let writer_static = parse_static(t).is_some();
            let (table, diags) = fsa1_model::NameTable::build(vec![fsa1_model::RawNameEntry {
                scope: fsa1_model::NameScope::Workbook,
                entry_name: "N".to_string(),
                form: fsa1_model::NameRepr::RefFile {
                    content: t.to_string(),
                },
            }]);
            assert!(diags.is_empty(), "{t:?}: {diags:?}");
            let reader_ref = matches!(table.names()[0].target, fsa1_model::NameTarget::Ref(_));
            assert_eq!(
                writer_static, reader_ref,
                "firewall drift on {t:?}: writer static={writer_static}, reader Ref={reader_ref}"
            );
        }
    }

    #[test]
    fn a_range_name_over_a_blank_corner_materializes_the_missing_corner_cell() {
        let dest = std::env::temp_dir().join(format!(
            "FSA1-blank-corner-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        // A real tab with content only at A2 — the range B2:B4's corners are blank/absent.
        std::fs::create_dir_all(dest.join("Sheet1")).unwrap();
        std::fs::write(dest.join("Sheet1").join("A2"), "10").unwrap();
        let names = vec![DefinedName {
            name: "Block".to_string(),
            scope: Some("Sheet1".to_string()),
            target: "Sheet1!$B$2:$B$4".to_string(),
        }];
        emit_names(&dest, &names, &mut Vec::new()).expect("emit ok");
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
        assert_eq!(
            std::fs::read_to_string(dest.join("Sheet1").join("A2")).unwrap(),
            "10",
            "a pre-existing cell is never clobbered"
        );
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

    /// A coordinate a RANGE file covers has no path of its own, so a symlink onto it would resolve to
    /// nothing and an agent reading the tree with `cat` would hit a dead entry. The name crosses as
    /// the ref-file the format already reads, spelled from the canonical unanchored corners.
    #[test]
    fn a_name_inside_a_range_file_crosses_as_a_readable_ref_never_a_dangling_link() {
        let dest = std::env::temp_dir().join(format!(
            "FSA1-covered-name-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(dest.join("Data")).unwrap();
        std::fs::write(
            dest.join("Data").join(fsa1_model::range_file_name("A1:H4")),
            "1\n2\n3\n4",
        )
        .unwrap();
        let name = |n: &str, target: &str| DefinedName {
            name: n.to_string(),
            scope: None,
            target: target.to_string(),
        };
        let mut warnings = Vec::new();
        emit_names(
            &dest,
            &[
                name("TaxRate", "Data!$H$1"),
                name("AllQOne", "Data!$B$2:$B$4"),
            ],
            &mut warnings,
        )
        .expect("emit ok");

        assert!(
            warnings.is_empty(),
            "a ref-file is not a skip: {warnings:?}"
        );
        for (entry, want) in [("TaxRate", "=Data!H1"), ("AllQOne", "=Data!B2:B4")] {
            let path = dest.join(entry);
            assert!(
                path.exists(),
                "{entry} must resolve; a dangling symlink does not",
            );
            assert_eq!(std::fs::read_to_string(&path).unwrap(), want);
        }
        // Read back canonically: the assertion below names a REGION, not a spelling.
        let mut inside: Vec<String> = std::fs::read_dir(dest.join("Data"))
            .unwrap()
            .map(|e| {
                let n = e.unwrap().file_name();
                fsa1_model::canonical_range_name(&n.to_string_lossy())
            })
            .collect();
        inside.sort();
        assert_eq!(
            inside,
            vec!["A1:H4"],
            "and no corner is laid over the range file",
        );
        std::fs::remove_dir_all(&dest).ok();
    }

    #[test]
    fn a_name_targeting_a_missing_tab_falls_back_to_a_ref_file_no_phantom_tab() {
        let dest = std::env::temp_dir().join(format!(
            "FSA1-emit-phantom-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(dest.join("Real")).unwrap();
        let names = vec![DefinedName {
            name: "Ghosted".to_string(),
            scope: None,
            target: "Ghost!$A$1".to_string(),
        }];
        let mut warnings = Vec::new();
        emit_names(&dest, &names, &mut warnings).expect("emit ok");
        assert!(
            !dest.join("Ghost").exists(),
            "no phantom tab folder for the missing target sheet"
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("Ghosted")).unwrap(),
            "=Ghost!$A$1",
            "the name is still emitted, as a ref-file at the root"
        );
        assert!(
            warnings.is_empty(),
            "a ref-file fallback emits the name; it is not a skip"
        );
        std::fs::remove_dir_all(&dest).ok();
    }

    #[test]
    fn a_skipped_name_pushes_a_located_warning_with_its_reason() {
        let dest = std::env::temp_dir().join(format!(
            "FSA1-name-skip-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(dest.join("Data")).unwrap();
        std::fs::write(dest.join("Data").join("H1"), "5").unwrap();
        let name = |n: &str, scope: Option<&str>, target: &str| DefinedName {
            name: n.to_string(),
            scope: scope.map(str::to_string),
            target: target.to_string(),
        };
        let names = vec![
            name("A1", None, "Data!$C$1"),
            name("B2", Some("Data"), "Data!$C$2"),
            name("_xlnm.Print_Area", None, "Data!$A$1"),
            name("", None, "Data!$A$1"),
            name("TaxRate", None, "Data!$H$1"),
        ];
        let mut warnings = Vec::new();
        emit_names(&dest, &names, &mut warnings).expect("emit ok");

        assert_eq!(
            warnings.len(),
            4,
            "four skips, TaxRate emitted: {warnings:?}"
        );
        assert!(warnings.contains(&UnpackWarning::NameSkipped {
            name: "A1".to_string(),
            scope: None,
            reason: "identifier parses as an A1 address".to_string(),
        }));
        assert!(warnings.contains(&UnpackWarning::NameSkipped {
            name: "B2".to_string(),
            scope: Some("Data".to_string()),
            reason: "identifier parses as an A1 address".to_string(),
        }));
        assert!(warnings.iter().any(|w| matches!(
            w,
            UnpackWarning::NameSkipped { name, reason, .. }
                if name == "_xlnm.Print_Area" && reason.contains("built-in")
        )));
        assert!(warnings.iter().any(|w| matches!(
            w,
            UnpackWarning::NameSkipped { name, reason, .. } if name.is_empty() && reason == "empty identifier"
        )));
        assert!(
            dest.join("TaxRate").exists() || dest.join("Data").join("TaxRate").exists(),
            "the representable name is emitted and reported nothing"
        );
        std::fs::remove_dir_all(&dest).ok();
    }

    #[test]
    fn a_name_colliding_with_a_sheet_folder_is_skipped_and_reported_not_fatal() {
        let dest = std::env::temp_dir().join(format!(
            "FSA1-collide-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(dest.join("BG")).expect("the sheet folder exists first");
        let mut warnings = Vec::new();
        let names = vec![DefinedName {
            name: "BG".to_string(),
            scope: None,
            target: "#REF!".to_string(),
        }];

        emit_names(&dest, &names, &mut warnings).expect("must NOT abort the unpack");

        assert!(
            warnings.iter().any(|w| matches!(
                w,
                UnpackWarning::NameSkipped { name, reason, .. }
                    if name == "BG" && reason.contains("sheet of the same name")
            )),
            "the loss must be located and reported, not silent: {warnings:?}"
        );
        assert!(
            dest.join("BG").is_dir(),
            "the sheet folder must be left untouched"
        );
        std::fs::remove_dir_all(&dest).ok();
    }
}
