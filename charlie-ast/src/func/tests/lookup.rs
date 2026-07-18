// Concern: UNIT-TEST pins for the lookup & reference family built-ins (INDEX MATCH VLOOKUP XLOOKUP CHOOSE ROW COLUMN + the reserved INDIRECT/OFFSET) exercised through `FUNCS` dispatch — scalar/whole-row/whole-col indexing with bounds, exact-vs-approximate matching in both directions, lazy CHOOSE, reference-node reads, and the parse-time reserved-ref-function refusals | Non-concern: the lookup impls (`func/lookup.rs`) and the shared test fixtures (the parent `tests` module owns `num`/`call`/`arr`/`n`/`t`) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
use super::*;

#[test]
fn index_scalar_whole_row_whole_col_and_bounds() {
    let g = Grid::new(1, vec![Value::Blank]);
    // A 2×3 block {1,2,3;4,5,6}.
    let block = || arr(2, 3, vec![n(1.0), n(2.0), n(3.0), n(4.0), n(5.0), n(6.0)]);
    // Single cell (row 2, col 3) = 6.
    assert_eq!(
        eval(&call("INDEX", vec![block(), num(2.0), num(3.0)]), &g),
        n(6.0)
    );
    // Whole row 1 → 1×3 array.
    assert_eq!(
        eval(&call("INDEX", vec![block(), num(1.0), num(0.0)]), &g),
        Value::Array(
            crate::value::Shape { rows: 1, cols: 3 },
            vec![n(1.0), n(2.0), n(3.0)]
        )
    );
    // Whole col 2 → 2×1 array.
    assert_eq!(
        eval(&call("INDEX", vec![block(), num(0.0), num(2.0)]), &g),
        Value::Array(
            crate::value::Shape { rows: 2, cols: 1 },
            vec![n(2.0), n(5.0)]
        )
    );
    // Out-of-bounds row → #REF!.
    assert_eq!(
        eval(&call("INDEX", vec![block(), num(3.0), num(1.0)]), &g),
        Value::Error(ErrKind::Ref)
    );
    // Negative index → #VALUE!.
    assert_eq!(
        eval(&call("INDEX", vec![block(), num(-1.0), num(1.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    // 2-arg over a single column indexes the row; a blank cell reads as 0.
    let colv = arr(3, 1, vec![n(10.0), Value::Blank, n(30.0)]);
    assert_eq!(eval(&call("INDEX", vec![colv, num(2.0)]), &g), n(0.0));
}

#[test]
fn match_exact_and_approximate_both_directions() {
    let g = Grid::new(1, vec![Value::Blank]);
    let asc = || arr(4, 1, vec![n(10.0), n(20.0), n(30.0), n(40.0)]);
    // Approx ascending (default match_type 1): largest <= 25 is 20 → position 2.
    assert_eq!(eval(&call("MATCH", vec![num(25.0), asc()]), &g), n(2.0));
    // Needle below every key → #N/A.
    assert_eq!(
        eval(&call("MATCH", vec![num(5.0), asc(), num(1.0)]), &g),
        Value::Error(ErrKind::Na)
    );
    // Exact (match_type 0).
    assert_eq!(
        eval(&call("MATCH", vec![num(30.0), asc(), num(0.0)]), &g),
        n(3.0)
    );
    assert_eq!(
        eval(&call("MATCH", vec![num(31.0), asc(), num(0.0)]), &g),
        Value::Error(ErrKind::Na)
    );
    // Approx descending (match_type -1): smallest >= 25 is 30 → position 2.
    let desc = arr(4, 1, vec![n(40.0), n(30.0), n(20.0), n(10.0)]);
    assert_eq!(
        eval(&call("MATCH", vec![num(25.0), desc, num(-1.0)]), &g),
        n(2.0)
    );
    // Exact text needle honors wildcards, case-insensitively, first hit.
    let words = arr(2, 1, vec![t("apple"), t("banana")]);
    assert_eq!(
        eval(
            &call("MATCH", vec![Expr::Lit(t("BAN*")), words, num(0.0)]),
            &g
        ),
        n(2.0)
    );
    // A 2-D array is #N/A.
    let two_d = arr(2, 2, vec![n(1.0), n(2.0), n(3.0), n(4.0)]);
    assert_eq!(
        eval(&call("MATCH", vec![num(1.0), two_d, num(0.0)]), &g),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn vlookup_approximate_default_and_exact() {
    let g = Grid::new(1, vec![Value::Blank]);
    // {1,"one";2,"two";3,"three"} — first column sorted ascending.
    let table = || {
        arr(
            3,
            2,
            vec![n(1.0), t("one"), n(2.0), t("two"), n(3.0), t("three")],
        )
    };
    // Approx default: exact key present.
    assert_eq!(
        eval(&call("VLOOKUP", vec![num(2.0), table(), num(2.0)]), &g),
        t("two")
    );
    // Approx default: 2.5 falls to the largest key <= 2.5 → row of key 2.
    assert_eq!(
        eval(&call("VLOOKUP", vec![num(2.5), table(), num(2.0)]), &g),
        t("two")
    );
    // Exact (FALSE): 2.5 is not present → #N/A.
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
    // col_index past the table width → #REF!.
    assert_eq!(
        eval(&call("VLOOKUP", vec![num(2.0), table(), num(3.0)]), &g),
        Value::Error(ErrKind::Ref)
    );
    // A needle below every key → #N/A (approximate).
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
    // Exact by default.
    assert_eq!(
        eval(&call("XLOOKUP", vec![num(2.0), keys(), vals()]), &g),
        t("b")
    );
    // Miss returns the if_not_found argument (evaluated only on the miss).
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
    // Miss with no if_not_found → #N/A.
    assert_eq!(
        eval(&call("XLOOKUP", vec![num(9.0), keys(), vals()]), &g),
        Value::Error(ErrKind::Na)
    );
    // match_mode 1 = exact-or-next-LARGER: 2.5 → key 3 → "c".
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
    // match_mode -1 = exact-or-next-SMALLER: 2.5 → key 2 → "b".
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
    // Reverse search returns the LAST exact hit on duplicate keys.
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
    // Mismatched array lengths → #VALUE!.
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
    // Only the selected branch is evaluated: an error in an unpicked branch never surfaces.
    let bad = Expr::Binary(
        crate::expr::BinOp::Div,
        Box::new(num(1.0)),
        Box::new(num(0.0)),
    );
    assert_eq!(
        eval(&call("CHOOSE", vec![num(1.0), num(7.0), bad]), &g),
        n(7.0)
    );
    // Out-of-range index → #VALUE!.
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
    // A range reads its top-left.
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
    assert_eq!(eval(&call("ROW", vec![rng.clone()]), &g), n(5.0));
    assert_eq!(eval(&call("COLUMN", vec![rng]), &g), n(3.0));
    // A non-reference argument is #VALUE!.
    assert_eq!(
        eval(&call("ROW", vec![num(5.0)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn indirect_and_offset_are_reserved_ref_function_refusals() {
    use crate::diag::DiagCode;
    // Both refuse at PARSE with the located reserved-ref-function code — never unknown-function,
    // never a value. The span points at the call name (offset 1, right after `=`).
    for (formula, name_len) in [("=INDIRECT(\"A1\")", 8), ("=OFFSET(A1,1,1)", 6)] {
        let err = crate::parse(formula).expect_err("reserved ref function refuses");
        assert_eq!(err.code, DiagCode::ReservedRefFunction, "{formula}");
        assert_eq!(err.span.start, 1, "located on the name: {formula}");
        assert_eq!(err.span.end, 1 + name_len, "spans the name: {formula}");
    }
    // The refusal fires REGARDLESS of argument count (arity never gates first).
    assert_eq!(
        crate::parse("=INDIRECT()").expect_err("still refuses").code,
        DiagCode::ReservedRefFunction
    );
}
