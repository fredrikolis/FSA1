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

// --- Array-broadcast batch: scalar TEXT functions map over an array argument; IF maps over an
//     array condition; reducers collapse the mapped array. ---

#[test]
fn a_scalar_text_function_maps_over_an_array_argument() {
    // LEN broadcasts its (sole) scalar position: LEN({"a","bb","ccc"}) -> {1,2,3} (a 1×3 array).
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        run("=LEN({\"a\",\"bb\",\"ccc\"})", &g),
        Value::Array(Shape { rows: 1, cols: 3 }, vec![n(1.0), n(2.0), n(3.0)])
    );
    // A reducer collapses it: SUMPRODUCT(LEN({"ab","cde"})) = 2 + 3 = 5.
    assert_eq!(
        run("=SUMPRODUCT(LEN({\"ab\",\"cde\"}))", &g),
        Value::Number(5.0)
    );
}

#[test]
fn text_functions_broadcast_a_range_argument_element_wise() {
    // A real range in the text position maps element-wise; scalar positions (the count) broadcast.
    // A1:A3 = "hello","world","!" -> LEFT(A1:A3,2) = {"he","wo","!"} -> CONCAT flattens to "hewo!".
    let g = Grid::new(1, vec![t("hello"), t("world"), t("!")]);
    assert_eq!(
        run("=LEFT(A1:A3,2)", &g),
        Value::Array(Shape { rows: 3, cols: 1 }, vec![t("he"), t("wo"), t("!")])
    );
    assert_eq!(
        run("=CONCAT(LEFT(A1:A3,2))", &g),
        Value::Text("hewo!".into())
    );
}

#[test]
fn the_trim_right_substitute_value_extraction_idiom() {
    // The classic "last token after the space" extractor over a column, summed:
    //   SUBSTITUTE pads each space to 100 spaces, RIGHT(…,100) keeps the tail (spaces + number),
    //   TRIM strips the spaces, VALUE parses the number, IFERROR guards, SUMPRODUCT collapses.
    // {"London 20";"Bristol 50";"Brighton 30"} -> {20;50;30} -> 100. Before the fix the inner chain
    // was a scalar #VALUE! that IFERROR silently turned into 0 (SUMPRODUCT=0) — the dangerous case.
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        run(
            "=SUMPRODUCT(IFERROR(VALUE(TRIM(RIGHT(SUBSTITUTE({\"London 20\";\"Bristol 50\";\"Brighton 30\"},\" \",REPT(\" \",100)),100))),0))",
            &g
        ),
        Value::Number(100.0)
    );
}

#[test]
fn if_maps_over_an_array_condition_and_broadcasts_scalar_branches() {
    // IF({1;0;1},{"a";"b";"c"},"") -> {"a";"";"c"} (element-wise pick; "" broadcasts to false cells).
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        run("=IF({1;0;1},{\"a\";\"b\";\"c\"},\"\")", &g),
        Value::Array(Shape { rows: 3, cols: 1 }, vec![t("a"), t(""), t("c")])
    );
    // The TEXTJOIN idiom that drops the empty false cells: "a, c".
    assert_eq!(
        run(
            "=TEXTJOIN(\", \",TRUE,IF({1;0;1},{\"a\";\"b\";\"c\"},\"\"))",
            &g
        ),
        Value::Text("a, c".into())
    );
    // A scalar branch broadcasts on both sides: IF({1,0,1},10,0) -> {10,0,10}, summing to 20.
    assert_eq!(run("=SUM(IF({1,0,1},10,0))", &g), Value::Number(20.0));
}

#[test]
fn if_with_a_scalar_condition_is_unchanged_and_lazy() {
    // A scalar (or 1×1) condition keeps the lazy scalar path: the unselected branch is never
    // evaluated, so IF(TRUE,1,1/0) is 1, and the whole then-branch (an array) passes through whole
    // (NOT mapped) — Excel `IF(TRUE,{1,2,3},0)` is the array {1,2,3}.
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(run("=IF(TRUE,1,1/0)", &g), Value::Number(1.0));
    assert_eq!(run("=SUM(IF(1=1,{1,2,3},0))", &g), Value::Number(6.0));
}

#[test]
fn if_array_branch_of_a_mismatched_shape_is_a_value_error() {
    // A branch that is a genuinely multi-cell array of a DIFFERENT shape than the condition is a
    // static #VALUE! (the same shape-conformance stance the operators/SUMPRODUCT take).
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        run("=IF({1;0},{1;2;3},0)", &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn if_array_condition_propagates_a_per_cell_error() {
    // An error CELL inside the condition array is that element's error (per-cell totality), while the
    // ok cells still select: IF({1;#DIV/0!;0},{10;20;30},99) -> {10;#DIV/0!;99}, and a reducer that
    // hits the error propagates it.
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        run("=IF({1;#DIV/0!;0},{10;20;30},99)", &g),
        Value::Array(
            Shape { rows: 3, cols: 1 },
            vec![n(10.0), Value::Error(ErrKind::Div0), n(99.0)]
        )
    );
}

#[test]
fn the_operator_broadcast_that_feeds_the_lookup_idiom_builds_the_array() {
    // The `1/(cond)` half of the LOOKUP "last TRUE" idiom is pure operator broadcasting and already
    // works: 1/({1,0,3}<>0) = 1/{TRUE,FALSE,TRUE} = {1,#DIV/0!,1}. (The LOOKUP-over-errors search
    // that consumes this array is a SEPARATE lookup-family concern, out of this array batch.)
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        run("=1/({1,0,3}<>0)", &g),
        Value::Array(
            Shape { rows: 1, cols: 3 },
            vec![n(1.0), Value::Error(ErrKind::Div0), n(1.0)]
        )
    );
}
