// Concern: assembles grids, harness, layers and figures into a document, and settles which placements FILL cells | Non-concern: spelling a cell, expanding a spec | IO: (a view + figures) -> a page

mod bar;
pub mod carrier;
mod escape;
mod figures;
mod grid;

use fsa1_model::{Declaration, Overlay, Placement, Rect, View, WhiteSpace, Workbook};

/// One figure as the document draws it: the name it captions, its ALREADY-bound spec, the sheet it
/// belongs to and where it sits — `None` for the figure whose position the writer derives.
pub struct BoundFigure {
    pub name: String,
    pub spec: String,
    pub sheet: u32,
    pub placement: Option<Placement>,
}

impl BoundFigure {
    fn fills(&self) -> Option<Rect> {
        fills(self.placement)
    }
}

/// The cells a placement FILLS, and so the grid area the sheet draws the figure in. A fixed box is
/// no such figure: it states a size of its own, which is what the document appends it whole at.
pub fn fills(placement: Option<Placement>) -> Option<Rect> {
    match placement {
        Some(Placement::Cells(rect)) => Some(rect),
        Some(Placement::Box { .. }) | None => None,
    }
}

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
/// the browser's cascade the model's, so no `<style>`'s position in the document matters. Each spec
/// is ALREADY expanded: this crate resolves no binding.
pub fn document(
    workbook: &Workbook,
    overlay: &Overlay,
    view: &View<'_>,
    figures: &[BoundFigure],
) -> String {
    let mut names: Vec<String> = vec!["fsa1-harness".to_string()];
    let mut sheets: Vec<String> = Vec::with_capacity(view.sheets.len());
    let mut after: Vec<&BoundFigure> = figures.iter().filter(|f| f.fills().is_none()).collect();
    for sheet in &view.sheets {
        let scopes = overlay.scopes(workbook, sheet.sheet);
        names.extend((0..scopes.len()).map(|at| grid::layer(sheet.sheet, at)));
        // A figure filling cells is drawn IN them, by the sheet that holds them. One placed by the writer, and one whose cells this view does not reach, has no grid area to take and follows the sheets instead.
        let mut drawn: Vec<(Rect, &BoundFigure)> = Vec::new();
        for figure in figures.iter().filter(|f| f.sheet == sheet.sheet) {
            match (figure.fills(), sheet.region) {
                (Some(cover), Some(vp)) if cover.intersect(&vp).is_some() => {
                    drawn.push((cover, figure));
                }
                (Some(_), _) => after.push(figure),
                (None, _) => {}
            }
        }
        sheets.push(grid::sheet(workbook, overlay, sheet, &scopes, &drawn));
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
        figures = match figures.is_empty() {
            true => String::new(),
            false => figures::block(&after),
        },
    )
}

#[cfg(test)]
mod document_test;
