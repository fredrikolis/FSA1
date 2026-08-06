// Concern: pins the lookup and reference built-ins | Non-concern: the impls, the shared fixtures | IO: (Grid, Expr) -> asserted Value
use super::*;

#[test]
fn index_scalar_whole_row_whole_col_and_bounds() {
    let g = Grid::new(1, vec![Value::Blank]);
    let block = || arr(2, 3, vec![n(1.0), n(2.0), n(3.0), n(4.0), n(5.0), n(6.0)]);
    assert_eq!(
        eval(&call("INDEX", vec![block(), num(2.0), num(3.0)]), &g),
        n(6.0)
    );
    assert_eq!(
        eval(&call("INDEX", vec![block(), num(1.0), num(0.0)]), &g),
        Value::Array(
            crate::value::Shape { rows: 1, cols: 3 },
            vec![n(1.0), n(2.0), n(3.0)]
        )
    );
    assert_eq!(
        eval(&call("INDEX", vec![block(), num(0.0), num(2.0)]), &g),
        Value::Array(
            crate::value::Shape { rows: 2, cols: 1 },
            vec![n(2.0), n(5.0)]
        )
    );
    assert_eq!(
        eval(&call("INDEX", vec![block(), num(3.0), num(1.0)]), &g),
        Value::Error(ErrKind::Ref)
    );
    assert_eq!(
        eval(&call("INDEX", vec![block(), num(-1.0), num(1.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    let colv = arr(3, 1, vec![n(10.0), Value::Blank, n(30.0)]);
    assert_eq!(eval(&call("INDEX", vec![colv, num(2.0)]), &g), n(0.0));
}

#[test]
fn match_exact_and_approximate_both_directions() {
    let g = Grid::new(1, vec![Value::Blank]);
    let asc = || arr(4, 1, vec![n(10.0), n(20.0), n(30.0), n(40.0)]);
    assert_eq!(eval(&call("MATCH", vec![num(25.0), asc()]), &g), n(2.0));
    assert_eq!(
        eval(&call("MATCH", vec![num(5.0), asc(), num(1.0)]), &g),
        Value::Error(ErrKind::Na)
    );
    assert_eq!(
        eval(&call("MATCH", vec![num(30.0), asc(), num(0.0)]), &g),
        n(3.0)
    );
    assert_eq!(
        eval(&call("MATCH", vec![num(31.0), asc(), num(0.0)]), &g),
        Value::Error(ErrKind::Na)
    );
    let desc = arr(4, 1, vec![n(40.0), n(30.0), n(20.0), n(10.0)]);
    assert_eq!(
        eval(&call("MATCH", vec![num(25.0), desc, num(-1.0)]), &g),
        n(2.0)
    );
    let words = arr(2, 1, vec![t("apple"), t("banana")]);
    assert_eq!(
        eval(
            &call("MATCH", vec![Expr::Lit(t("BAN*")), words, num(0.0)]),
            &g
        ),
        n(2.0)
    );
    let two_d = arr(2, 2, vec![n(1.0), n(2.0), n(3.0), n(4.0)]);
    assert_eq!(
        eval(&call("MATCH", vec![num(1.0), two_d, num(0.0)]), &g),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn vlookup_approximate_default_and_exact() {
    let g = Grid::new(1, vec![Value::Blank]);
    let table = || {
        arr(
            3,
            2,
            vec![n(1.0), t("one"), n(2.0), t("two"), n(3.0), t("three")],
        )
    };
    assert_eq!(
        eval(&call("VLOOKUP", vec![num(2.0), table(), num(2.0)]), &g),
        t("two")
    );
    assert_eq!(
        eval(&call("VLOOKUP", vec![num(2.5), table(), num(2.0)]), &g),
        t("two")
    );
    assert_eq!(
        eval(
            &call(
                "VLOOKUP",
                vec![num(2.5), table(), num(2.0), Expr::Lit(Value::Bool(false))]
            ),
            &g
        ),
        Value::Error(ErrKind::Na)
    );
    assert_eq!(
        eval(&call("VLOOKUP", vec![num(2.0), table(), num(3.0)]), &g),
        Value::Error(ErrKind::Ref)
    );
    assert_eq!(
        eval(&call("VLOOKUP", vec![num(0.0), table(), num(2.0)]), &g),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn xlookup_exact_default_if_not_found_and_approximate_modes() {
    let g = Grid::new(1, vec![Value::Blank]);
    let keys = || arr(3, 1, vec![n(1.0), n(2.0), n(3.0)]);
    let vals = || arr(3, 1, vec![t("a"), t("b"), t("c")]);
    assert_eq!(
        eval(&call("XLOOKUP", vec![num(2.0), keys(), vals()]), &g),
        t("b")
    );
    assert_eq!(
        eval(
            &call(
                "XLOOKUP",
                vec![num(9.0), keys(), vals(), Expr::Lit(t("none"))]
            ),
            &g
        ),
        t("none")
    );
    assert_eq!(
        eval(&call("XLOOKUP", vec![num(9.0), keys(), vals()]), &g),
        Value::Error(ErrKind::Na)
    );
    assert_eq!(
        eval(
            &call(
                "XLOOKUP",
                vec![num(2.5), keys(), vals(), Expr::Lit(t("x")), num(1.0)]
            ),
            &g
        ),
        t("c")
    );
    assert_eq!(
        eval(
            &call(
                "XLOOKUP",
                vec![num(2.5), keys(), vals(), Expr::Lit(t("x")), num(-1.0)]
            ),
            &g
        ),
        t("b")
    );
    let dupk = arr(3, 1, vec![n(5.0), n(5.0), n(5.0)]);
    let dupv = arr(3, 1, vec![t("first"), t("mid"), t("last")]);
    assert_eq!(
        eval(
            &call(
                "XLOOKUP",
                vec![num(5.0), dupk, dupv, Expr::Lit(t("x")), num(0.0), num(-1.0)]
            ),
            &g
        ),
        t("last")
    );
    let short = arr(2, 1, vec![t("a"), t("b")]);
    assert_eq!(
        eval(&call("XLOOKUP", vec![num(1.0), keys(), short]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn choose_selects_lazily_and_bounds_check() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(
            &call(
                "CHOOSE",
                vec![num(2.0), Expr::Lit(t("a")), Expr::Lit(t("b"))]
            ),
            &g
        ),
        t("b")
    );
    let bad = Expr::Binary(
        crate::expr::BinOp::Div,
        Box::new(num(1.0)),
        Box::new(num(0.0)),
    );
    assert_eq!(
        eval(&call("CHOOSE", vec![num(1.0), num(7.0), bad]), &g),
        n(7.0)
    );
    assert_eq!(
        eval(&call("CHOOSE", vec![num(3.0), num(7.0), num(8.0)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn row_and_column_read_the_reference_node() {
    let g = Grid::new(1, vec![Value::Blank]);
    let b3 = Expr::Ref(crate::refs::RefNode {
        col: 1,
        row: 2,
        col_abs: false,
        row_abs: false,
        sheet: None,
    });
    assert_eq!(eval(&call("ROW", vec![b3.clone()]), &g), n(3.0));
    assert_eq!(eval(&call("COLUMN", vec![b3]), &g), n(2.0));
    let rng = Expr::Range(RangeNode {
        start_col: 2,
        start_row: 4,
        end_col: 5,
        end_row: 9,
        start_col_abs: false,
        start_row_abs: false,
        end_col_abs: false,
        end_row_abs: false,
        sheet: None,
    });
    assert_eq!(
        eval(&call("ROW", vec![rng.clone()]), &g),
        Value::Array(
            crate::value::Shape { rows: 6, cols: 1 },
            vec![n(5.0), n(6.0), n(7.0), n(8.0), n(9.0), n(10.0)]
        )
    );
    assert_eq!(
        eval(&call("COLUMN", vec![rng]), &g),
        Value::Array(
            crate::value::Shape { rows: 1, cols: 4 },
            vec![n(3.0), n(4.0), n(5.0), n(6.0)]
        )
    );
    let one_row = Expr::Range(RangeNode {
        start_col: 0,
        start_row: 2,
        end_col: 4,
        end_row: 2,
        start_col_abs: false,
        start_row_abs: false,
        end_col_abs: false,
        end_row_abs: false,
        sheet: None,
    });
    assert_eq!(eval(&call("ROW", vec![one_row]), &g), n(3.0));
    assert_eq!(
        eval(&call("ROW", vec![num(5.0)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn row_and_column_no_argument_read_the_current_cell() {
    use crate::eval::eval_at;
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(eval_at(&call("ROW", vec![]), &g, 4, 2), n(5.0));
    assert_eq!(eval_at(&call("COLUMN", vec![]), &g, 4, 2), n(3.0));
    assert_eq!(eval(&call("ROW", vec![]), &g), n(1.0));
    assert_eq!(eval(&call("COLUMN", vec![]), &g), n(1.0));
    let a10 = Expr::Ref(crate::refs::RefNode {
        col: 0,
        row: 9,
        col_abs: false,
        row_abs: false,
        sheet: None,
    });
    let c1 = Expr::Ref(crate::refs::RefNode {
        col: 2,
        row: 0,
        col_abs: false,
        row_abs: false,
        sheet: None,
    });
    assert_eq!(eval_at(&call("ROW", vec![a10]), &g, 4, 2), n(10.0));
    assert_eq!(eval_at(&call("COLUMN", vec![c1]), &g, 4, 2), n(3.0));
}

#[test]
fn hlookup_horizontal_exact_and_approximate() {
    let g = Grid::new(1, vec![Value::Blank]);
    let table = || {
        arr(
            2,
            3,
            vec![n(1.0), n(2.0), n(3.0), n(10.0), n(20.0), n(30.0)],
        )
    };
    assert_eq!(
        eval(
            &call(
                "HLOOKUP",
                vec![num(2.0), table(), num(2.0), Expr::Lit(Value::Bool(false))]
            ),
            &g
        ),
        n(20.0)
    );
    assert_eq!(
        eval(&call("HLOOKUP", vec![num(2.5), table(), num(2.0)]), &g),
        n(20.0)
    );
    assert_eq!(
        eval(&call("HLOOKUP", vec![num(2.0), table(), num(3.0)]), &g),
        Value::Error(ErrKind::Ref)
    );
    assert_eq!(
        eval(&call("HLOOKUP", vec![num(2.0), table(), num(0.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(
            &call(
                "HLOOKUP",
                vec![num(0.0), table(), num(2.0), Expr::Lit(Value::Bool(false))]
            ),
            &g
        ),
        Value::Error(ErrKind::Na)
    );
    let words = arr(
        2,
        3,
        vec![t("apple"), t("banana"), t("cherry"), n(1.0), n(2.0), n(3.0)],
    );
    assert_eq!(
        eval(
            &call(
                "HLOOKUP",
                vec![
                    Expr::Lit(t("BAN*")),
                    words,
                    num(2.0),
                    Expr::Lit(Value::Bool(false))
                ]
            ),
            &g
        ),
        n(2.0)
    );
}

#[test]
fn lookup_vector_form_with_and_without_result_vector() {
    let g = Grid::new(1, vec![Value::Blank]);
    let keys = || arr(1, 3, vec![n(1.0), n(2.0), n(3.0)]);
    let results = || arr(1, 3, vec![n(10.0), n(20.0), n(30.0)]);
    assert_eq!(
        eval(&call("LOOKUP", vec![num(2.0), keys(), results()]), &g),
        n(20.0)
    );
    assert_eq!(
        eval(&call("LOOKUP", vec![num(2.5), keys(), results()]), &g),
        n(20.0)
    );
    assert_eq!(eval(&call("LOOKUP", vec![num(2.0), keys()]), &g), n(2.0));
    assert_eq!(
        eval(&call("LOOKUP", vec![num(0.0), keys(), results()]), &g),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn lookup_array_form_searches_by_aspect_ratio() {
    let g = Grid::new(1, vec![Value::Blank]);
    let wide = || {
        arr(
            2,
            3,
            vec![n(1.0), n(2.0), n(3.0), n(10.0), n(20.0), n(30.0)],
        )
    };
    assert_eq!(eval(&call("LOOKUP", vec![num(2.0), wide()]), &g), n(20.0));
    assert_eq!(eval(&call("LOOKUP", vec![num(2.5), wide()]), &g), n(20.0));
    let tall = || arr(3, 2, vec![n(1.0), t("a"), n(2.0), t("b"), n(3.0), t("c")]);
    assert_eq!(eval(&call("LOOKUP", vec![num(2.0), tall()]), &g), t("b"));
    assert_eq!(eval(&call("LOOKUP", vec![num(2.5), tall()]), &g), t("b"));
    assert_eq!(
        eval(&call("LOOKUP", vec![num(0.0), wide()]), &g),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn rows_and_columns_report_shape() {
    let g = Grid::new(1, vec![Value::Blank]);
    let colv = arr(3, 1, vec![n(1.0), n(2.0), n(3.0)]);
    assert_eq!(eval(&call("ROWS", vec![colv]), &g), n(3.0));
    let rowv = arr(1, 3, vec![n(1.0), n(2.0), n(3.0)]);
    assert_eq!(eval(&call("COLUMNS", vec![rowv]), &g), n(3.0));
    let block = arr(2, 3, vec![n(1.0), n(2.0), n(3.0), n(4.0), n(5.0), n(6.0)]);
    assert_eq!(eval(&call("ROWS", vec![block.clone()]), &g), n(2.0));
    assert_eq!(eval(&call("COLUMNS", vec![block]), &g), n(3.0));
    assert_eq!(eval(&call("ROWS", vec![num(5.0)]), &g), n(1.0));
    assert_eq!(
        eval(
            &call("ROWS", vec![Expr::Lit(Value::Error(ErrKind::Div0))]),
            &g
        ),
        Value::Error(ErrKind::Div0)
    );
    assert_eq!(
        eval(
            &call("COLUMNS", vec![Expr::Lit(Value::Error(ErrKind::Ref))]),
            &g
        ),
        Value::Error(ErrKind::Ref)
    );
}

#[test]
fn xmatch_exact_default_and_next_modes() {
    let g = Grid::new(1, vec![Value::Blank]);
    let data = || arr(1, 3, vec![n(10.0), n(20.0), n(30.0)]);
    assert_eq!(eval(&call("XMATCH", vec![num(20.0), data()]), &g), n(2.0));
    assert_eq!(
        eval(&call("XMATCH", vec![num(25.0), data()]), &g),
        Value::Error(ErrKind::Na)
    );
    assert_eq!(
        eval(&call("XMATCH", vec![num(25.0), data(), num(-1.0)]), &g),
        n(2.0)
    );
    assert_eq!(
        eval(&call("XMATCH", vec![num(25.0), data(), num(1.0)]), &g),
        n(3.0)
    );
    assert_eq!(
        eval(&call("XMATCH", vec![num(20.0), data(), num(3.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    let words = arr(1, 3, vec![t("apple"), t("banana"), t("cherry")]);
    assert_eq!(
        eval(
            &call("XMATCH", vec![Expr::Lit(t("BAN*")), words, num(2.0)]),
            &g
        ),
        n(2.0)
    );
    let two_d = arr(2, 2, vec![n(1.0), n(2.0), n(3.0), n(4.0)]);
    assert_eq!(
        eval(&call("XMATCH", vec![num(1.0), two_d]), &g),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn search_family_ignores_error_cells_in_the_lookup_vector() {
    let g = Grid::new(1, vec![Value::Blank]);
    let err = || Value::Error(ErrKind::Na);
    let mvec = arr(3, 1, vec![n(10.0), err(), n(30.0)]);
    assert_eq!(
        eval(&call("MATCH", vec![num(30.0), mvec, num(0.0)]), &g),
        n(3.0)
    );
    let table = arr(3, 2, vec![n(1.0), t("a"), err(), t("b"), n(3.0), t("c")]);
    assert_eq!(
        eval(
            &call(
                "VLOOKUP",
                vec![num(3.0), table, num(2.0), Expr::Lit(Value::Bool(false))]
            ),
            &g
        ),
        t("c")
    );
    let htable = arr(2, 3, vec![n(1.0), err(), n(3.0), n(10.0), n(20.0), n(30.0)]);
    assert_eq!(
        eval(
            &call(
                "HLOOKUP",
                vec![num(3.0), htable, num(2.0), Expr::Lit(Value::Bool(false))]
            ),
            &g
        ),
        n(30.0)
    );
    let keys = arr(1, 3, vec![n(1.0), err(), n(3.0)]);
    let results = arr(1, 3, vec![n(10.0), n(20.0), n(30.0)]);
    assert_eq!(
        eval(&call("LOOKUP", vec![num(3.0), keys, results]), &g),
        n(30.0)
    );
    let wide = arr(2, 3, vec![n(1.0), err(), n(3.0), n(10.0), n(20.0), n(30.0)]);
    assert_eq!(eval(&call("LOOKUP", vec![num(3.0), wide]), &g), n(30.0));
}

#[test]
fn index_accepts_an_omitted_middle_argument_as_whole_column() {
    let g = Grid::new(1, vec![Value::Blank]);
    let e = crate::parse("=INDEX({1,2;3,4;5,6},,2)").expect("omitted middle arg parses");
    assert_eq!(
        eval(&e, &g),
        Value::Array(
            crate::value::Shape { rows: 3, cols: 1 },
            vec![n(2.0), n(4.0), n(6.0)]
        )
    );
    let whole = crate::parse("=INDEX({1,2;3,4},,)").expect("both omitted parses");
    assert_eq!(
        eval(&whole, &g),
        Value::Array(
            crate::value::Shape { rows: 2, cols: 2 },
            vec![n(1.0), n(2.0), n(3.0), n(4.0)]
        )
    );
}

#[test]
fn xmatch_search_mode_reverses_and_validates() {
    let g = Grid::new(1, vec![Value::Blank]);
    let dup = || arr(1, 4, vec![n(5.0), n(7.0), n(5.0), n(7.0)]);
    assert_eq!(
        eval(
            &call("XMATCH", vec![num(5.0), dup(), num(0.0), num(-1.0)]),
            &g
        ),
        n(3.0)
    );
    assert_eq!(
        eval(
            &call("XMATCH", vec![num(5.0), dup(), num(0.0), num(1.0)]),
            &g
        ),
        n(1.0)
    );
    let asc = || arr(1, 4, vec![n(2.0), n(4.0), n(6.0), n(8.0)]);
    let desc = || arr(1, 4, vec![n(8.0), n(6.0), n(4.0), n(2.0)]);
    assert_eq!(
        eval(
            &call("XMATCH", vec![num(6.0), asc(), num(0.0), num(2.0)]),
            &g
        ),
        n(3.0)
    );
    assert_eq!(
        eval(
            &call("XMATCH", vec![num(6.0), desc(), num(0.0), num(-2.0)]),
            &g
        ),
        n(2.0)
    );
    assert_eq!(
        eval(
            &call("XMATCH", vec![num(5.0), dup(), num(0.0), num(3.0)]),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn address_builds_a1_and_r1c1_forms() {
    let g = Grid::new(1, vec![Value::Blank]);
    let s = |args: Vec<Expr>| text(eval(&call("ADDRESS", args), &g));
    assert_eq!(s(vec![num(2.0), num(3.0)]), "$C$2");
    assert_eq!(s(vec![num(2.0), num(3.0), num(2.0)]), "C$2");
    assert_eq!(s(vec![num(2.0), num(3.0), num(3.0)]), "$C2");
    assert_eq!(s(vec![num(2.0), num(3.0), num(4.0)]), "C2");
    assert_eq!(s(vec![num(1.0), num(27.0), num(4.0)]), "AA1");
    let r1c1_style = || Expr::Lit(Value::Bool(false));
    assert_eq!(s(vec![num(2.0), num(3.0), num(1.0), r1c1_style()]), "R2C3");
    assert_eq!(
        s(vec![num(2.0), num(3.0), num(4.0), r1c1_style()]),
        "R[2]C[3]"
    );
    let a1 = Expr::Lit(Value::Bool(true));
    assert_eq!(
        s(vec![
            num(2.0),
            num(3.0),
            num(1.0),
            a1.clone(),
            Expr::Lit(t("Sheet1"))
        ]),
        "Sheet1!$C$2"
    );
    assert_eq!(
        s(vec![
            num(1.0),
            num(1.0),
            num(1.0),
            a1,
            Expr::Lit(t("My Sheet"))
        ]),
        "'My Sheet'!$A$1"
    );
    assert_eq!(s(vec![num(1_048_577.0), num(1.0)]), "$A$1048577");
    assert_eq!(
        s(vec![num(1_048_577.0), num(1.0), num(1.0), r1c1_style()]),
        "R1048577C1"
    );
    assert_eq!(s(vec![num(1.0), num(16_385.0)]), "$XFE$1");
    assert_eq!(
        eval(&call("ADDRESS", vec![num(2.0), num(3.0), num(5.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("ADDRESS", vec![num(0.0), num(3.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(
            &call("ADDRESS", vec![num(0.0), num(1.0), num(4.0), r1c1_style()]),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(
            &call("ADDRESS", vec![num(-1.0), num(1.0), num(4.0), r1c1_style()]),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn indirect_and_offset_now_parse_as_calls_for_the_forge_pass() {
    use crate::diag::DiagCode;
    use crate::expr::Expr;
    for formula in [
        "=INDIRECT(\"A1\")",
        "=OFFSET(A1,1,1)",
        "=SUM(OFFSET(A1,0,0,3,1))",
    ] {
        let expr = crate::parse(formula).unwrap_or_else(|d| panic!("{formula} must parse: {d}"));
        assert!(matches!(expr, Expr::Call(..)), "{formula} -> {expr:?}");
    }
    assert_eq!(
        crate::parse("=INDIRECT()").expect_err("under-arity").code,
        DiagCode::BadArity
    );
    assert_eq!(
        crate::parse("=OFFSET(A1,1)").expect_err("under-arity").code,
        DiagCode::BadArity
    );
    assert_eq!(
        crate::parse("=INDIRECT(\"A1\",TRUE,1)")
            .expect_err("over-arity")
            .code,
        DiagCode::BadArity
    );
}

#[test]
fn a_forger_reaching_eval_unrewritten_is_a_ref_backstop_never_a_panic() {
    let expr = crate::parse("=INDIRECT(\"A1\")").expect("parses");
    let g = Grid::new(1, vec![Value::Number(1.0)]);
    assert_eq!(eval(&expr, &g), Value::Error(ErrKind::Ref));
}
