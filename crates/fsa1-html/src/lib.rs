// Concern: assembles the tables, stylesheet, bar and figures into one standalone document | Non-concern: spelling a cell, expanding a spec | IO: (Workbook, Overlay, View, bound specs) -> a document

mod bar;
mod escape;
mod figures;
mod stylesheet;
mod table;

use fsa1_model::{Overlay, View, Workbook};

use crate::stylesheet::Classes;

/// ONE self-contained document — no fetch, no asset — each cell classed by the [`fsa1_model::CellStyle`]
/// `overlay` resolves to over `workbook`. The tables are framed first, so the stylesheet above them
/// is complete and in document order. Each of `figures` is `(name, bound spec)`, ALREADY expanded:
/// this crate resolves no binding, and handed none the document is byte-identical to a pre-figure one.
pub fn document(
    workbook: &Workbook,
    overlay: &Overlay,
    view: &View<'_>,
    figures: &[(String, String)],
) -> String {
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
{body}{figures}
<script>{bar_js}</script>
</body>
</html>"#,
        title = escape::text(&names.join(", ")),
        css = classes.css(any_width),
        bar_css = bar::CSS,
        bar = bar::MARKUP,
        bar_js = bar::SCRIPT,
        body = tables.join("\n"),
        figures = figures::block(figures),
    )
}

#[cfg(test)]
mod document_test;
