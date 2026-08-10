// Concern: frames one sheet as a grid of addressed cells, scoping each sidecar into its layer | Non-concern: the harness, a rule's grammar | IO: (wb, overlay, view, scopes) -> markup; a layer's name

use std::fmt::Write;

use fsa1_model::{Overlay, Rect, SheetView, SidecarScope, Workbook};

use crate::escape;

/// The cell text is [`SheetView`]'s, already spelled under the view's mode, so an HTML cell and an
/// ASCII cell can never disagree. `scopes` is [`Overlay::scopes`] for this sheet, in the order
/// [`Overlay::cell_style`] folds it: index `i` there is layer `fsa1-s<n>-<i>` here, and the `<head>`
/// statement that orders those layers is what makes the page's cascade the model's.
pub(crate) fn sheet(
    wb: &Workbook,
    overlay: &Overlay,
    view: &SheetView<'_>,
    scopes: &[SidecarScope<'_>],
) -> String {
    let n = view.sheet;
    let tab = escape::text(view.name);
    let mut out = format!("<fsa1-caption>{tab}</fsa1-caption>");
    let (Some(grid), Some(vp)) = (&view.grid, view.region) else {
        let _ = write!(
            out,
            "<fsa1-sheet id=\"fsa1-s{n}\" data-tab=\"{tab}\"></fsa1-sheet>"
        );
        return out;
    };
    let _ = write!(
        out,
        "<fsa1-sheet id=\"fsa1-s{n}\" data-tab=\"{tab}\" style=\"grid-template-columns:{};grid-template-rows:{}\">",
        tracks(&overlay.column_widths(wb, n), vp.min_col, vp.max_col, |s| s
            .spell()),
        tracks(&overlay.row_heights(wb, n), vp.min_row, vp.max_row, |s| s
            .spell()),
    );

    out.push_str(&head(1, 1, ""));
    for (across, label) in grid.col_labels.iter().enumerate() {
        out.push_str(&head(1, across as u32 + 2, label));
    }
    for (down, line) in grid.rows.iter().enumerate() {
        out.push_str(&head(down as u32 + 2, 1, &line.row_label));
    }

    let outer = scopes.iter().find(|s| s.tab_layer).map(|s| s.root);
    let regions: Vec<(usize, Rect)> = scopes
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.tab_layer && area(s.root) > 1)
        .map(|(i, s)| (i, s.root))
        .collect();
    for (i, root) in &regions {
        let _ = write!(
            out,
            "<fsa1-region data-root=\"{}\"><fsa1-rows>",
            root.label()
        );
        for row in root.min_row..=root.max_row {
            out.push_str("<fsa1-row>");
            for col in root.min_col..=root.max_col {
                out.push_str(&cell(wb, view, vp, outer, col, row));
            }
            out.push_str("</fsa1-row>");
        }
        out.push_str("</fsa1-rows>");
        out.push_str(&carried(n, *i, "", scopes[*i].text));
        out.push_str("</fsa1-region>");
    }

    out.push_str("<fsa1-rows>");
    if let Some(stated) = overlay.stated_region(wb, n) {
        for row in stated.min_row..=stated.max_row {
            out.push_str("<fsa1-row>");
            for col in stated.min_col..=stated.max_col {
                if regions.iter().all(|(_, root)| !root.contains(col, row)) {
                    out.push_str(&cell(wb, view, vp, outer, col, row));
                }
            }
            out.push_str("</fsa1-row>");
        }
    }
    out.push_str("</fsa1-rows>");

    for (i, scope) in scopes.iter().enumerate() {
        if scope.tab_layer {
            let prelude = format!("(#fsa1-s{n}) to (fsa1-cell[data-outside]) ");
            out.push_str(&carried(n, i, &prelude, scope.text));
        } else if area(scope.root) == 1 {
            let at = scope.root.label();
            let prelude = format!(
                "(#fsa1-s{n} fsa1-row:has(> fsa1-cell[data-ref=\"{at}\"])) to (fsa1-cell:not([data-ref=\"{at}\"])) "
            );
            out.push_str(&carried(n, i, &prelude, scope.text));
        }
    }
    out.push_str("</fsa1-sheet>");
    out
}

/// The sidecar's bytes reach the document UNCHANGED, wrapped and nothing else: this is the whole of
/// what the exporter adds around them, so the text is a literal substring of every page carrying it.
fn carried(sheet: u32, at: usize, prelude: &str, text: &str) -> String {
    format!(
        "<style>@layer {} {{ @scope {prelude}{{{text}}} }}</style>",
        layer(sheet, at)
    )
}

/// One name per scope, over the SHEET INDEX rather than the tab name — a directory name is free to
/// hold a space or an apostrophe, which would invalidate and so DROP the statement that orders them.
pub(crate) fn layer(sheet: u32, at: usize) -> String {
    format!("fsa1-s{sheet}-{at}")
}

/// The label track and then one per VIEWPORT column, which is where an authored size BINDS: the cell
/// filling it states `width: 100%` inline, so the axis run overwrites what the cell cascade resolved.
fn tracks<T: Copy>(
    runs: &[fsa1_model::AxisRun<T>],
    lo: u32,
    hi: u32,
    spell: fn(T) -> String,
) -> String {
    let mut out = String::from("auto");
    for index in lo..=hi {
        match runs.iter().find(|r| index >= r.start && index <= r.end) {
            Some(run) => {
                let _ = write!(out, " {}", spell(run.size));
            }
            None => out.push_str(" auto"),
        }
    }
    out
}

fn head(row: u32, col: u32, text: &str) -> String {
    format!(
        "<fsa1-head style=\"grid-row:{row};grid-column:{col};width:100%;height:100%\">{}</fsa1-head>",
        escape::text(text)
    )
}

/// A cell outside the viewport carries `hidden` and NOTHING else: it counts for `nth-child`, so a
/// region-relative index cannot move with a viewport, while the harness takes it off the grid.
fn cell(
    wb: &Workbook,
    view: &SheetView<'_>,
    vp: Rect,
    outer: Option<Rect>,
    col: u32,
    row: u32,
) -> String {
    let Some((at, text)) = view.cell(col, row) else {
        return "<fsa1-cell hidden></fsa1-cell>".to_string();
    };
    let formula = formula_of(wb, view.sheet, col, row)
        .map(|src| format!(" data-formula=\"{}\"", escape::text(&src)))
        .unwrap_or_default();
    let outside = match outer {
        Some(root) if !root.contains(col, row) => " data-outside",
        _ => "",
    };
    format!(
        "<fsa1-cell data-ref=\"{at}\"{outside}{formula} tabindex=\"0\" \
         style=\"grid-row:{};grid-column:{};width:100%;height:100%\">{}</fsa1-cell>",
        row - vp.min_row + 2,
        col - vp.min_col + 2,
        escape::text(text)
    )
}

fn area(root: Rect) -> u64 {
    u64::from(root.max_col - root.min_col + 1) * u64::from(root.max_row - root.min_row + 1)
}

/// The cell's own `=…` text, read off the workbook the grid already spelled — never re-derived, so
/// the formula bar and the cell it names cannot disagree.
fn formula_of(wb: &Workbook, sheet: u32, col: u32, row: u32) -> Option<String> {
    let source = wb.source_at(sheet, col, row)?;
    match (source.array_continuation, source.cell) {
        (false, fsa1_model::Cell::Formula { src, .. }) => Some(src.clone()),
        _ => None,
    }
}
