// Concern: assembles the grids, harness, layer order and figures into one document | Non-concern: spelling a cell, expanding a spec | IO: (Workbook, Overlay, View, bound specs) -> a document

mod bar;
pub mod carrier;
mod escape;
mod figures;
mod grid;

use fsa1_model::{Declaration, Overlay, View, WhiteSpace, Workbook};

/// Everything the exporter itself paints, sorted FIRST of all layers so the narrowest sidecar root
/// stands whatever a selector's specificity; a cell states only its own top and left edge, and
/// `fsa1-sheet` closes the far two. A grid item is blockified, where `vertical-align` does nothing,
/// so the strut fills the cell and takes its computed value by `inherit` — [`base`] states it.
fn harness() -> String {
    format!(
        "\
fsa1-sheet {{ display: grid; width: max-content; \
border-right: 1px solid #dddddd; border-bottom: 1px solid #dddddd }}
fsa1-caption {{ font-weight: bold; padding: 4px 0 }}
fsa1-region, fsa1-rows, fsa1-row {{ display: contents }}
fsa1-cell, fsa1-head {{ display: block; box-sizing: border-box; padding: 2px 6px; \
{base}; \
border-top: 1px solid #dddddd; border-left: 1px solid #dddddd }}
fsa1-cell::before, fsa1-head::before {{ content: \"\"; display: inline-block; width: 0; \
height: 100%; vertical-align: inherit }}
fsa1-cell[hidden], fsa1-head[hidden] {{ display: none }}
fsa1-head {{ background-color: #f0f0f0 }}
",
        base = base(),
    )
}

/// [`fsa1_model::default_style`] spelled as CSS, which is PRES2's second half by construction: what
/// a cell declaring nothing paints is read off the same value `Overlay::cell_style` resolves for it,
/// so the harness cannot drift from the model. `nowrap` widens to `pre` — the same answer about
/// WRAPPING, over text whose own spaces and newlines this carrier preserves as the ASCII one does.
fn base() -> String {
    fsa1_model::default_style()
        .declarations()
        .iter()
        .map(|declaration| match declaration {
            Declaration::WhiteSpace(WhiteSpace::Nowrap) => "white-space: pre".to_string(),
            Declaration::WhiteSpace(WhiteSpace::Normal) => "white-space: pre-wrap".to_string(),
            other => other.spell(),
        })
        .collect::<Vec<String>>()
        .join("; ")
}

/// ONE self-contained document — no fetch, no asset — carrying every sidecar's BYTES unchanged in a
/// scoped, layered `<style>` over the region its filename names. The layer statement is what makes
/// the browser's cascade the model's, so no `<style>`'s position in the document matters. Each of
/// `figures` is `(name, bound spec)`, ALREADY expanded: this crate resolves no binding.
pub fn document(
    workbook: &Workbook,
    overlay: &Overlay,
    view: &View<'_>,
    figures: &[(String, String)],
) -> String {
    let mut names: Vec<String> = vec!["fsa1-harness".to_string()];
    let mut sheets: Vec<String> = Vec::with_capacity(view.sheets.len());
    for sheet in &view.sheets {
        let scopes = overlay.scopes(workbook, sheet.sheet);
        names.extend((0..scopes.len()).map(|at| grid::layer(sheet.sheet, at)));
        sheets.push(grid::sheet(workbook, overlay, sheet, &scopes));
    }
    let tabs: Vec<&str> = view.sheets.iter().map(|sheet| sheet.name).collect();
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>
@layer {order};
@layer fsa1-harness {{
{harness}}}
{bar_css}</style>
</head>
<body>
{bar}
{body}{figures}
<script>{bar_js}</script>
</body>
</html>"#,
        title = escape::text(&tabs.join(", ")),
        harness = harness(),
        order = names.join(", "),
        bar_css = bar::CSS,
        bar = bar::MARKUP,
        bar_js = bar::SCRIPT,
        body = sheets.join("\n"),
        figures = figures::block(figures),
    )
}

#[cfg(test)]
mod document_test;
