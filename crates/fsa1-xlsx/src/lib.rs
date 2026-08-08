// Concern: serializes a loaded workbook, its presentation and its charts into one .xlsx | Non-concern: reading a spreadsheet file, pivots/tables/media | IO: (Workbook, Overlay, charts, dest) -> a .xlsx

mod cell;
mod chart;
mod content_types;
mod doc_props;
mod export;
mod figure_chart;
mod package;
mod rels;
mod shared_strings;
mod styles;
mod theme;
mod workbook;
mod worksheet;

pub use chart::{Chart, chart_xml};
pub use export::ExportError;
pub use figure_chart::chart_for;

use fsa1_model::{Overlay, Workbook};
use std::path::Path;

/// Serialize `workbook`, presented as `overlay` states and drawing `charts`, into a fresh `.xlsx` at
/// `dest`, refusing an already-occupied `dest`. Which figures became charts is the caller's decision:
/// it is settled by reading each chart back, which this crate cannot do.
pub fn write_xlsx(
    workbook: &Workbook,
    overlay: &Overlay,
    charts: &[Chart],
    dest: &Path,
) -> Result<(), ExportError> {
    export::run(workbook, overlay, charts, dest)
}

#[cfg(test)]
mod checkpoint_test;
#[cfg(test)]
mod smoke_test;
