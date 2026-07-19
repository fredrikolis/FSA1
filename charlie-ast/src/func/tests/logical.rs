// Concern: UNIT-TEST pins for the logical family built-ins (IF IFERROR AND OR XOR TRUE FALSE IFS NOT IFNA SWITCH) exercised through `FUNCS` dispatch — lazy branch evaluation, coercion/propagation, first-true-wins selection, element-wise error-catching over arrays, and #N/A-vs-#VALUE! structural refusals | Non-concern: the logical impls (`func/logical.rs`) and the shared test fixtures (the parent `tests` module owns `num`/`call`) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
use super::*;

#[test]
fn if_is_lazy_and_iferror_catches() {
    let g = Grid::new(1, vec![Value::Blank]);
    // IF(TRUE, 1, 1/0) -> 1 (else branch not evaluated).
    let div0 = Expr::Binary(
        crate::expr::BinOp::Div,
        Box::new(num(1.0)),
        Box::new(num(0.0)),
    );
    let e = call(
        "IF",
        vec![Expr::Lit(Value::Bool(true)), num(1.0), div0.clone()],
    );
    assert_eq!(eval(&e, &g), Value::Number(1.0));
    // Two-arg false -> FALSE.
    let e = call("IF", vec![Expr::Lit(Value::Bool(false)), num(1.0)]);
    assert_eq!(eval(&e, &g), Value::Bool(false));
    // IFERROR(1/0, 99) -> 99.
    let e = call("IFERROR", vec![div0, num(99.0)]);
    assert_eq!(eval(&e, &g), Value::Number(99.0));
    // IFERROR passes a non-error through.
    let e = call("IFERROR", vec![num(7.0), num(99.0)]);
    assert_eq!(eval(&e, &g), Value::Number(7.0));
}

#[test]
fn true_false_constants_and_xor() {
    let g = Grid::new(1, vec![Value::Blank]);
    let b = |v: bool| Expr::Lit(Value::Bool(v));
    // The zero-arg logical constants in call form.
    assert_eq!(eval(&call("TRUE", vec![]), &g), Value::Bool(true));
    assert_eq!(eval(&call("FALSE", vec![]), &g), Value::Bool(false));
    // XOR is TRUE iff an ODD number of the logical data are TRUE.
    assert_eq!(
        eval(&call("XOR", vec![b(true), b(false)]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("XOR", vec![b(true), b(true)]), &g),
        Value::Bool(false)
    );
    assert_eq!(
        eval(&call("XOR", vec![b(true), b(true), b(true)]), &g),
        Value::Bool(true)
    );
    // Numbers coerce (non-zero = TRUE): 1,0,5 -> two TRUEs -> even -> FALSE.
    assert_eq!(
        eval(&call("XOR", vec![num(1.0), num(0.0), num(5.0)]), &g),
        Value::Bool(false)
    );
    // An error propagates; a direct non-logical text is #VALUE!; no logical datum is #VALUE!.
    assert_eq!(
        eval(
            &call("XOR", vec![Expr::Lit(Value::Error(ErrKind::Div0)), b(true)]),
            &g
        ),
        Value::Error(ErrKind::Div0)
    );
    assert_eq!(
        eval(&call("XOR", vec![Expr::Lit(Value::Text("x".into()))]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn iferror_and_ifna_catch_element_wise_over_arrays() {
    let g = Grid::new(1, vec![Value::Blank]);
    let shape = crate::value::Shape { rows: 3, cols: 1 };
    // IFERROR over an array: each error cell -> the scalar fallback; non-error cells kept.
    let a = arr(3, 1, vec![n(1.0), Value::Error(ErrKind::Div0), n(3.0)]);
    assert_eq!(
        eval(&call("IFERROR", vec![a, num(0.0)]), &g),
        Value::Array(shape, vec![n(1.0), n(0.0), n(3.0)])
    );
    // An array with no error passes through unchanged (the fallback is never consulted).
    let clean = arr(3, 1, vec![n(1.0), n(2.0), n(3.0)]);
    assert_eq!(
        eval(&call("IFERROR", vec![clean, num(99.0)]), &g),
        Value::Array(shape, vec![n(1.0), n(2.0), n(3.0)])
    );
    // IFNA catches ONLY #N/A element-wise: a #DIV/0! cell is KEPT, a #N/A cell is replaced.
    let mixed = arr(
        3,
        1,
        vec![
            Value::Error(ErrKind::Na),
            Value::Error(ErrKind::Div0),
            n(5.0),
        ],
    );
    assert_eq!(
        eval(&call("IFNA", vec![mixed, num(0.0)]), &g),
        Value::Array(shape, vec![n(0.0), Value::Error(ErrKind::Div0), n(5.0)])
    );
    // IFERROR with a matching-shape array fallback contributes its i-th cell for each error.
    let with_errs = arr(
        3,
        1,
        vec![
            n(1.0),
            Value::Error(ErrKind::Value),
            Value::Error(ErrKind::Na),
        ],
    );
    let fallback = arr(3, 1, vec![n(10.0), n(20.0), n(30.0)]);
    assert_eq!(
        eval(&call("IFERROR", vec![with_errs, fallback]), &g),
        Value::Array(shape, vec![n(1.0), n(20.0), n(30.0)])
    );
}

#[test]
fn and_or_semantics() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(
            &call("AND", vec![Expr::Lit(Value::Bool(true)), num(1.0)]),
            &g
        ),
        Value::Bool(true)
    );
    assert_eq!(
        eval(
            &call("AND", vec![Expr::Lit(Value::Bool(true)), num(0.0)]),
            &g
        ),
        Value::Bool(false)
    );
    assert_eq!(
        eval(
            &call(
                "OR",
                vec![num(0.0), Expr::Lit(Value::Bool(false)), num(1.0)]
            ),
            &g
        ),
        Value::Bool(true)
    );
    // error propagates
    assert_eq!(
        eval(
            &call(
                "AND",
                vec![
                    Expr::Lit(Value::Error(ErrKind::Ref)),
                    Expr::Lit(Value::Bool(true))
                ]
            ),
            &g
        ),
        Value::Error(ErrKind::Ref)
    );
}

#[test]
fn ifs_first_true_wins_lazily_and_none_is_na() {
    let g = Grid::new(1, vec![Value::Blank]);
    let t = || Expr::Lit(Value::Bool(true));
    let f = || Expr::Lit(Value::Bool(false));
    let div0 = || {
        Expr::Binary(
            crate::expr::BinOp::Div,
            Box::new(num(1.0)),
            Box::new(num(0.0)),
        )
    };
    // First TRUE test's value wins; the earlier FALSE pair's value is skipped.
    assert_eq!(
        eval(
            &call("IFS", vec![f(), num(1.0), t(), num(2.0), t(), num(3.0)]),
            &g
        ),
        Value::Number(2.0)
    );
    // Lazy: the unreached value (1/0) after the first match is never evaluated.
    assert_eq!(
        eval(&call("IFS", vec![t(), num(1.0), t(), div0()]), &g),
        Value::Number(1.0)
    );
    // No TRUE test -> #N/A.
    assert_eq!(
        eval(&call("IFS", vec![f(), num(1.0), f(), num(2.0)]), &g),
        Value::Error(ErrKind::Na)
    );
    // A test that errors propagates (evaluated before any match).
    assert_eq!(
        eval(&call("IFS", vec![div0(), num(1.0), t(), num(2.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
    // An odd argument count (dangling test) is a structural #VALUE!.
    assert_eq!(
        eval(&call("IFS", vec![f(), num(1.0), t()]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn not_coerces_and_propagates() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("NOT", vec![Expr::Lit(Value::Bool(true))]), &g),
        Value::Bool(false)
    );
    // A non-zero number coerces to TRUE -> NOT is FALSE; zero -> TRUE.
    assert_eq!(eval(&call("NOT", vec![num(5.0)]), &g), Value::Bool(false));
    assert_eq!(eval(&call("NOT", vec![num(0.0)]), &g), Value::Bool(true));
    // A non-logical text is #VALUE!; an error propagates.
    assert_eq!(
        eval(&call("NOT", vec![Expr::Lit(Value::Text("x".into()))]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("NOT", vec![Expr::Lit(Value::Error(ErrKind::Na))]), &g),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn ifna_catches_only_na() {
    let g = Grid::new(1, vec![Value::Blank]);
    // Catches #N/A.
    assert_eq!(
        eval(
            &call(
                "IFNA",
                vec![Expr::Lit(Value::Error(ErrKind::Na)), num(99.0)]
            ),
            &g
        ),
        Value::Number(99.0)
    );
    // Passes a normal value through.
    assert_eq!(
        eval(&call("IFNA", vec![num(42.0), num(99.0)]), &g),
        Value::Number(42.0)
    );
    // Does NOT catch a different error (the distinction from IFERROR).
    assert_eq!(
        eval(
            &call(
                "IFNA",
                vec![Expr::Lit(Value::Error(ErrKind::Div0)), num(99.0)]
            ),
            &g
        ),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn switch_matches_first_with_optional_default() {
    let g = Grid::new(1, vec![Value::Blank]);
    let txt = |s: &str| Expr::Lit(Value::Text(s.into()));
    // Matches the second value.
    assert_eq!(
        eval(
            &call(
                "SWITCH",
                vec![
                    num(2.0),
                    num(1.0),
                    txt("one"),
                    num(2.0),
                    txt("two"),
                    num(3.0),
                    txt("three")
                ]
            ),
            &g
        ),
        Value::Text("two".into())
    );
    // No match + trailing default -> the default; no match + no default -> #N/A.
    assert_eq!(
        eval(
            &call("SWITCH", vec![num(9.0), num(1.0), txt("one"), txt("none")]),
            &g
        ),
        Value::Text("none".into())
    );
    assert_eq!(
        eval(&call("SWITCH", vec![num(9.0), num(1.0), txt("one")]), &g),
        Value::Error(ErrKind::Na)
    );
    // Text matching is case-insensitive (Excel `=`).
    assert_eq!(
        eval(
            &call("SWITCH", vec![txt("hello"), txt("HELLO"), num(1.0)]),
            &g
        ),
        Value::Number(1.0)
    );
    // The expression's error propagates.
    let div0 = Expr::Binary(
        crate::expr::BinOp::Div,
        Box::new(num(1.0)),
        Box::new(num(0.0)),
    );
    assert_eq!(
        eval(
            &call("SWITCH", vec![div0, num(1.0), txt("one"), txt("def")]),
            &g
        ),
        Value::Error(ErrKind::Div0)
    );
}
