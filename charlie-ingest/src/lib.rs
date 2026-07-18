// Concern: charlie-ingest — CONVERT a real spreadsheet FILE into a charlie workbook on disk, so the existing format-blind engine then renders/evaluates it (GRID2/GRID3: a new format is a new deserializer; the engine is untouched). The crate is the FORMAT FIREWALL — calamine (+ its zip/xml) lives ONLY here (reader.rs), and charlie-model/ast stay calamine-free. `import_ods` orchestrates the pipeline: read the `.ods` into a format-neutral `SourceBook` (reader) → for each sheet spell the used rectangle as charlie grid file(s) (serialize, which drives translate for formulas) → write a tab folder of grid-only TSV files, refusing to clobber a non-empty destination; every failure a located `IngestError` (CORE2) | Non-concern: the CLI surface (charlie-cli owns argv/exit codes/`import` subcommand), the formula LANGUAGE (charlie-ast), and the on-disk grammar it targets (charlie-model owns filename/grid/overlap) | IO: (a `.ods` path, a destination workbook dir) -> a written tab-of-range-files tree + an `ImportReport`, or a located `IngestError`
//! # charlie-ingest — spreadsheet ingest (ODS first)
//!
//! [`import_ods`] converts a real `.ods` file into a charlie workbook directory the format-blind engine
//! reads unchanged. The pipeline is four small concerns behind one seam ([`source::SourceBook`]): the
//! calamine-confined [`reader`], the OpenFormula→Excel-A1 [`translate`], the value/grid [`serialize`],
//! and this orchestration. A second format (xlsx) reuses everything but [`reader`].

mod dates;
pub mod error;
mod reader;
mod serialize;
mod source;
mod translate;

use std::path::Path;

pub use error::{ErrorKind, IngestError};
pub use source::{SheetSource, SourceBook, SourceCell};

use serialize::sheet_files;

/// What an import wrote: the tab names (in source order) and the count of range files written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportReport {
    pub tabs: Vec<String>,
    pub files: usize,
}

/// Import a `.ods` spreadsheet into a charlie workbook directory `dest`. Each sheet becomes a tab folder
/// holding one range file (`A1:<lastcol><lastrow>`, or `A1` for a 1×1 sheet) whose grid-only TSV fills
/// the used rectangle exactly (GRID4). Refuses (never clobbers) a `dest` that already exists and is
/// non-empty. Every failure is a located [`IngestError`] (CORE2).
pub fn import_ods(src: &Path, dest: &Path) -> Result<ImportReport, IngestError> {
    let book = reader::read_ods(src)?;

    // Never clobber: refuse a destination that already exists and is non-empty (an empty or absent dir
    // is fine — the writes create it). Mirrors `charlie-cli sample`'s never-clobber guarantee.
    if dest.exists() {
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
        for (filename, content) in sheet_files(sheet)? {
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
