// Concern: resolves cells and ranges from an in-memory named-sheet grid | Non-concern: the engine, the fs model, what any test asserts | IO: (CellRef) -> Value; (RangeRef) -> ArrayView

use crate::refs::{CellRef, RangeRef, SheetId};
use crate::resolver::Resolver;
use crate::value::{ArrayView, Shape, Value};

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

/// Sheet `0` is the default sheet (`Sheet1`) — what a `sheet: None` reference resolves against.
pub(crate) struct Grid {
    sheets: Vec<Sheet>,
    formula_cells: Vec<(usize, u32, u32)>,
}

impl Grid {
    pub(crate) fn new(cols: u32, cells: Vec<Value>) -> Grid {
        Grid {
            sheets: vec![Sheet {
                name: "Sheet1".to_string(),
                cols,
                cells,
            }],
            formula_cells: Vec::new(),
        }
    }

    pub(crate) fn with_formula(mut self, col: u32, row: u32) -> Grid {
        self.formula_cells.push((0, col, row));
        self
    }

    pub(crate) fn with_sheet(mut self, name: &str, cols: u32, cells: Vec<Value>) -> Grid {
        self.sheets.push(Sheet {
            name: name.to_string(),
            cols,
            cells,
        });
        self
    }

    fn sheet_idx(&self, sheet: Option<SheetId>) -> usize {
        sheet.map_or(0, |SheetId(i)| i as usize)
    }

    fn sheet_of(&self, sheet: Option<SheetId>) -> &Sheet {
        let idx = sheet.map_or(0, |SheetId(i)| i as usize);
        // The resolver contract is total: an out-of-range id falls back to sheet 0, never panics.
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

    /// Materializes WHOLE ROWS of the requested row span, so the view is the true sub-rectangle
    /// only when the range spans the sheet's full width.
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

    fn now_serial(&self) -> f64 {
        crate::resolver::PINNED_NOW_SERIAL
    }

    fn is_formula(&self, cell: CellRef) -> bool {
        let idx = self.sheet_idx(cell.sheet);
        self.formula_cells
            .iter()
            .any(|&(s, c, r)| s == idx && c == cell.col && r == cell.row)
    }
}
