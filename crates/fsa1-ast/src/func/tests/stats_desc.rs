// Concern: pins the descriptive-statistics built-ins | Non-concern: the impls, the shared fixtures | IO: (Grid, Expr) -> asserted Value
use super::*;

/// Assert a `Value::Number` is within `tol` of `expected`.
fn close(v: Value, expected: f64, tol: f64) {
    match v {
        Value::Number(got) => assert!(
            (got - expected).abs() <= tol,
            "expected ~{expected}, got {got}"
        ),
        other => panic!("expected a Number, got {other:?}"),
    }
}

#[test]
fn averagea_counts_in_range_text_as_zero_and_logicals() {
    let g = Grid::new(1, vec![Value::Blank]);
    close(
        eval(
            &call(
                "AVERAGEA",
                vec![arr(1, 4, vec![n(1.0), n(2.0), n(3.0), t("x")])],
            ),
            &g,
        ),
        1.5,
        1e-12,
    );
    close(
        eval(
            &call(
                "AVERAGEA",
                vec![arr(
                    1,
                    4,
                    vec![n(1.0), n(2.0), Value::Bool(false), Value::Bool(true)],
                )],
            ),
            &g,
        ),
        1.0,
        1e-12,
    );
    close(
        eval(
            &call(
                "AVERAGEA",
                vec![num(1.0), num(2.0), Expr::Lit(Value::Bool(true))],
            ),
            &g,
        ),
        4.0 / 3.0,
        1e-12,
    );
    assert_eq!(
        eval(&call("AVERAGEA", vec![arr(1, 1, vec![Value::Blank])]), &g),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn maxa_mina_count_text_and_false_as_zero() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(
            &call("MAXA", vec![arr(1, 3, vec![n(-1.0), n(-2.0), t("x")])]),
            &g
        ),
        n(0.0)
    );
    assert_eq!(
        eval(
            &call("MINA", vec![arr(1, 3, vec![n(1.0), n(2.0), t("x")])]),
            &g
        ),
        n(0.0)
    );
    assert_eq!(
        eval(
            &call("MAXA", vec![num(-1.0), Expr::Lit(Value::Bool(false))]),
            &g
        ),
        n(0.0)
    );
    assert_eq!(
        eval(&call("MAXA", vec![arr(1, 1, vec![Value::Blank])]), &g),
        n(0.0)
    );
}

#[test]
fn geomean_harmean_values_and_domain() {
    let g = Grid::new(1, vec![Value::Blank]);
    close(
        eval(&call("GEOMEAN", vec![num(1.0), num(2.0), num(4.0)]), &g),
        2.0,
        1e-12,
    );
    close(
        eval(&call("HARMEAN", vec![num(1.0), num(2.0), num(4.0)]), &g),
        12.0 / 7.0,
        1e-12,
    );
    assert_eq!(
        eval(&call("GEOMEAN", vec![num(1.0), num(-2.0), num(4.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("HARMEAN", vec![num(1.0), num(0.0), num(4.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(
            &call("GEOMEAN", vec![arr(1, 3, vec![n(0.0), n(1.0), n(2.0)])]),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn avedev_mean_absolute_deviation_and_empty() {
    let g = Grid::new(1, vec![Value::Blank]);
    close(
        eval(
            &call("AVEDEV", vec![num(1.0), num(2.0), num(3.0), num(4.0)]),
            &g,
        ),
        1.0,
        1e-12,
    );
    assert_eq!(
        eval(&call("AVEDEV", vec![arr(1, 1, vec![t("x")])]), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn skew_kurt_values_and_undersize_guards() {
    let g = Grid::new(1, vec![Value::Blank]);
    close(
        eval(
            &call("SKEW", vec![num(1.0), num(2.0), num(3.0), num(4.0)]),
            &g,
        ),
        0.0,
        1e-12,
    );
    close(
        eval(
            &call(
                "KURT",
                vec![num(1.0), num(2.0), num(3.0), num(4.0), num(5.0)],
            ),
            &g,
        ),
        -1.2,
        1e-12,
    );
    assert_eq!(
        eval(&call("SKEW", vec![num(1.0), num(2.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
    assert_eq!(
        eval(&call("KURT", vec![num(1.0), num(2.0), num(3.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
    assert_eq!(
        eval(&call("SKEW", vec![num(2.0), num(2.0), num(2.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
}
