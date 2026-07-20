// Concern: the whole-column/row REFERENCE fitness pins (issue #1) — over in-memory workbooks whose formulas use `A:A`/`1:1`/`Sheet!B:B`: (a) `SUM`/`COUNTA` over a whole column bind to the tab's used region and match a hand-bounded range; (b) `SUMIF`/`COUNTIF` over whole columns aggregate the used range (blanks past it contribute nothing, Excel-exact); (c) the QA-surfaced idiom `SUMIF(ALI!B:B,"SALES",ALI!C:C)` cross-sheet criteria-sum evaluates; (d) a whole ROW `1:1` binds to the used columns; (e) a mixed `VLOOKUP(x, A:D, n)` binds the open ROWS while keeping the named columns; (f) an EMPTY target sheet's whole column aggregates to 0/empty; (g) an UNKNOWN sheet qualifier is a located `#REF!`; (h) ad-hoc `eval_formula` binds identically to a stored formula; (i) whole-column and hand-bounded forms agree via the naive differential | Non-concern: the parse of `A:A` into an `Expr::WholeRange` (charlie-ast owns that) and the used-region computation (`used_region`/`overlap` own it) — this grades the model's load-time bind on VALUES | IO: in-memory `Workbook`s -> asserted `Value`s / `Diagnostic` codes / `FormulaOutcome`s
use charlie_ast::{ErrKind, Value};

use super::{assert_agrees, load_one_tab};
use crate::workbook::{FormulaOutcome, Workbook};

#[test]
fn sum_over_a_whole_column_binds_to_the_used_region() {
    // C1 = SUM(A:A): A1:A4 hold 1,2,3,4 (used rows 0..3), so A:A binds to A1:A4 -> 10. It equals the
    // hand-bounded SUM(A1:A4) (D1), the whole point of the feature (no manual extent discovery).
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1:A4", "1\n2\n3\n4"),
            ("C1", "=SUM(A:A)"),
            ("D1", "=SUM(A1:A4)"),
        ],
    );
    assert_eq!(wb.value_at(0, 2, 0), Value::Number(10.0)); // C1
    assert_eq!(wb.value_at(0, 3, 0), Value::Number(10.0)); // D1 (hand-bounded, same answer)
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn counta_and_sumif_over_whole_columns_aggregate_the_used_range() {
    // A1:A3 hold 4,7,9 (used rows 0..2). COUNTA(A:A) = 3 non-blank cells; SUMIF(A:A,">6") sums 7+9 =
    // 16 — blanks past the used region are ignored, so binding to the used range is Excel-exact.
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1:A3", "4\n7\n9"),
            ("D1", "=COUNTA(A:A)"),
            ("D2", "=SUMIF(A:A,\">6\")"),
        ],
    );
    assert_eq!(wb.value_at(0, 3, 0), Value::Number(3.0)); // D1: COUNTA(A:A) = 3 non-blank
    assert_eq!(wb.value_at(0, 3, 1), Value::Number(16.0)); // D2: 7 + 9
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn cross_sheet_criteria_sum_over_whole_columns_evaluates() {
    // The QA-surfaced idiom (loop 3 / instance 91-3): SUMIF(ALI!B:B,"SALES",ALI!C:C). B labels, C
    // amounts; sum the C where B = "SALES" -> 10 + 30 = 40. Both whole columns bind to ALI's extent.
    let wb = Workbook::from_tabs(&[
        ("ALI", &[("B1:C3", "SALES\t10\nOPS\t20\nSALES\t30")]),
        ("Summary", &[("A1", "=SUMIF(ALI!B:B,\"SALES\",ALI!C:C)")]),
    ])
    .expect("loads clean");
    assert_eq!(wb.value_at(1, 0, 0), Value::Number(40.0)); // Summary!A1
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn a_whole_row_binds_to_the_used_columns() {
    // Row 1 across A1:D1 = 1,2,3,4 (used cols 0..3); SUM(1:1) binds to A1:D1 -> 10. A2 (outside row 1)
    // is not counted. The result cell C2 sits off row 1 to avoid a self-reference.
    let wb = load_one_tab(
        "Sheet1",
        &[("A1:D1", "1\t2\t3\t4"), ("A2", "100"), ("C2", "=SUM(1:1)")],
    );
    assert_eq!(wb.value_at(0, 2, 1), Value::Number(10.0)); // C2 = SUM(row 1) = 1+2+3+4
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn vlookup_over_whole_columns_binds_the_open_rows() {
    // VLOOKUP(30, A:D, 4, FALSE): A:D is whole columns A..D with rows bound to the used region
    // (A1:D3). Row where col A = 30 is row 3; column 4 (D) = 300. Mixed column span, open rows.
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1:D3", "10\t1\t2\t100\n20\t3\t4\t200\n30\t5\t6\t300"),
            ("F1", "=VLOOKUP(30,A:D,4,FALSE)"),
        ],
    );
    assert_eq!(wb.value_at(0, 5, 0), Value::Number(300.0)); // F1
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn a_whole_column_on_an_empty_sheet_aggregates_to_zero() {
    // Data has a single blank cell; SUM(Data!A:A) over an empty extent is 0 (Excel: an empty whole
    // column sums 0), never a crash or #REF!.
    let wb = Workbook::from_tabs(&[
        ("Data", &[("A1", "")]),
        ("Summary", &[("A1", "=SUM(Data!A:A)")]),
    ])
    .expect("loads clean");
    assert_eq!(wb.value_at(1, 0, 0), Value::Number(0.0)); // Summary!A1
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn a_whole_column_on_an_unknown_sheet_is_a_ref_refusal() {
    // SUM(NoSuchSheet!A:A): the qualifier names no tab, so the bound range keeps the unknown name and
    // resolves to #REF! at eval — matching Excel's `NoSuchSheet!A:A`.
    let wb = load_one_tab("Sheet1", &[("A1", "=SUM(NoSuchSheet!A:A)")]);
    assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Ref)); // A1
}

#[test]
fn ad_hoc_eval_binds_a_whole_column_like_a_stored_formula() {
    // The `charlie-cli eval` path binds `A:A` to the used region too, so an ad-hoc SUM(A:A) matches a
    // stored one over the same data.
    let wb = load_one_tab("Sheet1", &[("A1:A3", "5\n10\n15")]);
    assert_eq!(
        wb.eval_formula(0, "SUM(A:A)").unwrap(),
        FormulaOutcome::Value("30".to_string())
    );
}

#[test]
fn extent_functions_over_a_whole_range_report_the_used_extent_deliberately() {
    // DELIBERATE v1 DIVERGENCE (see `bound_whole_range`): a whole reference binds to the USED region,
    // so extent-sensitive functions report the used extent, NOT Excel's full 1,048,576 rows / 16,384
    // columns. This test PINS that divergence so it can never drift silently. A1:A4 has a blank at A2
    // (used rows 0..3); B1:E1 spans four columns (used cols 0..4).
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1:A4", "1\n\n3\n4"),
            ("B1:E1", "9\t8\t7\t6"),
            ("G1", "=ROWS(A:A)"),       // Excel: 1048576
            ("G2", "=COLUMNS(A:A)"),    // Excel: 1
            ("G3", "=COUNTBLANK(A:A)"), // Excel: 1048576 - 3
            ("G4", "=COLUMNS(1:1)"),    // Excel: 16384
        ],
    );
    assert_eq!(wb.value_at(0, 6, 0), Value::Number(4.0)); // G1: used rows A1..A4
    assert_eq!(wb.value_at(0, 6, 1), Value::Number(1.0)); // G2: one column
    assert_eq!(wb.value_at(0, 6, 2), Value::Number(1.0)); // G3: the one blank A2 in the used region
    // G4: used cols run A..G (the G-column formula cells themselves extend the used region) -> 7.
    assert_eq!(wb.value_at(0, 6, 3), Value::Number(7.0));
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn differential_whole_column_equals_a_hand_bounded_range() {
    // The naive oracle vs the two-pass engine agree on a whole-column aggregate — the bind produces
    // an ordinary bounded range both sides then evaluate identically (ENG3).
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1:A5", "2\n4\n6\n8\n10"),
            ("C1", "=SUM(A:A)"),
            ("C2", "=AVERAGE(A:A)"),
            ("C3", "=COUNTIF(A:A,\">5\")"),
        ],
    );
    assert_agrees(&wb, &[(0, 2, 0), (0, 2, 1), (0, 2, 2), (0, 0, 2)]);
}
