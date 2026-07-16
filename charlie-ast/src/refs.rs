// Concern: the A1 REFERENCE layer — `SheetId` (a cross-sheet handle), `CellRef`/`RangeRef` (resolved coordinates the `Resolver` receives), and `RefNode` (the AST reference node carrying `$`-absolute/relative flags so copy/fill offset math is trivial) | Non-concern: resolving a sheet NAME to a `SheetId` or reading a cell's value (the `Resolver` impl in charlie-model does that) and rendering coordinates back to A1 text (later) | IO: none — coordinate types
//! Reference layer: [`SheetId`], [`CellRef`], [`RangeRef`], [`RefNode`].

/// A handle for a sheet, minted by a [`crate::Resolver`] from a sheet name.
///
/// The `u32` is intentionally `pub`: W0 has no minting authority yet, so the `Resolver` impl (in
/// charlie-model) constructs these directly. It is a plain newtype, *not* an opaque token — do not
/// rely on callers being unable to fabricate one. If a minting invariant is ever needed, make the
/// field private behind a `new`/`index` pair (as [`crate::NodeId`] already does).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SheetId(pub u32);

/// A fully-resolved single-cell coordinate — what the [`crate::Resolver`] is asked to read.
///
/// `col`/`row` are zero-based. `sheet` is `None` for a same-sheet reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CellRef {
    pub col: u32,
    pub row: u32,
    pub sheet: Option<SheetId>,
}

/// A fully-resolved rectangular range `start..=end` (inclusive on both corners).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RangeRef {
    pub start: CellRef,
    pub end: CellRef,
}

/// The AST node for a cell reference — the *syntactic* form, before it is resolved to a
/// [`CellRef`].
///
/// It carries the `$`-anchor flags (`col_abs`/`row_abs`) that distinguish `A1`, `$A1`, `A$1`, and
/// `$A$1`. These are **meaning** (they change what a copy/fill produces), so they participate in
/// equality — `A1` and `$A$1` are different references. `col`/`row` are stored zero-based; the
/// intended internal form makes offset math for copy/fill trivial and renders back to A1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RefNode {
    pub col: u32,
    pub row: u32,
    pub col_abs: bool,
    pub row_abs: bool,
    pub sheet: Option<SheetId>,
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
}
