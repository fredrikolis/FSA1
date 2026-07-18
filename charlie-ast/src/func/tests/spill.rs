// Concern: UNIT-TEST pins for the DYNAMIC-ARRAY (spill) built-ins — SORT/UNIQUE/FILTER/SEQUENCE/TRANSPOSE returning `array` Values with Excel arg order + error semantics (bad sort index, empty filter/unique, non-positive/over-large SEQUENCE), driven through literal-array arguments so the shape/orientation of each result is pinned directly | Non-concern: the func::spill impls under test (func/spill.rs owns them), the GRID5 range-file PLACEMENT of a returned array (charlie-model owns that), and cross-crate conformance (the conformance crate owns the corpus) | IO: literal `Expr` args -> asserted `Value`s
use super::*;

/// Evaluate a call over literal args (the resolver is unused — a blank stub grid).
fn ev(name: &str, args: Vec<Expr>) -> Value {
    let g = Grid::new(1, vec![Value::Blank]);
    eval(&call(name, args), &g)
}

fn a1(cells: Vec<Value>) -> Expr {
    arr(cells.len() as u32, 1, cells)
}

#[test]
fn sort_orders_a_column_ascending_then_descending() {
    // SORT({3;1;2}) -> {1;2;3} (default ascending), the smoke-test shape.
    assert_eq!(
        ev("SORT", vec![a1(vec![n(3.0), n(1.0), n(2.0)])]),
        Value::Array(
            crate::value::Shape { rows: 3, cols: 1 },
            vec![n(1.0), n(2.0), n(3.0)]
        )
    );
    // sort_order -1 is descending.
    assert_eq!(
        ev(
            "SORT",
            vec![a1(vec![n(3.0), n(1.0), n(2.0)]), num(1.0), num(-1.0)]
        ),
        Value::Array(
            crate::value::Shape { rows: 3, cols: 1 },
            vec![n(3.0), n(2.0), n(1.0)]
        )
    );
}

#[test]
fn sort_keys_a_2d_block_by_a_column_and_moves_whole_rows() {
    // A 3x2 block sorted by column 2 ascending carries each row's other cells along.
    let block = arr(3, 2, vec![t("a"), n(3.0), t("b"), n(1.0), t("c"), n(2.0)]);
    assert_eq!(
        ev("SORT", vec![block, num(2.0)]),
        Value::Array(
            crate::value::Shape { rows: 3, cols: 2 },
            vec![t("b"), n(1.0), t("c"), n(2.0), t("a"), n(3.0)]
        )
    );
}

#[test]
fn sort_index_out_of_range_is_a_located_value_error_not_a_panic() {
    // A 3x1 column has one key column; index 2 is out of range -> #VALUE! (CORE2: no panic).
    assert_eq!(
        ev("SORT", vec![a1(vec![n(1.0), n(2.0)]), num(2.0)]),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn sort_order_other_than_plus_or_minus_one_is_a_located_value_error() {
    // Excel accepts only 1 or -1 for `sort_order`; any other value is #VALUE! (never a silent
    // ascending fall-back for 0 / 5). CORE2: a located error value, not a panic.
    assert_eq!(
        ev(
            "SORT",
            vec![a1(vec![n(3.0), n(1.0), n(2.0)]), num(1.0), num(0.0)]
        ),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        ev(
            "SORT",
            vec![a1(vec![n(3.0), n(1.0), n(2.0)]), num(1.0), num(2.0)]
        ),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn unique_keeps_first_occurrence_and_exactly_once() {
    // {1;2;2;3;1} distinct (first-occurrence order) -> {1;2;3}.
    assert_eq!(
        ev(
            "UNIQUE",
            vec![a1(vec![n(1.0), n(2.0), n(2.0), n(3.0), n(1.0)])]
        ),
        Value::Array(
            crate::value::Shape { rows: 3, cols: 1 },
            vec![n(1.0), n(2.0), n(3.0)]
        )
    );
    // exactly_once=TRUE keeps only values appearing exactly once -> {3}.
    assert_eq!(
        ev(
            "UNIQUE",
            vec![
                a1(vec![n(1.0), n(2.0), n(2.0), n(3.0), n(1.0)]),
                Expr::Lit(Value::Bool(false)),
                Expr::Lit(Value::Bool(true))
            ]
        ),
        Value::Array(crate::value::Shape { rows: 1, cols: 1 }, vec![n(3.0)])
    );
}

#[test]
fn unique_folds_text_case_insensitively() {
    // "EMEA" and "emea" rank Equal under value_cmp, so UNIQUE keeps the first only.
    assert_eq!(
        ev("UNIQUE", vec![a1(vec![t("EMEA"), t("emea"), t("APAC")])]),
        Value::Array(
            crate::value::Shape { rows: 2, cols: 1 },
            vec![t("EMEA"), t("APAC")]
        )
    );
}

#[test]
fn filter_keeps_the_rows_the_boolean_vector_selects() {
    // FILTER({10;20;30}, {TRUE;FALSE;TRUE}) -> {10;30}.
    let data = a1(vec![n(10.0), n(20.0), n(30.0)]);
    let inc = a1(vec![
        Value::Bool(true),
        Value::Bool(false),
        Value::Bool(true),
    ]);
    assert_eq!(
        ev("FILTER", vec![data, inc]),
        Value::Array(
            crate::value::Shape { rows: 2, cols: 1 },
            vec![n(10.0), n(30.0)]
        )
    );
}

#[test]
fn filter_empty_returns_if_empty_or_calc_error() {
    let data = a1(vec![n(10.0), n(20.0)]);
    let none = a1(vec![Value::Bool(false), Value::Bool(false)]);
    // With if_empty supplied, return it; omitted -> #CALC! (Excel).
    assert_eq!(
        ev("FILTER", vec![data.clone(), none.clone(), txt("none")]),
        t("none")
    );
    assert_eq!(ev("FILTER", vec![data, none]), Value::Error(ErrKind::Calc));
}

#[test]
fn sequence_generates_row_major_and_refuses_empty() {
    // SEQUENCE(3) -> {1;2;3} (3x1).
    assert_eq!(
        ev("SEQUENCE", vec![num(3.0)]),
        Value::Array(
            crate::value::Shape { rows: 3, cols: 1 },
            vec![n(1.0), n(2.0), n(3.0)]
        )
    );
    // SEQUENCE(2,3,10,5) -> 2x3 row-major from 10 step 5.
    assert_eq!(
        ev("SEQUENCE", vec![num(2.0), num(3.0), num(10.0), num(5.0)]),
        Value::Array(
            crate::value::Shape { rows: 2, cols: 3 },
            vec![n(10.0), n(15.0), n(20.0), n(25.0), n(30.0), n(35.0)]
        )
    );
    // A zero (empty) rows is Excel's empty-array #CALC! (CORE2: never a panic).
    assert_eq!(ev("SEQUENCE", vec![num(0.0)]), Value::Error(ErrKind::Calc));
}

#[test]
fn sequence_negative_dimension_is_a_value_error_not_calc() {
    // Excel returns #CALC! only for the empty (zero) case; a NEGATIVE rows/cols is #VALUE!.
    assert_eq!(
        ev("SEQUENCE", vec![num(-1.0)]),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        ev("SEQUENCE", vec![num(3.0), num(-2.0)]),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn sequence_huge_area_is_num_not_an_overflow_panic() {
    // rows*cols overflows u64 (2e10 * 2e10 = 4e20 > u64::MAX): the SATURATING area computation must
    // still trip the cap as a located #NUM!, never panic under overflow-checks nor wrap past it.
    assert_eq!(
        ev("SEQUENCE", vec![num(2e10), num(2e10)]),
        Value::Error(ErrKind::Num)
    );
    // A merely over-cap (but non-overflowing) area is likewise #NUM!.
    assert_eq!(
        ev("SEQUENCE", vec![num(1e6), num(1e6)]),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn sort_pathological_index_truncating_to_valid_range_is_a_value_error() {
    // A 3x1 column has one key column. An index of 2^32+2 must be #VALUE!, not truncate under
    // `as u32` (2^32+2 -> 2) into the valid range and silently key a non-existent column.
    assert_eq!(
        ev(
            "SORT",
            vec![a1(vec![n(1.0), n(2.0), n(3.0)]), num(4_294_967_298.0)]
        ),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn sort_places_blanks_last_in_both_directions() {
    // Excel sorts empty cells to the END regardless of order, so a Blank never intermixes with 0.
    // Ascending: {2; blank; 0; 1} -> {0; 1; 2; blank}.
    assert_eq!(
        ev("SORT", vec![a1(vec![n(2.0), Value::Blank, n(0.0), n(1.0)])]),
        Value::Array(
            crate::value::Shape { rows: 4, cols: 1 },
            vec![n(0.0), n(1.0), n(2.0), Value::Blank]
        )
    );
    // Descending: the Blank still sorts last, after the reversed present values.
    assert_eq!(
        ev(
            "SORT",
            vec![
                a1(vec![n(2.0), Value::Blank, n(0.0), n(1.0)]),
                num(1.0),
                num(-1.0)
            ]
        ),
        Value::Array(
            crate::value::Shape { rows: 4, cols: 1 },
            vec![n(2.0), n(1.0), n(0.0), Value::Blank]
        )
    );
}

#[test]
fn transpose_swaps_the_axes() {
    // TRANSPOSE({1,2,3}) [1x3] -> {1;2;3} [3x1].
    assert_eq!(
        ev("TRANSPOSE", vec![arr(1, 3, vec![n(1.0), n(2.0), n(3.0)])]),
        Value::Array(
            crate::value::Shape { rows: 3, cols: 1 },
            vec![n(1.0), n(2.0), n(3.0)]
        )
    );
    // A 2x2 block transposes across the diagonal.
    assert_eq!(
        ev(
            "TRANSPOSE",
            vec![arr(2, 2, vec![n(1.0), n(2.0), n(3.0), n(4.0)])]
        ),
        Value::Array(
            crate::value::Shape { rows: 2, cols: 2 },
            vec![n(1.0), n(3.0), n(2.0), n(4.0)]
        )
    );
}

#[test]
fn a_spill_function_propagates_an_error_argument() {
    // An error handed to a spill function propagates (Excel), never panics.
    assert_eq!(
        ev("SORT", vec![Expr::Lit(Value::Error(ErrKind::Ref))]),
        Value::Error(ErrKind::Ref)
    );
    assert_eq!(
        ev("TRANSPOSE", vec![Expr::Lit(Value::Error(ErrKind::Div0))]),
        Value::Error(ErrKind::Div0)
    );
}
