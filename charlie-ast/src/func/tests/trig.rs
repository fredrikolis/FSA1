// Concern: UNIT-TEST pins for the transcendental family (PI EXP LN LOG LOG10 SIN COS TAN ASIN ACOS ATAN ATAN2 SINH COSH TANH RADIANS DEGREES) exercised through `FUNCS` dispatch — the hand-verified Excel values (compared within a tight tolerance since the results are irrational f64s) and the domain-error mappings (non-positive LN/LOG/LOG10 → #NUM!, LOG base 1 → #DIV/0!, out-of-[-1,1] ASIN/ACOS → #NUM!, ATAN2 origin → #DIV/0!, overflowing COSH → #NUM!) | Non-concern: the trig impls (`func/trig.rs`) and the shared test fixtures (the parent `tests` module owns `num`/`call`) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
use super::*;
use std::f64::consts::PI;

/// Assert an evaluated result is a number within a tight tolerance of `expected` (the transcendental
/// results are irrational, so `Value`'s bit-exact `Eq` is the wrong comparison here).
fn approx(v: Value, expected: f64) {
    match v {
        Value::Number(n) => assert!(
            (n - expected).abs() < 1e-12,
            "got {n}, want {expected} (Δ={})",
            (n - expected).abs()
        ),
        other => panic!("expected a number, got {other:?}"),
    }
}

#[test]
fn pi_exp_log_family() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(eval(&call("PI", vec![]), &g), Value::Number(PI));
    approx(eval(&call("EXP", vec![num(0.0)]), &g), 1.0);
    approx(eval(&call("EXP", vec![num(1.0)]), &g), std::f64::consts::E);
    approx(eval(&call("LN", vec![num(1.0)]), &g), 0.0);
    approx(eval(&call("LN", vec![num(std::f64::consts::E)]), &g), 1.0);
    // Non-positive LN is #NUM!.
    assert_eq!(
        eval(&call("LN", vec![num(0.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("LN", vec![num(-1.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    // LOG10 and LOG (default base 10, explicit base 2).
    approx(eval(&call("LOG10", vec![num(1000.0)]), &g), 3.0);
    approx(eval(&call("LOG", vec![num(100.0)]), &g), 2.0);
    approx(eval(&call("LOG", vec![num(8.0), num(2.0)]), &g), 3.0);
    // LOG domain: non-positive number/base → #NUM!, base 1 → #DIV/0!.
    assert_eq!(
        eval(&call("LOG", vec![num(-1.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("LOG", vec![num(10.0), num(-2.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("LOG", vec![num(10.0), num(1.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn trig_and_inverse_and_hyperbolic() {
    let g = Grid::new(1, vec![Value::Blank]);
    approx(eval(&call("SIN", vec![num(0.0)]), &g), 0.0);
    approx(eval(&call("COS", vec![num(0.0)]), &g), 1.0);
    approx(eval(&call("TAN", vec![num(0.0)]), &g), 0.0);
    approx(eval(&call("SIN", vec![num(PI / 2.0)]), &g), 1.0);
    // Inverse trig (radians) + domain errors.
    approx(eval(&call("ASIN", vec![num(1.0)]), &g), PI / 2.0);
    approx(eval(&call("ACOS", vec![num(1.0)]), &g), 0.0);
    approx(eval(&call("ATAN", vec![num(1.0)]), &g), PI / 4.0);
    assert_eq!(
        eval(&call("ASIN", vec![num(2.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("ACOS", vec![num(-2.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    // ATAN2 uses the Excel (x, y) argument order; the origin is #DIV/0!.
    approx(eval(&call("ATAN2", vec![num(1.0), num(1.0)]), &g), PI / 4.0);
    approx(
        eval(&call("ATAN2", vec![num(-1.0), num(-1.0)]), &g),
        -3.0 * PI / 4.0,
    );
    assert_eq!(
        eval(&call("ATAN2", vec![num(0.0), num(0.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
    // Hyperbolic + overflow guard.
    approx(eval(&call("SINH", vec![num(0.0)]), &g), 0.0);
    approx(eval(&call("COSH", vec![num(0.0)]), &g), 1.0);
    approx(eval(&call("TANH", vec![num(0.0)]), &g), 0.0);
    assert_eq!(
        eval(&call("COSH", vec![num(1000.0)]), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn radians_degrees_round_trip() {
    let g = Grid::new(1, vec![Value::Blank]);
    approx(eval(&call("RADIANS", vec![num(180.0)]), &g), PI);
    approx(eval(&call("DEGREES", vec![num(PI)]), &g), 180.0);
    approx(eval(&call("RADIANS", vec![num(90.0)]), &g), PI / 2.0);
}
