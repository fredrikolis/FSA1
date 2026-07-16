// Concern: the shared in-memory TEST-STUB `Resolver` (extending the W0 resolver stub) — a set of named row-major cell grids (a default sheet plus zero or more additional named sheets), so the lexer/parser/evaluator are exercised entirely against a deterministic, FILESYSTEM-BLIND resolver (the engine's whole outside world is this struct); it maps a sheet NAME to a `SheetId`, routes `value`/`range` to the addressed sheet, and hands `range` a BORROWED `ArrayView` over that grid's own contiguous rows, honoring the resolver contract with no `unsafe` | Non-concern: any real backing store (charlie-model owns the filesystem `Resolver`; this exists only under `#[cfg(test)]`) and non-full-width range windows (each sheet materializes whole rows — tests query full-width ranges) | IO: none at runtime — in-memory grids; compiled only for tests
//! `#[cfg(test)]`-only shared resolver stub, so every engine test walks the same deterministic,
//! filesystem-blind [`Resolver`] rather than re-deriving one per module (DRY). It borrows range
//! cells out of an owned contiguous `Vec` — the firewall's "swap the impl, the engine is unchanged"
//! property, demonstrated in memory — and now backs ≥2 NAMED sheets so cross-sheet resolution
//! (name → [`SheetId`] → value/range) is exercised end-to-end.

use crate::refs::{CellRef, RangeRef, SheetId};
use crate::resolver::Resolver;
use crate::value::{ArrayView, Shape, Value};

/// One named sheet: a row-major grid `cells` of `cols`-wide rows.
struct Sheet {
    name: String,
    cols: u32,
    cells: Vec<Value>,
}

impl Sheet {
    fn idx(&self, col: u32, row: u32) -> usize {
        (row * self.cols + col) as usize
    }
}

/// A collection of named sheets. Sheet `0` is the DEFAULT sheet (what a same-sheet ref — `sheet:
/// None` — resolves against) and is named `Sheet1`. Additional sheets are appended via
/// [`Grid::with_sheet`] and addressed by name.
pub(crate) struct Grid {
    sheets: Vec<Sheet>,
}

impl Grid {
    /// A single-sheet grid (the default sheet `Sheet1`), row-major `cols`-wide — the shape the bulk
    /// of the engine tests use.
    pub(crate) fn new(cols: u32, cells: Vec<Value>) -> Grid {
        Grid {
            sheets: vec![Sheet {
                name: "Sheet1".to_string(),
                cols,
                cells,
            }],
        }
    }

    /// Append an additional NAMED sheet, so a cross-sheet reference (`Other!A1`) resolves to it. The
    /// [`SheetId`] a name maps to is its position in `sheets` (the default sheet is `SheetId(0)`).
    pub(crate) fn with_sheet(mut self, name: &str, cols: u32, cells: Vec<Value>) -> Grid {
        self.sheets.push(Sheet {
            name: name.to_string(),
            cols,
            cells,
        });
        self
    }

    /// The sheet a resolved [`CellRef`]/[`RangeRef`] addresses: its `SheetId` indexes `sheets`, and
    /// `None` is the default sheet `0`.
    fn sheet_of(&self, sheet: Option<SheetId>) -> &Sheet {
        let idx = sheet.map_or(0, |SheetId(i)| i as usize);
        // A synthesized out-of-range id defensively falls back to the default sheet rather than
        // panicking (the resolver contract is total).
        self.sheets.get(idx).unwrap_or(&self.sheets[0])
    }
}

impl Resolver for Grid {
    fn value(&self, cell: CellRef) -> Value {
        let sheet = self.sheet_of(cell.sheet);
        sheet
            .cells
            .get(sheet.idx(cell.col, cell.row))
            .cloned()
            .unwrap_or(Value::Blank)
    }

    /// Borrow the requested rows as a contiguous window out of the ADDRESSED sheet. This stub
    /// materializes **whole rows** (every column) for the requested row span, which is exactly the
    /// sub-rectangle when the range spans the sheet's full width — the shape tests use. The returned
    /// [`ArrayView`] borrows the sheet's own `cells`, so no `unsafe` and no copy.
    fn range(&self, range: RangeRef) -> ArrayView<'_> {
        let sheet = self.sheet_of(range.start.sheet);
        let cols = sheet.cols;
        let r0 = range.start.row;
        let r1 = range.end.row.max(r0);
        let start = ((r0 * cols) as usize).min(sheet.cells.len());
        let end = (((r1 + 1) * cols) as usize).min(sheet.cells.len());
        let cells = &sheet.cells[start..end];
        let rows = (cells.len() as u32) / cols.max(1);
        ArrayView {
            shape: Shape {
                rows,
                cols: cols.max(1),
            },
            cells,
        }
    }

    fn sheet_id(&self, name: &str) -> Option<SheetId> {
        self.sheets
            .iter()
            .position(|s| s.name == name)
            .map(|i| SheetId(i as u32))
    }
}
