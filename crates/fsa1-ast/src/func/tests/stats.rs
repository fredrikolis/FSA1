// Concern: pins the order-statistic and counting built-ins | Non-concern: the impls, the shared fixtures | IO: (Grid, Expr) -> asserted Value
use super::*;

/// Assert a `Value::Number` is within `tol` of `expected` (dispersion/percentile are computed floats).
fn assert_close(v: Value, expected: f64, tol: f64) {
    match v {
        Value::Number(got) => assert!(
            (got - expected).abs() <= tol,
            "expected ~{expected}, got {got}"
        ),
        other => panic!("expected a Number, got {other:?}"),
    }
}

#[test]
fn min_max_range_vs_direct_arg_asymmetry() {
    let g = Grid::new(
        1,
        vec![
            Value::Number(-5.0),
            Value::Bool(true),
            Value::Blank,
            Value::Number(-2.0),
        ],
    );
    assert_eq!(
        eval(&call("MIN", vec![col_range(4)]), &g),
        Value::Number(-5.0)
    );
    assert_eq!(
        eval(&call("MAX", vec![col_range(4)]), &g),
        Value::Number(-2.0)
    );
    let b = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(
            &call(
                "MAX",
                vec![num(-5.0), Expr::Lit(Value::Bool(true)), num(-2.0)]
            ),
            &b
        ),
        Value::Number(1.0)
    );
    assert_eq!(
        eval(
            &call("MIN", vec![num(3.0), Expr::Lit(Value::Text("2".into()))]),
            &b
        ),
        Value::Number(2.0)
    );
    let empty = Grid::new(1, vec![Value::Text("x".into()), Value::Blank]);
    assert_eq!(
        eval(&call("MIN", vec![col_range(2)]), &empty),
        Value::Number(0.0)
    );
    let with_err = Grid::new(1, vec![Value::Number(5.0), Value::Error(ErrKind::Div0)]);
    assert_eq!(
        eval(&call("MAX", vec![col_range(2)]), &with_err),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn median_even_averages_two_middles_and_empty_is_num() {
    let g = Grid::new(
        1,
        vec![
            Value::Number(1.0),
            Value::Number(2.0),
            Value::Text("x".into()),
            Value::Number(3.0),
            Value::Number(4.0),
        ],
    );
    assert_eq!(
        eval(&call("MEDIAN", vec![col_range(5)]), &g),
        Value::Number(2.5)
    );
    let odd = Grid::new(
        1,
        vec![
            Value::Number(5.0),
            Value::Number(3.0),
            Value::Number(1.0),
            Value::Number(4.0),
            Value::Number(2.0),
        ],
    );
    assert_eq!(
        eval(&call("MEDIAN", vec![col_range(5)]), &odd),
        Value::Number(3.0)
    );
    let empty = Grid::new(1, vec![Value::Text("x".into()), Value::Blank]);
    assert_eq!(
        eval(&call("MEDIAN", vec![col_range(2)]), &empty),
        Value::Error(ErrKind::Num)
    );
    let with_err = Grid::new(1, vec![Value::Number(1.0), Value::Error(ErrKind::Ref)]);
    assert_eq!(
        eval(&call("MEDIAN", vec![col_range(2)]), &with_err),
        Value::Error(ErrKind::Ref)
    );
}

#[test]
fn rank_descending_default_ties_share_lowest_and_missing_is_na() {
    let g = Grid::new(
        1,
        vec![
            Value::Number(10.0),
            Value::Number(8.0),
            Value::Number(8.0),
            Value::Number(5.0),
        ],
    );
    assert_eq!(
        eval(&call("RANK", vec![num(8.0), col_range(4)]), &g),
        Value::Number(2.0)
    );
    assert_eq!(
        eval(&call("RANK", vec![num(10.0), col_range(4), num(1.0)]), &g),
        Value::Number(4.0)
    );
    assert_eq!(
        eval(&call("RANK", vec![num(7.0), col_range(4)]), &g),
        Value::Error(ErrKind::Na)
    );
    assert_eq!(
        eval(
            &call(
                "RANK",
                vec![Expr::Lit(Value::Text("x".into())), col_range(4)]
            ),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn counta_and_countblank_over_a_range() {
    let g = Grid::new(
        1,
        vec![
            Value::Number(1.0),
            Value::Text(String::new()),
            Value::Text("x".into()),
            Value::Blank,
            Value::Error(ErrKind::Na),
        ],
    );
    assert_eq!(
        eval(&call("COUNTA", vec![col_range(5)]), &g),
        Value::Number(4.0)
    );
    assert_eq!(
        eval(&call("COUNTBLANK", vec![col_range(5)]), &g),
        Value::Number(2.0)
    );
    let b = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("COUNTA", vec![Expr::Lit(Value::Blank), num(1.0)]), &b),
        Value::Number(1.0)
    );
}

#[test]
fn dispersion_sample_vs_population_and_undercount() {
    let g = Grid::new(1, vec![Value::Blank]);
    let data = || arr(1, 5, vec![n(1.0), n(2.0), n(3.0), n(4.0), n(5.0)]);
    assert_close(eval(&call("STDEV", vec![data()]), &g), 2.5_f64.sqrt(), 1e-9);
    assert_close(
        eval(&call("STDEV.S", vec![data()]), &g),
        2.5_f64.sqrt(),
        1e-9,
    );
    assert_close(eval(&call("VAR", vec![data()]), &g), 2.5, 1e-9);
    assert_close(eval(&call("VAR.S", vec![data()]), &g), 2.5, 1e-9);
    assert_close(eval(&call("VARP", vec![data()]), &g), 2.0, 1e-9);
    assert_close(eval(&call("VAR.P", vec![data()]), &g), 2.0, 1e-9);
    assert_close(
        eval(&call("STDEVP", vec![data()]), &g),
        std::f64::consts::SQRT_2,
        1e-9,
    );
    assert_close(
        eval(&call("STDEV.P", vec![data()]), &g),
        std::f64::consts::SQRT_2,
        1e-9,
    );
    let one = || arr(1, 1, vec![n(7.0)]);
    assert_eq!(
        eval(&call("STDEV", vec![one()]), &g),
        Value::Error(ErrKind::Div0)
    );
    assert_eq!(
        eval(&call("VAR", vec![one()]), &g),
        Value::Error(ErrKind::Div0)
    );
    assert_eq!(eval(&call("VARP", vec![one()]), &g), Value::Number(0.0));
    let empty = arr(1, 1, vec![t("x")]);
    assert_eq!(
        eval(&call("VARP", vec![empty]), &g),
        Value::Error(ErrKind::Div0)
    );
    let with_err = arr(1, 3, vec![n(1.0), Value::Error(ErrKind::Div0), n(3.0)]);
    assert_eq!(
        eval(&call("STDEV", vec![with_err]), &g),
        Value::Error(ErrKind::Div0)
    );
    assert_eq!(
        eval(
            &call("VAR", vec![num(1.0), Expr::Lit(Value::Bool(true))]),
            &g
        ),
        n(0.0)
    );
    let in_range_bool = arr(2, 1, vec![n(1.0), Value::Bool(true)]);
    assert_eq!(
        eval(&call("VAR", vec![in_range_bool]), &g),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn large_small_and_order_bounds() {
    let g = Grid::new(1, vec![Value::Blank]);
    let data = || arr(1, 5, vec![n(3.0), n(1.0), n(4.0), n(1.0), n(5.0)]);
    assert_eq!(eval(&call("LARGE", vec![data(), num(2.0)]), &g), n(4.0));
    assert_eq!(eval(&call("SMALL", vec![data(), num(2.0)]), &g), n(1.0));
    assert_eq!(
        eval(&call("LARGE", vec![data(), num(0.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("SMALL", vec![data(), num(6.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    let empty = arr(1, 1, vec![t("x")]);
    assert_eq!(
        eval(&call("LARGE", vec![empty, num(1.0)]), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn percentile_and_quartile_inclusive() {
    let g = Grid::new(1, vec![Value::Blank]);
    let data = || arr(1, 4, vec![n(1.0), n(2.0), n(3.0), n(4.0)]);
    assert_close(
        eval(&call("PERCENTILE", vec![data(), num(0.5)]), &g),
        2.5,
        1e-9,
    );
    assert_close(
        eval(&call("PERCENTILE.INC", vec![data(), num(0.0)]), &g),
        1.0,
        1e-9,
    );
    assert_close(
        eval(&call("PERCENTILE", vec![data(), num(1.0)]), &g),
        4.0,
        1e-9,
    );
    assert_eq!(
        eval(&call("PERCENTILE", vec![data(), num(1.5)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_close(
        eval(&call("QUARTILE", vec![data(), num(2.0)]), &g),
        2.5,
        1e-9,
    );
    assert_close(
        eval(&call("QUARTILE.INC", vec![data(), num(0.0)]), &g),
        1.0,
        1e-9,
    );
    assert_close(
        eval(&call("QUARTILE", vec![data(), num(2.9)]), &g),
        2.5,
        1e-9,
    );
    assert_eq!(
        eval(&call("QUARTILE", vec![data(), num(5.0)]), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn mode_first_of_ties_and_no_repeat_is_na() {
    let g = Grid::new(1, vec![Value::Blank]);
    let tie = arr(1, 5, vec![n(1.0), n(2.0), n(2.0), n(3.0), n(3.0)]);
    assert_eq!(eval(&call("MODE", vec![tie]), &g), n(2.0));
    let rep = arr(1, 4, vec![n(4.0), n(4.0), n(1.0), n(2.0)]);
    assert_eq!(eval(&call("MODE.SNGL", vec![rep]), &g), n(4.0));
    let uniq = arr(1, 3, vec![n(1.0), n(2.0), n(3.0)]);
    assert_eq!(
        eval(&call("MODE", vec![uniq]), &g),
        Value::Error(ErrKind::Na)
    );
    assert_eq!(
        eval(
            &call("MODE", vec![num(1.0), num(2.0), num(2.0), num(3.0)]),
            &g
        ),
        n(2.0)
    );
    assert_eq!(
        eval(
            &call(
                "MODE",
                vec![
                    arr(1, 3, vec![n(5.0), n(1.0), t("x")]),
                    arr(1, 2, vec![n(2.0), n(5.0)]),
                ]
            ),
            &g
        ),
        n(5.0)
    );
}
