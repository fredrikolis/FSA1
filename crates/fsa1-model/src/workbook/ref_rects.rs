// Concern: extracts the rectangles an expression references | Non-concern: the PLAN pass over them | IO: (&Expr, home sheet, sheets, content) -> [(sheet, Rect)]

use fsa1_ast::{Expr, RangeNode, SheetId};

use crate::overlap::Rect;

/// The rectangles an expression references, on the sheets it names them on — the closure's step, and
/// the ONE place an open axis is clamped for it. Sheet names and content rects rather than a
/// `&Workbook`, the closure running before one exists; a SUPERSET of what `collect_deps` later
/// demands, [`super::MAX_RANGE_CELLS`] bounding enumerated CELLS and never a set of rectangles.
pub(super) fn ref_rects(
    expr: &Expr,
    home: u32,
    sheets: &[String],
    content: &[Option<Rect>],
) -> Vec<(u32, Rect)> {
    let mut out = Vec::new();
    collect_rects(expr, home, sheets, content, &mut out);
    out
}

fn collect_rects(
    expr: &Expr,
    home: u32,
    sheets: &[String],
    content: &[Option<Rect>],
    out: &mut Vec<(u32, Rect)>,
) {
    let sheet_of = |name: &str| {
        sheets
            .iter()
            .position(|s| s == name)
            .map(|i| SheetId(i as u32))
    };
    let mut descend = |e: &Expr| collect_rects(e, home, sheets, content, out);
    match expr {
        Expr::Lit(_) => {}
        Expr::Ref(r) => {
            if let Some(cr) = r.resolve(sheet_of) {
                let sheet = cr.sheet.map_or(home, |SheetId(i)| i);
                out.push((sheet, Rect::cell(cr.col, cr.row)));
            }
        }
        Expr::Range(rn) => {
            if let Some(rr) = rn.resolve(sheet_of).map(|r| r.normalized()) {
                let sheet = rr.start.sheet.map_or(home, |SheetId(i)| i);
                let used = content.get(sheet as usize).copied().flatten();
                out.push((
                    sheet,
                    Rect {
                        min_col: rr.start.col,
                        min_row: rr.start.row,
                        max_col: open_end(rr.end.col, used.map(|u| u.max_col), rr.start.col),
                        max_row: open_end(rr.end.row, used.map(|u| u.max_row), rr.start.row),
                    },
                ));
            }
        }
        Expr::Unary(_, e) | Expr::ImplicitIntersect(e) | Expr::SpillRef(e) => descend(e),
        Expr::Binary(_, a, b) => {
            descend(a);
            descend(b);
        }
        Expr::Call(_, args) => {
            for a in args {
                descend(a);
            }
        }
    }
}

/// [`super::Workbook::clamp_open`]'s rule over a content rect rather than a loaded workbook, which is
/// all the closure has: an empty tab clamps to the near corner.
fn open_end(end: u32, used: Option<u32>, near: u32) -> u32 {
    if end == RangeNode::OPEN {
        used.unwrap_or(near)
    } else {
        end
    }
}
