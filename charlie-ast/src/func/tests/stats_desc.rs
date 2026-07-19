// Concern: UNIT-TEST pins for the descriptive-statistic extensions (AVERAGEA MAXA MINA GEOMEAN HARMEAN AVEDEV SKEW KURT) exercised through `FUNCS` dispatch — the "A"-suffix in-range text/logical coercion (text as 0), the alternative-mean domain guards, the mean-absolute-deviation, and the sample shape moments with their `n`/zero-spread `#DIV/0!` guards | Non-concern: the impls (func/stats_desc.rs) and the shared test fixtures (the parent `tests` module owns `num`/`call`/`arr`/`n`/`t`) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
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
    // In-range text counts as 0: (1+2+3+0)/4 = 1.5.
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
    // In-range logicals count (TRUE→1, FALSE→0): (1+2+0+1)/4 = 1.
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
    // A direct boolean coerces: (1+2+1)/3 = 1.333…
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
    // No counted datum (in-range blank only) → #DIV/0!.
    assert_eq!(
        eval(&call("AVERAGEA", vec![arr(1, 1, vec![Value::Blank])]), &g),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn maxa_mina_count_text_and_false_as_zero() {
    let g = Grid::new(1, vec![Value::Blank]);
    // MAXA: text/FALSE count as 0, which beats all-negative data.
    assert_eq!(
        eval(
            &call("MAXA", vec![arr(1, 3, vec![n(-1.0), n(-2.0), t("x")])]),
            &g
        ),
        n(0.0)
    );
    // MINA: text counts as 0, which is below all-positive data.
    assert_eq!(
        eval(
            &call("MINA", vec![arr(1, 3, vec![n(1.0), n(2.0), t("x")])]),
            &g
        ),
        n(0.0)
    );
    // Direct FALSE coerces to 0: MAXA(-1, FALSE) = 0.
    assert_eq!(
        eval(
            &call("MAXA", vec![num(-1.0), Expr::Lit(Value::Bool(false))]),
            &g
        ),
        n(0.0)
    );
    // No datum → 0 (matches MAX/MIN empty).
    assert_eq!(
        eval(&call("MAXA", vec![arr(1, 1, vec![Value::Blank])]), &g),
        n(0.0)
    );
}

#[test]
fn geomean_harmean_values_and_domain() {
    let g = Grid::new(1, vec![Value::Blank]);
    // GEOMEAN(1,2,4) = (8)^(1/3) = 2.
    close(
        eval(&call("GEOMEAN", vec![num(1.0), num(2.0), num(4.0)]), &g),
        2.0,
        1e-12,
    );
    // HARMEAN(1,2,4) = 3/(1+0.5+0.25) = 1.714285…
    close(
        eval(&call("HARMEAN", vec![num(1.0), num(2.0), num(4.0)]), &g),
        12.0 / 7.0,
        1e-12,
    );
    // A non-positive value is #NUM! for both.
    assert_eq!(
        eval(&call("GEOMEAN", vec![num(1.0), num(-2.0), num(4.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("HARMEAN", vec![num(1.0), num(0.0), num(4.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    // GEOMEAN of a single zero is #NUM! (0 not allowed).
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
    // AVEDEV(1,2,3,4): mean 2.5, |devs| = 1.5,0.5,0.5,1.5 → 4/4 = 1.
    close(
        eval(
            &call("AVEDEV", vec![num(1.0), num(2.0), num(3.0), num(4.0)]),
            &g,
        ),
        1.0,
        1e-12,
    );
    // No numeric datum (in-range text ignored under the SUM rule) → #NUM!.
    assert_eq!(
        eval(&call("AVEDEV", vec![arr(1, 1, vec![t("x")])]), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn skew_kurt_values_and_undersize_guards() {
    let g = Grid::new(1, vec![Value::Blank]);
    // A symmetric sample has zero skew.
    close(
        eval(
            &call("SKEW", vec![num(1.0), num(2.0), num(3.0), num(4.0)]),
            &g,
        ),
        0.0,
        1e-12,
    );
    // KURT(1,2,3,4,5) = -1.2 (hand-verified against Excel).
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
    // SKEW needs ≥3 numbers; KURT needs ≥4 — otherwise #DIV/0!.
    assert_eq!(
        eval(&call("SKEW", vec![num(1.0), num(2.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
    assert_eq!(
        eval(&call("KURT", vec![num(1.0), num(2.0), num(3.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
    // A zero-spread sample is #DIV/0! (divides by the sample stdev).
    assert_eq!(
        eval(&call("SKEW", vec![num(2.0), num(2.0), num(2.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
}
