// Concern: how an .xlsx is spelled both ways — packing a workbook into one, reading the facts out of one | Non-concern: the import pipeline, pivots/media | IO: (a workbook) -> .xlsx; (a path) -> facts

mod cell;
mod chart;
mod content_types;
mod doc_props;
mod error;
mod export;
mod figure_chart;
mod package;
mod rels;
mod shared_strings;
mod styles;
mod theme;
mod workbook;
mod worksheet;
mod xlsx_chart;
mod xlsx_meta;
mod xlsx_style;

pub use chart::{BAR_CHART_PART, CHART_PART, Chart, chart_xml};
pub use error::{ErrorKind, IngestError};
pub use export::ExportError;
pub use figure_chart::{chart_for, mark_for, no_mark_reason};
pub use xlsx_chart::{
    Package, SourceAnchor, SourceChart, SourceDrawing, SourceSeries, inline_values_reason,
    parse_chart, read_package,
};
pub use xlsx_meta::{
    CoercedCell, NumFmtMap, RawName, RawTable, UncarriedPart, XlsxMeta, numfmt_coercions,
    numfmt_map, read_meta, strict_roundtrip_check, uncarried_parts,
};
pub use xlsx_style::{
    AxisRun, AxisSize, AxisStyle, BorderStyle, FillPattern, HorizontalAlign, MergedRegion,
    SheetVisuals, StyleTable, Styling, Underline, VertAlign, VerticalAlign, XlsxBorder, XlsxFill,
    XlsxFont, XlsxStyle, read_styling,
};

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
