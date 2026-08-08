// Concern: drives the part emitters into a fresh dest, refusing an occupied one | Non-concern: any part's bytes, the CLI envelope | IO: (Workbook, Overlay, charts, dest) -> .xlsx or ExportError

use std::fmt;
use std::fs::File;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use fsa1_model::{Overlay, Workbook};

use crate::chart::{self, Chart};
use crate::package::{self, Part};
use crate::shared_strings::SharedStrings;
use crate::{content_types, doc_props, rels, styles, theme, workbook, worksheet};

/// Why [`crate::write_xlsx`] refused, or failed, to produce the `.xlsx`.
#[derive(Debug)]
pub enum ExportError {
    DestExists(PathBuf),
    Io(std::io::Error),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::DestExists(p) => {
                write!(
                    f,
                    "export destination {:?} already exists (CORE3: a materialized artifact must land in a not-already-occupied location)",
                    p.display()
                )
            }
            ExportError::Io(e) => write!(f, "export I/O failure: {e}"),
        }
    }
}

impl std::error::Error for ExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ExportError::Io(e) => Some(e),
            ExportError::DestExists(_) => None,
        }
    }
}

pub(crate) fn run(
    workbook: &Workbook,
    overlay: &Overlay,
    charts: &[Chart],
    dest: &Path,
) -> Result<(), ExportError> {
    let parts = build_parts(workbook, overlay, charts);
    let file = match File::create_new(dest) {
        Ok(f) => f,
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            return Err(ExportError::DestExists(dest.to_path_buf()));
        }
        Err(e) => return Err(ExportError::Io(e)),
    };
    package::write_package(&parts, file).map_err(|e| match e {
        zip::result::ZipError::Io(io) => ExportError::Io(io),
        other => ExportError::Io(std::io::Error::other(other.to_string())),
    })?;
    Ok(())
}

fn build_parts(workbook: &Workbook, overlay: &Overlay, charts: &[Chart]) -> Vec<Part> {
    let names = workbook.sheet_names();
    let sheet_count = names.len();

    let style_table = styles::build(workbook, overlay);
    let drawings = drawings(charts, sheet_count);

    let mut ss = SharedStrings::new();
    let mut worksheet_parts = Vec::with_capacity(sheet_count);
    for i in 0..sheet_count {
        let drawn = drawings.iter().any(|d| d.sheet == i as u32);
        let bytes = worksheet::emit(workbook, overlay, i as u32, &mut ss, &style_table, drawn);
        worksheet_parts.push(Part::new(
            format!("xl/worksheets/sheet{}.xml", i + 1),
            bytes,
        ));
    }

    let mut parts = vec![
        Part::new(
            "[Content_Types].xml",
            content_types::emit(sheet_count, charts.len(), drawings.len()),
        ),
        Part::new("_rels/.rels", rels::emit_package_rels()),
        Part::new(
            "xl/workbook.xml",
            workbook::emit(&names, workbook.name_table().names()),
        ),
        Part::new(
            "xl/_rels/workbook.xml.rels",
            rels::emit_workbook_rels(sheet_count),
        ),
    ];
    parts.extend(worksheet_parts);
    parts.extend([
        Part::new("xl/sharedStrings.xml", ss.emit()),
        Part::new("xl/styles.xml", style_table.bytes),
        Part::new("xl/theme/theme1.xml", theme::emit()),
        Part::new("docProps/core.xml", doc_props::emit_core()),
        Part::new("docProps/app.xml", doc_props::emit_app()),
    ]);
    parts.extend(chart_parts(workbook, charts, &drawings));
    parts
}

/// One drawing per sheet that has a chart, holding the 1-based chart numbers it anchors in the order
/// they were given — which is the order the anchors and the drawing's own `rIdN` follow.
struct Drawing {
    sheet: u32,
    charts: Vec<usize>,
}

fn drawings(charts: &[Chart], sheet_count: usize) -> Vec<Drawing> {
    (0..sheet_count as u32)
        .filter_map(|sheet| {
            let on_sheet: Vec<usize> = charts
                .iter()
                .enumerate()
                .filter(|(_, c)| c.sheet() == sheet)
                .map(|(at, _)| at + 1)
                .collect();
            (!on_sheet.is_empty()).then_some(Drawing {
                sheet,
                charts: on_sheet,
            })
        })
        .collect()
}

/// The chart parts, their drawings, and the two `_rels` that wire a sheet to a drawing and a drawing
/// to its charts. A workbook stating no figure emits none of them and packs exactly as it did.
fn chart_parts(workbook: &Workbook, charts: &[Chart], drawings: &[Drawing]) -> Vec<Part> {
    let mut parts = Vec::new();
    for (at, chart) in charts.iter().enumerate() {
        parts.push(Part::new(
            format!("xl/charts/chart{}.xml", at + 1),
            chart::emit_chart(chart),
        ));
    }
    for (at, drawing) in drawings.iter().enumerate() {
        let number = at + 1;
        parts.push(Part::new(
            format!("xl/drawings/drawing{number}.xml"),
            chart::emit_drawing(
                drawing.charts.len(),
                chart::anchor_column(workbook.content_region(drawing.sheet)),
            ),
        ));
        parts.push(Part::new(
            format!("xl/drawings/_rels/drawing{number}.xml.rels"),
            rels::emit_drawing_rels(&drawing.charts),
        ));
        parts.push(Part::new(
            format!("xl/worksheets/_rels/sheet{}.xml.rels", drawing.sheet + 1),
            rels::emit_sheet_rels(number),
        ));
    }
    parts
}
