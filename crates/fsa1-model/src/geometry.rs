// Concern: which sheet axes a block's declarations size, and at what | Non-concern: reading a declaration, applying one, resolving two blocks over one axis | IO: (root, &Presentation) -> AxisRuns

use crate::declaration::{Chars, Declaration, Points};
use crate::overlap::Rect;
use crate::presentation::{Presentation, Target};

/// A run of consecutive sheet columns, or of rows, inclusive on both ends and zero-based, carrying
/// one declared size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AxisRun<T> {
    pub start: u32,
    pub end: u32,
    pub size: T,
}

/// The sheet columns the block sizes, ascending and disjoint: a block's column `k` is sheet column
/// `root.min_col + k - 1`, sized by the All -> Col half of the cascade [`crate::style::resolve`]
/// runs per cell — a `fsa1-cell` width sizes every column the root spans, a `fsa1-cell:nth-child(k)` one of
/// them. The row and cell halves carry no width, the parser refusing one there.
pub fn declared_widths(root: Rect, presentation: &Presentation) -> Vec<AxisRun<Chars>> {
    let mut base = None;
    let mut overrides = Vec::new();
    for rule in &presentation.rules {
        for declaration in &rule.declarations {
            match (rule.target, declaration) {
                (Target::All, Declaration::Width(w)) => base = Some(*w),
                (Target::Col(k), Declaration::Width(w)) => {
                    overrides.push((root.min_col + k - 1, *w));
                }
                _ => {}
            }
        }
    }
    runs(root.min_col, root.max_col, base, &overrides)
}

/// [`declared_widths`] on the other axis: a block's row `r` is sheet row `root.min_row + r - 1`.
pub fn declared_heights(root: Rect, presentation: &Presentation) -> Vec<AxisRun<Points>> {
    let mut base = None;
    let mut overrides = Vec::new();
    for rule in &presentation.rules {
        for declaration in &rule.declarations {
            match (rule.target, declaration) {
                (Target::All, Declaration::Height(h)) => base = Some(*h),
                (Target::Row(r), Declaration::Height(h)) => {
                    overrides.push((root.min_row + r - 1, *h));
                }
                _ => {}
            }
        }
    }
    runs(root.min_row, root.max_row, base, &overrides)
}

/// `overrides` are ascending and within `lo..=hi`: a parsed presentation's rules ascend by target,
/// and every index is one the parser bounded by the root's own extent.
fn runs<T: Copy>(lo: u32, hi: u32, base: Option<T>, overrides: &[(u32, T)]) -> Vec<AxisRun<T>> {
    let mut out = Vec::new();
    let mut at = lo;
    for (axis, size) in overrides {
        if let Some(base) = base
            && at < *axis
        {
            out.push(AxisRun {
                start: at,
                end: axis - 1,
                size: base,
            });
        }
        out.push(AxisRun {
            start: *axis,
            end: *axis,
            size: *size,
        });
        at = axis + 1;
    }
    if let Some(base) = base
        && at <= hi
    {
        out.push(AxisRun {
            start: at,
            end: hi,
            size: base,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse_filename, parse_rules};

    fn widths(root: &str, rules: &str) -> Vec<AxisRun<Chars>> {
        let region = parse_filename(root).expect("a root names a range").region;
        let presentation =
            parse_rules(&format!("Sheet1/{root}.css"), region, &format!("{rules}\n"))
                .unwrap_or_else(|d| panic!("{rules:?} should parse: {:?}", d[0]));
        declared_widths(region, &presentation)
    }

    /// A bare `fsa1-cell` sizes every column the root spans, and a column selector overrides that one; the
    /// offset is the root's, so a block anchored at B sizes sheet columns B onward.
    #[test]
    fn a_bare_td_covers_the_root_and_a_column_rule_carves_it() {
        assert_eq!(
            widths(
                "B2:D4",
                "  fsa1-cell { width: 10ch }\n  fsa1-cell:nth-child(2) { width: 4ch }",
            ),
            vec![
                AxisRun {
                    start: 1,
                    end: 1,
                    size: Chars(10.0)
                },
                AxisRun {
                    start: 2,
                    end: 2,
                    size: Chars(4.0)
                },
                AxisRun {
                    start: 3,
                    end: 3,
                    size: Chars(10.0)
                },
            ],
        );
        assert_eq!(
            widths("B2:D4", "  fsa1-cell:first-child { width: 4ch }"),
            vec![AxisRun {
                start: 1,
                end: 1,
                size: Chars(4.0)
            }],
        );
        assert!(widths("B2:D4", "  fsa1-cell { color: #3f0421 }").is_empty());
    }
}
