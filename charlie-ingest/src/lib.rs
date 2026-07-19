// Concern: charlie-ingest — CONVERT a real spreadsheet FILE (`.ods` or `.xlsx`) into a charlie workbook on disk, so the existing format-blind engine then renders/evaluates it (GRID2/GRID3: a new format is a new deserializer; the engine is untouched). The crate is the FORMAT FIREWALL — calamine (+ its zip/xml) lives ONLY here (reader.rs), and charlie-model/ast stay calamine-free. `import_file` orchestrates the pipeline: read the source (`.ods`/`.xlsx`, dispatched by extension) into a format-neutral `SourceBook` (reader) → for each sheet spell each NON-BLANK cell as its own A1-named grid file (serialize, which drives translate for formulas) → MATERIALIZE a tab folder of grid-only per-cell files into a location that is not already a workbook (CORE3), refusing to clobber a non-empty destination and ATOMICALLY cleaning up its partial output on any failure (so a failed import never leaves a half-written workbook that blocks a retry); every failure a located `IngestError` (CORE2) | Non-concern: the CLI surface (charlie-cli owns argv/exit codes/`import` subcommand), the formula LANGUAGE (charlie-ast), and the on-disk grammar it targets (charlie-model owns filename/grid/overlap) | IO: (a `.ods`/`.xlsx` path, a destination workbook dir) -> a written tab-of-per-cell-files tree + an `ImportReport`, or a located `IngestError` (with any partial output removed)
//! # charlie-ingest — spreadsheet ingest (ODS + xlsx)
//!
//! [`import_file`] converts a real `.ods` or `.xlsx` file into a charlie workbook directory the
//! format-blind engine reads unchanged. The pipeline is four small concerns behind one seam
//! ([`source::SourceBook`]): the calamine-confined [`reader`] (the ONLY format-specific step — it
//! dispatches the opener by extension), the source-dialect→Excel-A1 [`translate`] (one translator for
//! both dialects), the value/grid [`serialize`], and this orchestration. The second format (xlsx)
//! reuses everything but the [`reader`]'s opener — the seam Batch 5 designed for.

mod dates;
pub mod error;
mod reader;
mod resolve;
mod serialize;
mod source;
mod translate;
mod xlsx_meta;

use std::path::Path;

pub use error::{ErrorKind, IngestError};
pub use resolve::Resolution;
pub use source::{SheetSource, SourceBook, SourceCell};

use serialize::sheet_files;

/// What an import wrote: the tab names (in source order) and the count of per-cell files written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportReport {
    pub tabs: Vec<String>,
    pub files: usize,
}

/// Import a `.ods` or `.xlsx` spreadsheet (dispatched by extension) into a charlie workbook directory
/// `dest`. Each sheet becomes a tab folder holding ONE FILE PER NON-BLANK CELL, named by that cell's
/// A1 coordinate (`A1`, `H3`), whose grid-only content is the single cell's literal or `=formula`
/// (CORE3: a cell is its own file). Refuses (never clobbers) a `dest` that already exists and is
/// non-empty. The materialization is ATOMIC: any failure removes the partial output (recreating a
/// pre-existing empty `dest`) so a retry is never blocked by a half-written workbook. Every failure is
/// a located [`IngestError`] (CORE2).
pub fn import_file(src: &Path, dest: &Path) -> Result<ImportReport, IngestError> {
    // Read runs BEFORE any write, so a source refusal never creates output.
    let book = reader::read_file(src)?;
    write_book(&book, dest)
}

/// Materialize a neutral [`SourceBook`] into `dest` atomically (the write half of [`import_file`], split
/// out so the never-clobber guard + cleanup contract is unit-testable without a real source file).
/// Never clobbers a non-empty `dest`; on ANY materialize failure removes the partial output so a retry
/// is never blocked, restoring a pre-existing empty `dest` as an empty dir (a `dest` the import itself
/// created is removed entirely).
fn write_book(book: &SourceBook, dest: &Path) -> Result<ImportReport, IngestError> {
    // Never clobber: refuse a destination that already exists and is non-empty (an empty or absent dir
    // is fine — the writes create it). Mirrors `charlie-cli sample`'s never-clobber guarantee (CORE3:
    // materialize only into a location that is not already a workbook). This guard runs before any
    // write, and its conflict refusal must NOT clean up the user's own pre-existing content.
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

    // Materialize into `dest`; on ANY failure clean up the partial output so no half-written workbook
    // survives to block a retry (atomic import).
    match materialize(book, dest) {
        Ok(report) => Ok(report),
        Err(e) => {
            let _ = std::fs::remove_dir_all(dest);
            if dest_existed {
                let _ = std::fs::create_dir_all(dest);
            }
            Err(e)
        }
    }
}

/// Write every sheet's per-cell files under `dest`, returning the [`ImportReport`]. The caller
/// ([`import_file`]) owns the atomic cleanup of a partial write, so this only reports the first failure.
fn materialize(book: &SourceBook, dest: &Path) -> Result<ImportReport, IngestError> {
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
        for (filename, content) in sheet_files(sheet, &book.resolution)? {
            let full = dir.join(&filename);
            std::fs::write(&full, content).map_err(|e| {
                IngestError::io(
                    ErrorKind::DestIo,
                    format!("cannot write {:?}: {e}", full.display()),
                )
            })?;
            files += 1;
        }
        tabs.push(sheet.name.clone());
    }
    Ok(ImportReport { tabs, files })
}

/// A sheet name becomes a tab FOLDER name and is also how a cross-sheet formula names it (`Tab!A1`), so
/// it must be a safe single path component. A name with a path separator, a NUL, or a `.`/`..`/empty
/// spelling is a located refusal — never silently sanitized (that would break the cross-sheet refs that
/// still spell the original name).
fn validate_tab_name(name: &str) -> Result<(), IngestError> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        return Err(IngestError::at_sheet(
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
    use crate::source::{SheetSource, SourceCell};

    /// A unique temp dest so parallel test runs never collide (never pre-created unless a test does so).
    fn temp_dest(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "charlie-ingest-atomic-{tag}-{}-{nanos}",
            std::process::id()
        ))
    }

    /// A two-sheet book whose SECOND sheet has an invalid tab name (`bad/name`), so materialize writes
    /// the first sheet's cell files then fails on the second — the atomic-cleanup trigger.
    fn book_failing_on_second_sheet() -> SourceBook {
        let cell = |n: f64| SheetSource {
            name: String::new(),
            rows: 1,
            cols: 1,
            cells: vec![SourceCell::Number(n)],
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
        }
    }

    #[test]
    fn a_failed_import_removes_a_dest_it_created() {
        // Atomicity: dest did not exist; the second sheet's invalid name fails materialize AFTER the
        // first sheet wrote files, so the cleanup must remove the whole partial output (no half-written
        // workbook survives to block a retry via the non-empty-dest refusal).
        let dest = temp_dest("created");
        assert!(!dest.exists());
        let err = write_book(&book_failing_on_second_sheet(), &dest).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Invalid, "{}", err.message);
        assert!(
            !dest.exists(),
            "a failed import must leave no partial output at all"
        );
    }

    #[test]
    fn a_failed_import_restores_a_preexisting_empty_dest() {
        // Atomicity: dest existed EMPTY before (a directory the user made). A failed materialize must
        // wipe the partial output but restore dest as an empty dir — never delete the user's directory,
        // and never leave it non-empty (which would then block a retry).
        let dest = temp_dest("preexisting");
        std::fs::create_dir_all(&dest).unwrap();
        let err = write_book(&book_failing_on_second_sheet(), &dest).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Invalid, "{}", err.message);
        assert!(dest.exists(), "a pre-existing empty dest is restored");
        assert!(
            std::fs::read_dir(&dest).unwrap().next().is_none(),
            "restored dest must be empty so a retry is not blocked"
        );
        std::fs::remove_dir_all(&dest).ok();
    }

    #[test]
    fn a_successful_write_book_emits_per_cell_files() {
        // The happy path through the extracted write half: one non-blank cell -> one A1-named file.
        let dest = temp_dest("ok");
        let book = SourceBook {
            sheets: vec![SheetSource {
                name: "S".to_string(),
                rows: 1,
                cols: 2,
                cells: vec![SourceCell::Number(7.0), SourceCell::Blank],
            }],
            resolution: Resolution::empty(),
        };
        let report = write_book(&book, &dest).unwrap();
        assert_eq!(
            report.files, 1,
            "one non-blank cell -> one file (B1 blank -> none)"
        );
        assert!(dest.join("S").join("A1").exists());
        assert!(!dest.join("S").join("B1").exists());
        std::fs::remove_dir_all(&dest).ok();
    }
}
