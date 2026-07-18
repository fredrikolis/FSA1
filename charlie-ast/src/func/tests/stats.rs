// Concern: UNIT-TEST pins for the stats family built-ins (MIN MAX MEDIAN RANK COUNTA COUNTBLANK) exercised through `FUNCS` dispatch — the range-vs-direct-arg coercion asymmetry, empty-datum results (0 vs #NUM!), tie/order ranking, and the non-empty/blank counting split | Non-concern: the stats impls (`func/stats.rs`) and the shared test fixtures (the parent `tests` module owns `num`/`call`/`col_range`) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
use super::*;

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
