// Concern: UNIT-TEST pins for the Database family built-ins (DSUM DAVERAGE DCOUNT DCOUNTA DGET DMAX DMIN) exercised through `FUNCS` dispatch — the labelled-block filter (header row + records, criteria OR'd across rows and AND'd within a row, blank cells as no-constraint), the DATABASE criteria grammar (bare text is BEGINS-WITH, a leading `=` forces exact — pinned with a strict-prefix fixture `Apple`/`Apple2`/`Pineapple`), field selection by NAME and by 1-based NUMBER, the numeric reducers, the two counts (numbers vs non-blank), DGET's single-match / no-match(`#VALUE!`) / multi-match(`#NUM!`) contract, and the error semantics (error condition cell propagates; error in a matching field cell propagates for the numeric reducers but not the counts) — every value hand-verified against Excel (the `formulas` reference lacks the `D*` family, so these are the parity oracle; see conformance/xl-oracle/KNOWN-LIB-GAPS.md) | Non-concern: the Database impls (`func/database.rs`), the criteria grammar (`criteria.rs` owns its own tests), and the shared test fixtures (the parent `tests` module owns `num`/`txt`/`call`/`arr`/`n`/`t`) | IO: literal `Expr` arrays -> asserted `Value`s
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
    // Two matches: profit 105 + 75 = 180.
    assert_eq!(
        eval(
            &call("DSUM", vec![orchard(), txt("Profit"), apple_tall_old()]),
            &g
        ),
        n(180.0)
    );
    // Their yields: (14 + 10) / 2 = 12.
    assert_eq!(
        eval(
            &call("DAVERAGE", vec![orchard(), txt("Yield"), apple_tall_old()]),
            &g
        ),
        n(12.0)
    );
    // Field name folds case, exactly like Excel's header match.
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
    // Column 5 is Profit; same 180 as the by-name selection.
    assert_eq!(
        eval(
            &call("DSUM", vec![orchard(), num(5.0), apple_tall_old()]),
            &g
        ),
        n(180.0)
    );
    // A non-integer field number truncates toward zero (5.9 -> column 5).
    assert_eq!(
        eval(
            &call("DSUM", vec![orchard(), num(5.9), apple_tall_old()]),
            &g
        ),
        n(180.0)
    );
    // Out-of-range column number -> #VALUE!.
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
    // A field name that matches no header -> #VALUE!.
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
    // Two matching records, both with a numeric Age.
    assert_eq!(
        eval(
            &call("DCOUNT", vec![orchard(), txt("Age"), apple_tall_old()]),
            &g
        ),
        n(2.0)
    );
    // DCOUNTA over the (text) Tree column: both matches are non-blank.
    assert_eq!(
        eval(
            &call("DCOUNTA", vec![orchard(), txt("Tree"), apple_tall_old()]),
            &g
        ),
        n(2.0)
    );
    // DCOUNT over the TEXT Tree column counts NO numbers -> 0.
    assert_eq!(
        eval(
            &call("DCOUNT", vec![orchard(), txt("Tree"), apple_tall_old()]),
            &g
        ),
        n(0.0)
    );
    // An omitted field (an empty middle argument, parsed to Blank) counts the matching records.
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
    // Height>15 matches exactly the 18-tall Apple; DGET Yield = 14.
    let tallest = arr(2, 1, vec![t("Height"), txt_v(">15")]);
    assert_eq!(
        eval(&call("DGET", vec![orchard(), txt("Yield"), tallest]), &g),
        n(14.0)
    );
    // The Apple/>10/>12 criteria matches TWO records -> #NUM!.
    assert_eq!(
        eval(
            &call("DGET", vec![orchard(), txt("Yield"), apple_tall_old()]),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
    // Height>100 matches nothing -> #VALUE!.
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
    // Excel's DATABASE criteria match bare text on a BEGINS-WITH basis (NOT the exact match the
    // `*IF(S)` grammar uses): criterion "Apple" selects "Apple" AND "Apple2" — 10 + 20 = 30 — but not
    // "Pineapple" (contains, does not begin with) nor "Pear". Hand-verified against Excel (the
    // `formulas` oracle lacks the D* family; see conformance/xl-oracle/KNOWN-LIB-GAPS.md).
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
    // A leading `=` (entered in Excel as `="=Apple"`) forces EXACT match: only "Apple", profit 10.
    let exact = arr(2, 1, vec![t("Tree"), txt_v("=Apple")]);
    assert_eq!(
        eval(
            &call("DSUM", vec![prefix_orchard(), txt("Profit"), exact.clone()]),
            &g
        ),
        n(10.0)
    );
    // DGET consequences of the two grammars: the exact criterion matches a SINGLE record (⇒ 10), while
    // the begins-with criterion matches TWO (⇒ `#NUM!`).
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
    // Tree in {Apple, Pear} (two OR'd condition rows): Apple(105+75+45)+Pear(96+77) = 398.
    let apple_or_pear = arr(3, 1, vec![t("Tree"), t("Apple"), t("Pear")]);
    assert_eq!(
        eval(
            &call("DSUM", vec![orchard(), txt("Profit"), apple_or_pear]),
            &g
        ),
        n(398.0)
    );
    // Wildcard text criterion "A*" selects the Apple rows: 105+75+45 = 225.
    let a_star = arr(2, 1, vec![t("Tree"), txt_v("A*")]);
    assert_eq!(
        eval(&call("DSUM", vec![orchard(), txt("Profit"), a_star]), &g),
        n(225.0)
    );
}

#[test]
fn a_blank_condition_row_matches_every_record() {
    let g = Grid::new(1, vec![Value::Blank]);
    // Header + one all-blank condition row imposes no constraint -> every record matches.
    let match_all = arr(2, 1, vec![t("Tree"), Value::Blank]);
    // All six profits: 105+96+105+75+77+45 = 503.
    assert_eq!(
        eval(&call("DSUM", vec![orchard(), txt("Profit"), match_all]), &g),
        n(503.0)
    );
}

#[test]
fn no_matching_records_edge_values() {
    let g = Grid::new(1, vec![Value::Blank]);
    let none = arr(2, 1, vec![t("Height"), txt_v(">100")]);
    // DSUM/DMAX/DMIN over no matches are 0; DAVERAGE is #DIV/0!; DCOUNT is 0.
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
    // An error-valued condition cell propagates as the whole result (Excel: an error criterion is
    // not swallowed).
    let bad = arr(2, 1, vec![t("Height"), Value::Error(ErrKind::Na)]);
    assert_eq!(
        eval(&call("DSUM", vec![orchard(), txt("Profit"), bad]), &g),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn error_in_a_matching_field_cell_propagates_only_for_the_numeric_reducers() {
    let g = Grid::new(1, vec![Value::Blank]);
    // A 2-column database whose first Apple record has an error Profit.
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
    // DSUM sees the error at a matching position -> propagates.
    assert_eq!(
        eval(&call("DSUM", vec![db(), txt("Profit"), all_apple()]), &g),
        Value::Error(ErrKind::Div0)
    );
    // DCOUNT counts only the one number, ignoring the error (no propagation).
    assert_eq!(
        eval(&call("DCOUNT", vec![db(), txt("Profit"), all_apple()]), &g),
        n(1.0)
    );
    // DCOUNTA counts BOTH non-blank cells (the error included).
    assert_eq!(
        eval(&call("DCOUNTA", vec![db(), txt("Profit"), all_apple()]), &g),
        n(2.0)
    );
}

#[test]
fn dget_of_a_blank_field_cell_reads_as_zero() {
    let g = Grid::new(1, vec![Value::Blank]);
    // Single matching record whose field cell is blank -> Excel returns 0.
    let db = arr(2, 2, vec![t("Tree"), t("Val"), t("Apple"), Value::Blank]);
    let one = arr(2, 1, vec![t("Tree"), t("Apple")]);
    assert_eq!(eval(&call("DGET", vec![db, txt("Val"), one]), &g), n(0.0));
}

#[test]
fn database_or_criteria_error_propagates() {
    let g = Grid::new(1, vec![Value::Blank]);
    // An error handed as the database (or criteria) argument propagates rather than panicking.
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
