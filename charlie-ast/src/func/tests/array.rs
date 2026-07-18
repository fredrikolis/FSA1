// Concern: UNIT-TEST pins for array-aware evaluation — `{…}` array LITERALS parsing to an `array` Value (row-major shape, signed/mixed constants, ragged/non-constant refusals), and IMPLICIT ARRAY EVALUATION of a function over an array in its scalar position (`COUNTIF(range, A1:A6)` -> an array; the distinct-count idiom; a scalar criterion still scalar), plus the preserved reducer/operator array behavior | Non-concern: the array-literal parser (parser.rs) and the broadcaster impl (func/array.rs) — this pins their observable behavior through parse+eval | IO: in-memory `Grid` fixtures + formula `&str` -> asserted `Value`s
use super::*;
use crate::parse;
use crate::value::Shape;

/// Parse then evaluate a formula against a grid.
fn run(formula: &str, g: &Grid) -> Value {
    eval(&parse(formula).expect("should parse"), g)
}

/// A1:A6 = EMEA APAC EMEA AMER APAC EMEA (three EMEA, two APAC, one AMER).
fn regions() -> Grid {
    Grid::new(
        1,
        vec![
            t("EMEA"),
            t("APAC"),
            t("EMEA"),
            t("AMER"),
            t("APAC"),
            t("EMEA"),
        ],
    )
}

#[test]
fn array_literals_reduce_through_sum() {
    let g = Grid::new(1, vec![Value::Blank]);
    // A 1-row literal and a 2x2 literal both reduce through SUM.
    assert_eq!(run("=SUM({1,2,3})", &g), Value::Number(6.0));
    assert_eq!(run("=SUM({1,2;3,4})", &g), Value::Number(10.0));
}

#[test]
fn array_literal_folds_to_a_row_major_array_value() {
    // `;` separates rows, `,` separates columns: {1,2;3,4} is a 2x2 row-major array constant.
    assert_eq!(
        parse("={1,2;3,4}").unwrap(),
        Expr::Lit(Value::Array(
            Shape { rows: 2, cols: 2 },
            vec![n(1.0), n(2.0), n(3.0), n(4.0)]
        ))
    );
    // Signed numbers and mixed constant kinds (text/logical/error) are all legal elements.
    assert_eq!(
        parse("={-1,\"x\";TRUE,#DIV/0!}").unwrap(),
        Expr::Lit(Value::Array(
            Shape { rows: 2, cols: 2 },
            vec![
                n(-1.0),
                t("x"),
                Value::Bool(true),
                Value::Error(ErrKind::Div0)
            ]
        ))
    );
}

#[test]
fn malformed_array_literals_are_located_refusals() {
    // A ragged literal (rows of unequal width) is a located refusal.
    assert_eq!(
        parse("={1,2;3}").unwrap_err().code,
        DiagCode::MalformedArray
    );
    // A non-constant element (a reference, a call) is refused — Excel array constants hold constants.
    assert_eq!(parse("={A1,2}").unwrap_err().code, DiagCode::MalformedArray);
    assert_eq!(
        parse("={SUM(1),2}").unwrap_err().code,
        DiagCode::MalformedArray
    );
    // Empty / dangling separators are malformed.
    assert_eq!(parse("={}").unwrap_err().code, DiagCode::MalformedArray);
    assert_eq!(parse("={1,}").unwrap_err().code, DiagCode::MalformedArray);
    // An unterminated literal is an end-of-input refusal (input ended mid-construct).
    assert_eq!(parse("={1,2").unwrap_err().code, DiagCode::UnexpectedEof);
}

#[test]
fn countif_maps_over_an_array_criterion() {
    // COUNTIF(A1:A6, A1:A6): the criterion (arg 1) is an array, so the call maps element-wise —
    // each cell's count of its own value across the range: {3;2;3;1;2;3}.
    let g = regions();
    assert_eq!(
        run("=COUNTIF(A1:A6,A1:A6)", &g),
        Value::Array(
            Shape { rows: 6, cols: 1 },
            vec![n(3.0), n(2.0), n(3.0), n(1.0), n(2.0), n(3.0)]
        )
    );
    // A reducer collapses that mapped array: its SUM is 14 (3+2+3+1+2+3).
    assert_eq!(run("=SUM(COUNTIF(A1:A6,A1:A6))", &g), Value::Number(14.0));
}

#[test]
fn distinct_count_idiom() {
    // The classic distinct-count: SUMPRODUCT(1/COUNTIF(range, range)) = number of DISTINCT values.
    // 1/{3;2;3;1;2;3} = {1/3;1/2;1/3;1;1/2;1/3}, summing to 3 (EMEA, APAC, AMER).
    let g = regions();
    assert_eq!(
        run("=SUMPRODUCT(1/COUNTIF(A1:A6,A1:A6))", &g),
        Value::Number(3.0)
    );
}

#[test]
fn scalar_criterion_still_returns_a_scalar() {
    // A scalar in the criterion position keeps the pre-existing scalar behavior (no broadcast).
    let g = regions();
    assert_eq!(run("=COUNTIF(A1:A6,\"EMEA\")", &g), Value::Number(3.0));
    assert_eq!(run("=COUNTIF(A1:A6,\"APAC\")", &g), Value::Number(2.0));
}

#[test]
fn sumif_broadcasts_an_array_criterion_too() {
    // The broadcaster is general across the single-criterion family: A1:A3 = 1,2,3 as both range
    // and criterion; SUMIF(A1:A3, A1:A3) sums each cell equal to itself -> {1;2;3}, summing to 6.
    let g = Grid::new(1, vec![n(1.0), n(2.0), n(3.0)]);
    assert_eq!(run("=SUM(SUMIF(A1:A3,A1:A3))", &g), Value::Number(6.0));
}

#[test]
fn scalar_error_in_a_range_position_short_circuits_instead_of_tiling() {
    // A scalar error in a NON-broadcast (range) position of a call whose criterion is an array
    // returns Excel's SCALAR error, not a shape-tiled array of it: COUNTIF(#REF!, A1:A6) is a
    // scalar #REF!, never a 6x1 array of #REF! (error propagation short-circuits the map).
    let g = regions();
    assert_eq!(run("=COUNTIF(#REF!,A1:A6)", &g), Value::Error(ErrKind::Ref));
}

#[test]
fn elementwise_operator_over_a_range_still_reduces() {
    // The pre-existing element-wise operator broadcast is unchanged: A1:A3 * 2 = {2;4;6}, SUM = 12.
    let g = Grid::new(1, vec![n(1.0), n(2.0), n(3.0)]);
    assert_eq!(run("=SUM(A1:A3*2)", &g), Value::Number(12.0));
    assert_eq!(run("=SUMPRODUCT(A1:A3,A1:A3)", &g), Value::Number(14.0));
}
