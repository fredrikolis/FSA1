// Concern: UNIT-TEST pins for the bivariate regression/association functions (CORREL RSQ COVARIANCE.P/S SLOPE INTERCEPT FORECAST[.LINEAR] TREND) exercised through `FUNCS` dispatch — the arg order (`ys` before `xs`, `x` first for FORECAST), the pairwise same-length `#N/A` rule, the zero-spread `#DIV/0!`, and TREND's array prediction | Non-concern: the impls (func/stats_reg.rs) and the shared test fixtures (`num`/`call`/`arr`/`n`) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
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

// A 1x4 helper for the paired samples used across these pins.
fn xs() -> Expr {
    arr(1, 4, vec![n(1.0), n(2.0), n(3.0), n(4.0)])
}
fn ys() -> Expr {
    arr(1, 4, vec![n(2.0), n(4.0), n(5.0), n(8.0)])
}

#[test]
fn correlation_and_covariances() {
    let g = Grid::new(1, vec![Value::Blank]);
    // Hand-verified against Excel/`formulas`.
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
    // Mismatched lengths → #N/A.
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
    // SLOPE/INTERCEPT of ys on xs: 1.9 and 0.
    close(eval(&call("SLOPE", vec![ys(), xs()]), &g), 1.9, 1e-12);
    close(eval(&call("INTERCEPT", vec![ys(), xs()]), &g), 0.0, 1e-12);
    // FORECAST at x=5 → 0 + 1.9*5 = 9.5 (both spellings).
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
    // A zero x-spread is #DIV/0!.
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
    // Predict at new_x = 5 → 9.5, returned shaped like new_xs (1x1).
    assert_eq!(
        eval(
            &call("TREND", vec![ys(), xs(), arr(1, 1, vec![n(5.0)])]),
            &g
        ),
        Value::Array(Shape { rows: 1, cols: 1 }, vec![n(9.5)])
    );
    // With new_xs omitted, TREND fits the known xs (default counter here since known_xs omitted too):
    // ys = {2,4,5,8} on x = {1,2,3,4} → fitted values, shaped like known_ys (1x4).
    match eval(&call("TREND", vec![ys(), xs()]), &g) {
        Value::Array(shape, cells) => {
            assert_eq!(shape, Shape { rows: 1, cols: 4 });
            // Fitted line 1.9x + 0 at x=1..4.
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
