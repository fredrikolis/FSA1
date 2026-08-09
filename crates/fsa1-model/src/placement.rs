// Concern: where a figure sits on the sheet, in its own EMU geometry, and the cells that geometry covers | Non-concern: writing an anchor | IO: (text) -> Placement; (runs, cell) <-> EMU; (runs) -> Rect

use crate::declaration::{Chars, Points};
use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::geometry::AxisRun;
use crate::overlap::Rect;
use fsa1_ast::a1::parse_a1;

/// English Metric Units per length unit, the four exact ratios OOXML is defined in.
const PX: f64 = 9525.0;
const IN: f64 = 914400.0;
const CM: f64 = 360000.0;
const PT: f64 = 12700.0;

/// A column no sidecar sizes: Excel's 8.43ch, which is 64 pixels under the same rounding.
pub const DEFAULT_COL_EMU: i64 = 64 * 9525;
/// A row no sidecar sizes: Excel's 15pt.
pub const DEFAULT_ROW_EMU: i64 = 15 * 12700;

/// A figure with no `width`/`height`: 15cm by 7.5cm, Excel's own default chart box.
const DEFAULT_W_EMU: i64 = 5_400_000;
const DEFAULT_H_EMU: i64 = 2_700_000;

/// The five properties a placement rule may declare, alphabetical — the canonical order every
/// sidecar is held to, RE-STATED here because `presentation.rs`'s own order check is private and
/// these five are no part of the declaration vocabulary a cell's rules draw on.
const PROPERTIES: [&str; 5] = ["anchor", "height", "left", "top", "width"];

/// Where one figure sits. The `anchor`'s SHAPE is the mode: a range fills those cells and resizes
/// with them, a cell pins a fixed box at that corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// The rectangle of cells the figure fills.
    Cells(Rect),
    /// A box of fixed EMU size, offset from the top-left corner of the cell `at` (col, row).
    Box {
        at: (u32, u32),
        left: i64,
        top: i64,
        w: i64,
        h: i64,
    },
}

impl Placement {
    /// `located` is the sidecar as it locates — `<tab>/<figure>.css` — so every refusal anchors on
    /// the file an author edits.
    pub fn parse(located: &str, text: &str) -> Result<Placement, Diagnostic> {
        let declarations = read_rule(located, text)?;
        let mut anchor = None;
        let (mut left, mut top, mut w, mut h) = (None, None, None, None);
        for (property, value) in &declarations {
            let slot = match *property {
                "anchor" => {
                    anchor = Some(read_anchor(located, value)?);
                    continue;
                }
                "left" => &mut left,
                "top" => &mut top,
                "width" => &mut w,
                _ => &mut h,
            };
            *slot = Some(length(value).ok_or_else(|| {
                refuse(
                    located,
                    format!(
                        "`{property}: {value}` is no length; write a non-negative number with one \
                         of the units `px`, `in`, `cm`, `pt`"
                    ),
                )
            })?);
        }
        let Some(anchor) = anchor else {
            return Err(refuse(
                located,
                "the rule states no `anchor`, and a placement is where the figure sits".to_string(),
            ));
        };
        match anchor {
            Anchor::Cells(rect) => {
                if left.or(top).or(w).or(h).is_some() {
                    return Err(refuse(
                        located,
                        "a RANGE anchor sizes the figure with its cells, so it takes none of \
                         `left`, `top`, `width`, `height`; anchor one cell to state a fixed box"
                            .to_string(),
                    ));
                }
                Ok(Placement::Cells(rect))
            }
            Anchor::At(col, row) => Ok(Placement::Box {
                at: (col, row),
                left: left.unwrap_or(0),
                top: top.unwrap_or(0),
                w: w.unwrap_or(DEFAULT_W_EMU),
                h: h.unwrap_or(DEFAULT_H_EMU),
            }),
        }
    }

    /// The cells this placement occupies, which a drawer that cannot draw a figure marks instead.
    /// The `- 1` EMU is why a box ending on a boundary does not claim the next column, and `.max`
    /// is why a zero-length box still covers its own cell. Every step SATURATES: `length` takes any
    /// finite literal, so input near `i64::MAX` lands on the sheet's far edge, panicking on none.
    pub fn cover(&self, cols: &Axis, rows: &Axis) -> Rect {
        match *self {
            Placement::Cells(rect) => rect,
            Placement::Box {
                at,
                left,
                top,
                w,
                h,
            } => {
                let x0 = cols.edge(at.0).saturating_add(left);
                let y0 = rows.edge(at.1).saturating_add(top);
                let x1 = x0.max(x0.saturating_add(w).saturating_sub(1));
                let y1 = y0.max(y0.saturating_add(h).saturating_sub(1));
                Rect {
                    min_col: cols.locate(x0).0,
                    min_row: rows.locate(y0).0,
                    max_col: cols.locate(x1).0,
                    max_row: rows.locate(y1).0,
                }
            }
        }
    }
}

enum Anchor {
    Cells(Rect),
    At(u32, u32),
}

/// The file is ONE rule, spelled as every sidecar's is: two-space indent, a space inside each brace,
/// `; ` between declarations, no trailing `;`, one closing newline. Anything else is a second
/// spelling of one appearance, which is the very thing a sidecar has none of.
fn read_rule<'a>(located: &str, text: &'a str) -> Result<Vec<(&'a str, &'a str)>, Diagnostic> {
    let malformed = |found: &str| {
        refuse(
            located,
            format!(
                "a placement sidecar holds one rule, spelled \
                 `  figure {{ <property>: <value>; ... }}` and closed by a newline; found {found:?}"
            ),
        )
    };
    let body = text.strip_suffix('\n').ok_or_else(|| malformed(text))?;
    if body.contains('\n') {
        return Err(malformed(text));
    }
    let inner = body
        .strip_prefix("  figure { ")
        .and_then(|rest| rest.strip_suffix(" }"))
        .ok_or_else(|| malformed(body))?;
    if inner.is_empty() || inner.contains(['{', '}']) {
        return Err(malformed(body));
    }
    let mut declarations: Vec<(&str, &str)> = Vec::new();
    for segment in inner.split("; ") {
        if segment.contains(';') {
            return Err(refuse(
                located,
                "declarations are separated by `; `, and a rule never ends on one".to_string(),
            ));
        }
        let (property, value) = segment.split_once(": ").ok_or_else(|| {
            refuse(
                located,
                format!("a declaration is `<property>: <value>`; found {segment:?}"),
            )
        })?;
        if property.trim() != property || value.trim() != value || value.is_empty() {
            return Err(refuse(
                located,
                format!("non-canonical declaration {segment:?}: one space follows the `:`"),
            ));
        }
        if !PROPERTIES.contains(&property) {
            return Err(refuse(
                located,
                format!(
                    "`{property}` is no placement property; a figure states {}",
                    PROPERTIES.join(", ")
                ),
            ));
        }
        if let Some((before, _)) = declarations.last() {
            if *before == property {
                return Err(refuse(
                    located,
                    format!("`{property}` is declared twice in one rule; give it one declaration"),
                ));
            } else if property < *before {
                return Err(refuse(
                    located,
                    format!("declarations are alphabetical: write `{property}` before `{before}`"),
                ));
            }
        }
        declarations.push((property, value));
    }
    Ok(declarations)
}

/// A CELL is a fixed box's corner, a RANGE the cells the figure fills. Both corners are spelled as a
/// filename spells one: uppercase column, no leading-zero row, no `$`.
fn read_anchor(located: &str, value: &str) -> Result<Anchor, Diagnostic> {
    let bad = |why: &str| {
        refuse(
            located,
            format!(
                "`anchor: {value}` {why}; write one cell (`D2`) for a fixed box, or two corners \
                 joined by `:` (`D2:K17`) for a figure that fills them"
            ),
        )
    };
    match value.split_once(':') {
        None => {
            let (col, row) = corner(value).ok_or_else(|| bad("is no A1 cell"))?;
            Ok(Anchor::At(col, row))
        }
        Some((from, to)) => {
            let (min_col, min_row) = corner(from).ok_or_else(|| bad("has no A1 first corner"))?;
            let (max_col, max_row) = corner(to).ok_or_else(|| bad("has no A1 second corner"))?;
            if max_col < min_col || max_row < min_row {
                return Err(bad("puts its second corner above or left of its first"));
            }
            Ok(Anchor::Cells(Rect {
                min_col,
                min_row,
                max_col,
                max_row,
            }))
        }
    }
}

/// `None` for anything a filename would refuse, so a placement's corner is spelled exactly as the
/// cell it names would be on disk.
fn corner(text: &str) -> Option<(u32, u32)> {
    let a1 = parse_a1(text).ok()?;
    (!a1.col_abs && !a1.row_abs && !a1.col_had_lowercase && !a1.row_had_leading_zero)
        .then_some((a1.col, a1.row))
}

/// A CSS length in EMU. A unitless number is `None` rather than read as pixels, as is a negative.
fn length(text: &str) -> Option<i64> {
    let (number, per) = [("px", PX), ("in", IN), ("cm", CM), ("pt", PT)]
        .into_iter()
        .find_map(|(unit, per)| text.strip_suffix(unit).map(|n| (n, per)))?;
    let n: f64 = number.parse().ok()?;
    (n.is_finite() && n >= 0.0).then(|| (n * per).round() as i64)
}

fn refuse(located: &str, message: String) -> Diagnostic {
    Diagnostic::new(Code::FigurePlacement, Loc::file(located), message)
}

/// One sheet axis in EMU: the runs a tab's sidecars size, and the default every other index takes.
/// Both directions are answered here, so a placement and the `<cols>`/`<row ht>` the same export
/// writes measure the sheet by one ruler.
#[derive(Clone, Debug, Default)]
pub struct Axis {
    /// Ascending, disjoint, non-overlapping — what [`crate::Overlay`] already coalesced them into.
    runs: Vec<(u32, u32, i64)>,
    default: i64,
}

impl Axis {
    /// Chars become WHOLE pixels per column before summing, because a `<col width>` renders per
    /// column and a sum of fractions would drift off the grid the anchor lands on. Two narrowings:
    /// the 7/5 pair is Calibri 11's, and hiddenness is not in the model, so a hidden column is
    /// measured as if visible.
    pub fn columns(runs: &[AxisRun<Chars>]) -> Axis {
        Axis {
            runs: runs
                .iter()
                .map(|run| (run.start, run.end, chars_emu(run.size)))
                .collect(),
            default: DEFAULT_COL_EMU,
        }
    }

    /// [`Axis::columns`] on the other axis, where a `<row ht>` is already points.
    pub fn rows(runs: &[AxisRun<Points>]) -> Axis {
        Axis {
            runs: runs
                .iter()
                .map(|run| {
                    (
                        run.start,
                        run.end,
                        (run.size.0 * PT).round().max(0.0) as i64,
                    )
                })
                .collect(),
            default: DEFAULT_ROW_EMU,
        }
    }

    /// The EMU distance from the sheet's origin to the leading edge of `index`. Saturating, for the
    /// same reason [`Placement::cover`] is: an authored width and a far column are both input.
    pub fn edge(&self, index: u32) -> i64 {
        let mut emu = i64::from(index).saturating_mul(self.default);
        for (start, end, size) in &self.runs {
            let covered = i64::from((*end + 1).min(index).saturating_sub(*start));
            emu = emu.saturating_add(covered.saturating_mul(size - self.default));
        }
        emu
    }

    /// The index `emu` falls in and how far into it, which is the `<xdr:col>`/`<xdr:colOff>` pair.
    /// Past the last run the remainder divides by the default, so the walk always terminates.
    pub fn locate(&self, emu: i64) -> (u32, i64) {
        let mut at = 0u32;
        let mut acc = 0i64;
        let emu = emu.max(0);
        for (start, end, size) in &self.runs {
            if *start > at {
                let span = i64::from(*start - at).saturating_mul(self.default);
                if emu < acc.saturating_add(span) {
                    return step(at, emu - acc, self.default);
                }
                acc = acc.saturating_add(span);
                at = *start;
            }
            let span = i64::from(*end - *start + 1).saturating_mul(*size);
            if emu < acc.saturating_add(span) {
                return step(at, emu - acc, *size);
            }
            acc = acc.saturating_add(span);
            at = *end + 1;
        }
        step(at, emu - acc, self.default)
    }
}

/// `size` is positive at every call site: a zero-sized run spans nothing, so the caller's `<` test
/// never admits one. A distance no sheet is that wide saturates on the INDEX, because `as u32`
/// would truncate it into a small, wrong column and there is none past `u32::MAX` to name anyway.
fn step(from: u32, into: i64, size: i64) -> (u32, i64) {
    let whole = into / size;
    let index = u32::try_from(whole).unwrap_or(u32::MAX);
    (from.saturating_add(index), into - whole * size)
}

fn chars_emu(width: Chars) -> i64 {
    let w = width.0.max(0.0);
    let px = if w >= 1.0 {
        (w * 7.0).round() + 5.0
    } else {
        (w * 12.0).round()
    };
    (px * PX) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placed(text: &str) -> Placement {
        Placement::parse("Sheet1/Units.css", text)
            .unwrap_or_else(|d| panic!("{text:?} should place: {d}"))
    }

    fn refused(text: &str) -> Diagnostic {
        Placement::parse("Sheet1/Units.css", text).expect_err("this placement must refuse")
    }

    /// A default column is 8.43 characters, which is 64 pixels, which is what an anchor counts in.
    #[test]
    fn an_unstated_axis_is_excels_own_default_and_a_stated_one_is_whole_pixels() {
        let empty = Axis::columns(&[]);
        assert_eq!(empty.edge(1), 609600);
        assert_eq!(empty.edge(3), 3 * 609600);
        assert_eq!(
            Axis::columns(&[AxisRun {
                start: 0,
                end: 0,
                size: Chars(8.43)
            }])
            .edge(1),
            64 * 9525,
            "8.43 chars rounds to the same 64 pixels the default is",
        );
        assert_eq!(Axis::rows(&[]).edge(1), 190500);
        assert_eq!(
            Axis::rows(&[AxisRun {
                start: 0,
                end: 1,
                size: Points(30.0)
            }])
            .edge(2),
            2 * 30 * 12700,
        );
    }

    /// `locate` is `edge` read backward, and a sub-cell remainder is the anchor's own offset.
    #[test]
    fn locate_inverts_edge_and_keeps_the_remainder() {
        let cols = Axis::columns(&[AxisRun {
            start: 1,
            end: 2,
            size: Chars(20.0),
        }]);
        for index in [0u32, 1, 2, 3, 9] {
            assert_eq!(cols.locate(cols.edge(index)), (index, 0), "column {index}");
        }
        assert_eq!(cols.locate(cols.edge(2) + 1234), (2, 1234));
        assert_eq!(cols.locate(-5), (0, 0));
    }

    /// The four units, exact.
    #[test]
    fn every_unit_converts_to_emu_exactly() {
        assert_eq!(length("15cm"), Some(5_400_000));
        assert_eq!(length("7.5cm"), Some(2_700_000));
        assert_eq!(length("1px"), Some(9525));
        assert_eq!(length("1in"), Some(914400));
        assert_eq!(length("1pt"), Some(12700));
        assert_eq!(length("480"), None, "a unitless number is not pixels");
        assert_eq!(length("-1cm"), None);
    }

    #[test]
    fn a_cell_anchor_is_a_box_and_a_range_anchor_fills_its_cells() {
        assert_eq!(
            placed("  figure { anchor: D2 }\n"),
            Placement::Box {
                at: (3, 1),
                left: 0,
                top: 0,
                w: 5_400_000,
                h: 2_700_000,
            },
        );
        assert_eq!(
            placed("  figure { anchor: D2; height: 5cm; left: 10px; top: 1in; width: 5cm }\n"),
            Placement::Box {
                at: (3, 1),
                left: 95250,
                top: 914400,
                w: 1_800_000,
                h: 1_800_000,
            },
        );
        assert_eq!(
            placed("  figure { anchor: D2:K17 }\n"),
            Placement::Cells(Rect {
                min_col: 3,
                min_row: 1,
                max_col: 10,
                max_row: 16,
            }),
        );
    }

    /// Excel's own default box, 15cm by 7.5cm, over 64px columns and 15pt rows.
    #[test]
    fn a_sizeless_box_covers_the_default_chart_box_worth_of_cells() {
        assert_eq!(
            placed("  figure { anchor: D2 }\n").cover(&Axis::columns(&[]), &Axis::rows(&[])),
            Rect {
                min_col: 3,
                min_row: 1,
                max_col: 11,
                max_row: 15,
            },
            "D2:L16",
        );
    }

    /// A box ending inside a cell claims it, and one ending on a boundary does not claim the next.
    #[test]
    fn a_stated_box_covers_the_cells_its_edges_land_in() {
        assert_eq!(
            placed("  figure { anchor: A1; height: 1.5cm; width: 2cm }\n")
                .cover(&Axis::columns(&[]), &Axis::rows(&[])),
            Rect {
                min_col: 0,
                min_row: 0,
                max_col: 1,
                max_row: 2,
            },
            "A1:B3",
        );
    }

    #[test]
    fn a_range_anchor_covers_exactly_the_cells_it_names() {
        assert_eq!(
            placed("  figure { anchor: B2:C3 }\n").cover(&Axis::columns(&[]), &Axis::rows(&[])),
            Rect {
                min_col: 1,
                min_row: 1,
                max_col: 2,
                max_row: 2,
            },
        );
    }

    /// A length is any finite non-negative literal, so `cover` is handed EMU near `i64::MAX` by a
    /// sidecar that grades clean. It answers the far edge of the sheet — it does not panic in debug,
    /// and it does not wrap into a small wrong rectangle in release. Both the OFFSET that moves the
    /// corner and the SIZE that extends past it saturate.
    #[test]
    fn an_absurd_length_saturates_instead_of_overflowing() {
        let (cols, rows) = (Axis::columns(&[]), Axis::rows(&[]));
        let far = placed(
            "  figure { anchor: A1; height: 1cm; left: 99999999999999999999cm; width: 1cm }\n",
        )
        .cover(&cols, &rows);
        assert_eq!((far.min_col, far.max_col), (u32::MAX, u32::MAX));
        assert!(far.min_col <= far.max_col && far.min_row <= far.max_row);

        let tall = placed(
            "  figure { anchor: A1; height: 1cm; top: 99999999999999999999cm; width: 1cm }\n",
        )
        .cover(&cols, &rows);
        assert_eq!((tall.min_row, tall.max_row), (u32::MAX, u32::MAX));

        let wide = placed(
            "  figure { anchor: B2; height: 99999999999999999999cm; width: 99999999999999999999cm }\n",
        )
        .cover(&cols, &rows);
        assert_eq!(
            (wide.min_col, wide.min_row),
            (1, 1),
            "it still begins at B2"
        );
        assert_eq!((wide.max_col, wide.max_row), (u32::MAX, u32::MAX));
    }

    /// One refusal per rule an author can break, each located on the sidecar.
    #[test]
    fn every_malformed_placement_is_one_located_refusal() {
        for text in [
            "",                                                   // an empty file
            "  figure { anchor: D2 }",                            // no closing newline
            "  figure {  }\n",                                    // declaring nothing
            "figure { anchor: D2 }\n",                            // no two-space indent
            "  figure {anchor: D2}\n",                            // no space inside the braces
            "  td { color: #ff0000 }\n",                          // another selector
            "  anchor: D2\n",                                     // a top-level declaration
            "  figure { anchor: D2 }\n  figure { anchor: E3 }\n", // a second rule
            "  figure { anchor: D2; }\n",                         // a trailing `;`
            "  figure { anchor: D2;width: 5cm }\n",               // no space after the `;`
            "  figure { width: 5cm; anchor: D2 }\n",              // out of alphabetical order
            "  figure { anchor: D2; anchor: E3 }\n",              // declared twice
            "  figure { anchor: D2; colour: red }\n",             // no such property
            "  figure { width: 5cm }\n",                          // no anchor at all
            "  figure { anchor: D2:K17; width: 5cm }\n",          // a length beside a range
            "  figure { anchor: D2; width: 480 }\n",              // a unitless length
            "  figure { anchor: D2; width: -1cm }\n",             // a negative length
            "  figure { anchor: d2 }\n",                          // a lowercase column
            "  figure { anchor: D02 }\n",                         // a leading-zero row
            "  figure { anchor: $D$2 }\n",                        // a `$`
            "  figure { anchor: K17:D2 }\n",                      // an inverted range
        ] {
            let d = refused(text);
            assert_eq!(d.code, Code::FigurePlacement, "{text:?}: {d}");
            assert_eq!(d.loc, Loc::file("Sheet1/Units.css"), "{text:?}");
        }
    }
}
