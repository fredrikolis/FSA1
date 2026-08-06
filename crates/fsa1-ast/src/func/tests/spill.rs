// Concern: pins the dynamic-array built-ins | Non-concern: the impls, where a returned array lands | IO: (Expr) -> asserted Value
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
    assert_eq!(
        ev("SORT", vec![a1(vec![n(3.0), n(1.0), n(2.0)])]),
        Value::Array(
            crate::value::Shape { rows: 3, cols: 1 },
            vec![n(1.0), n(2.0), n(3.0)]
        )
    );
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
    assert_eq!(
        ev("SORT", vec![a1(vec![n(1.0), n(2.0)]), num(2.0)]),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn sort_order_other_than_plus_or_minus_one_is_a_located_value_error() {
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
    assert_eq!(
        ev("FILTER", vec![data.clone(), none.clone(), txt("none")]),
        t("none")
    );
    assert_eq!(ev("FILTER", vec![data, none]), Value::Error(ErrKind::Calc));
}

#[test]
fn sequence_generates_row_major_and_refuses_empty() {
    assert_eq!(
        ev("SEQUENCE", vec![num(3.0)]),
        Value::Array(
            crate::value::Shape { rows: 3, cols: 1 },
            vec![n(1.0), n(2.0), n(3.0)]
        )
    );
    assert_eq!(
        ev("SEQUENCE", vec![num(2.0), num(3.0), num(10.0), num(5.0)]),
        Value::Array(
            crate::value::Shape { rows: 2, cols: 3 },
            vec![n(10.0), n(15.0), n(20.0), n(25.0), n(30.0), n(35.0)]
        )
    );
    assert_eq!(ev("SEQUENCE", vec![num(0.0)]), Value::Error(ErrKind::Calc));
}

#[test]
fn sequence_negative_dimension_is_a_value_error_not_calc() {
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
    assert_eq!(
        ev("SEQUENCE", vec![num(2e10), num(2e10)]),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        ev("SEQUENCE", vec![num(1e6), num(1e6)]),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn sort_pathological_index_truncating_to_valid_range_is_a_value_error() {
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
    assert_eq!(
        ev("SORT", vec![a1(vec![n(2.0), Value::Blank, n(0.0), n(1.0)])]),
        Value::Array(
            crate::value::Shape { rows: 4, cols: 1 },
            vec![n(0.0), n(1.0), n(2.0), Value::Blank]
        )
    );
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
    assert_eq!(
        ev("TRANSPOSE", vec![arr(1, 3, vec![n(1.0), n(2.0), n(3.0)])]),
        Value::Array(
            crate::value::Shape { rows: 3, cols: 1 },
            vec![n(1.0), n(2.0), n(3.0)]
        )
    );
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

/// A 3×3 block {1..9} row-major — the shared fixture for the reshaper tests.
fn block3() -> Expr {
    arr(
        3,
        3,
        vec![
            n(1.0),
            n(2.0),
            n(3.0),
            n(4.0),
            n(5.0),
            n(6.0),
            n(7.0),
            n(8.0),
            n(9.0),
        ],
    )
}

#[test]
fn vstack_concatenates_rows_and_pads_ragged_widths() {
    let sh = |rows, cols| crate::value::Shape { rows, cols };
    assert_eq!(
        ev(
            "VSTACK",
            vec![
                arr(1, 2, vec![n(1.0), n(2.0)]),
                arr(1, 2, vec![n(3.0), n(4.0)])
            ]
        ),
        Value::Array(sh(2, 2), vec![n(1.0), n(2.0), n(3.0), n(4.0)])
    );
    let na = Value::Error(ErrKind::Na);
    assert_eq!(
        ev(
            "VSTACK",
            vec![
                arr(1, 3, vec![n(1.0), n(2.0), n(3.0)]),
                arr(1, 1, vec![n(4.0)])
            ]
        ),
        Value::Array(
            sh(2, 3),
            vec![n(1.0), n(2.0), n(3.0), n(4.0), na.clone(), na]
        )
    );
}

#[test]
fn hstack_concatenates_columns_and_pads_ragged_heights() {
    let sh = |rows, cols| crate::value::Shape { rows, cols };
    assert_eq!(
        ev(
            "HSTACK",
            vec![a1(vec![n(1.0), n(2.0)]), a1(vec![n(3.0), n(4.0)])]
        ),
        Value::Array(sh(2, 2), vec![n(1.0), n(3.0), n(2.0), n(4.0)])
    );
    let na = Value::Error(ErrKind::Na);
    assert_eq!(
        ev(
            "HSTACK",
            vec![a1(vec![n(1.0), n(2.0), n(3.0)]), a1(vec![n(4.0)])]
        ),
        Value::Array(
            sh(3, 2),
            vec![n(1.0), n(4.0), n(2.0), na.clone(), n(3.0), na]
        )
    );
}

#[test]
fn take_keeps_leading_or_trailing_rows_and_cols() {
    let sh = |rows, cols| crate::value::Shape { rows, cols };
    assert_eq!(
        ev("TAKE", vec![block3(), num(2.0), num(2.0)]),
        Value::Array(sh(2, 2), vec![n(1.0), n(2.0), n(4.0), n(5.0)])
    );
    assert_eq!(
        ev("TAKE", vec![block3(), num(-1.0), num(-2.0)]),
        Value::Array(sh(1, 2), vec![n(8.0), n(9.0)])
    );
    assert_eq!(
        ev("TAKE", vec![block3(), num(1.0)]),
        Value::Array(sh(1, 3), vec![n(1.0), n(2.0), n(3.0)])
    );
    assert_eq!(
        ev("TAKE", vec![block3(), num(0.0)]),
        Value::Error(ErrKind::Calc)
    );
}

#[test]
fn drop_removes_leading_or_trailing_rows_and_cols() {
    let sh = |rows, cols| crate::value::Shape { rows, cols };
    assert_eq!(
        ev("DROP", vec![block3(), num(1.0), num(1.0)]),
        Value::Array(sh(2, 2), vec![n(5.0), n(6.0), n(8.0), n(9.0)])
    );
    assert_eq!(
        ev("DROP", vec![block3(), num(-2.0)]),
        Value::Array(sh(1, 3), vec![n(1.0), n(2.0), n(3.0)])
    );
    assert_eq!(
        ev("DROP", vec![block3(), num(3.0)]),
        Value::Error(ErrKind::Calc)
    );
}

#[test]
fn chooserows_and_choosecols_select_and_reorder_by_index() {
    let sh = |rows, cols| crate::value::Shape { rows, cols };
    assert_eq!(
        ev("CHOOSEROWS", vec![block3(), num(3.0), num(1.0)]),
        Value::Array(
            sh(2, 3),
            vec![n(7.0), n(8.0), n(9.0), n(1.0), n(2.0), n(3.0)]
        )
    );
    assert_eq!(
        ev("CHOOSEROWS", vec![block3(), num(-1.0)]),
        Value::Array(sh(1, 3), vec![n(7.0), n(8.0), n(9.0)])
    );
    assert_eq!(
        ev("CHOOSEROWS", vec![block3(), num(4.0)]),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        ev("CHOOSECOLS", vec![block3(), num(2.0)]),
        Value::Array(sh(3, 1), vec![n(2.0), n(5.0), n(8.0)])
    );
    assert_eq!(
        ev("CHOOSECOLS", vec![block3(), num(3.0), num(1.0)]),
        Value::Array(
            sh(3, 2),
            vec![n(3.0), n(1.0), n(6.0), n(4.0), n(9.0), n(7.0)]
        )
    );
}

#[test]
fn sortby_orders_rows_by_a_parallel_key_vector() {
    let sh = |rows, cols| crate::value::Shape { rows, cols };
    let data = || {
        arr(
            3,
            2,
            vec![t("a"), n(10.0), t("b"), n(20.0), t("c"), n(30.0)],
        )
    };
    let key = || a1(vec![n(3.0), n(1.0), n(2.0)]);
    assert_eq!(
        ev("SORTBY", vec![data(), key()]),
        Value::Array(
            sh(3, 2),
            vec![t("b"), n(20.0), t("c"), n(30.0), t("a"), n(10.0)]
        )
    );
    assert_eq!(
        ev("SORTBY", vec![data(), key(), num(-1.0)]),
        Value::Array(
            sh(3, 2),
            vec![t("a"), n(10.0), t("c"), n(30.0), t("b"), n(20.0)]
        )
    );
    assert_eq!(
        ev("SORTBY", vec![data(), a1(vec![n(1.0), n(2.0)])]),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn a_spill_function_propagates_an_error_argument() {
    assert_eq!(
        ev("SORT", vec![Expr::Lit(Value::Error(ErrKind::Ref))]),
        Value::Error(ErrKind::Ref)
    );
    assert_eq!(
        ev("TRANSPOSE", vec![Expr::Lit(Value::Error(ErrKind::Div0))]),
        Value::Error(ErrKind::Div0)
    );
}
