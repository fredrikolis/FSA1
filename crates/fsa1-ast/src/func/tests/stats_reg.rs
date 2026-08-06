// Concern: pins the bivariate regression built-ins | Non-concern: the impls, the shared fixtures | IO: (Grid, Expr) -> asserted Value
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

fn xs() -> Expr {
    arr(1, 4, vec![n(1.0), n(2.0), n(3.0), n(4.0)])
}
fn ys() -> Expr {
    arr(1, 4, vec![n(2.0), n(4.0), n(5.0), n(8.0)])
}

#[test]
fn correlation_and_covariances() {
    let g = Grid::new(1, vec![Value::Blank]);
    close(
        eval(&call("CORREL", vec![xs(), ys()]), &g),
        0.981_155_781_039_212_2,
        1e-12,
    );
    close(
        eval(&call("RSQ", vec![ys(), xs()]), &g),
        0.962_666_666_666_666_7,
        1e-12,
    );
    close(
        eval(&call("COVARIANCE.P", vec![xs(), ys()]), &g),
        2.375,
        1e-12,
    );
    close(
        eval(&call("COVARIANCE.S", vec![xs(), ys()]), &g),
        3.166_666_666_666_666_5,
        1e-12,
    );
    assert_eq!(
        eval(
            &call(
                "CORREL",
                vec![arr(1, 3, vec![n(1.0), n(2.0), n(3.0)]), xs()]
            ),
            &g
        ),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn slope_intercept_forecast() {
    let g = Grid::new(1, vec![Value::Blank]);
    close(eval(&call("SLOPE", vec![ys(), xs()]), &g), 1.9, 1e-12);
    close(eval(&call("INTERCEPT", vec![ys(), xs()]), &g), 0.0, 1e-12);
    close(
        eval(&call("FORECAST", vec![num(5.0), ys(), xs()]), &g),
        9.5,
        1e-12,
    );
    close(
        eval(&call("FORECAST.LINEAR", vec![num(5.0), ys(), xs()]), &g),
        9.5,
        1e-12,
    );
    assert_eq!(
        eval(
            &call(
                "SLOPE",
                vec![
                    arr(1, 3, vec![n(1.0), n(2.0), n(3.0)]),
                    arr(1, 3, vec![n(4.0), n(4.0), n(4.0)])
                ]
            ),
            &g
        ),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn trend_predicts_new_xs_as_an_array() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(
            &call("TREND", vec![ys(), xs(), arr(1, 1, vec![n(5.0)])]),
            &g
        ),
        Value::Array(Shape { rows: 1, cols: 1 }, vec![n(9.5)])
    );
    match eval(&call("TREND", vec![ys(), xs()]), &g) {
        Value::Array(shape, cells) => {
            assert_eq!(shape, Shape { rows: 1, cols: 4 });
            let want = [1.9, 3.8, 5.7, 7.6];
            for (c, w) in cells.iter().zip(want) {
                match c {
                    Value::Number(v) => assert!((v - w).abs() < 1e-9, "got {v}, want {w}"),
                    other => panic!("expected Number, got {other:?}"),
                }
            }
        }
        other => panic!("expected an Array, got {other:?}"),
    }
}
