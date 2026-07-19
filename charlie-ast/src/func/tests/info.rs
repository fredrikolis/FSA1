// Concern: UNIT-TEST pins for the information family built-ins (ISBLANK ISNUMBER ISTEXT ISERROR ISERR ISNA ISLOGICAL ISNONTEXT NA TYPE ERROR.TYPE N ISEVEN ISODD ISFORMULA) exercised through `FUNCS` dispatch — the error-TRANSPARENT inspection contract (an operand's kind is reported, never propagated), the ISERROR-includes-#N/A vs ISERR/ISNA split, the array-transparent TYPE/IS-predicates (a genuinely multi-cell array is reported AS an array, a degenerate 1×1 array/range collapses to its cell first), the error-PROPAGATING number coercers N/ISEVEN/ISODD (a boolean is `#VALUE!` for ISEVEN/ISODD), the ERROR.TYPE error-code map, and ISFORMULA reading the `Resolver::is_formula` content seam (never the cell value) | Non-concern: the info impls (`func/info.rs`) and the shared test fixtures (the parent `tests` module owns `num`/`call`/`arr`/`n`/`t`) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
use super::*;
use crate::refs::RefNode;

#[test]
fn information_predicates_report_operand_kind() {
    let g = Grid::new(1, vec![Value::Blank]);
    // ISBLANK: a blank literal is TRUE; a number/text/empty-string is FALSE.
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
    // ISNUMBER / ISTEXT discriminate the scalar kinds (a bool is NEITHER a number nor text).
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
    // The defining case: an operand that EVALUATES to an error is CAUGHT (TRUE), never returned.
    let div0 = Expr::Binary(
        crate::expr::BinOp::Div,
        Box::new(num(1.0)),
        Box::new(num(0.0)),
    );
    assert_eq!(eval(&call("ISERROR", vec![div0]), &g), Value::Bool(true));
    // A literal #N/A is an error too (ISERROR catches #N/A, unlike the deferred ISERR).
    assert_eq!(
        eval(
            &call("ISERROR", vec![Expr::Lit(Value::Error(ErrKind::Na))]),
            &g
        ),
        Value::Bool(true)
    );
    // NA() itself is an error operand -> ISERROR(NA()) is TRUE.
    assert_eq!(
        eval(&call("ISERROR", vec![call("NA", vec![])]), &g),
        Value::Bool(true)
    );
    // A non-error operand is FALSE — the error path is not spuriously taken.
    assert_eq!(
        eval(&call("ISERROR", vec![num(5.0)]), &g),
        Value::Bool(false)
    );
    // NA() with no argument mints #N/A directly (the sole error-PRODUCING member).
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
    // An error operand reports 16 — TRANSPARENT, not propagated as the error itself.
    assert_eq!(eval(&call("TYPE", vec![call("NA", vec![])]), &g), n(16.0));
    // An empty cell reports as a number (1), Excel-faithful.
    assert_eq!(
        eval(&call("TYPE", vec![Expr::Lit(Value::Blank)]), &g),
        n(1.0)
    );
    // A multi-cell array operand reports 64 — inspected AS an array, not scalarized to #VALUE!.
    assert_eq!(
        eval(&call("TYPE", vec![arr(1, 2, vec![n(1.0), n(2.0)])]), &g),
        n(64.0)
    );
    // And the IS-predicates see an array as "not a number/text/blank/error" -> FALSE (transparent).
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
    // Excel implicit-intersection: a 1×1 array/range collapses to its single cell before the kind is
    // inspected (matching the sibling ERROR.TYPE), so the predicate sees the CELL, not "an array".
    let g = Grid::new(1, vec![Value::Blank]);
    // 1×1 array LITERAL of each kind collapses to that kind.
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
    // TYPE collapses too: a 1×1 array of a number reports 1, not 64.
    assert_eq!(
        eval(&call("TYPE", vec![arr(1, 1, vec![n(5.0)])]), &g),
        n(1.0)
    );

    // The SAME through the RANGE path: A1:A1 over a 1-wide grid evaluates to a 1×1 array.
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

    // A GENUINELY multi-cell array is still "an array" -> FALSE / TYPE 64 (collapse leaves it intact).
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
    // ISERR is every error EXCEPT #N/A; ISNA is #N/A ONLY (both error-transparent).
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
    // ISLOGICAL: only a boolean (a number 1 or the text "TRUE" do NOT coerce).
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
    // ISNONTEXT: TRUE for everything that is NOT text — a number, a blank, an error, an array.
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
    // A live error operand is CAUGHT (transparent), not propagated: ERROR.TYPE(1/0) is 2.
    assert_eq!(eval(&call("ERROR.TYPE", vec![div0()]), &g), n(2.0));
    // A non-error operand is #N/A.
    assert_eq!(
        eval(&call("ERROR.TYPE", vec![num(5.0)]), &g),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn n_coerces_to_number_and_propagates_errors() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(eval(&call("N", vec![num(5.0)]), &g), n(5.0));
    // A boolean coerces to 1/0 (Excel: N(TRUE)=1, N(FALSE)=0 — the formulas lib returns the bool,
    // a lib quirk, so this is pinned here with the hand-verified Excel value, not in the oracle).
    assert_eq!(
        eval(&call("N", vec![Expr::Lit(Value::Bool(true))]), &g),
        n(1.0)
    );
    assert_eq!(
        eval(&call("N", vec![Expr::Lit(Value::Bool(false))]), &g),
        n(0.0)
    );
    // ANY text is 0 — even numeric-looking text is NOT parsed by N.
    assert_eq!(eval(&call("N", vec![Expr::Lit(t("hi"))]), &g), n(0.0));
    assert_eq!(eval(&call("N", vec![Expr::Lit(t("123"))]), &g), n(0.0));
    assert_eq!(eval(&call("N", vec![Expr::Lit(Value::Blank)]), &g), n(0.0));
    // An error PROPAGATES (N is NOT error-transparent).
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
    // Truncation toward zero: 2.5 -> 2 (even); -1.5 -> -1 (odd).
    assert_eq!(eval(&call("ISEVEN", vec![num(2.5)]), &g), Value::Bool(true));
    assert_eq!(
        eval(&call("ISEVEN", vec![num(-1.5)]), &g),
        Value::Bool(false)
    );
    assert_eq!(eval(&call("ISODD", vec![num(3.0)]), &g), Value::Bool(true));
    assert_eq!(eval(&call("ISODD", vec![num(4.0)]), &g), Value::Bool(false));
    assert_eq!(eval(&call("ISODD", vec![num(-1.5)]), &g), Value::Bool(true));
    // Numeric text coerces; a blank is 0 (even).
    assert_eq!(
        eval(&call("ISEVEN", vec![Expr::Lit(t("4"))]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISEVEN", vec![Expr::Lit(Value::Blank)]), &g),
        Value::Bool(true)
    );
    // A BOOLEAN is #VALUE! (Excel-exact: NOT coerced to 1/0 like arithmetic).
    assert_eq!(
        eval(&call("ISEVEN", vec![Expr::Lit(Value::Bool(true))]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("ISODD", vec![Expr::Lit(Value::Bool(false))]), &g),
        Value::Error(ErrKind::Value)
    );
    // Non-numeric text is #VALUE!; an operand error PROPAGATES.
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
    // A 2-cell row; A1 is MARKED a formula, B1 is not (see `Grid::with_formula`).
    let g = Grid::new(2, vec![n(10.0), n(20.0)]).with_formula(0, 0);
    // A1 is a formula -> TRUE; B1 is a literal -> FALSE.
    assert_eq!(
        eval(&call("ISFORMULA", vec![cell_ref(0, 0)]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISFORMULA", vec![cell_ref(1, 0)]), &g),
        Value::Bool(false)
    );
    // A range reference anchors on its top-left cell (A1 -> TRUE).
    assert_eq!(
        eval(&call("ISFORMULA", vec![col_range(2)]), &g),
        Value::Bool(true)
    );
    // A non-reference argument is #VALUE!.
    assert_eq!(
        eval(&call("ISFORMULA", vec![num(1.0)]), &g),
        Value::Error(ErrKind::Value)
    );
}
