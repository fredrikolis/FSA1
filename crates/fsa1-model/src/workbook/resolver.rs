// Concern: serves fsa1-ast's Resolver over a loaded workbook, owning the buffers it lends | Non-concern: planning or ordering the computation | IO: (CellRef) -> Value; (RangeRef) -> ArrayView

use std::cell::RefCell;
use std::collections::HashMap;

use fsa1_ast::{ArrayView, CellRef, ErrKind, RangeRef, Resolver, Shape, SheetId, Value};

use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::grid::Cell as GridCell;

use super::{MAX_RANGE_CELLS, Workbook};

impl Workbook {
    fn resolve_sheet(&self, sheet: Option<SheetId>) -> u32 {
        sheet.map_or_else(|| self.current_sheet.get(), |SheetId(i)| i)
    }

    /// The anchor for a refusal raised from INSIDE a formula's evaluation: the file on the stack, or
    /// defensively the current tab, since nothing here runs outside an evaluation.
    fn eval_loc(&self) -> Loc {
        match self.current_file.get() {
            Some(id) => Loc::tab_file(&self.tab_name(id.0), &self.file_name(id)),
            None => Loc::tab(&self.tab_name(self.current_sheet.get())),
        }
    }
}

impl Resolver for Workbook {
    /// A pure READ, in precedence order: the pass results, then the memo, then the grid. A formula
    /// cell is never recomputed here — the plan guaranteed it computed first, in dependency order.
    fn value(&self, cell: CellRef) -> Value {
        let sheet = self.resolve_sheet(cell.sheet);
        let key = (sheet, cell.col, cell.row);
        if let Some(v) = self.results.borrow().get(&key) {
            return v.clone();
        }
        if let Some(v) = self.memo.borrow().get(&key) {
            return v.clone();
        }
        let Some((_, file)) = self.covering(sheet, cell.col, cell.row) else {
            return Value::Blank; // a gap: no file claims this cell
        };
        let dr = cell.row - file.region.min_row;
        let dc = cell.col - file.region.min_col;
        match file.grid.cell_at(dr, dc) {
            GridCell::Value { value, .. } => value.clone(),
            GridCell::LoadError { diag, .. } => crate::grid::load_error_value(diag),
            // Unreachable in a proper demand, since the plan is a superset of what eval reads: the assert fails loud if a planning change under-approximates deps, while release stays total.
            GridCell::Formula { .. } => {
                debug_assert!(
                    false,
                    "unplanned formula cell read at ({sheet}, {}, {})",
                    cell.col, cell.row
                );
                Value::Blank
            }
        }
    }

    fn range(&self, range: RangeRef) -> ArrayView<'_> {
        // The arena key is QUALIFIED and NORMALIZED, so `A1:A3` on two sheets are two entries while `B2:A1` and `A1:B2` are one.
        let eff = SheetId(self.resolve_sheet(range.start.sheet));
        let norm = range.normalized();
        // An open axis binds to the tab's used bounds through the SAME helper the plan pass and the computation hash use, so the three cannot measure differently.
        let m = self.clamped_rect(
            eff.0,
            norm.start.col,
            norm.end.col,
            norm.start.row,
            norm.end.row,
        );
        let (c0, r0, c1, r1) = (m.c0, m.r0, m.c1, m.r1);
        let key = RangeRef {
            start: CellRef {
                col: c0,
                row: r0,
                sheet: Some(eff),
            },
            end: CellRef {
                col: c1,
                row: r1,
                sheet: Some(eff),
            },
        };
        if let Some(view) = self.arena.get(key) {
            return view;
        }

        let (rows, cols, area) = (m.rows, m.cols, m.area);
        if area > MAX_RANGE_CELLS {
            // Deterministic — a function of the range size, not of order — so caching it is sound.
            self.refuse(Diagnostic::new(
                Code::RangeTooLarge,
                self.eval_loc(),
                format!(
                    "referenced range spans {area} cells ({rows} rows x {cols} cols), over the \
                     materialization bound of {MAX_RANGE_CELLS} -- refused as #NUM!-class rather \
                     than allocating every cell"
                ),
            ));
            return self.arena.insert(
                key,
                Shape { rows: 1, cols: 1 },
                vec![Value::Error(ErrKind::Num)],
            );
        }
        // No arena borrow is held across these `value` calls, which may recursively push more buffers.
        let mut buf = Vec::with_capacity((rows as usize) * (cols as usize));
        for r in r0..=r1 {
            for c in c0..=c1 {
                buf.push(self.value(CellRef {
                    col: c,
                    row: r,
                    sheet: Some(eff),
                }));
            }
        }
        self.arena.insert(key, Shape { rows, cols }, buf)
    }

    fn sheet_id(&self, name: &str) -> Option<SheetId> {
        self.tabs
            .iter()
            .position(|t| t.name == name)
            .map(|i| SheetId(i as u32))
    }

    fn now_serial(&self) -> f64 {
        self.now
    }

    /// Reads the CONTENT, never the value, so a formula erroring still reports TRUE and nothing is
    /// evaluated. A load-error cell is not a formula.
    fn is_formula(&self, cell: CellRef) -> bool {
        let sheet = self.resolve_sheet(cell.sheet);
        matches!(
            self.grid_cell_at(sheet, cell.col, cell.row),
            Some(GridCell::Formula { .. })
        )
    }
}

/// `range()` must return a view BORROWING the resolver's store, but the store is built lazily under
/// `&self`. So this is append-only: a materialized buffer is boxed and never moved, freed, or
/// mutated while `&self` lives, which keeps a reference into it valid for the whole borrow.
#[derive(Default, Debug)]
pub(super) struct Arena {
    /// `Box<[Value]>` heap data is address-stable across `Vec` growth.
    bufs: RefCell<Vec<Box<[Value]>>>,
    index: RefCell<HashMap<RangeRef, (Shape, usize)>>,
}

impl Arena {
    /// SAFETY: `bufs` entries are boxed slices whose heap data is independent of the `Vec`'s
    /// reallocations, and are never freed or mutated while `&self` lives — so the pointee outlives
    /// this borrow and no `&mut` to it is ever created.
    pub(super) fn get(&self, key: RangeRef) -> Option<ArrayView<'_>> {
        let (shape, i) = {
            let index = self.index.borrow();
            *index.get(&key)?
        };
        let ptr: *const [Value] = &*self.bufs.borrow()[i];
        let cells: &[Value] = unsafe { &*ptr };
        Some(ArrayView { shape, cells })
    }

    pub(super) fn insert(&self, key: RangeRef, shape: Shape, cells: Vec<Value>) -> ArrayView<'_> {
        {
            let mut bufs = self.bufs.borrow_mut();
            let i = bufs.len();
            bufs.push(cells.into_boxed_slice());
            self.index.borrow_mut().insert(key, (shape, i));
        }
        self.get(key).expect("just inserted the key")
    }
}
