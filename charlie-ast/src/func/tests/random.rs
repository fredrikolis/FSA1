// Concern: UNIT-TEST pins for the volatile random family (RAND RANDBETWEEN) exercised through `FUNCS` dispatch — since the values are non-deterministic, these assert the Excel PROPERTIES that must hold on every draw (RAND ∈ [0,1); RANDBETWEEN an integer within its inclusive band; an empty band → #NUM!; per-call distinctness) rather than a fixed value | Non-concern: the random impls (`func/random.rs`) and the entropy seam (resolver.rs owns `system_rand_unit`) — this checks the built-in's contract, not the RNG's statistical quality | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
use super::*;

/// Extract the number from an evaluated result (panicking on any non-number), for property checks.
fn number(v: Value) -> f64 {
    match v {
        Value::Number(n) => n,
        other => panic!("expected a number, got {other:?}"),
    }
}

#[test]
fn rand_is_in_the_unit_interval_and_varies() {
    let g = Grid::new(1, vec![Value::Blank]);
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..200 {
        let x = number(eval(&call("RAND", vec![]), &g));
        assert!((0.0..1.0).contains(&x), "RAND() = {x} outside [0, 1)");
        seen.insert(x.to_bits());
    }
    // Volatile: successive draws must not all collapse to one value.
    assert!(
        seen.len() > 1,
        "RAND() returned a constant across 200 draws"
    );
}

#[test]
fn randbetween_stays_within_its_inclusive_band() {
    let g = Grid::new(1, vec![Value::Blank]);
    for _ in 0..200 {
        let x = number(eval(&call("RANDBETWEEN", vec![num(1.0), num(6.0)]), &g));
        assert_eq!(x, x.trunc(), "RANDBETWEEN produced a non-integer {x}");
        assert!((1.0..=6.0).contains(&x), "RANDBETWEEN out of band: {x}");
    }
    // Non-integer bounds round inward (⌈1.2⌉=2 .. ⌊3.8⌋=3), so every draw is 2 or 3.
    for _ in 0..100 {
        let x = number(eval(&call("RANDBETWEEN", vec![num(1.2), num(3.8)]), &g));
        assert!(x == 2.0 || x == 3.0, "expected 2 or 3, got {x}");
    }
    // A degenerate single-value band returns that value.
    assert_eq!(
        eval(&call("RANDBETWEEN", vec![num(5.0), num(5.0)]), &g),
        Value::Number(5.0)
    );
    // An empty band (bottom > top after rounding) is #NUM!.
    assert_eq!(
        eval(&call("RANDBETWEEN", vec![num(3.0), num(1.0)]), &g),
        Value::Error(ErrKind::Num)
    );
}
