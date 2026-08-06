// Concern: frames one sheet's grid as a <table> headed by its column and row labels | Non-concern: spelling a cell, naming a class | IO: (a Workbook + a SheetView) -> markup + a class per styled cell

use std::fmt::Write;

use fsa1_model::{SheetView, Workbook};

use crate::escape;
use crate::stylesheet::Classes;

/// The cell text is [`SheetView`]'s, already spelled under the view's mode, so an HTML cell and an
/// ASCII cell can never disagree. A sheet with no used region keeps its caption and drops the rows.
pub(crate) fn sheet(wb: &Workbook, view: &SheetView<'_>, classes: &mut Classes) -> String {
    let mut out = format!("<table>\n<caption>{}</caption>\n", escape::text(view.name));
    let (Some(grid), Some(region)) = (&view.grid, view.region) else {
        out.push_str("</table>");
        return out;
    };

    out.push_str("<thead>\n<tr><th></th>");
    for label in &grid.col_labels {
        let _ = write!(out, "<th>{}</th>", escape::text(label));
    }
    out.push_str("</tr>\n</thead>\n<tbody>\n");

    for (down, line) in grid.rows.iter().enumerate() {
        let _ = write!(out, "<tr><th>{}</th>", escape::text(&line.row_label));
        for (across, text) in line.cells.iter().enumerate() {
            let col = region.min_col + across as u32;
            let row = region.min_row + down as u32;
            let class = wb
                .cell_style(view.sheet, col, row)
                .and_then(|style| classes.intern(style))
                .map(|c| format!(" class=\"{c}\""))
                .unwrap_or_default();
            let _ = write!(out, "<td{class}>{}</td>", escape::text(text));
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</tbody>\n</table>");
    out
}
