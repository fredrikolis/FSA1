// Concern: declares the syntactic and resolved reference types and their crossing | Non-concern: A1 text syntax, the sheet table, reading a cell | IO: (RefNode, name lookup) -> CellRef

/// A sheet name as written; equality is exact — case-folding is a [`crate::Resolver`] concern.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SheetName(Box<str>);

impl SheetName {
    pub fn new(name: impl Into<Box<str>>) -> SheetName {
        SheetName(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SheetId(pub u32);

/// A resolved cell coordinate; `col`/`row` are zero-based and `sheet` is `None` for the same sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CellRef {
    pub col: u32,
    pub row: u32,
    pub sheet: Option<SheetId>,
}

/// A resolved rectangle, inclusive on both corners.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RangeRef {
    pub start: CellRef,
    pub end: CellRef,
}

impl RangeRef {
    /// The one home of the corner-order rule: top-left/bottom-right, each corner's `sheet` kept.
    pub fn normalized(self) -> RangeRef {
        RangeRef {
            start: CellRef {
                col: self.start.col.min(self.end.col),
                row: self.start.row.min(self.end.row),
                sheet: self.start.sheet,
            },
            end: CellRef {
                col: self.start.col.max(self.end.col),
                row: self.start.row.max(self.end.row),
                sheet: self.end.sheet,
            },
        }
    }
}

/// A syntactic cell reference; the `$`-anchor flags and the sheet name are meaning, so `A1`,
/// `$A$1` and `Sheet2!A1` are three different nodes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RefNode {
    pub col: u32,
    pub row: u32,
    pub col_abs: bool,
    pub row_abs: bool,
    pub sheet: Option<SheetName>,
}

impl RefNode {
    /// The syntax->semantics crossing. `None` iff a named sheet is unknown; the evaluator maps
    /// that to `#REF!`.
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
}

/// A syntactic range; each corner's `$`-anchor flags and the sheet name are meaning, so `$E$2:E2`
/// and `E2:E2` are different nodes. The parser folds corners to top-left..bottom-right.
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
    /// The open-axis sentinel for `A:A` / `1:1`. Deliberately NOT a bounded height like Excel's
    /// 1,048,576: a tab is an unbounded sparse sheet, and the [`crate::Resolver`] clamps an open
    /// axis to the tab's used bounds, so an open reference costs O(populated).
    pub const OPEN: u32 = u32::MAX;

    pub fn is_open_rows(&self) -> bool {
        self.end_row == Self::OPEN
    }

    pub fn is_open_cols(&self) -> bool {
        self.end_col == Self::OPEN
    }

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
        assert_ne!(a1, abs_a1, "$-anchoring is meaning");
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
        assert_ne!(here, there, "the sheet name is meaning");
        assert_ne!(
            RefNode {
                sheet: Some(SheetName::new("SHEET2")),
                ..here.clone()
            },
            there,
            "the name is carried verbatim; case-folding is a Resolver concern"
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
        assert_eq!(
            cross.resolve(|n| (n == "Data").then_some(SheetId(7))),
            Some(CellRef {
                col: 3,
                row: 4,
                sheet: Some(SheetId(7)),
            })
        );
        assert_eq!(
            cross.resolve(|_| None),
            None,
            "an unknown sheet is None; the evaluator maps it to #REF!"
        );
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
    fn normalized_puts_corners_in_canonical_order_and_preserves_sheet() {
        let inverted = RangeRef {
            start: CellRef {
                col: 1,
                row: 1,
                sheet: Some(SheetId(2)),
            },
            end: CellRef {
                col: 0,
                row: 0,
                sheet: Some(SheetId(2)),
            },
        };
        let canonical = RangeRef {
            start: CellRef {
                col: 0,
                row: 0,
                sheet: Some(SheetId(2)),
            },
            end: CellRef {
                col: 1,
                row: 1,
                sheet: Some(SheetId(2)),
            },
        };
        assert_eq!(inverted.normalized(), canonical);
        assert_eq!(
            canonical.normalized(),
            canonical,
            "normalized is idempotent"
        );
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
        assert_eq!(
            rn.resolve(|_| None),
            None,
            "an unknown sheet flags the whole range"
        );
    }
}
