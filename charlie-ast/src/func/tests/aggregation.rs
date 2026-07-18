// Concern: UNIT-TEST pins for the aggregation family built-ins (SUM AVERAGE COUNT) exercised through `FUNCS` dispatch — the range-vs-direct coercion asymmetry, the empty/blank handling, and error propagation vs COUNT's error-ignoring | Non-concern: the aggregation impls (`func/aggregation.rs`) and the shared test fixtures (the parent `tests` module owns `num`/`call`/`col_range`) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
use super::*;

#[test]
fn sum_average_count_over_a_range_with_mixed_cells() {
    // A1..A5 = 1, "x"(text), TRUE(bool), <blank>, 4  -> numbers are {1, 4}
    let g = Grid::new(
        1,
        vec![
            Value::Number(1.0),
            Value::Text("x".into()),
            Value::Bool(true),
            Value::Blank,
            Value::Number(4.0),
        ],
    );
    assert_eq!(
        eval(&call("SUM", vec![col_range(5)]), &g),
        Value::Number(5.0)
    );
    assert_eq!(
        eval(&call("AVERAGE", vec![col_range(5)]), &g),
        Value::Number(2.5)
    );
    // COUNT counts only the two numbers (in-range bool/text ignored).
    assert_eq!(
        eval(&call("COUNT", vec![col_range(5)]), &g),
        Value::Number(2.0)
    );
}

#[test]
fn direct_vs_in_range_coercion_asymmetry() {
    let g = Grid::new(1, vec![Value::Blank]);
    // Direct booleans/numeric-text coerce and count.
    assert_eq!(
        eval(
            &call(
                "SUM",
                vec![
                    num(1.0),
                    Expr::Lit(Value::Bool(true)),
                    Expr::Lit(Value::Text("2".into()))
                ]
            ),
            &g
        ),
        Value::Number(4.0)
    );
    assert_eq!(
        eval(
            &call(
                "COUNT",
                vec![
                    Expr::Lit(Value::Bool(true)),
                    Expr::Lit(Value::Text("3".into()))
                ]
            ),
            &g
        ),
        Value::Number(2.0)
    );
    // A direct non-numeric text is #VALUE! for SUM.
    assert_eq!(
        eval(&call("SUM", vec![Expr::Lit(Value::Text("x".into()))]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn sum_propagates_but_count_ignores_errors() {
    let g = Grid::new(
        1,
        vec![
            Value::Number(1.0),
            Value::Error(ErrKind::Div0),
            Value::Number(2.0),
        ],
    );
    assert_eq!(
        eval(&call("SUM", vec![col_range(3)]), &g),
        Value::Error(ErrKind::Div0)
    );
    // COUNT never returns an error from its data.
    assert_eq!(
        eval(&call("COUNT", vec![col_range(3)]), &g),
        Value::Number(2.0)
    );
}
