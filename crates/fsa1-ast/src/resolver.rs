// Concern: declares the Resolver seam: cells, sheets, clock, entropy | Non-concern: any concrete store, evaluation, date arithmetic on a serial | IO: (CellRef) -> Value; () -> serial

use crate::refs::{CellRef, RangeRef, SheetId};
use crate::value::{ArrayView, Value};

/// The instant tests and conformance pin the clock to: 2023-01-01T12:00:00.
pub const PINNED_NOW_SERIAL: f64 = 44927.5;

/// The Excel date-time serial of 1970-01-01T00:00:00.
pub const UNIX_EPOCH_SERIAL: f64 = 25569.0;

pub fn unix_secs_to_serial(secs: f64) -> f64 {
    UNIX_EPOCH_SERIAL + secs / 86_400.0
}

pub fn system_now_secs() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// One pseudo-random `f64` in `[0, 1)`, distinct on every call: a process-wide counter folded with
/// the wall clock through the SplitMix64 finalizer, top 53 bits scaled. Not cryptographic.
pub fn system_rand_unit() -> f64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let clock = (system_now_secs() * 1_000_000.0) as u64;
    let mut z = clock.wrapping_add(seq.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / ((1u64 << 53) as f64)
}

pub trait Resolver {
    fn value(&self, cell: CellRef) -> Value;

    fn range(&self, range: RangeRef) -> ArrayView<'_>;

    fn sheet_id(&self, name: &str) -> Option<SheetId>;

    /// The engine's one clock seam, as an Excel date-time serial. The default reads the system
    /// clock; override it (e.g. to [`PINNED_NOW_SERIAL`]) for a deterministic resolver.
    fn now_serial(&self) -> f64 {
        unix_secs_to_serial(system_now_secs())
    }

    /// The seam `ISFORMULA` reads — the cell's content KIND, never its value. The default `false`
    /// keeps it well-defined against a bare value store; a resolver that knows the source overrides.
    fn is_formula(&self, _cell: CellRef) -> bool {
        false
    }

    /// The engine's one entropy seam, in `[0, 1)`. The default draws process entropy; override it
    /// for a reproducible stream.
    fn rand_unit(&self) -> f64 {
        system_rand_unit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Shape;

    struct StubResolver {
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
        assert!(
            std::ptr::eq(view.cells, r.store.as_slice()),
            "range borrows the store, never copies it"
        );
    }

    #[test]
    fn unix_secs_to_serial_maps_the_epoch_and_a_day() {
        assert_eq!(unix_secs_to_serial(0.0), UNIX_EPOCH_SERIAL);
        assert_eq!(unix_secs_to_serial(86_400.0), UNIX_EPOCH_SERIAL + 1.0);
    }
}
