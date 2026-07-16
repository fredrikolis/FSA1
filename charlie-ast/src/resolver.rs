// Concern: the fs↔AST BOUNDARY — the `Resolver` trait that is the engine's ENTIRE view of the outside world (value/range/sheet_id), decoupling the evaluator from any concrete store so the impl is swappable (in-memory, filesystem-backed, xlsx-backed, or a test stub) | Non-concern: any CONCRETE implementation of the trait — charlie-model owns the filesystem impl; this crate ships none | IO: (via impls) a `CellRef`/`RangeRef`/sheet name -> a resolved `Value`/`ArrayView`/`SheetId`
//! The fs↔AST boundary: the [`Resolver`] trait.

use crate::refs::{CellRef, RangeRef, SheetId};
use crate::value::{ArrayView, Value};

/// The engine's entire view of the outside world.
///
/// `charlie-ast` evaluates against a `Resolver` it is handed, never a concrete store. Because the
/// AST only ever calls these three methods, it has **no knowledge** that cells might be files on
/// disk — swap the impl and the engine is unchanged (`docs/architecture.md` §2, the swappability
/// contract). `charlie-model` implements it over the filesystem; a test stub implements it in memory.
///
/// Evaluation is **synchronous over a pre-loaded model**: every impl materializes its backing store
/// before eval begins, so the evaluator walks a fully in-memory model with no lazy per-cell I/O —
/// hence the trait is deliberately non-`async` (`docs/architecture.md` §2).
pub trait Resolver {
    /// Resolve a single cell to its value.
    fn value(&self, cell: CellRef) -> Value;

    /// Resolve a rectangular range to a **borrowed** [`ArrayView`] over the resolver's store.
    ///
    /// The view borrows the backing cells (`&[Value]`) rather than copying them, so the evaluator
    /// reads range cells in place (`docs/architecture.md` §2, "a rectangular block, borrowed"). The
    /// elided lifetime ties the view to `&self`: it cannot outlive the store it names.
    fn range(&self, range: RangeRef) -> ArrayView<'_>;

    /// Map a sheet name to its id, or `None` if there is no such sheet.
    fn sheet_id(&self, name: &str) -> Option<SheetId>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Shape;

    /// A trivial in-memory `Resolver` — proof that the boundary is implementable and swappable with
    /// no filesystem in sight, and that `range` hands back a **borrowed** view over a store the
    /// resolver owns (not a fresh copy per call).
    struct StubResolver {
        /// A tiny backing store the `range` view borrows from — the in-memory stand-in for the
        /// on-disk cells charlie-model's real impl would materialize before eval.
        store: Vec<Value>,
    }

    impl Resolver for StubResolver {
        fn value(&self, _cell: CellRef) -> Value {
            Value::Blank
        }

        fn range(&self, _range: RangeRef) -> ArrayView<'_> {
            ArrayView {
                shape: Shape {
                    rows: 1,
                    cols: self.store.len() as u32,
                },
                cells: &self.store,
            }
        }

        fn sheet_id(&self, name: &str) -> Option<SheetId> {
            (name == "Sheet1").then_some(SheetId(0))
        }
    }

    #[test]
    fn stub_resolver_satisfies_the_boundary() {
        let r = StubResolver {
            store: vec![Value::Number(1.0), Value::Blank],
        };
        assert_eq!(
            r.value(CellRef {
                col: 0,
                row: 0,
                sheet: None
            }),
            Value::Blank
        );
        assert_eq!(r.sheet_id("Sheet1"), Some(SheetId(0)));
        assert_eq!(r.sheet_id("Nope"), None);

        let view = r.range(RangeRef {
            start: CellRef {
                col: 0,
                row: 0,
                sheet: None,
            },
            end: CellRef {
                col: 1,
                row: 0,
                sheet: None,
            },
        });
        assert_eq!(view.shape, Shape { rows: 1, cols: 2 });
        assert_eq!(view.cells.len(), 2);
        // The view BORROWS the resolver's store rather than copying it: same backing slice.
        assert!(std::ptr::eq(view.cells, r.store.as_slice()));
    }
}
