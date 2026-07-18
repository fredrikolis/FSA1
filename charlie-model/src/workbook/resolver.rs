// Concern: the RESOLVER surface of the two-pass engine — the [`Resolver`] impl the EVALUATE pass hands to `charlie_ast::eval` (`value` READS an already-computed cell from the pass results, then the memo, then the grid; `range` MATERIALIZES a rectangle once into the append-only [`Arena`] backing the borrowed `ArrayView`s, refusing an over-[`MAX_RANGE_CELLS`] rectangle as a located `#NUM!` and refusing to freeze a depth-tainted buffer; `sheet_id`/`now_serial` expose the tab table and the pinned clock), plus the eval-time anchor `eval_loc` and the `sheet: None` resolution `resolve_sheet` | Non-concern: BUILDING the dependency graph or computing formula nodes (the `plan`/`evaluate` siblings own the passes — `range` only READS values they already computed), and the arena type's escape from this module (it is `pub(super)`, re-exported by no one) | IO: (a `CellRef`/`RangeRef` + the pass results/memo/grids) -> a `Value`/`ArrayView`, plus the located over-large-range `Diagnostic`s `range` pushes
use std::cell::RefCell;
use std::collections::HashMap;

use charlie_ast::{ArrayView, CellRef, ErrKind, RangeRef, Resolver, Shape, SheetId, Value};

use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::grid::Cell as GridCell;

use super::{MAX_RANGE_CELLS, Workbook};

impl Workbook {
    /// The tab a `sheet: None` reference resolves against, or the explicit sheet index.
    fn resolve_sheet(&self, sheet: Option<SheetId>) -> u32 {
        sheet.map_or_else(|| self.current_sheet.get(), |SheetId(i)| i)
    }

    /// The anchor for an eval-time refusal raised from *inside* a formula's evaluation (e.g. a
    /// range-too-large refusal in [`Resolver::range`]): the sheet-qualified formula file currently on
    /// the stack, or the current sheet's tab if none is active (defensive — `range` only runs mid-eval).
    fn eval_loc(&self) -> Loc {
        match self.current_file.get() {
            Some(id) => Loc::tab_file(&self.tab_name(id.0), &self.file_name(id)),
            None => Loc::tab(&self.tab_name(self.current_sheet.get())),
        }
    }
}

impl Resolver for Workbook {
    /// READ a cell's value — during the EVALUATE pass the value has already been computed (a formula
    /// node) or is read straight from the grid (a literal or a gap). The pass results win, then the
    /// memo, then the grid; a formula cell is never recomputed here (the plan guaranteed it computes
    /// first, in dependency order).
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
            // A gap (no file claims this cell) reads as Blank (the overlap policy: gaps are Blank).
            return Value::Blank;
        };
        let dr = cell.row - file.region.min_row;
        let dc = cell.col - file.region.min_col;
        match file.grid.cell_at(dr, dc) {
            GridCell::Value(v) => v.clone(),
            // A formula cell that is neither in the results nor the memo was not planned — unreachable
            // in a proper demand (the plan is a superset of what eval reads). The debug_assert fails
            // loud in tests if a future planning change under-approximates deps (fail-fast); release
            // stays total and never panics (CORE2).
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
        // Resolve `sheet: None` to the current context and key the arena by the qualified range, so a
        // memoized `A1:A3` on one sheet is never mistaken for `A1:A3` on another. Canonicalize the
        // key's corners via [`RangeRef::normalized`] so a reversed spelling (`B2:A1`) maps to the SAME
        // arena entry as `A1:B2` rather than materializing the identical rectangle twice under two keys.
        let eff = SheetId(self.resolve_sheet(range.start.sheet));
        let norm = range.normalized();
        let (c0, c1) = (norm.start.col, norm.end.col);
        let (r0, r1) = (norm.start.row, norm.end.row);
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

        let (rows, cols) = (r1 - r0 + 1, c1 - c0 + 1);
        let area = u64::from(rows) * u64::from(cols);
        if area > MAX_RANGE_CELLS {
            // A syntactically-valid but pathologically-large reference (`A2:ZZ100000`): refuse
            // (located) instead of materializing a `Value` per cell into an OOM abort. The range
            // resolves to a single #NUM! cell that the referencing aggregation propagates. The
            // refusal is deterministic (a function of the range size, not of order), so it caches.
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
        // Materialize the rectangle by READING each cell (its value was computed earlier in the pass,
        // as a planned dependency). If any cell is DEPTH-TAINTED, the buffer's `#NUM!` is a function of
        // the DEPTH the range was first demanded at, not of the range — caching it would poison a later
        // shallower (computable) demand — so return a borrowed view over a stable buffer WITHOUT
        // recording the key (mirrors the per-cell memo's depth guard). No arena borrow is held across
        // these `value` calls, which may recursively push more range buffers.
        let mut buf = Vec::with_capacity((rows as usize) * (cols as usize));
        let mut tainted = false;
        for r in r0..=r1 {
            for c in c0..=c1 {
                if self.pass_tainted.borrow().contains(&(eff.0, c, r)) {
                    tainted = true;
                }
                buf.push(self.value(CellRef {
                    col: c,
                    row: r,
                    sheet: Some(eff),
                }));
            }
        }
        let shape = Shape { rows, cols };
        if tainted {
            self.arena.insert_uncached(shape, buf)
        } else {
            self.arena.insert(key, shape, buf)
        }
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
}

/// An append-only arena that owns the cell buffers behind [`Resolver::range`]'s borrowed
/// [`ArrayView`]s, keyed by the (qualified) [`RangeRef`] so each distinct range materializes once.
///
/// The evaluator's `range()` must return `ArrayView<'a> = &'a [Value]` borrowing the resolver's
/// store, but the store is built lazily under `&self`. This arena resolves that: a materialized
/// buffer is boxed and **never moved or freed** while `&self` lives (entries are only appended, never
/// removed or mutated), so a reference into a boxed slice stays valid for the whole `&self` borrow.
#[derive(Default, Debug)]
pub(super) struct Arena {
    /// The owned buffers. `Box<[Value]>` heap data is address-stable across `Vec` growth.
    bufs: RefCell<Vec<Box<[Value]>>>,
    /// Range -> (shape, index into `bufs`).
    index: RefCell<HashMap<RangeRef, (Shape, usize)>>,
}

impl Arena {
    /// A borrowed view of an already-materialized range, or `None` if it has not been materialized.
    pub(super) fn get(&self, key: RangeRef) -> Option<ArrayView<'_>> {
        let (shape, i) = {
            let index = self.index.borrow();
            *index.get(&key)?
        };
        let ptr: *const [Value] = &*self.bufs.borrow()[i];
        // SAFETY: the arena is append-only — `bufs` entries are boxed slices that are never moved
        // (the box's heap data is independent of the `Vec`'s reallocations) and never freed or
        // mutated while `&self` lives. So the pointee outlives this `&self` borrow, and no `&mut`
        // to the same data is ever created. The returned view's lifetime is tied to `&self`.
        let cells: &[Value] = unsafe { &*ptr };
        Some(ArrayView { shape, cells })
    }

    /// Materialize a range: store its buffer (append-only) and return the borrowed view.
    pub(super) fn insert(&self, key: RangeRef, shape: Shape, cells: Vec<Value>) -> ArrayView<'_> {
        {
            let mut bufs = self.bufs.borrow_mut();
            let i = bufs.len();
            bufs.push(cells.into_boxed_slice());
            self.index.borrow_mut().insert(key, (shape, i));
        }
        self.get(key).expect("just inserted the key")
    }

    /// Own a range buffer for the lifetime of `&self` (so its [`ArrayView`] can be returned) but do
    /// **not** record it in the key index — a later demand for the same key misses and re-materializes.
    /// Used for a DEPTH-TAINTED buffer (a cell of the range consumed a depth refusal): its `#NUM!` is a
    /// function of the depth the range was first reached at, not the range, so committing it to the
    /// keyed cache would poison a later shallower (computable) demand. Keeps range evaluation
    /// order-independent, mirroring the per-cell memo's depth guard in [`Workbook::finish_pass`].
    fn insert_uncached(&self, shape: Shape, cells: Vec<Value>) -> ArrayView<'_> {
        let ptr: *const [Value] = {
            let mut bufs = self.bufs.borrow_mut();
            bufs.push(cells.into_boxed_slice());
            &*bufs[bufs.len() - 1]
        };
        // SAFETY: same append-only invariant as `get` — the boxed slice's heap data is never moved
        // (independent of `Vec` reallocation), freed, or mutated while `&self` lives, so the pointee
        // outlives this `&self` borrow and no `&mut` to it is ever created.
        let cells: &[Value] = unsafe { &*ptr };
        ArrayView { shape, cells }
    }
}
