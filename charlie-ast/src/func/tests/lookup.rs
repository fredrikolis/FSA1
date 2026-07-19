// Concern: UNIT-TEST pins for the lookup & reference family built-ins (INDEX MATCH VLOOKUP HLOOKUP LOOKUP XMATCH XLOOKUP CHOOSE ROW COLUMN ROWS COLUMNS ADDRESS + the reserved INDIRECT/OFFSET) exercised through `FUNCS` dispatch — scalar/whole-row/whole-col indexing with bounds, INDEX's omitted-middle-argument (whole column), exact-vs-approximate matching in both directions, the whole family SKIPPING error cells in the lookup vector, horizontal + vector lookups, shape queries, ROW/COLUMN yielding the range's coordinate ARRAY, XMATCH's reverse/binary search_mode, ADDRESS's A1/R1C1 forms, lazy CHOOSE, reference-node reads, and the parse-time reserved-ref-function refusals | Non-concern: the lookup impls (`func/lookup.rs`) and the shared test fixtures (the parent `tests` module owns `num`/`call`/`arr`/`n`/`t`/`text`) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
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
    // A multi-cell range yields the coordinate ARRAY (Excel): ROW(C5:F10) = {5;6;7;8;9;10} (vertical),
    // COLUMN(C5:F10) = {3,4,5,6} (horizontal).
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
    // A SINGLE-row range's ROW (and a single-column range's COLUMN) is the scalar coordinate.
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
    // A non-reference argument is #VALUE!.
    assert_eq!(
        eval(&call("ROW", vec![num(5.0)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn hlookup_horizontal_exact_and_approximate() {
    let g = Grid::new(1, vec![Value::Blank]);
    // {1,2,3;10,20,30} — first row sorted ascending, second row the payload.
    let table = || {
        arr(
            2,
            3,
            vec![n(1.0), n(2.0), n(3.0), n(10.0), n(20.0), n(30.0)],
        )
    };
    // Exact (FALSE): find 2 in the first row → row 2's cell = 20.
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
    // Approximate default: 2.5 falls to the largest first-row key <= 2.5 → column of key 2 → 20.
    assert_eq!(
        eval(&call("HLOOKUP", vec![num(2.5), table(), num(2.0)]), &g),
        n(20.0)
    );
    // row_index past the table height → #REF!.
    assert_eq!(
        eval(&call("HLOOKUP", vec![num(2.0), table(), num(3.0)]), &g),
        Value::Error(ErrKind::Ref)
    );
    // row_index < 1 → #VALUE!.
    assert_eq!(
        eval(&call("HLOOKUP", vec![num(2.0), table(), num(0.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    // A needle below every key → #N/A.
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
    // Exact (FALSE) with a TEXT first row honors wildcards, case-insensitively: "BAN*" → column 2.
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
    // With a result vector: approx-match 2 in keys (position 2) → results position 2 = 20.
    assert_eq!(
        eval(&call("LOOKUP", vec![num(2.0), keys(), results()]), &g),
        n(20.0)
    );
    // Approximate: 2.5 lands on key 2's position → 20.
    assert_eq!(
        eval(&call("LOOKUP", vec![num(2.5), keys(), results()]), &g),
        n(20.0)
    );
    // Without a result vector: returns the matched key itself.
    assert_eq!(eval(&call("LOOKUP", vec![num(2.0), keys()]), &g), n(2.0));
    // A needle below every key → #N/A.
    assert_eq!(
        eval(&call("LOOKUP", vec![num(0.0), keys(), results()]), &g),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn lookup_array_form_searches_by_aspect_ratio() {
    let g = Grid::new(1, vec![Value::Blank]);
    // WIDER than tall (2×3): {1,2,3 ; 10,20,30}. The 2-arg array form searches the FIRST ROW and
    // returns the aligned cell of the LAST ROW — never the flattened-vector value the old v1 gave.
    let wide = || {
        arr(
            2,
            3,
            vec![n(1.0), n(2.0), n(3.0), n(10.0), n(20.0), n(30.0)],
        )
    };
    assert_eq!(eval(&call("LOOKUP", vec![num(2.0), wide()]), &g), n(20.0));
    // Approximate: 2.5 lands on the largest first-row key <= 2.5 (key 2) → last row = 20.
    assert_eq!(eval(&call("LOOKUP", vec![num(2.5), wide()]), &g), n(20.0));
    // SQUARE-or-TALLER (3×2): {1,"a" ; 2,"b" ; 3,"c"}. Searches the FIRST COLUMN, returns the LAST.
    let tall = || arr(3, 2, vec![n(1.0), t("a"), n(2.0), t("b"), n(3.0), t("c")]);
    assert_eq!(eval(&call("LOOKUP", vec![num(2.0), tall()]), &g), t("b"));
    assert_eq!(eval(&call("LOOKUP", vec![num(2.5), tall()]), &g), t("b"));
    // A needle below every key → #N/A (not a spurious flattened hit).
    assert_eq!(
        eval(&call("LOOKUP", vec![num(0.0), wide()]), &g),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn rows_and_columns_report_shape() {
    let g = Grid::new(1, vec![Value::Blank]);
    // A column literal {1;2;3} → 3 rows, 1 column.
    let colv = arr(3, 1, vec![n(1.0), n(2.0), n(3.0)]);
    assert_eq!(eval(&call("ROWS", vec![colv]), &g), n(3.0));
    // A row literal {1,2,3} → 1 row, 3 columns.
    let rowv = arr(1, 3, vec![n(1.0), n(2.0), n(3.0)]);
    assert_eq!(eval(&call("COLUMNS", vec![rowv]), &g), n(3.0));
    // A 2×3 block.
    let block = arr(2, 3, vec![n(1.0), n(2.0), n(3.0), n(4.0), n(5.0), n(6.0)]);
    assert_eq!(eval(&call("ROWS", vec![block.clone()]), &g), n(2.0));
    assert_eq!(eval(&call("COLUMNS", vec![block]), &g), n(3.0));
    // A bare scalar is 1×1.
    assert_eq!(eval(&call("ROWS", vec![num(5.0)]), &g), n(1.0));
    // An error argument propagates (the shape query never masks an upstream error).
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
    // Exact by default → position 2.
    assert_eq!(eval(&call("XMATCH", vec![num(20.0), data()]), &g), n(2.0));
    // Exact miss → #N/A.
    assert_eq!(
        eval(&call("XMATCH", vec![num(25.0), data()]), &g),
        Value::Error(ErrKind::Na)
    );
    // match_mode -1 = exact-or-next-SMALLER: 25 → 20 → position 2.
    assert_eq!(
        eval(&call("XMATCH", vec![num(25.0), data(), num(-1.0)]), &g),
        n(2.0)
    );
    // match_mode 1 = exact-or-next-LARGER: 25 → 30 → position 3.
    assert_eq!(
        eval(&call("XMATCH", vec![num(25.0), data(), num(1.0)]), &g),
        n(3.0)
    );
    // An out-of-domain match_mode is #VALUE!.
    assert_eq!(
        eval(&call("XMATCH", vec![num(20.0), data(), num(3.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    // match_mode 2 = exact WITH WILDCARDS on a text needle (case-insensitive), first hit.
    let words = arr(1, 3, vec![t("apple"), t("banana"), t("cherry")]);
    assert_eq!(
        eval(
            &call("XMATCH", vec![Expr::Lit(t("BAN*")), words, num(2.0)]),
            &g
        ),
        n(2.0)
    );
    // A 2-D array is #N/A.
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
    // MATCH: {10; #N/A; 30} — the error cell is skipped; exact-match 30 lands on its ORIGINAL row 3.
    let mvec = arr(3, 1, vec![n(10.0), err(), n(30.0)]);
    assert_eq!(
        eval(&call("MATCH", vec![num(30.0), mvec, num(0.0)]), &g),
        n(3.0)
    );
    // VLOOKUP exact: {1,"a"; #N/A,"b"; 3,"c"} — 3 is found in row 3 despite the error in row 2.
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
    // HLOOKUP exact: {1,#N/A,3 ; 10,20,30} — 3 is found in column 3 → row 2's cell = 30.
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
    // LOOKUP vector form: keys {1,#N/A,3}, results {10,20,30} — approx-match 3 maps to ORIGINAL pos 3.
    let keys = arr(1, 3, vec![n(1.0), err(), n(3.0)]);
    let results = arr(1, 3, vec![n(10.0), n(20.0), n(30.0)]);
    assert_eq!(
        eval(&call("LOOKUP", vec![num(3.0), keys, results]), &g),
        n(30.0)
    );
    // LOOKUP array form (wider-than-tall): search the first row skipping the error, return the last.
    let wide = arr(2, 3, vec![n(1.0), err(), n(3.0), n(10.0), n(20.0), n(30.0)]);
    assert_eq!(eval(&call("LOOKUP", vec![num(3.0), wide]), &g), n(30.0));
}

#[test]
fn index_accepts_an_omitted_middle_argument_as_whole_column() {
    let g = Grid::new(1, vec![Value::Blank]);
    // INDEX(array,,col) — the omitted row_num means the WHOLE column (Excel treats it as 0). The
    // empty slot is only expressible through the parser, so this exercises the parse→eval path.
    let e = crate::parse("=INDEX({1,2;3,4;5,6},,2)").expect("omitted middle arg parses");
    assert_eq!(
        eval(&e, &g),
        Value::Array(
            crate::value::Shape { rows: 3, cols: 1 },
            vec![n(2.0), n(4.0), n(6.0)]
        )
    );
    // Both indices omitted → the whole array.
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
    // search_mode -1 (last-to-first): the LAST exact 5 → position 3.
    assert_eq!(
        eval(
            &call("XMATCH", vec![num(5.0), dup(), num(0.0), num(-1.0)]),
            &g
        ),
        n(3.0)
    );
    // search_mode 1 (default forward): the FIRST exact 5 → position 1.
    assert_eq!(
        eval(
            &call("XMATCH", vec![num(5.0), dup(), num(0.0), num(1.0)]),
            &g
        ),
        n(1.0)
    );
    // The binary modes ±2 request a binary search over data ASSUMED already sorted in the mode's
    // direction; charlie collapses each to the equivalent directional linear scan, which is exact
    // ONLY on that correctly-sorted input (a binary search over unsorted data is undefined in Excel,
    // so it is not pinned here). +2 (ascending binary) on an ascending vector and -2 (descending
    // binary) on a descending vector each land on the sole hit — oracle-pinned vs formulas-lib.
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
    // An out-of-domain search_mode is #VALUE!.
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
    // A1 style, the four abs_num modes.
    assert_eq!(s(vec![num(2.0), num(3.0)]), "$C$2");
    assert_eq!(s(vec![num(2.0), num(3.0), num(2.0)]), "C$2");
    assert_eq!(s(vec![num(2.0), num(3.0), num(3.0)]), "$C2");
    assert_eq!(s(vec![num(2.0), num(3.0), num(4.0)]), "C2");
    // A column past Z rolls over: 27 → AA.
    assert_eq!(s(vec![num(1.0), num(27.0), num(4.0)]), "AA1");
    // R1C1 style: absolute vs bracketed-relative.
    let r1c1_style = || Expr::Lit(Value::Bool(false));
    assert_eq!(s(vec![num(2.0), num(3.0), num(1.0), r1c1_style()]), "R2C3");
    assert_eq!(
        s(vec![num(2.0), num(3.0), num(4.0), r1c1_style()]),
        "R[2]C[3]"
    );
    // A sheet prefix: bare vs quoted (a space forces quoting).
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
    // ADDRESS has NO upper grid bound — it is a pure address-text builder — and the SAME coordinate
    // agrees across display styles: `=ADDRESS(1048577,1)` and its R1C1 form are both values (the
    // A1/R1C1 branches used to disagree here). Pinned vs formulas-lib.
    assert_eq!(s(vec![num(1_048_577.0), num(1.0)]), "$A$1048577");
    assert_eq!(
        s(vec![num(1_048_577.0), num(1.0), num(1.0), r1c1_style()]),
        "R1048577C1"
    );
    // A column past the 16,384 grid still renders (no upper bound): 16385 → XFE.
    assert_eq!(s(vec![num(1.0), num(16_385.0)]), "$XFE$1");
    // Bad abs_num and a non-positive coordinate are located #VALUE!, never a panic (CORE2). The
    // `< 1` check is style-INDEPENDENT: a non-positive coordinate errors even in relative R1C1.
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
