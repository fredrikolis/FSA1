// Concern: assembles the tables, the stylesheet and the bar into one standalone document | Non-concern: spelling a cell, the bar's own content | IO: (Workbook, Overlay, View) -> a document

mod bar;
mod escape;
mod stylesheet;
mod table;

use fsa1_model::{Overlay, View, Workbook};

use crate::stylesheet::Classes;

/// Serialize `view` as ONE self-contained document — no fetch, no asset — each cell classed by the
/// [`fsa1_model::CellStyle`] `overlay` resolves to over `workbook`. The tables are framed first: a
/// class exists once the cell wearing it has been seen, so the stylesheet above them is complete and
/// in document order. The bar reads the `<td>` the table already spelled and derives nothing.
pub fn document(workbook: &Workbook, overlay: &Overlay, view: &View<'_>) -> String {
    let mut classes = Classes::default();
    let tables: Vec<String> = view
        .sheets
        .iter()
        .map(|sheet| table::sheet(workbook, overlay, sheet, &mut classes))
        .collect();
    // Keyed off what the tables EMIT: `fixed` also stops an unwidened column sizing to its content.
    let any_width = tables.iter().any(|t| t.contains("<col style="));
    let names: Vec<&str> = view.sheets.iter().map(|sheet| sheet.name).collect();
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>
{css}{bar_css}</style>
</head>
<body>
{bar}
{body}
<script>{bar_js}</script>
</body>
</html>"#,
        title = escape::text(&names.join(", ")),
        css = classes.css(any_width),
        bar_css = bar::CSS,
        bar = bar::MARKUP,
        bar_js = bar::SCRIPT,
        body = tables.join("\n"),
    )
}

#[cfg(test)]
mod document_test;
