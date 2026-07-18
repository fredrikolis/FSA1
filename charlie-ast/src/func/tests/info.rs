// Concern: UNIT-TEST pins for the information family built-ins (ISBLANK ISNUMBER ISTEXT ISERROR NA TYPE) exercised through `FUNCS` dispatch — the error-TRANSPARENT inspection contract (an operand's kind is reported, never propagated), the ISERROR-catches-#N/A vs NA-produces-#N/A split, and array-transparent TYPE/IS-predicates | Non-concern: the info impls (`func/info.rs`) and the shared test fixtures (the parent `tests` module owns `num`/`call`/`arr`/`n`/`t`) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
use super::*;

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
