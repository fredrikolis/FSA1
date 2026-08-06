// Concern: pins the D* built-ins | Non-concern: the impls, the criteria grammar | IO: (Expr) -> asserted Value
use super::*;

/// The canonical Excel `D*` example orchard: 5 columns (Tree Height Age Yield Profit), 6 records.
#[rustfmt::skip]
fn orchard() -> Expr {
    arr(
        7,
        5,
        vec![
            t("Tree"), t("Height"), t("Age"), t("Yield"), t("Profit"),
            t("Apple"), n(18.0), n(20.0), n(14.0), n(105.0),
            t("Pear"), n(12.0), n(12.0), n(10.0), n(96.0),
            t("Cherry"), n(13.0), n(14.0), n(9.0), n(105.0),
            t("Apple"), n(14.0), n(15.0), n(10.0), n(75.0),
            t("Pear"), n(9.0), n(8.0), n(8.0), n(77.0),
            t("Apple"), n(8.0), n(9.0), n(6.0), n(45.0),
        ],
    )
}

/// Criteria selecting `Tree=Apple AND Height>10 AND Age>12` — matches the 105-profit and 75-profit
/// Apple records (the 45-profit Apple has height 8, so it fails).
#[rustfmt::skip]
fn apple_tall_old() -> Expr {
    arr(
        2,
        3,
        vec![
            t("Tree"), t("Height"), t("Age"),
            t("Apple"), txt_v(">10"), txt_v(">12"),
        ],
    )
}

/// A criteria `Value::Text` cell (the `arr` fixture wants `Value`s, not `Expr`s).
fn txt_v(s: &str) -> Value {
    Value::Text(s.into())
}

#[test]
fn dsum_daverage_over_the_matching_records_by_field_name() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(
            &call("DSUM", vec![orchard(), txt("Profit"), apple_tall_old()]),
            &g
        ),
        n(180.0)
    );
    assert_eq!(
        eval(
            &call("DAVERAGE", vec![orchard(), txt("Yield"), apple_tall_old()]),
            &g
        ),
        n(12.0)
    );
    assert_eq!(
        eval(
            &call("DSUM", vec![orchard(), txt("profit"), apple_tall_old()]),
            &g
        ),
        n(180.0)
    );
}

#[test]
fn field_selects_by_one_based_column_number_too() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(
            &call("DSUM", vec![orchard(), num(5.0), apple_tall_old()]),
            &g
        ),
        n(180.0)
    );
    assert_eq!(
        eval(
            &call("DSUM", vec![orchard(), num(5.9), apple_tall_old()]),
            &g
        ),
        n(180.0)
    );
    assert_eq!(
        eval(
            &call("DSUM", vec![orchard(), num(0.0), apple_tall_old()]),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(
            &call("DSUM", vec![orchard(), num(6.0), apple_tall_old()]),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(
            &call("DSUM", vec![orchard(), txt("Nope"), apple_tall_old()]),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn dcount_counts_numbers_dcounta_counts_nonblank_and_omitted_field_counts_records() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(
            &call("DCOUNT", vec![orchard(), txt("Age"), apple_tall_old()]),
            &g
        ),
        n(2.0)
    );
    assert_eq!(
        eval(
            &call("DCOUNTA", vec![orchard(), txt("Tree"), apple_tall_old()]),
            &g
        ),
        n(2.0)
    );
    assert_eq!(
        eval(
            &call("DCOUNT", vec![orchard(), txt("Tree"), apple_tall_old()]),
            &g
        ),
        n(0.0)
    );
    let blank = Expr::Lit(Value::Blank);
    assert_eq!(
        eval(
            &call("DCOUNT", vec![orchard(), blank.clone(), apple_tall_old()]),
            &g
        ),
        n(2.0)
    );
    assert_eq!(
        eval(
            &call("DCOUNTA", vec![orchard(), blank, apple_tall_old()]),
            &g
        ),
        n(2.0)
    );
}

#[test]
fn dmax_dmin_over_the_matching_records() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(
            &call("DMAX", vec![orchard(), txt("Profit"), apple_tall_old()]),
            &g
        ),
        n(105.0)
    );
    assert_eq!(
        eval(
            &call("DMIN", vec![orchard(), txt("Profit"), apple_tall_old()]),
            &g
        ),
        n(75.0)
    );
}

#[test]
fn dget_single_no_and_multi_match_contract() {
    let g = Grid::new(1, vec![Value::Blank]);
    let tallest = arr(2, 1, vec![t("Height"), txt_v(">15")]);
    assert_eq!(
        eval(&call("DGET", vec![orchard(), txt("Yield"), tallest]), &g),
        n(14.0)
    );
    assert_eq!(
        eval(
            &call("DGET", vec![orchard(), txt("Yield"), apple_tall_old()]),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
    let none = arr(2, 1, vec![t("Height"), txt_v(">100")]);
    assert_eq!(
        eval(&call("DGET", vec![orchard(), txt("Yield"), none]), &g),
        Value::Error(ErrKind::Value)
    );
}

/// A 4-record orchard whose tree names share prefixes (`Apple` is a strict prefix of `Apple2`, and a
/// substring of `Pineapple`) — the fixture needed to distinguish BEGINS-WITH from exact matching.
#[rustfmt::skip]
fn prefix_orchard() -> Expr {
    arr(
        5,
        2,
        vec![
            t("Tree"),      t("Profit"),
            t("Apple"),     n(10.0),
            t("Apple2"),    n(20.0),
            t("Pineapple"), n(40.0),
            t("Pear"),      n(80.0),
        ],
    )
}

#[test]
fn bare_text_criteria_match_begins_with_and_leading_eq_forces_exact() {
    let g = Grid::new(1, vec![Value::Blank]);
    let begins = arr(2, 1, vec![t("Tree"), txt_v("Apple")]);
    assert_eq!(
        eval(
            &call(
                "DSUM",
                vec![prefix_orchard(), txt("Profit"), begins.clone()]
            ),
            &g
        ),
        n(30.0)
    );
    let exact = arr(2, 1, vec![t("Tree"), txt_v("=Apple")]);
    assert_eq!(
        eval(
            &call("DSUM", vec![prefix_orchard(), txt("Profit"), exact.clone()]),
            &g
        ),
        n(10.0)
    );
    assert_eq!(
        eval(
            &call("DGET", vec![prefix_orchard(), txt("Profit"), exact]),
            &g
        ),
        n(10.0)
    );
    assert_eq!(
        eval(
            &call("DGET", vec![prefix_orchard(), txt("Profit"), begins]),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn criteria_or_across_rows_and_wildcards() {
    let g = Grid::new(1, vec![Value::Blank]);
    let apple_or_pear = arr(3, 1, vec![t("Tree"), t("Apple"), t("Pear")]);
    assert_eq!(
        eval(
            &call("DSUM", vec![orchard(), txt("Profit"), apple_or_pear]),
            &g
        ),
        n(398.0)
    );
    let a_star = arr(2, 1, vec![t("Tree"), txt_v("A*")]);
    assert_eq!(
        eval(&call("DSUM", vec![orchard(), txt("Profit"), a_star]), &g),
        n(225.0)
    );
}

#[test]
fn a_blank_condition_row_matches_every_record() {
    let g = Grid::new(1, vec![Value::Blank]);
    let match_all = arr(2, 1, vec![t("Tree"), Value::Blank]);
    assert_eq!(
        eval(&call("DSUM", vec![orchard(), txt("Profit"), match_all]), &g),
        n(503.0)
    );
}

#[test]
fn no_matching_records_edge_values() {
    let g = Grid::new(1, vec![Value::Blank]);
    let none = arr(2, 1, vec![t("Height"), txt_v(">100")]);
    assert_eq!(
        eval(
            &call("DSUM", vec![orchard(), txt("Profit"), none.clone()]),
            &g
        ),
        n(0.0)
    );
    assert_eq!(
        eval(
            &call("DMAX", vec![orchard(), txt("Profit"), none.clone()]),
            &g
        ),
        n(0.0)
    );
    assert_eq!(
        eval(
            &call("DMIN", vec![orchard(), txt("Profit"), none.clone()]),
            &g
        ),
        n(0.0)
    );
    assert_eq!(
        eval(
            &call("DCOUNT", vec![orchard(), txt("Profit"), none.clone()]),
            &g
        ),
        n(0.0)
    );
    assert_eq!(
        eval(&call("DAVERAGE", vec![orchard(), txt("Profit"), none]), &g),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn error_condition_cell_propagates() {
    let g = Grid::new(1, vec![Value::Blank]);
    let bad = arr(2, 1, vec![t("Height"), Value::Error(ErrKind::Na)]);
    assert_eq!(
        eval(&call("DSUM", vec![orchard(), txt("Profit"), bad]), &g),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn error_in_a_matching_field_cell_propagates_only_for_the_numeric_reducers() {
    let g = Grid::new(1, vec![Value::Blank]);
    #[rustfmt::skip]
    let db = || {
        arr(
            3,
            2,
            vec![
                t("Tree"), t("Profit"),
                t("Apple"), Value::Error(ErrKind::Div0),
                t("Apple"), n(50.0),
            ],
        )
    };
    let all_apple = || arr(2, 1, vec![t("Tree"), t("Apple")]);
    assert_eq!(
        eval(&call("DSUM", vec![db(), txt("Profit"), all_apple()]), &g),
        Value::Error(ErrKind::Div0)
    );
    assert_eq!(
        eval(&call("DCOUNT", vec![db(), txt("Profit"), all_apple()]), &g),
        n(1.0)
    );
    assert_eq!(
        eval(&call("DCOUNTA", vec![db(), txt("Profit"), all_apple()]), &g),
        n(2.0)
    );
}

#[test]
fn dget_of_a_blank_field_cell_reads_as_zero() {
    let g = Grid::new(1, vec![Value::Blank]);
    let db = arr(2, 2, vec![t("Tree"), t("Val"), t("Apple"), Value::Blank]);
    let one = arr(2, 1, vec![t("Tree"), t("Apple")]);
    assert_eq!(eval(&call("DGET", vec![db, txt("Val"), one]), &g), n(0.0));
}

#[test]
fn database_or_criteria_error_propagates() {
    let g = Grid::new(1, vec![Value::Blank]);
    let err = Expr::Lit(Value::Error(ErrKind::Ref));
    assert_eq!(
        eval(
            &call("DSUM", vec![err.clone(), txt("Profit"), apple_tall_old()]),
            &g
        ),
        Value::Error(ErrKind::Ref)
    );
    assert_eq!(
        eval(&call("DSUM", vec![orchard(), txt("Profit"), err]), &g),
        Value::Error(ErrKind::Ref)
    );
}
