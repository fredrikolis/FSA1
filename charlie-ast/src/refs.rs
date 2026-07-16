// Concern: the A1 REFERENCE layer, split cleanly across the syntax/semantics seam (ast-standards PART 6) — the SYNTACTIC AST reference nodes `RefNode`/`RangeNode` (which carry the parsed sheet NAME `SheetName` as written, plus their `$`-absolute flags: `RefNode`'s `col_abs`/`row_abs` and `RangeNode`'s per-corner `start_col_abs`/`start_row_abs`/`end_col_abs`/`end_row_abs`, so the drag-fill offset math — `RefNode::offset`, `RangeNode::offset`, and the shared `offset_coord` here — pins absolute axes and shifts only relative ones), and the RESOLVED coordinate types `SheetId`/`CellRef`/`RangeRef` the `Resolver` receives (a sheet handle + zero-based col/row); `RefNode`/`RangeNode` carry a `SheetName`, `CellRef`/`RangeRef` carry a `SheetId`, and `resolve` is the one place a name becomes an id | Non-concern: mapping a sheet NAME to a `SheetId` or reading a cell's value (the `Resolver` impl in charlie-model does that; `resolve` only threads a caller-supplied lookup) and rendering coordinates back to A1 text (later) | IO: none — coordinate types
//! Reference layer: the syntactic [`RefNode`]/[`RangeNode`] (parsed, name-carrying) and the resolved
//! [`SheetId`]/[`CellRef`]/[`RangeRef`] (what the [`crate::Resolver`] is asked to read).
//!
//! The split is the syntax/semantics seam (ast-standards PART 6): a *reference node* holds the sheet
//! **name** exactly as it was written (`SheetName`, an owned string — syntax), and knows nothing of
//! any sheet table. Resolving that name to a [`SheetId`] is a [`crate::Resolver`] act performed at
//! eval; [`RefNode::resolve`] / [`RangeNode::resolve`] are the single seam that maps a syntactic node
//! to the resolved [`CellRef`]/[`RangeRef`] a resolver reads, threading a caller-supplied name→id
//! lookup so this module stays filesystem- (and sheet-table-) blind.

/// A sheet name exactly as it was parsed from a reference (`Sheet1` in `Sheet1!A1`, or the interior
/// of a quoted `'My Sheet'!A1`).
///
/// This is **syntax**: an owned string carried verbatim from the formula text, *not* a resolved
/// handle. Equality is exact string equality (two nodes are equal iff they name the same sheet the
/// same way) — case-folding and name→handle resolution are semantic concerns the [`crate::Resolver`]
/// owns, never baked into the syntactic node (ast-standards PART 6: "no semantics baked into
/// syntax"). It is a leaf value, parsed once at the lowering boundary (ast-standards PART 2), never
/// retained source to re-parse.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SheetName(Box<str>);

impl SheetName {
    /// Intern a parsed sheet name (owned).
    pub fn new(name: impl Into<Box<str>>) -> SheetName {
        SheetName(name.into())
    }

    /// The name as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A handle for a sheet, minted by a [`crate::Resolver`] from a [`SheetName`].
///
/// The `u32` is intentionally `pub`: W0 has no minting authority yet, so the `Resolver` impl (in
/// charlie-model) constructs these directly. It is a plain newtype, *not* an opaque token — do not
/// rely on callers being unable to fabricate one. If a minting invariant is ever needed, make the
/// field private behind a `new`/`index` pair (as [`crate::NodeId`] already does).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SheetId(pub u32);

/// A fully-resolved single-cell coordinate — what the [`crate::Resolver`] is asked to read.
///
/// `col`/`row` are zero-based. `sheet` is a resolved [`SheetId`] (semantics), or `None` for a
/// same-sheet reference. A [`RefNode`]'s syntactic sheet *name* becomes this `SheetId` only at
/// [`RefNode::resolve`] time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CellRef {
    pub col: u32,
    pub row: u32,
    pub sheet: Option<SheetId>,
}

/// A fully-resolved rectangular range `start..=end` (inclusive on both corners) — what the
/// [`crate::Resolver`] is asked to read.
///
/// NOTE: `start..=end` may be **un-normalized** (`start` above/left-of `end` is not guaranteed) when
/// this range came from a drag-fill offset — [`RangeNode::offset`] can invert the corner ordering
/// when the corners carry differing `$`-anchor flags. Callers that iterate the rectangle normalize
/// per axis (`Workbook::range` takes min/max on each corner before materializing/keying).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RangeRef {
    pub start: CellRef,
    pub end: CellRef,
}

/// The AST node for a cell reference — the *syntactic* form, before it is resolved to a [`CellRef`].
///
/// It carries the `$`-anchor flags (`col_abs`/`row_abs`) that distinguish `A1`, `$A1`, `A$1`, and
/// `$A$1`, and the parsed sheet **name** (`sheet`) for a cross-sheet reference (`Sheet1!A1`). These
/// are **meaning** (they change what a copy/fill produces, and *which* sheet is named), so they
/// participate in equality — `A1` and `$A$1` differ, as do `A1` and `Sheet2!A1`. `col`/`row` are
/// stored zero-based so offset math for copy/fill is trivial and renders back to A1. Because it
/// carries an owned [`SheetName`], `RefNode` is not `Copy` (unlike the resolved [`CellRef`]).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RefNode {
    pub col: u32,
    pub row: u32,
    pub col_abs: bool,
    pub row_abs: bool,
    pub sheet: Option<SheetName>,
}

impl RefNode {
    /// Resolve this syntactic reference to the [`CellRef`] a [`crate::Resolver`] reads, mapping the
    /// sheet **name** (if any) to a [`SheetId`] via the caller-supplied `lookup`. Returns `None` iff
    /// a named sheet is unknown (the evaluator maps that to `#REF!`). A same-sheet ref (`sheet:
    /// None`) resolves without consulting `lookup`. This is the syntax→semantics crossing for a ref.
    pub fn resolve(&self, lookup: impl FnOnce(&str) -> Option<SheetId>) -> Option<CellRef> {
        let sheet = match &self.sheet {
            None => None,
            Some(name) => Some(lookup(name.as_str())?),
        };
        Some(CellRef {
            col: self.col,
            row: self.row,
            sheet,
        })
    }

    /// DRAG-FILL this reference: shift each RELATIVE axis by the delta (a `$`-anchored axis stays
    /// put), returning the offset node or `None` if a relative axis would move off-sheet (a
    /// coordinate `< 0`, or past `u32::MAX`). The sheet name and the `$`-anchor flags are preserved.
    /// This is the single-cell half of the drag-fill transform (`offset_refs` walks a whole tree).
    pub fn offset(&self, d_row: i64, d_col: i64) -> Option<RefNode> {
        Some(RefNode {
            col: offset_coord(self.col, self.col_abs, d_col)?,
            row: offset_coord(self.row, self.row_abs, d_row)?,
            col_abs: self.col_abs,
            row_abs: self.row_abs,
            sheet: self.sheet.clone(),
        })
    }
}

/// Shift one coordinate for a drag-fill: an ABSOLUTE (`$`-anchored) axis is unchanged; a relative one
/// moves by `delta`. `None` iff the relative move lands off-sheet (`< 0` or past `u32::MAX`), which
/// the evaluator maps to `#REF!` for that filled cell.
fn offset_coord(base: u32, is_abs: bool, delta: i64) -> Option<u32> {
    if is_abs {
        return Some(base);
    }
    let moved = i64::from(base).checked_add(delta)?;
    if moved < 0 {
        return None;
    }
    u32::try_from(moved).ok()
}

/// The AST node for a range reference — the *syntactic* form, before it is resolved to a
/// [`RangeRef`]. The syntactic analogue of [`RefNode`] for `A1:B10` / `Sheet1!A1:B10`.
///
/// Corners are stored zero-based and normalized to top-left..bottom-right at fold time, so a reversed
/// spelling (`B2:A1`) still resolves. The sheet **name** (if any) qualifies the whole range (Excel:
/// `Sheet1!A1:B2` reads A1:B2 on Sheet1). Like [`RefNode`], the name is syntax; it becomes a
/// [`SheetId`] only at [`RangeNode::resolve`].
///
/// Each corner carries its own `$`-anchor flags (`start_col_abs`/`start_row_abs`/`end_col_abs`/
/// `end_row_abs`) so a DRAG-FILL of the enclosing formula ([`RangeNode::offset`]) can shift only the
/// relative axes of each corner — a mixed range like `$E$2:E2` (running-count) shifts its end but not
/// its start. The flags travel with their normalized corner (the min-corner keeps whichever endpoint's
/// flag it came from, independently per axis), and they are **meaning** (a copy/fill of an absolute
/// range produces different cells than a relative one), so they participate in equality.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RangeNode {
    pub start_col: u32,
    pub start_row: u32,
    pub end_col: u32,
    pub end_row: u32,
    pub start_col_abs: bool,
    pub start_row_abs: bool,
    pub end_col_abs: bool,
    pub end_row_abs: bool,
    pub sheet: Option<SheetName>,
}

impl RangeNode {
    /// Resolve this syntactic range to the [`RangeRef`] a [`crate::Resolver`] reads, mapping the sheet
    /// **name** (if any) to a [`SheetId`] via `lookup`. Returns `None` iff a named sheet is unknown
    /// (the evaluator maps that to `#REF!`). Both corners carry the same resolved sheet.
    pub fn resolve(&self, lookup: impl FnOnce(&str) -> Option<SheetId>) -> Option<RangeRef> {
        let sheet = match &self.sheet {
            None => None,
            Some(name) => Some(lookup(name.as_str())?),
        };
        Some(RangeRef {
            start: CellRef {
                col: self.start_col,
                row: self.start_row,
                sheet,
            },
            end: CellRef {
                col: self.end_col,
                row: self.end_row,
                sheet,
            },
        })
    }

    /// DRAG-FILL this range: shift each corner's RELATIVE axes by the delta, each `$`-anchored axis
    /// staying put (so `$E$2:E2` shifts only its end row). `None` iff any relative corner would move
    /// off-sheet. The sheet name and every `$`-anchor flag are preserved. NOTE: a uniform delta is
    /// **not** applied to both corners when they carry different per-axis `$`-anchor flags — the
    /// absolute corner stays put while the relative one moves, so the top-left..bottom-right ordering
    /// MAY INVERT (e.g. `E2:$E$10` dragged past row 10 yields `start_row > end_row`). Callers must
    /// re-normalize (`Workbook::range()` does, via min/max on each axis); `resolve()` does not.
    pub fn offset(&self, d_row: i64, d_col: i64) -> Option<RangeNode> {
        Some(RangeNode {
            start_col: offset_coord(self.start_col, self.start_col_abs, d_col)?,
            start_row: offset_coord(self.start_row, self.start_row_abs, d_row)?,
            end_col: offset_coord(self.end_col, self.end_col_abs, d_col)?,
            end_row: offset_coord(self.end_row, self.end_row_abs, d_row)?,
            start_col_abs: self.start_col_abs,
            start_row_abs: self.start_row_abs,
            end_col_abs: self.end_col_abs,
            end_row_abs: self.end_row_abs,
            sheet: self.sheet.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_and_relative_refs_are_distinct() {
        let a1 = RefNode {
            col: 0,
            row: 0,
            col_abs: false,
            row_abs: false,
            sheet: None,
        };
        let abs_a1 = RefNode {
            col: 0,
            row: 0,
            col_abs: true,
            row_abs: true,
            sheet: None,
        };
        // Same coordinate, different `$`-anchoring => different meaning => not equal.
        assert_ne!(a1, abs_a1);
    }

    #[test]
    fn a_sheet_name_is_meaning_and_participates_in_equality() {
        let here = RefNode {
            col: 0,
            row: 0,
            col_abs: false,
            row_abs: false,
            sheet: None,
        };
        let there = RefNode {
            sheet: Some(SheetName::new("Sheet2")),
            ..here.clone()
        };
        // Same coordinate, a different sheet name => different reference.
        assert_ne!(here, there);
        // The name is carried verbatim (exact-equality syntax; case-folding is a Resolver concern).
        assert_ne!(
            RefNode {
                sheet: Some(SheetName::new("SHEET2")),
                ..here.clone()
            },
            there
        );
    }

    #[test]
    fn resolve_maps_a_name_to_a_sheet_id_and_flags_unknown_sheets() {
        let same = RefNode {
            col: 3,
            row: 4,
            col_abs: false,
            row_abs: false,
            sheet: None,
        };
        // A same-sheet ref resolves without consulting the lookup.
        assert_eq!(
            same.resolve(|_| unreachable!("no name to resolve")),
            Some(CellRef {
                col: 3,
                row: 4,
                sheet: None,
            })
        );

        let cross = RefNode {
            sheet: Some(SheetName::new("Data")),
            ..same
        };
        // A known sheet name resolves to its id.
        assert_eq!(
            cross.resolve(|n| (n == "Data").then_some(SheetId(7))),
            Some(CellRef {
                col: 3,
                row: 4,
                sheet: Some(SheetId(7)),
            })
        );
        // An unknown sheet name resolves to `None` (the evaluator maps this to `#REF!`).
        assert_eq!(cross.resolve(|_| None), None);
    }

    #[test]
    fn cross_sheet_ref_differs_from_same_sheet() {
        let here = CellRef {
            col: 3,
            row: 4,
            sheet: None,
        };
        let there = CellRef {
            col: 3,
            row: 4,
            sheet: Some(SheetId(1)),
        };
        assert_ne!(here, there);
    }

    #[test]
    fn a_range_node_resolves_both_corners_onto_the_named_sheet() {
        let rn = RangeNode {
            start_col: 0,
            start_row: 0,
            end_col: 1,
            end_row: 1,
            start_col_abs: false,
            start_row_abs: false,
            end_col_abs: false,
            end_row_abs: false,
            sheet: Some(SheetName::new("Data")),
        };
        let rr = rn.resolve(|n| (n == "Data").then_some(SheetId(2))).unwrap();
        assert_eq!(rr.start.sheet, Some(SheetId(2)));
        assert_eq!(rr.end.sheet, Some(SheetId(2)));
        // An unknown sheet flags the whole range.
        assert_eq!(rn.resolve(|_| None), None);
    }

    #[test]
    fn offset_shifts_relative_axes_and_pins_absolute_ones() {
        // A fully relative C2 (col 2, row 1) dragged down 2 / right 1 -> D4 (col 3, row 3).
        let rel = RefNode {
            col: 2,
            row: 1,
            col_abs: false,
            row_abs: false,
            sheet: None,
        };
        let moved = rel.offset(2, 1).unwrap();
        assert_eq!((moved.col, moved.row), (3, 3));

        // A mixed E$2 (row absolute) dragged down 3 keeps its row, shifts nothing off it.
        let mixed = RefNode {
            col: 4,
            row: 1,
            col_abs: false,
            row_abs: true,
            sheet: Some(SheetName::new("Sales")),
        };
        let m2 = mixed.offset(3, 0).unwrap();
        assert_eq!((m2.col, m2.row), (4, 1));
        assert_eq!(m2.sheet.as_ref().map(SheetName::as_str), Some("Sales"));

        // A fully absolute $A$1 never moves.
        let abs = RefNode {
            col: 0,
            row: 0,
            col_abs: true,
            row_abs: true,
            sheet: None,
        };
        assert_eq!(abs.offset(9, 9).unwrap(), abs);

        // A relative axis driven below zero is off-sheet -> None (#REF!).
        assert_eq!(rel.offset(-5, 0), None);
    }

    #[test]
    fn range_offset_respects_per_corner_anchors() {
        // The running-count range $E$2:E2 -> start ($E$2) is fully absolute, end (E2) fully relative.
        let running = RangeNode {
            start_col: 4,
            start_row: 1,
            end_col: 4,
            end_row: 1,
            start_col_abs: true,
            start_row_abs: true,
            end_col_abs: false,
            end_row_abs: false,
            sheet: None,
        };
        // Dragged down 3 rows: the start stays $E$2, the end grows to E5.
        let g = running.offset(3, 0).unwrap();
        assert_eq!((g.start_col, g.start_row), (4, 1));
        assert_eq!((g.end_col, g.end_row), (4, 4));
    }
}
