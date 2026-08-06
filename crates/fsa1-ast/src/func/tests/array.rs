// Concern: pins array literals and implicit array evaluation | Non-concern: the parser, the broadcaster impl | IO: (Grid, formula) -> asserted Value
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
    assert_eq!(run("=SUM({1,2,3})", &g), Value::Number(6.0));
    assert_eq!(run("=SUM({1,2;3,4})", &g), Value::Number(10.0));
}

#[test]
fn array_literal_folds_to_a_row_major_array_value() {
    assert_eq!(
        parse("={1,2;3,4}").unwrap(),
        Expr::Lit(Value::Array(
            Shape { rows: 2, cols: 2 },
            vec![n(1.0), n(2.0), n(3.0), n(4.0)]
        ))
    );
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
    assert_eq!(
        parse("={1,2;3}").unwrap_err().code,
        DiagCode::MalformedArray
    );
    assert_eq!(parse("={A1,2}").unwrap_err().code, DiagCode::MalformedArray);
    assert_eq!(
        parse("={SUM(1),2}").unwrap_err().code,
        DiagCode::MalformedArray
    );
    assert_eq!(parse("={}").unwrap_err().code, DiagCode::MalformedArray);
    assert_eq!(parse("={1,}").unwrap_err().code, DiagCode::MalformedArray);
    assert_eq!(parse("={1,2").unwrap_err().code, DiagCode::UnexpectedEof);
}

#[test]
fn countif_maps_over_an_array_criterion() {
    let g = regions();
    assert_eq!(
        run("=COUNTIF(A1:A6,A1:A6)", &g),
        Value::Array(
            Shape { rows: 6, cols: 1 },
            vec![n(3.0), n(2.0), n(3.0), n(1.0), n(2.0), n(3.0)]
        )
    );
    assert_eq!(run("=SUM(COUNTIF(A1:A6,A1:A6))", &g), Value::Number(14.0));
}

#[test]
fn distinct_count_idiom() {
    let g = regions();
    assert_eq!(
        run("=SUMPRODUCT(1/COUNTIF(A1:A6,A1:A6))", &g),
        Value::Number(3.0)
    );
}

#[test]
fn scalar_criterion_still_returns_a_scalar() {
    let g = regions();
    assert_eq!(run("=COUNTIF(A1:A6,\"EMEA\")", &g), Value::Number(3.0));
    assert_eq!(run("=COUNTIF(A1:A6,\"APAC\")", &g), Value::Number(2.0));
}

#[test]
fn sumif_broadcasts_an_array_criterion_too() {
    let g = Grid::new(1, vec![n(1.0), n(2.0), n(3.0)]);
    assert_eq!(run("=SUM(SUMIF(A1:A3,A1:A3))", &g), Value::Number(6.0));
}

#[test]
fn scalar_error_in_a_range_position_short_circuits_instead_of_tiling() {
    let g = regions();
    assert_eq!(run("=COUNTIF(#REF!,A1:A6)", &g), Value::Error(ErrKind::Ref));
}

#[test]
fn elementwise_operator_over_a_range_still_reduces() {
    let g = Grid::new(1, vec![n(1.0), n(2.0), n(3.0)]);
    assert_eq!(run("=SUM(A1:A3*2)", &g), Value::Number(12.0));
    assert_eq!(run("=SUMPRODUCT(A1:A3,A1:A3)", &g), Value::Number(14.0));
}

#[test]
fn a_scalar_text_function_maps_over_an_array_argument() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        run("=LEN({\"a\",\"bb\",\"ccc\"})", &g),
        Value::Array(Shape { rows: 1, cols: 3 }, vec![n(1.0), n(2.0), n(3.0)])
    );
    assert_eq!(
        run("=SUMPRODUCT(LEN({\"ab\",\"cde\"}))", &g),
        Value::Number(5.0)
    );
}

#[test]
fn text_functions_broadcast_a_range_argument_element_wise() {
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
fn a_scalar_date_function_maps_over_an_array_argument() {
    let g = Grid::new(1, vec![n(44927.0), n(44958.0), n(44986.0)]);
    assert_eq!(
        run("=MONTH(A1:A3)", &g),
        Value::Array(Shape { rows: 3, cols: 1 }, vec![n(1.0), n(2.0), n(3.0)])
    );
    assert_eq!(
        run("=YEAR(A1:A3)", &g),
        Value::Array(
            Shape { rows: 3, cols: 1 },
            vec![n(2023.0), n(2023.0), n(2023.0)]
        )
    );
}

#[test]
fn date_constructor_maps_element_wise_over_an_array_position() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        run("=DATE({2020;2021},1,1)", &g),
        Value::Array(Shape { rows: 2, cols: 1 }, vec![n(43831.0), n(44197.0)])
    );
}

#[test]
fn workday_intl_forms_map_over_an_array_weekend_code() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        run("=NETWORKDAYS.INTL(45292,45298,{1;11})", &g),
        Value::Array(Shape { rows: 2, cols: 1 }, vec![n(5.0), n(6.0)])
    );
}

#[test]
fn a_scalar_math_function_maps_over_an_array_argument() {
    let g = Grid::new(1, vec![n(-3.0), n(4.0), Value::Error(ErrKind::Div0)]);
    assert_eq!(
        run("=ABS(A1:A3)", &g),
        Value::Array(
            Shape { rows: 3, cols: 1 },
            vec![n(3.0), n(4.0), Value::Error(ErrKind::Div0)]
        )
    );
}

#[test]
fn a_multi_arg_scalar_function_broadcasts_the_array_and_the_scalar() {
    let g = Grid::new(1, vec![n(1.23), n(4.56), n(7.89)]);
    assert_eq!(
        run("=ROUND(A1:A3,1)", &g),
        Value::Array(Shape { rows: 3, cols: 1 }, vec![n(1.2), n(4.6), n(7.9)])
    );
}

#[test]
fn sumproduct_month_equals_k_times_values_is_the_8942_idiom() {
    let g = Grid::new(1, vec![n(44927.0), n(44958.0), n(44941.0), n(44986.0)]);
    assert_eq!(
        run("=SUMPRODUCT((MONTH(A1:A4)=1)*{5000;1000;3942;2000})", &g),
        Value::Number(8942.0)
    );
}

#[test]
fn array_consuming_reducers_and_lookups_still_reduce_not_map() {
    let g = Grid::new(1, vec![n(10.0), n(20.0), n(30.0)]);
    assert_eq!(run("=SUM(A1:A3)", &g), Value::Number(60.0));
    assert_eq!(run("=SUMPRODUCT(A1:A3)", &g), Value::Number(60.0));
    assert_eq!(run("=COUNT(A1:A3)", &g), Value::Number(3.0));
    assert_eq!(run("=MATCH(20,A1:A3,0)", &g), Value::Number(2.0));
    let t = Grid::new(
        2,
        vec![n(10.0), n(100.0), n(20.0), n(200.0), n(30.0), n(300.0)],
    );
    assert_eq!(run("=VLOOKUP(20,A1:B3,2,FALSE)", &t), Value::Number(200.0));
    assert_eq!(run("=COUNTIF(A1:A3,\">=20\")", &g), Value::Number(2.0));
}

#[test]
fn if_maps_over_an_array_condition_and_broadcasts_scalar_branches() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        run("=IF({1;0;1},{\"a\";\"b\";\"c\"},\"\")", &g),
        Value::Array(Shape { rows: 3, cols: 1 }, vec![t("a"), t(""), t("c")])
    );
    assert_eq!(
        run(
            "=TEXTJOIN(\", \",TRUE,IF({1;0;1},{\"a\";\"b\";\"c\"},\"\"))",
            &g
        ),
        Value::Text("a, c".into())
    );
    assert_eq!(run("=SUM(IF({1,0,1},10,0))", &g), Value::Number(20.0));
}

#[test]
fn if_with_a_scalar_condition_is_unchanged_and_lazy() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(run("=IF(TRUE,1,1/0)", &g), Value::Number(1.0));
    assert_eq!(run("=SUM(IF(1=1,{1,2,3},0))", &g), Value::Number(6.0));
}

#[test]
fn if_array_branch_of_a_mismatched_shape_is_a_value_error() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        run("=IF({1;0},{1;2;3},0)", &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn if_array_condition_propagates_a_per_cell_error() {
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
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        run("=1/({1,0,3}<>0)", &g),
        Value::Array(
            Shape { rows: 1, cols: 3 },
            vec![n(1.0), Value::Error(ErrKind::Div0), n(1.0)]
        )
    );
}
