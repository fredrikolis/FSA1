// Concern: pins GCD LCM FACT COMBIN | Non-concern: the impls, the shared fixtures | IO: (Grid, Expr) -> asserted Value
use super::*;

#[test]
fn gcd_and_lcm() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("GCD", vec![num(24.0), num(36.0)]), &g),
        Value::Number(12.0)
    );
    assert_eq!(
        eval(&call("GCD", vec![num(5.0), num(0.0)]), &g),
        Value::Number(5.0)
    );
    assert_eq!(
        eval(&call("GCD", vec![num(7.0), num(1.0)]), &g),
        Value::Number(1.0)
    );
    assert_eq!(
        eval(&call("GCD", vec![num(24.9), num(36.1)]), &g),
        Value::Number(12.0)
    );
    assert_eq!(
        eval(&call("GCD", vec![num(-5.0), num(10.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    let data = Grid::new(1, vec![Value::Number(12.0), Value::Number(18.0)]);
    assert_eq!(
        eval(&call("GCD", vec![col_range(2)]), &data),
        Value::Number(6.0)
    );

    assert_eq!(
        eval(&call("LCM", vec![num(4.0), num(6.0)]), &g),
        Value::Number(12.0)
    );
    assert_eq!(
        eval(&call("LCM", vec![num(3.0), num(4.0), num(5.0)]), &g),
        Value::Number(60.0)
    );
    assert_eq!(
        eval(&call("LCM", vec![num(0.0), num(5.0)]), &g),
        Value::Number(0.0)
    );
    assert_eq!(
        eval(&call("LCM", vec![num(-4.0), num(6.0)]), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn fact_and_combin() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("FACT", vec![num(5.0)]), &g),
        Value::Number(120.0)
    );
    assert_eq!(eval(&call("FACT", vec![num(0.0)]), &g), Value::Number(1.0));
    assert_eq!(eval(&call("FACT", vec![num(1.9)]), &g), Value::Number(1.0));
    assert_eq!(
        eval(&call("FACT", vec![num(-1.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("FACT", vec![num(171.0)]), &g),
        Value::Error(ErrKind::Num)
    );

    assert_eq!(
        eval(&call("COMBIN", vec![num(8.0), num(2.0)]), &g),
        Value::Number(28.0)
    );
    assert_eq!(
        eval(&call("COMBIN", vec![num(10.0), num(3.0)]), &g),
        Value::Number(120.0)
    );
    assert_eq!(
        eval(&call("COMBIN", vec![num(5.0), num(5.0)]), &g),
        Value::Number(1.0)
    );
    assert_eq!(
        eval(&call("COMBIN", vec![num(5.0), num(0.0)]), &g),
        Value::Number(1.0)
    );
    assert_eq!(
        eval(&call("COMBIN", vec![num(52.0), num(5.0)]), &g),
        Value::Number(2_598_960.0)
    );
    assert_eq!(
        eval(&call("COMBIN", vec![num(4.0), num(5.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("COMBIN", vec![num(-1.0), num(1.0)]), &g),
        Value::Error(ErrKind::Num)
    );
}

/// Assert an evaluated result is a number within a tight RELATIVE tolerance of `expected` — for the
/// large COMBIN results whose exact integer exceeds `f64`'s 2^53 range, so the low bits are lossy.
fn rel_approx(v: Value, expected: f64) {
    match v {
        Value::Number(n) => assert!(
            (n - expected).abs() <= expected.abs() * 1e-12,
            "got {n}, want {expected} (rel Δ={})",
            (n - expected).abs() / expected.abs()
        ),
        other => panic!("expected a number, got {other:?}"),
    }
}

#[test]
fn combin_large_matches_excel_not_num() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("COMBIN", vec![num(58.0), num(29.0)]), &g),
        Value::Number(3.006_726_649_954_104e16)
    );
    assert_eq!(
        eval(&call("COMBIN", vec![num(60.0), num(30.0)]), &g),
        Value::Number(1.182_645_815_648_614_2e17)
    );
    rel_approx(
        eval(&call("COMBIN", vec![num(200.0), num(100.0)]), &g),
        9.054_851_465_610_328e58,
    );
    rel_approx(
        eval(&call("COMBIN", vec![num(1029.0), num(514.0)]), &g),
        1.429_820_686_498_904e308,
    );
    assert_eq!(
        eval(&call("COMBIN", vec![num(1030.0), num(515.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("COMBIN", vec![num(50.0), num(25.0)]), &g),
        Value::Number(126_410_606_437_752.0)
    );
}
