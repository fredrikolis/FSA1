// Concern: serializes a loaded workbook into one .xlsx | Non-concern: reading a spreadsheet file, charts/pivots/tables/media | IO: (a Workbook, an Overlay, a dest) -> a .xlsx

mod cell;
mod content_types;
mod doc_props;
mod export;
mod package;
mod rels;
mod shared_strings;
mod styles;
mod theme;
mod workbook;
mod worksheet;

pub use export::ExportError;

use fsa1_model::{Overlay, Workbook};
use std::path::Path;

/// Serialize `workbook`, presented as `overlay` states, into a fresh `.xlsx` at `dest`, refusing an
/// already-occupied `dest`.
pub fn write_xlsx(workbook: &Workbook, overlay: &Overlay, dest: &Path) -> Result<(), ExportError> {
    export::run(workbook, overlay, dest)
}

#[cfg(test)]
mod checkpoint_test;
#[cfg(test)]
mod smoke_test;
