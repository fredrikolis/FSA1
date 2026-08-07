// Concern: frames one sheet's grid as a <table>: its labels, its widths, each cell's address and formula | Non-concern: interning a style, computing a value | IO: (Workbook, Overlay, View) -> markup

use std::fmt::Write;

use fsa1_model::{Overlay, Rect, SheetView, Workbook};

use crate::escape;
use crate::stylesheet::Classes;

/// The cell text is [`SheetView`]'s, already spelled under the view's mode, so an HTML cell and an
/// ASCII cell can never disagree. A sheet with no used region keeps its caption and drops the rows.
pub(crate) fn sheet(
    wb: &Workbook,
    overlay: &Overlay,
    view: &SheetView<'_>,
    classes: &mut Classes,
) -> String {
    let mut out = format!("<table>\n<caption>{}</caption>\n", escape::text(view.name));
    let (Some(grid), Some(region)) = (&view.grid, view.region) else {
        out.push_str("</table>");
        return out;
    };

    out.push_str(&colgroup(overlay, wb, view.sheet, region));
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
            let class = overlay
                .cell_style(wb, view.sheet, col, row)
                .and_then(|style| classes.intern(style))
                .map(|c| format!(" class=\"{c}\""))
                .unwrap_or_default();
            let attrs = format!(
                "{class}{}{}",
                format_args!(" data-ref=\"{}\"", Rect::cell(col, row).label()),
                formula_of(wb, view.sheet, col, row)
                    .map(|src| format!(" data-formula=\"{}\"", escape::text(&src)))
                    .unwrap_or_default(),
            );
            let _ = write!(out, "<td{attrs} tabindex=\"0\">{}</td>", escape::text(text));
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</tbody>\n</table>");
    out
}

/// One `<col>` per column the view spans, the leading one standing for the row-label header. This is
/// where a width BINDS: `table-layout: fixed` reads the column's width here and nowhere else.
fn colgroup(overlay: &Overlay, wb: &Workbook, sheet: u32, region: Rect) -> String {
    let runs = overlay.column_widths(wb, sheet);
    let width = |col: u32| {
        runs.iter()
            .find(|r| col >= r.start && col <= r.end)
            .map(|r| r.size)
    };
    let mut out = String::from("<colgroup>\n<col>");
    for col in region.min_col..=region.max_col {
        match width(col) {
            Some(size) => {
                let _ = write!(out, "<col style=\"width: {}\">", size.spell());
            }
            None => out.push_str("<col>"),
        }
    }
    out.push_str("\n</colgroup>\n");
    out
}

/// The cell's own `=…` text, read off the grid the table already spelled — never re-derived, so the
/// formula bar and the cell it names cannot disagree.
fn formula_of(wb: &Workbook, sheet: u32, col: u32, row: u32) -> Option<String> {
    let source = wb.source_at(sheet, col, row)?;
    match (source.array_continuation, source.cell) {
        (false, fsa1_model::Cell::Formula { src, .. }) => Some(src.clone()),
        _ => None,
    }
}
