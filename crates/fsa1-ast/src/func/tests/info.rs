// Concern: pins the information predicates | Non-concern: the impls, the shared fixtures | IO: (Grid, Expr) -> asserted Value
use super::*;
use crate::refs::RefNode;

#[test]
fn information_predicates_report_operand_kind() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("ISBLANK", vec![Expr::Lit(Value::Blank)]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISBLANK", vec![num(0.0)]), &g),
        Value::Bool(false)
    );
    assert_eq!(
        eval(&call("ISBLANK", vec![Expr::Lit(t(""))]), &g),
        Value::Bool(false)
    );
    assert_eq!(
        eval(&call("ISNUMBER", vec![num(3.5)]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISNUMBER", vec![Expr::Lit(t("3"))]), &g),
        Value::Bool(false)
    );
    assert_eq!(
        eval(&call("ISTEXT", vec![Expr::Lit(t("hi"))]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISTEXT", vec![num(3.0)]), &g),
        Value::Bool(false)
    );
    assert_eq!(
        eval(&call("ISNUMBER", vec![Expr::Lit(Value::Bool(true))]), &g),
        Value::Bool(false)
    );
    assert_eq!(
        eval(&call("ISTEXT", vec![Expr::Lit(Value::Bool(true))]), &g),
        Value::Bool(false)
    );
}

#[test]
fn iserror_is_error_transparent_not_propagating() {
    let g = Grid::new(1, vec![Value::Blank]);
    let div0 = Expr::Binary(
        crate::expr::BinOp::Div,
        Box::new(num(1.0)),
        Box::new(num(0.0)),
    );
    assert_eq!(eval(&call("ISERROR", vec![div0]), &g), Value::Bool(true));
    assert_eq!(
        eval(
            &call("ISERROR", vec![Expr::Lit(Value::Error(ErrKind::Na))]),
            &g
        ),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISERROR", vec![call("NA", vec![])]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISERROR", vec![num(5.0)]), &g),
        Value::Bool(false)
    );
    assert_eq!(eval(&call("NA", vec![]), &g), Value::Error(ErrKind::Na));
}

#[test]
fn type_reports_the_code_and_is_error_and_array_transparent() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(eval(&call("TYPE", vec![num(1.0)]), &g), n(1.0));
    assert_eq!(eval(&call("TYPE", vec![Expr::Lit(t("x"))]), &g), n(2.0));
    assert_eq!(
        eval(&call("TYPE", vec![Expr::Lit(Value::Bool(true))]), &g),
        n(4.0)
    );
    assert_eq!(eval(&call("TYPE", vec![call("NA", vec![])]), &g), n(16.0));
    assert_eq!(
        eval(&call("TYPE", vec![Expr::Lit(Value::Blank)]), &g),
        n(1.0)
    );
    assert_eq!(
        eval(&call("TYPE", vec![arr(1, 2, vec![n(1.0), n(2.0)])]), &g),
        n(64.0)
    );
    assert_eq!(
        eval(&call("ISERROR", vec![arr(1, 2, vec![n(1.0), n(2.0)])]), &g),
        Value::Bool(false)
    );
    assert_eq!(
        eval(&call("ISNUMBER", vec![arr(1, 2, vec![n(1.0), n(2.0)])]), &g),
        Value::Bool(false)
    );
}

#[test]
fn predicates_collapse_a_degenerate_1x1_array_to_its_cell() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(
            &call(
                "ISERROR",
                vec![arr(1, 1, vec![Value::Error(ErrKind::Div0)])]
            ),
            &g
        ),
        Value::Bool(true)
    );
    assert_eq!(
        eval(
            &call("ISERR", vec![arr(1, 1, vec![Value::Error(ErrKind::Div0)])]),
            &g
        ),
        Value::Bool(true)
    );
    assert_eq!(
        eval(
            &call("ISNA", vec![arr(1, 1, vec![Value::Error(ErrKind::Na)])]),
            &g
        ),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISNUMBER", vec![arr(1, 1, vec![n(5.0)])]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISTEXT", vec![arr(1, 1, vec![t("hi")])]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(
            &call("ISLOGICAL", vec![arr(1, 1, vec![Value::Bool(true)])]),
            &g
        ),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISBLANK", vec![arr(1, 1, vec![Value::Blank])]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISNONTEXT", vec![arr(1, 1, vec![n(5.0)])]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("TYPE", vec![arr(1, 1, vec![n(5.0)])]), &g),
        n(1.0)
    );

    let err = Grid::new(1, vec![Value::Error(ErrKind::Div0)]);
    assert_eq!(
        eval(&call("ISERROR", vec![col_range(1)]), &err),
        Value::Bool(true)
    );
    let five = Grid::new(1, vec![n(5.0)]);
    assert_eq!(
        eval(&call("ISNUMBER", vec![col_range(1)]), &five),
        Value::Bool(true)
    );

    assert_eq!(
        eval(
            &call(
                "ISERROR",
                vec![arr(1, 2, vec![Value::Error(ErrKind::Div0), n(1.0)])]
            ),
            &g
        ),
        Value::Bool(false)
    );
    assert_eq!(
        eval(&call("ISNUMBER", vec![arr(2, 1, vec![n(1.0), n(2.0)])]), &g),
        Value::Bool(false)
    );
}

/// A single-cell reference literal for the `ISFORMULA` / reference tests.
fn cell_ref(col: u32, row: u32) -> Expr {
    Expr::Ref(RefNode {
        col,
        row,
        col_abs: false,
        row_abs: false,
        sheet: None,
    })
}

fn div0() -> Expr {
    Expr::Binary(
        crate::expr::BinOp::Div,
        Box::new(num(1.0)),
        Box::new(num(0.0)),
    )
}

#[test]
fn iserr_isna_split_over_the_errors() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(eval(&call("ISERR", vec![div0()]), &g), Value::Bool(true));
    assert_eq!(
        eval(&call("ISERR", vec![call("NA", vec![])]), &g),
        Value::Bool(false)
    );
    assert_eq!(eval(&call("ISERR", vec![num(5.0)]), &g), Value::Bool(false));
    assert_eq!(
        eval(&call("ISNA", vec![call("NA", vec![])]), &g),
        Value::Bool(true)
    );
    assert_eq!(eval(&call("ISNA", vec![div0()]), &g), Value::Bool(false));
    assert_eq!(eval(&call("ISNA", vec![num(5.0)]), &g), Value::Bool(false));
}

#[test]
fn islogical_and_isnontext_report_kind_transparently() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("ISLOGICAL", vec![Expr::Lit(Value::Bool(true))]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISLOGICAL", vec![num(1.0)]), &g),
        Value::Bool(false)
    );
    assert_eq!(
        eval(&call("ISLOGICAL", vec![Expr::Lit(t("TRUE"))]), &g),
        Value::Bool(false)
    );
    assert_eq!(
        eval(&call("ISNONTEXT", vec![num(5.0)]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISNONTEXT", vec![Expr::Lit(t("x"))]), &g),
        Value::Bool(false)
    );
    assert_eq!(
        eval(&call("ISNONTEXT", vec![Expr::Lit(Value::Blank)]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISNONTEXT", vec![call("NA", vec![])]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(
            &call("ISNONTEXT", vec![arr(1, 2, vec![n(1.0), n(2.0)])]),
            &g
        ),
        Value::Bool(true)
    );
}

#[test]
fn error_type_maps_each_error_to_its_code_and_nonerror_to_na() {
    let g = Grid::new(1, vec![Value::Blank]);
    let code = |k: ErrKind| eval(&call("ERROR.TYPE", vec![Expr::Lit(Value::Error(k))]), &g);
    assert_eq!(code(ErrKind::Null), n(1.0));
    assert_eq!(code(ErrKind::Div0), n(2.0));
    assert_eq!(code(ErrKind::Value), n(3.0));
    assert_eq!(code(ErrKind::Ref), n(4.0));
    assert_eq!(code(ErrKind::Name), n(5.0));
    assert_eq!(code(ErrKind::Num), n(6.0));
    assert_eq!(code(ErrKind::Na), n(7.0));
    assert_eq!(code(ErrKind::Spill), n(9.0));
    assert_eq!(code(ErrKind::Calc), n(14.0));
    assert_eq!(eval(&call("ERROR.TYPE", vec![div0()]), &g), n(2.0));
    assert_eq!(
        eval(&call("ERROR.TYPE", vec![num(5.0)]), &g),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn n_coerces_to_number_and_propagates_errors() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(eval(&call("N", vec![num(5.0)]), &g), n(5.0));
    assert_eq!(
        eval(&call("N", vec![Expr::Lit(Value::Bool(true))]), &g),
        n(1.0)
    );
    assert_eq!(
        eval(&call("N", vec![Expr::Lit(Value::Bool(false))]), &g),
        n(0.0)
    );
    assert_eq!(eval(&call("N", vec![Expr::Lit(t("hi"))]), &g), n(0.0));
    assert_eq!(eval(&call("N", vec![Expr::Lit(t("123"))]), &g), n(0.0));
    assert_eq!(eval(&call("N", vec![Expr::Lit(Value::Blank)]), &g), n(0.0));
    assert_eq!(
        eval(&call("N", vec![div0()]), &g),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn iseven_isodd_truncate_toward_zero_reject_bools_and_propagate() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(eval(&call("ISEVEN", vec![num(4.0)]), &g), Value::Bool(true));
    assert_eq!(
        eval(&call("ISEVEN", vec![num(3.0)]), &g),
        Value::Bool(false)
    );
    assert_eq!(eval(&call("ISEVEN", vec![num(2.5)]), &g), Value::Bool(true));
    assert_eq!(
        eval(&call("ISEVEN", vec![num(-1.5)]), &g),
        Value::Bool(false)
    );
    assert_eq!(eval(&call("ISODD", vec![num(3.0)]), &g), Value::Bool(true));
    assert_eq!(eval(&call("ISODD", vec![num(4.0)]), &g), Value::Bool(false));
    assert_eq!(eval(&call("ISODD", vec![num(-1.5)]), &g), Value::Bool(true));
    assert_eq!(
        eval(&call("ISEVEN", vec![Expr::Lit(t("4"))]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISEVEN", vec![Expr::Lit(Value::Blank)]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISEVEN", vec![Expr::Lit(Value::Bool(true))]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("ISODD", vec![Expr::Lit(Value::Bool(false))]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("ISEVEN", vec![Expr::Lit(t("x"))]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("ISEVEN", vec![div0()]), &g),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn isformula_reads_the_content_seam_not_the_value() {
    let g = Grid::new(2, vec![n(10.0), n(20.0)]).with_formula(0, 0);
    assert_eq!(
        eval(&call("ISFORMULA", vec![cell_ref(0, 0)]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISFORMULA", vec![cell_ref(1, 0)]), &g),
        Value::Bool(false)
    );
    assert_eq!(
        eval(&call("ISFORMULA", vec![col_range(2)]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISFORMULA", vec![num(1.0)]), &g),
        Value::Error(ErrKind::Value)
    );
}
