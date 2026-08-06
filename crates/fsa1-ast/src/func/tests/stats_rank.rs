// Concern: pins the ranking and percentile built-ins | Non-concern: the impls, the shared fixtures | IO: (Grid, Expr) -> asserted Value
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
fn rank_eq_and_avg_share_and_average_ties() {
    let g = Grid::new(1, vec![Value::Blank]);
    let data = || arr(1, 4, vec![n(10.0), n(8.0), n(8.0), n(5.0)]);
    assert_eq!(eval(&call("RANK.EQ", vec![num(8.0), data()]), &g), n(2.0));
    close(
        eval(&call("RANK.AVG", vec![num(8.0), data()]), &g),
        2.5,
        1e-12,
    );
    close(
        eval(&call("RANK.AVG", vec![num(5.0), data(), num(1.0)]), &g),
        1.0,
        1e-12,
    );
    assert_eq!(
        eval(&call("RANK.AVG", vec![num(7.0), data()]), &g),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn percentile_and_quartile_exclusive() {
    let g = Grid::new(1, vec![Value::Blank]);
    let data = || arr(1, 4, vec![n(1.0), n(2.0), n(3.0), n(4.0)]);
    close(
        eval(&call("PERCENTILE.EXC", vec![data(), num(0.5)]), &g),
        2.5,
        1e-12,
    );
    assert_eq!(
        eval(&call("PERCENTILE.EXC", vec![data(), num(0.1)]), &g),
        Value::Error(ErrKind::Num)
    );
    let d8 = || {
        arr(
            1,
            8,
            vec![
                n(1.0),
                n(2.0),
                n(3.0),
                n(4.0),
                n(5.0),
                n(6.0),
                n(7.0),
                n(8.0),
            ],
        )
    };
    close(
        eval(&call("QUARTILE.EXC", vec![d8(), num(2.0)]), &g),
        4.5,
        1e-12,
    );
    assert_eq!(
        eval(&call("QUARTILE.EXC", vec![d8(), num(0.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("QUARTILE.EXC", vec![d8(), num(4.0)]), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn percentrank_inclusive_exclusive_and_significance() {
    let g = Grid::new(1, vec![Value::Blank]);
    let data = || arr(1, 5, vec![n(10.0), n(20.0), n(30.0), n(40.0), n(50.0)]);
    close(
        eval(&call("PERCENTRANK", vec![data(), num(30.0)]), &g),
        0.5,
        1e-12,
    );
    close(
        eval(&call("PERCENTRANK.INC", vec![data(), num(30.0)]), &g),
        0.5,
        1e-12,
    );
    close(
        eval(&call("PERCENTRANK", vec![data(), num(25.0)]), &g),
        0.375,
        1e-12,
    );
    close(
        eval(&call("PERCENTRANK.EXC", vec![data(), num(30.0)]), &g),
        0.5,
        1e-12,
    );
    let d7 = || {
        arr(
            1,
            7,
            vec![n(1.0), n(2.0), n(3.0), n(4.0), n(5.0), n(6.0), n(7.0)],
        )
    };
    close(
        eval(&call("PERCENTRANK", vec![d7(), num(2.0)]), &g),
        0.166,
        1e-12,
    );
    close(
        eval(&call("PERCENTRANK", vec![d7(), num(2.0), num(5.0)]), &g),
        0.16666,
        1e-12,
    );
    assert_eq!(
        eval(&call("PERCENTRANK", vec![data(), num(99.0)]), &g),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn mode_mult_returns_all_modes_as_a_column() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(
            &call(
                "MODE.MULT",
                vec![arr(1, 5, vec![n(1.0), n(2.0), n(2.0), n(3.0), n(3.0)])]
            ),
            &g
        ),
        Value::Array(Shape { rows: 2, cols: 1 }, vec![n(2.0), n(3.0)])
    );
    assert_eq!(
        eval(
            &call("MODE.MULT", vec![arr(1, 3, vec![n(1.0), n(2.0), n(3.0)])]),
            &g
        ),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn frequency_bins_into_a_column() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(
            &call(
                "FREQUENCY",
                vec![
                    arr(1, 5, vec![n(1.0), n(2.0), n(3.0), n(4.0), n(5.0)]),
                    arr(1, 2, vec![n(2.0), n(4.0)])
                ]
            ),
            &g
        ),
        Value::Array(Shape { rows: 3, cols: 1 }, vec![n(2.0), n(2.0), n(1.0)])
    );
}
