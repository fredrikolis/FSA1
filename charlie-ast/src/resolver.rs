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

/// Draw one pseudo-random `f64` in the half-open unit interval `[0, 1)` — the entropy the VOLATILE
/// `RAND`/`RANDBETWEEN` built-ins read through the [`Resolver::rand_unit`] default. Single-homed here
/// (like [`system_now_secs`]) so the raw entropy read is written once and a resolver that overrides
/// the seam for determinism does not re-derive the mixing.
///
/// Distinct on every call (so two `RAND()`s in one formula differ, as Excel's do) WITHOUT an external
/// RNG crate: a process-wide monotone counter is folded with the wall clock through the SplitMix64
/// finalizer, then the top 53 bits — a full `f64` mantissa — are scaled to `[0, 1)`. Not
/// cryptographic; spreadsheet `RAND` does not require it.
pub fn system_rand_unit() -> f64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    // Fold the wall clock in so two processes started in lockstep still diverge; the counter alone
    // guarantees per-call distinctness within a process.
    let clock = (system_now_secs() * 1_000_000.0) as u64;
    let mut z = clock.wrapping_add(seq.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    // SplitMix64 finalizing mix — a strong avalanche from a weak (counter) input.
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    // Top 53 bits → a uniformly-distributed f64 in [0, 1) (the 2^53 mantissa granularity).
    (z >> 11) as f64 / ((1u64 << 53) as f64)
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

    /// Whether the cell at `cell` holds a **formula** (an `=…`) rather than a literal, a blank, or a
    /// gap — the ONE seam the `ISFORMULA` information predicate reads. It inspects the cell's CONTENT
    /// KIND, never its value, so it neither evaluates the cell nor propagates its error.
    ///
    /// The DEFAULT is `false`: a resolver backed by a bare value store (with no formula/literal
    /// distinction) reports every cell as a non-formula, so `ISFORMULA` is well-defined against any
    /// resolver without a boundary break. A resolver that knows a cell's source — charlie-model, which
    /// loads a grid of typed cells — OVERRIDES this to answer from the loaded `Cell` kind.
    fn is_formula(&self, _cell: CellRef) -> bool {
        false
    }

    /// One draw of entropy in `[0, 1)` — the mutable-seam-of-the-outside-world the VOLATILE
    /// `RAND`/`RANDBETWEEN` built-ins read (the randomness analogue of [`Self::now_serial`]'s clock).
    /// `RAND()` returns the draw directly; `RANDBETWEEN(bottom, top)` maps it onto its integer band.
    ///
    /// The DEFAULT reads process entropy via [`system_rand_unit`], so a production resolver gets a
    /// fresh value per call for free; a resolver that needs a REPRODUCIBLE stream (a test stub) can
    /// OVERRIDE this one method to return a fixed or seeded sequence. Recorded as a seam — not a
    /// `std::` call inline in the built-in — for exactly the same reason as the clock: determinism is
    /// injectable at the boundary, never baked into the engine.
    fn rand_unit(&self) -> f64 {
        system_rand_unit()
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
