// Concern: serializes a rendered view into one standalone HTML document | Non-concern: computing or spelling a cell, reading HTML, JavaScript | IO: (a Workbook + a View) -> an HTML document

mod escape;
mod stylesheet;
mod table;

use fsa1_model::{View, Workbook};

use crate::stylesheet::Classes;

/// Serialize `view` as one JavaScript-free document, each cell classed by the [`fsa1_model::CellStyle`]
/// the tab's stylesheet resolves to in `workbook`. The tables are framed first: a class exists once the
/// cell wearing it has been seen, so the stylesheet above them is complete and in document order.
pub fn document(workbook: &Workbook, view: &View<'_>) -> String {
    let mut classes = Classes::default();
    let tables: Vec<String> = view
        .sheets
        .iter()
        .map(|sheet| table::sheet(workbook, sheet, &mut classes))
        .collect();
    let names: Vec<&str> = view.sheets.iter().map(|sheet| sheet.name).collect();
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>
{css}</style>
</head>
<body>
{body}
</body>
</html>"#,
        title = escape::text(&names.join(", ")),
        css = classes.css(),
        body = tables.join("\n"),
    )
}

#[cfg(test)]
mod document_test;
