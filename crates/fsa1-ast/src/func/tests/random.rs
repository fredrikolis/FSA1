// Concern: pins the properties every RAND/RANDBETWEEN draw holds | Non-concern: the impls, the entropy seam, RNG quality | IO: (Grid, Expr) -> asserted Value
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
    for _ in 0..100 {
        let x = number(eval(&call("RANDBETWEEN", vec![num(1.2), num(3.8)]), &g));
        assert!(x == 2.0 || x == 3.0, "expected 2 or 3, got {x}");
    }
    assert_eq!(
        eval(&call("RANDBETWEEN", vec![num(5.0), num(5.0)]), &g),
        Value::Number(5.0)
    );
    assert_eq!(
        eval(&call("RANDBETWEEN", vec![num(3.0), num(1.0)]), &g),
        Value::Error(ErrKind::Num)
    );
}
