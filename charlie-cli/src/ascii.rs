// Concern: the ASCII PRESENTATION layer — lay a model-built `RenderGrid` (column-letter header + row-number gutter + cell strings) and a lint `Diagnostic` list into `comfy-table` ASCII grids the terminal shows; this is the ONLY place comfy-table glyphs/borders live | Non-concern: WHAT to show (charlie-model's `render`/`lint` own the demand-driven eval, the value spelling, and the diagnostic detection — this draws their output, never computes it), argv parsing and exit codes (main.rs) | IO: (a `&RenderGrid`) -> an ASCII table `String`; (`&[Diagnostic]`) -> an ASCII table `String`
//! comfy-table drawing: [`grid_table`] renders a spreadsheet viewport, [`diagnostics_table`]
//! renders a lint report. Both return the finished ASCII string; main.rs prints it.

use charlie_model::{Diagnostic, RenderGrid};
use comfy_table::presets::ASCII_FULL;
use comfy_table::{Cell, Table};

/// Draw a [`RenderGrid`] as an ASCII spreadsheet: a top-left corner cell, the column-letter header
/// row, and one row per viewport row prefixed by its 1-based number in the gutter.
pub fn grid_table(grid: &RenderGrid) -> String {
    let mut table = Table::new();
    table.load_preset(ASCII_FULL);

    let mut header: Vec<Cell> = Vec::with_capacity(grid.col_labels.len() + 1);
    header.push(Cell::new("")); // the gutter/header corner
    header.extend(grid.col_labels.iter().map(Cell::new));
    table.set_header(header);

    for row in &grid.rows {
        let mut cells: Vec<Cell> = Vec::with_capacity(row.cells.len() + 1);
        cells.push(Cell::new(&row.row_label));
        cells.extend(row.cells.iter().map(Cell::new));
        table.add_row(cells);
    }

    table.to_string()
}

/// Draw a lint report as an ASCII table: one row per diagnostic with its severity, stable code,
/// located pointer (the offending file / body position / tab), and message. An empty slice yields a
/// single "no diagnostics" row so the output is never ambiguous.
pub fn diagnostics_table(diags: &[Diagnostic]) -> String {
    let mut table = Table::new();
    table.load_preset(ASCII_FULL);
    table.set_header(vec![
        Cell::new("severity"),
        Cell::new("code"),
        Cell::new("location"),
        Cell::new("message"),
    ]);

    if diags.is_empty() {
        table.add_row(vec![
            Cell::new("ok"),
            Cell::new("none"),
            Cell::new("-"),
            Cell::new("no diagnostics: the workbook is clean"),
        ]);
        return table.to_string();
    }

    for d in diags {
        table.add_row(vec![
            Cell::new(severity_str(d)),
            Cell::new(d.code.code_str()),
            Cell::new(d.loc.to_string()),
            Cell::new(&d.message),
        ]);
    }
    table.to_string()
}

/// The lowercase severity word for a diagnostic (all W2/W4 model refusals are errors).
fn severity_str(d: &Diagnostic) -> &'static str {
    use charlie_model::Severity;
    match d.code.severity() {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}
