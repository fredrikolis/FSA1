// Concern: resolves a scope to a View, spelling its cells and names from one demand | Non-concern: drawing it (present.rs) | IO: (&Workbook, Option<&Overlay>, ViewScope, RenderMode, covers) -> View

use crate::names::{Name, NameScope, NameTarget};
use crate::overlap::Rect;
use crate::overlay::Overlay;
use crate::render::{
    MAX_VIEWPORT_CELLS, RenderGrid, RenderMode, combined_cell, render, viewport_cell_count,
};
use crate::workbook::{FileEntry, FormulaOutcome, Workbook};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewScope {
    Workbook,
    Tab(u32),
    /// The literal rectangle, padded where no file covers it — never clipped to the stated region.
    Region(u32, Rect),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameView {
    pub ident: String,
    pub text: String,
}

/// `files` is the authored structure a nested presenter groups the grid's cells by; it carries no
/// content of its own, and a flat presenter ignores it.
pub struct SheetView<'a> {
    pub sheet: u32,
    pub name: &'a str,
    pub grid: Option<RenderGrid>,
    /// `None` exactly when `grid` is.
    pub region: Option<Rect>,
    pub files: Vec<FileEntry<'a>>,
    pub names: Vec<NameView>,
    /// The cell rectangles a figure covers on this sheet; a drawer that cannot draw a figure marks
    /// them instead.
    pub covers: Vec<Rect>,
}

pub struct View<'a> {
    pub scope: ViewScope,
    pub mode: RenderMode,
    pub sheets: Vec<SheetView<'a>>,
    /// Non-empty only under [`ViewScope::Workbook`], whose root owns them.
    pub names: Vec<NameView>,
}

impl SheetView<'_> {
    /// The A1 label and the already-spelled text at an absolute coordinate — the label being the
    /// grid's own header and gutter, so a nested and a flat view can never disagree about a name.
    pub fn cell(&self, col: u32, row: u32) -> Option<(String, &str)> {
        let (grid, region) = (self.grid.as_ref()?, self.region?);
        if col < region.min_col
            || col > region.max_col
            || row < region.min_row
            || row > region.max_row
        {
            return None;
        }
        let r = grid.rows.get((row - region.min_row) as usize)?;
        let text = r.cells.get((col - region.min_col) as usize)?;
        let label = grid.col_labels.get((col - region.min_col) as usize)?;
        Some((format!("{label}{}", r.row_label), text.as_str()))
    }
}

/// Every coordinate the view shows and every in-scope name's cone accrete into ONE
/// [`Workbook::values_at`] demand, so no two parts of a view can disagree about a cell. Over
/// [`MAX_VIEWPORT_CELLS`] per sheet is a refusal, never a crash. A `covers` rectangle widens any
/// viewport but a [`ViewScope::Region`]'s while that widening still fits: no figure costs the grid.
pub fn view<'a>(
    wb: &'a Workbook,
    overlay: Option<&Overlay>,
    scope: ViewScope,
    mode: RenderMode,
    covers: &[(u32, Rect)],
) -> Result<View<'a>, String> {
    let sheets: Vec<u32> = match scope {
        ViewScope::Workbook => (0..wb.sheet_names().len() as u32).collect(),
        ViewScope::Tab(s) | ViewScope::Region(s, _) => vec![s],
    };
    let mut viewports: Vec<(u32, Option<Rect>, Vec<Rect>)> = Vec::with_capacity(sheets.len());
    for &s in &sheets {
        let mine: Vec<Rect> = covers
            .iter()
            .filter(|(sheet, _)| *sheet == s)
            .map(|(_, rect)| *rect)
            .collect();
        let mut vp = match scope {
            ViewScope::Region(_, rect) => Some(rect),
            // Without an overlay the viewport spans CONTENT alone: a view that will not draw a style has no reason to widen for one.
            _ => overlay.map_or_else(|| wb.content_region(s), |o| o.stated_region(wb, s)),
        };
        // Tested on what the CALLER demanded, before any cover widens it: the demand is the only thing a caller can narrow, so a refusal over a cover would prescribe a fix it has no argv for.
        if let Some(rect) = vp {
            let cells = viewport_cell_count(rect);
            if cells > MAX_VIEWPORT_CELLS {
                return Err(format!(
                    "the region spans {cells} cells, over the render bound of {MAX_VIEWPORT_CELLS} -- narrow it"
                ));
            }
        }
        if !matches!(scope, ViewScope::Region(..)) {
            for rect in &mine {
                // A figure past the content is worth reaching for; one a sheet away is not worth the whole grid, and either way the cells of it inside the viewport still mark.
                let widened = Rect::union(vp, Some(*rect));
                if widened.is_none_or(|r| viewport_cell_count(r) <= MAX_VIEWPORT_CELLS) {
                    vp = widened;
                }
            }
        }
        viewports.push((s, vp, mine));
    }

    let sheet_names: Vec<(u32, Vec<&Name>)> = match scope {
        ViewScope::Region(..) => sheets.iter().map(|&s| (s, Vec::new())).collect(),
        _ => sheets
            .iter()
            .map(|&s| {
                let tab = wb.sheet_names()[s as usize].to_string();
                (s, scoped_names(wb, &NameScope::Sheet(tab)))
            })
            .collect(),
    };
    let book_names: Vec<&Name> = match scope {
        ViewScope::Workbook => scoped_names(wb, &NameScope::Workbook),
        _ => Vec::new(),
    };

    // `Functions` spells authored source, so it demands nothing.
    if matches!(mode, RenderMode::Values | RenderMode::Combined) {
        let mut coords: Vec<(u32, u32, u32)> = Vec::new();
        for (s, vp, _) in &viewports {
            let (s, Some(r)) = (*s, *vp) else { continue };
            for row in r.min_row..=r.max_row {
                for col in r.min_col..=r.max_col {
                    coords.push((s, col, row));
                }
            }
        }
        for (s, names) in &sheet_names {
            coords.extend(name_cones(wb, *s, names));
        }
        coords.extend(name_cones(wb, 0, &book_names));
        wb.values_at(&coords);
    }

    let out_sheets = viewports
        .into_iter()
        .zip(&sheet_names)
        .map(|((s, vp, mine), (_, names))| SheetView {
            sheet: s,
            name: wb.sheet_names()[s as usize],
            grid: vp.map(|r| render(wb, s, r, mode)),
            region: vp,
            files: wb.tab_files(s).unwrap_or_default(),
            names: names.iter().map(|n| name_view(wb, s, n, mode)).collect(),
            covers: mine,
        })
        .collect();

    Ok(View {
        scope,
        mode,
        sheets: out_sheets,
        names: book_names
            .iter()
            .map(|n| name_view(wb, 0, n, mode))
            .collect(),
    })
}

fn scoped_names<'a>(wb: &'a Workbook, want: &NameScope) -> Vec<&'a Name> {
    wb.name_table()
        .names()
        .iter()
        .filter(|n| n.scope == *want)
        .collect()
}

/// The one place the `=(...)` wrapping lives, so the cone joined to the demand and the expression
/// later evaluated against the warm memo are the same text.
fn name_formula(expr: &str) -> String {
    format!("=({expr})")
}

fn name_cones(wb: &Workbook, sheet: u32, names: &[&Name]) -> Vec<(u32, u32, u32)> {
    names
        .iter()
        .filter_map(|n| match &n.target {
            NameTarget::Ref(_) => None,
            NameTarget::Expr(e) => Some(wb.formula_deps(sheet, &name_formula(e))),
        })
        .flatten()
        .collect()
}

/// A symlinked name shows its target reference under every mode. A definition that will not parse
/// falls back to its authored text; the fault surfaces in `check`, not here.
fn name_view(wb: &Workbook, sheet: u32, name: &Name, mode: RenderMode) -> NameView {
    let text = match &name.target {
        NameTarget::Ref(a1) => format!("→ {a1}"),
        NameTarget::Expr(expr) => {
            let source = format!("={expr}");
            let value = || match wb.eval_formula(sheet, &name_formula(expr)) {
                Ok(FormulaOutcome::Value(s) | FormulaOutcome::Error(s)) => s,
                Err(_) => source.clone(),
            };
            match mode {
                RenderMode::Functions => source,
                RenderMode::Values => value(),
                RenderMode::Combined => combined_cell(&value(), &source),
            }
        }
    };
    NameView {
        ident: name.ident.clone(),
        text,
    }
}
