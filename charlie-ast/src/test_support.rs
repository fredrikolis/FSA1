// Concern: the shared in-memory TEST-STUB `Resolver` (extending the W0 resolver stub) — a fixed row-major cell grid plus one named sheet, so the lexer/parser/evaluator are exercised entirely against a deterministic, FILESYSTEM-BLIND resolver (the engine's whole outside world is this struct); its `range` hands back a BORROWED `ArrayView` over the grid's own contiguous rows, honoring the resolver contract with no `unsafe` | Non-concern: any real backing store (charlie-model owns the filesystem `Resolver`; this exists only under `#[cfg(test)]`) and non-full-width range windows (the stub materializes whole rows — tests query full-width ranges) | IO: none at runtime — an in-memory grid; compiled only for tests
//! `#[cfg(test)]`-only shared resolver stub, so every engine test walks the same deterministic,
//! filesystem-blind [`Resolver`] rather than re-deriving one per module (DRY). It borrows range
//! cells out of an owned contiguous `Vec` — the firewall's "swap the impl, the engine is unchanged"
//! property, demonstrated in memory.

use crate::refs::{CellRef, RangeRef, SheetId};
use crate::resolver::Resolver;
use crate::value::{ArrayView, Shape, Value};

/// A row-major grid of `cols`-wide cells. `Sheet1` is the one known sheet.
pub(crate) struct Grid {
    cols: u32,
    cells: Vec<Value>,
}

impl Grid {
    pub(crate) fn new(cols: u32, cells: Vec<Value>) -> Grid {
        Grid { cols, cells }
    }

    fn idx(&self, col: u32, row: u32) -> usize {
        (row * self.cols + col) as usize
    }
}

impl Resolver for Grid {
    fn value(&self, cell: CellRef) -> Value {
        self.cells
            .get(self.idx(cell.col, cell.row))
            .cloned()
            .unwrap_or(Value::Blank)
    }

    /// Borrow the requested rows as a contiguous window. This stub materializes **whole rows**
    /// (every column) for the requested row span, which is exactly the sub-rectangle when the range
    /// spans the grid's full width — the shape tests use. The returned [`ArrayView`] borrows the
    /// grid's own `cells`, so no `unsafe` and no copy (the resolver contract's borrowed view).
    fn range(&self, range: RangeRef) -> ArrayView<'_> {
        let cols = self.cols;
        let r0 = range.start.row;
        let r1 = range.end.row.max(r0);
        let start = ((r0 * cols) as usize).min(self.cells.len());
        let end = (((r1 + 1) * cols) as usize).min(self.cells.len());
        let cells = &self.cells[start..end];
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
        (name == "Sheet1").then_some(SheetId(0))
    }
}
