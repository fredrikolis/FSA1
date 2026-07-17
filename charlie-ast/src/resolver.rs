// Concern: the fs↔AST BOUNDARY — the `Resolver` trait that is the engine's ENTIRE view of the outside world (value/range/sheet_id for cells, plus `now_serial` — the ONE injectable wall-clock seam the VOLATILE TODAY/NOW read, with a system-time default and the `PINNED_NOW_SERIAL` const tests/conformance override it to), plus the single-homed epoch↔serial conversion re-exported for cross-crate reuse by charlie-model's `Workbook` — the `UNIX_EPOCH_SERIAL` const, `system_now_secs` (the raw clock read), and `unix_secs_to_serial` (the epoch→serial map), decoupling the evaluator from any concrete store/clock so the impl is swappable (in-memory, filesystem-backed, xlsx-backed, or a test stub) | Non-concern: any CONCRETE implementation of the trait — charlie-model owns the filesystem impl; this crate ships none | IO: (via impls) a `CellRef`/`RangeRef`/sheet name -> a resolved `Value`/`ArrayView`/`SheetId`, and (via the default `now_serial`) a read of the system clock
//! The fs↔AST boundary: the [`Resolver`] trait.

use crate::refs::{CellRef, RangeRef, SheetId};
use crate::value::{ArrayView, Value};

/// The instant tests and conformance PIN the [`Resolver::now_serial`] clock to: 2023-01-01T12:00:00,
/// i.e. Excel date-time serial `44927.5` (date serial `44927` = 2023-01-01, `+0.5` = noon). A single
/// source of truth so the engine's test grid and the conformance stub agree, keeping every
/// `TODAY()`/`NOW()` fixture deterministic. (`44927` = 2023-01-01 is the same anchor the `TEXT`
/// `yyyy-mm-dd` date example uses — see `func::text::serial_to_ymd`.)
pub const PINNED_NOW_SERIAL: f64 = 44927.5;

/// The Excel date-time serial of the Unix epoch (1970-01-01T00:00:00). The single source of truth
/// for the epoch mapping so every impl that must map seconds-since-epoch to a serial — the default
/// [`Resolver::now_serial`] here, and any concrete resolver that stores its own pinnable clock —
/// agrees on the constant.
pub const UNIX_EPOCH_SERIAL: f64 = 25569.0;

/// Map seconds since the Unix epoch to an Excel date-time serial (a day is `86_400` s). Single-homed
/// here so the `25569 + secs/86_400` mapping is written once; a resolver that stores its own clock
/// (rather than reading the system one) calls this instead of re-deriving the formula.
pub fn unix_secs_to_serial(secs: f64) -> f64 {
    UNIX_EPOCH_SERIAL + secs / 86_400.0
}

/// Read the system wall clock as seconds since the Unix epoch. A clock reported *before* the epoch
/// (a `SystemTime` error) falls back to the epoch instant rather than panicking. Single-homed here
/// so the raw clock read is written once: the [`Resolver::now_serial`] default reads it lazily, and
/// a resolver that must read the clock EAGERLY (charlie-model's `Workbook` stores `now` so it can be
/// pinned, and so cannot use the trait default) calls this instead of re-deriving the boilerplate.
pub fn system_now_secs() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

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

    /// The current instant the VOLATILE `TODAY`/`NOW` functions read, as an Excel date-time serial
    /// (integer part = the 1900-system date serial, fractional part = time of day; noon = `0.5`).
    ///
    /// This is the engine's ONE clock seam: `TODAY`/`NOW` read "now" only through here, never
    /// `std::time` inline, so a deterministic resolver (a test grid, the conformance stub) can PIN it
    /// — override to a fixed instant, e.g. [`PINNED_NOW_SERIAL`] — and keep every volatile-function
    /// fixture reproducible. The DEFAULT reads the real system clock, so a production resolver gets
    /// wall-clock time for free; a resolver that needs determinism OVERRIDES this one method.
    fn now_serial(&self) -> f64 {
        // The raw clock read is single-homed in [`system_now_secs`] and the epoch->serial mapping in
        // [`unix_secs_to_serial`]; this default just composes them.
        unix_secs_to_serial(system_now_secs())
    }
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

    #[test]
    fn unix_secs_to_serial_maps_the_epoch_and_a_day() {
        // The epoch itself is the epoch serial; one day later is exactly one serial unit later.
        assert_eq!(unix_secs_to_serial(0.0), UNIX_EPOCH_SERIAL);
        assert_eq!(unix_secs_to_serial(86_400.0), UNIX_EPOCH_SERIAL + 1.0);
    }
}
