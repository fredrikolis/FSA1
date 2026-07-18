// Concern: UNIT-TEST pins for the stats family built-ins (MIN MAX MEDIAN RANK COUNTA COUNTBLANK; the dispersion family STDEV/STDEVP/VAR/VARP + dotted aliases; the order statistics LARGE/SMALL/PERCENTILE/QUARTILE; MODE) exercised through `FUNCS` dispatch — the range-vs-direct-arg coercion asymmetry, empty-datum results (0 vs #NUM! vs #DIV/0!), tie/order ranking, dispersion divisors, inclusive percentiles, and the non-empty/blank counting split | Non-concern: the stats impls (`func/stats.rs`) and the shared test fixtures (the parent `tests` module owns `num`/`call`/`col_range`/`arr`/`n`) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
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
    // In a RANGE, text/blank/logical are ignored (only numbers) — so TRUE does NOT count as 1.
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
    // DIRECT booleans/numeric-text coerce (TRUE -> 1), the asymmetry's other half.
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
    // No numeric datum -> 0 (Excel), and an in-range error propagates.
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
    // Even count {1,2,3,4} -> (2+3)/2 = 2.5 (in-range text ignored).
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
    // Odd count -> the exact middle.
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
    // No numeric datum -> #NUM! (distinct from MIN/MAX's 0); an error propagates.
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
    // {10,8,8,5}: RANK(8) descending -> 2 (one value strictly greater); both 8s share rank 2.
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
    // Ascending (non-zero order): RANK(10, …, 1) -> 4 (three strictly less).
    assert_eq!(
        eval(&call("RANK", vec![num(10.0), col_range(4), num(1.0)]), &g),
        Value::Number(4.0)
    );
    // A number not present in ref is #N/A.
    assert_eq!(
        eval(&call("RANK", vec![num(7.0), col_range(4)]), &g),
        Value::Error(ErrKind::Na)
    );
    // A non-numeric `number` argument is #VALUE!.
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
    // A1..A5 = 1, "", "x", <blank>, #N/A. COUNTA counts non-empty: 1, "", "x", #N/A = 4
    // (the empty-string "" is non-empty; error counts; only the blank does not).
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
    // COUNTBLANK counts the empty: the "" AND the <blank> = 2 (error/number/text not blank).
    assert_eq!(
        eval(&call("COUNTBLANK", vec![col_range(5)]), &g),
        Value::Number(2.0)
    );
    // COUNTA of a direct blank does not count it; a direct value does.
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
    // STDEV (sample, /4) = sqrt(2.5); pinned to the exact expression at 1e-9 (not a loose 0.001
    // neighborhood) so the computed digits are locked. The dotted alias resolves to the same fn.
    assert_close(eval(&call("STDEV", vec![data()]), &g), 2.5_f64.sqrt(), 1e-9);
    assert_close(
        eval(&call("STDEV.S", vec![data()]), &g),
        2.5_f64.sqrt(),
        1e-9,
    );
    // VAR (sample) = 2.5; VARP (population, /5) = 2; STDEVP = sqrt(2) ≈ 1.4142.
    assert_close(eval(&call("VAR", vec![data()]), &g), 2.5, 1e-9);
    assert_close(eval(&call("VAR.S", vec![data()]), &g), 2.5, 1e-9);
    assert_close(eval(&call("VARP", vec![data()]), &g), 2.0, 1e-9);
    assert_close(eval(&call("VAR.P", vec![data()]), &g), 2.0, 1e-9);
    // STDEVP = sqrt(VARP) = sqrt(2) = SQRT_2 (the constant, so clippy::approx_constant is happy).
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
    // Sample dispersion of a single number is #DIV/0! (divisor n-1 = 0); population is 0.
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
    // No numeric datum → population divisor 0 → #DIV/0!.
    let empty = arr(1, 1, vec![t("x")]);
    assert_eq!(
        eval(&call("VARP", vec![empty]), &g),
        Value::Error(ErrKind::Div0)
    );
    // An in-range error PROPAGATES (leftmost) — dispersion never masks an upstream error.
    let with_err = arr(1, 3, vec![n(1.0), Value::Error(ErrKind::Div0), n(3.0)]);
    assert_eq!(
        eval(&call("STDEV", vec![with_err]), &g),
        Value::Error(ErrKind::Div0)
    );
    // The SUM direct-vs-in-range coercion asymmetry, both halves:
    //   * a DIRECT boolean COERCES (TRUE → 1): VAR(1, TRUE) sees {1, 1} → sample variance 0.
    assert_eq!(
        eval(
            &call("VAR", vec![num(1.0), Expr::Lit(Value::Bool(true))]),
            &g
        ),
        n(0.0)
    );
    //   * an IN-RANGE boolean is IGNORED: VAR({1; TRUE}) gathers only {1} → under-count → #DIV/0!
    //     (were TRUE counted as 1 it would be {1, 1} → 0, so this pins the asymmetry, not just a miss).
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
    // 2nd largest of {3,1,4,1,5} = 4; 2nd smallest = 1.
    assert_eq!(eval(&call("LARGE", vec![data(), num(2.0)]), &g), n(4.0));
    assert_eq!(eval(&call("SMALL", vec![data(), num(2.0)]), &g), n(1.0));
    // k below 1 or above the count is #NUM!.
    assert_eq!(
        eval(&call("LARGE", vec![data(), num(0.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("SMALL", vec![data(), num(6.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    // An empty array is #NUM!.
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
    // PERCENTILE.INC at 0.5 interpolates rank 1.5 → 2.5; endpoints are the min/max.
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
    // k outside [0,1] is #NUM!.
    assert_eq!(
        eval(&call("PERCENTILE", vec![data(), num(1.5)]), &g),
        Value::Error(ErrKind::Num)
    );
    // QUARTILE: quart 2 = median = 2.5; quart 0 = min; quart 4 = max; quart 5 is #NUM!.
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
    // quart is TRUNCATED toward zero: 2.9 → 2 → the median 2.5 (not #NUM! and not a 72.5%-ish value).
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
    // {1,2,2,3,3}: both 2 and 3 occur twice; the FIRST to reach the max count wins → 2.
    let tie = arr(1, 5, vec![n(1.0), n(2.0), n(2.0), n(3.0), n(3.0)]);
    assert_eq!(eval(&call("MODE", vec![tie]), &g), n(2.0));
    // MODE.SNGL is the same fn.
    let rep = arr(1, 4, vec![n(4.0), n(4.0), n(1.0), n(2.0)]);
    assert_eq!(eval(&call("MODE.SNGL", vec![rep]), &g), n(4.0));
    // No value repeats → #N/A.
    let uniq = arr(1, 3, vec![n(1.0), n(2.0), n(3.0)]);
    assert_eq!(
        eval(&call("MODE", vec![uniq]), &g),
        Value::Error(ErrKind::Na)
    );
    // VARIADIC: MODE(1,2,2,3) tallies across all direct args (not just the first) → 2.
    assert_eq!(
        eval(
            &call("MODE", vec![num(1.0), num(2.0), num(2.0), num(3.0)]),
            &g
        ),
        n(2.0)
    );
    // VARIADIC over multiple array args: the repeat 5 spans the first and second arrays → 5,
    // with in-range non-numbers ignored under the SUM asymmetry.
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
