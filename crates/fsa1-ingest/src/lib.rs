// Concern: converts a .ods/.xlsx into an FSA1 workbook tree | Non-concern: the CLI, the formula language | IO: (a source, a dest dir) -> a tab tree + an ImportReport

#[cfg(test)]
mod block_probe;
mod dates;
mod decompose;
pub mod error;
mod names;
mod partition;
mod reader;
mod resolve;
mod scope_block;
mod serialize;
mod source;
mod translate;
mod warnings;
mod xlsx_meta;
mod xlsx_style;

use std::path::Path;

pub use error::{ErrorKind, IngestError};
pub use partition::Decomposition;
pub use warnings::{AxisRef, UnpackCategory, UnpackWarning};

use decompose::StyledCell;
use names::emit_names;
use serialize::sheet_files;
use source::{SheetSource, SourceBook};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportReport {
    /// In source order.
    pub tabs: Vec<String>,
    /// How many range files were written — one per block the decomposition cut, never one per cell.
    pub files: usize,
    pub warnings: Vec<UnpackWarning>,
    /// The categories this run actually EXAMINED, in report order. `warnings` is what the run found;
    /// this is where it looked, and the two together are what a reader may conclude anything from —
    /// an empty `warnings` says nothing at all about a category absent here.
    pub inspected: Vec<UnpackCategory>,
    /// The policy that actually cut this tree — the caller's, or the one the source resolved to.
    pub decomposition: Decomposition,
}

/// Each sheet becomes a tab folder holding one file per block the decomposition cuts, named by the
/// closed A1 range it fills; which policy cuts is [`resolve_decomposition`]'s. Refuses a non-empty
/// `dest`, and is ATOMIC: any failure removes the partial output. `strict` refuses an xlsx FSA1 cannot
/// serialize back identically — up front for the source's own faults, mid-write for the cut's.
pub fn import_file(src: &Path, dest: &Path, strict: bool) -> Result<ImportReport, IngestError> {
    import(src, dest, strict, None)
}

/// [`import_file`] with the policy named rather than resolved. Asking for one the source cannot feed
/// — [`Decomposition::Appearance`] where no appearance channel is read — is refused before any write.
pub fn import_file_as(
    src: &Path,
    dest: &Path,
    strict: bool,
    decomposition: Decomposition,
) -> Result<ImportReport, IngestError> {
    import(src, dest, strict, Some(decomposition))
}

fn import(
    src: &Path,
    dest: &Path,
    strict: bool,
    requested: Option<Decomposition>,
) -> Result<ImportReport, IngestError> {
    // The ONE reading of "strict reaches this source": the contract is the inverse of a faithful `pack`, and `pack` writes .xlsx.
    let strict_xlsx = strict && is_xlsx(src);
    let format_map = if strict_xlsx {
        xlsx_meta::strict_roundtrip_check(src)?;
        Some(xlsx_meta::numfmt_map(src)?)
    } else {
        None
    };
    let mut warnings: Vec<UnpackWarning> = Vec::new();
    if is_xlsx(src) {
        for part in xlsx_meta::uncarried_parts(src)? {
            warnings.push(UnpackWarning::WorkbookPartNotCarried { part: part.spell() });
        }
    }
    // Only the lossy path needs this: it passes `format_map = None` and never learns of a drop.
    if !strict && is_xlsx(src) {
        for c in xlsx_meta::numfmt_coercions(src)? {
            warnings.push(UnpackWarning::NumberFormatCoerced {
                sheet: c.sheet,
                cell: c.cell,
                num_fmt_id: c.num_fmt_id,
                format_code: c.format_code,
            });
        }
    }
    let inspected = inspected_categories(src);
    let decomposition = resolve_decomposition(src, requested, &inspected)?;
    let book = reader::read_file(src, format_map.as_ref(), &mut warnings)?;
    let (tabs, files) = write_book(&book, dest, decomposition, strict_xlsx, &mut warnings)?;
    Ok(ImportReport {
        tabs,
        files,
        warnings,
        inspected,
        decomposition,
    })
}

/// "Carries an appearance channel" is the FORMAT's property, never the file's — the `Styling`
/// inspection, never an extension test — since a source no styling reader opens states `None` for
/// every cell it could hold, and `Appearance` only coarsens that. Unnamed is always `Occupancy`:
/// `Appearance` is refused without a channel, and trees on disk pin it. Compatibility, not quality.
fn resolve_decomposition(
    src: &Path,
    requested: Option<Decomposition>,
    inspected: &[UnpackCategory],
) -> Result<Decomposition, IngestError> {
    let appearance = inspected.contains(&UnpackCategory::Styling);
    match (requested, appearance) {
        (Some(Decomposition::Appearance), false) => Err(IngestError::io(
            ErrorKind::Invalid,
            format!(
                "cannot decompose {:?} by {}: this source's format carries no appearance channel at \
                 all, so no source of it could ever state one; unpack it by {}",
                src.display(),
                Decomposition::Appearance.name(),
                Decomposition::Occupancy.name(),
            ),
        )),
        (Some(named), _) => Ok(named),
        (None, _) => Ok(Decomposition::Occupancy),
    }
}

fn is_xlsx(src: &Path) -> bool {
    src.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("xlsx"))
}

/// Which categories THIS source's readers looked at: every one but the formula translator's is read
/// out of the xlsx package parts, and an `.ods` reaches none of them — it crosses values and formulas
/// and inspects nothing else, deliberately. Naming that is what keeps an empty warning list from
/// reading as a clean bill for work no code on this path did.
fn inspected_categories(src: &Path) -> Vec<UnpackCategory> {
    let xlsx = is_xlsx(src);
    UnpackCategory::ALL
        .into_iter()
        .filter(|category| match category {
            UnpackCategory::Formula => true,
            UnpackCategory::NumberFormat
            | UnpackCategory::Table
            | UnpackCategory::Name
            | UnpackCategory::Styling
            | UnpackCategory::Geometry
            | UnpackCategory::WorkbookPart => xlsx,
        })
        .collect()
}

/// Split out of [`import_file`] so the never-clobber guard and the cleanup contract are testable
/// without a real source file.
fn write_book(
    book: &SourceBook,
    dest: &Path,
    decomposition: Decomposition,
    strict: bool,
    warnings: &mut Vec<UnpackWarning>,
) -> Result<(Vec<String>, usize), IngestError> {
    // The conflict refusal must NOT clean up the user's own pre-existing content, so it runs first.
    let dest_existed = dest.exists();
    if dest_existed {
        let non_empty = std::fs::read_dir(dest)
            .map_err(|e| {
                IngestError::io(
                    ErrorKind::DestIo,
                    format!("cannot read {:?}: {e}", dest.display()),
                )
            })?
            .next()
            .is_some();
        if non_empty {
            return Err(IngestError::io(
                ErrorKind::DestConflict,
                format!(
                    "{:?} already exists and is not empty -- refusing to overwrite; pick an empty or new directory",
                    dest.display()
                ),
            ));
        }
    }

    match materialize(book, dest, decomposition, strict, warnings) {
        Ok(out) => Ok(out),
        Err(e) => {
            let _ = std::fs::remove_dir_all(dest);
            if dest_existed {
                let _ = std::fs::create_dir_all(dest);
            }
            Err(e)
        }
    }
}

fn materialize(
    book: &SourceBook,
    dest: &Path,
    decomposition: Decomposition,
    strict: bool,
    warnings: &mut Vec<UnpackWarning>,
) -> Result<(Vec<String>, usize), IngestError> {
    let mut tabs = Vec::with_capacity(book.sheets.len());
    let mut files = 0usize;
    for sheet in &book.sheets {
        validate_tab_name(&sheet.name)?;
        let dir = dest.join(&sheet.name);
        std::fs::create_dir_all(&dir).map_err(|e| {
            IngestError::io(
                ErrorKind::DestIo,
                format!("cannot create {:?}: {e}", dir.display()),
            )
        })?;
        let blocks = decomposition.blocks(&occupancy(sheet));
        for (filename, content) in sheet_files(sheet, &blocks, &book.resolution, warnings) {
            let full = dir.join(&filename);
            std::fs::write(&full, content).map_err(|e| {
                IngestError::io(
                    ErrorKind::DestIo,
                    format!("cannot write {:?}: {e}", full.display()),
                )
            })?;
            // A sidecar is presentation, not a block the cut produced, so it is not one of the RANGE files this count reports.
            if !fsa1_model::is_presentation_entry(&filename) {
                files += 1;
            }
        }
        tabs.push(sheet.name.clone());
    }
    // AFTER every cell file exists, so a name symlink resolves and a blank corner can be materialized.
    emit_names(dest, &book.names, warnings)?;
    // A workbook is meant to live in git; pin it to LF so a Windows checkout cannot CRLF-mangle grids.
    fsa1_model::write_workbook_gitattributes(dest).map_err(|e| {
        IngestError::io(
            ErrorKind::DestIo,
            format!(
                "cannot write {:?}: {e}",
                dest.join(".gitattributes").display()
            ),
        )
    })?;
    if strict {
        refuse_dropped_geometry(warnings)?;
    }
    Ok((tabs, files))
}

/// A size no range file carries is a size `pack` writes back at its own default, so under `--strict`
/// it is a refusal and not a report line — an unverifiable round trip is never a passing one. Which
/// sizes cross is knowable only once the blocks are cut, so this lands mid-write and leaves the
/// destination absent through [`write_book`]'s cleanup.
fn refuse_dropped_geometry(warnings: &[UnpackWarning]) -> Result<(), IngestError> {
    let Some(dropped) = warnings
        .iter()
        .find(|w| w.category() == UnpackCategory::Geometry)
    else {
        return Ok(());
    };
    Err(IngestError::io(
        ErrorKind::Invalid,
        format!(
            "cannot strictly round-trip this workbook: {dropped}; a size the tree never states is one `pack` writes back differently -- import without --strict to import it lossily, and the fidelity report names every dropped size"
        ),
    ))
}

/// The sheet's whole content as the 1-based cells a [`Decomposition`] partitions, each carrying the
/// style slot the source stated — the ONE place the write leg decides which cells a file has to
/// reach.
fn occupancy(sheet: &SheetSource) -> Vec<StyledCell> {
    let mut out = Vec::new();
    for row in 0..sheet.rows {
        for col in 0..sheet.cols {
            if sheet.is_occupied(col, row) {
                out.push((col + 1, row + 1, sheet.style_index(col, row)));
            }
        }
    }
    out
}

/// Never silently sanitized: a cross-sheet formula still spells the original name, so a rewrite here
/// would break every reference to it.
fn validate_tab_name(name: &str) -> Result<(), IngestError> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        return Err(IngestError::invalid_at_sheet(
            name,
            format!(
                "sheet name {name:?} is not a valid tab-folder name (path separator, NUL, or reserved)"
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::Resolution;
    use crate::source::{SheetSource, SourceCell, SourceValue};

    /// Unique, and never pre-created unless a test does so.
    fn temp_dest(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "fsa1-ingest-atomic-{tag}-{}-{nanos}",
            std::process::id()
        ))
    }

    /// The second sheet's name is invalid, so materialize writes the first sheet's cell files and
    /// then fails — the atomic-cleanup trigger.
    fn book_failing_on_second_sheet() -> SourceBook {
        let cell = |n: f64| SheetSource {
            name: String::new(),
            rows: 1,
            cols: 1,
            cells: vec![SourceCell::unstyled(SourceValue::Number(n))],
            ..Default::default()
        };
        SourceBook {
            sheets: vec![
                SheetSource {
                    name: "Good".to_string(),
                    ..cell(1.0)
                },
                SheetSource {
                    name: "bad/name".to_string(),
                    ..cell(2.0)
                },
            ],
            resolution: Resolution::empty(),
            names: Vec::new(),
        }
    }

    #[test]
    fn a_failed_import_removes_a_dest_it_created() {
        let dest = temp_dest("created");
        assert!(!dest.exists());
        let err = write_book(
            &book_failing_on_second_sheet(),
            &dest,
            Decomposition::Occupancy,
            false,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::Invalid, "{}", err.message);
        assert!(
            !dest.exists(),
            "a failed import must leave no partial output at all"
        );
    }

    #[test]
    fn a_failed_import_restores_a_preexisting_empty_dest() {
        let dest = temp_dest("preexisting");
        std::fs::create_dir_all(&dest).unwrap();
        let err = write_book(
            &book_failing_on_second_sheet(),
            &dest,
            Decomposition::Occupancy,
            false,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::Invalid, "{}", err.message);
        assert!(dest.exists(), "a pre-existing empty dest is restored");
        assert!(
            std::fs::read_dir(&dest).unwrap().next().is_none(),
            "restored dest must be empty so a retry is not blocked"
        );
        std::fs::remove_dir_all(&dest).ok();
    }

    #[test]
    fn a_successful_write_book_emits_one_file_per_block() {
        let dest = temp_dest("ok");
        let book = SourceBook {
            sheets: vec![SheetSource {
                name: "S".to_string(),
                rows: 1,
                cols: 2,
                cells: vec![
                    SourceCell::unstyled(SourceValue::Number(7.0)),
                    SourceCell::default(),
                ],
                ..Default::default()
            }],
            resolution: Resolution::empty(),
            names: Vec::new(),
        };
        let (_, files) = write_book(
            &book,
            &dest,
            Decomposition::Occupancy,
            false,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(files, 1, "the occupancy is one block, which B1 is outside");
        assert!(dest.join("S").join("A1").exists());
        assert!(!dest.join("S").join("B1").exists());
        std::fs::remove_dir_all(&dest).ok();
    }
}
