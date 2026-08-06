// Concern: pins how a forging call resolves, refuses, and traces | Non-concern: parsing the forgers, the engine's own demand behaviour | IO: workbooks -> asserted values and diagnostics
use fsa1_ast::{ErrKind, Value};

use super::{assert_agrees, load_one_tab};
use crate::diagnostic::Code;
use crate::workbook::{Direction, Workbook};

#[test]
fn offset_with_a_dynamic_height_sums_the_resolved_range() {
    // The dynamic-named-range workhorse: COUNT sees 3 numbers, so OFFSET forges a 3-tall range.
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1", "10"),
            ("A2", "20"),
            ("A3", "30"),
            ("B1", "=SUM(OFFSET($A$1,0,0,COUNT($A$1:$A$5),1))"),
        ],
    );
    assert_eq!(wb.value_at(0, 1, 0), Value::Number(60.0)); // B1
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn sum_over_a_static_offset_range_rewrites_to_a_range() {
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1", "1"),
            ("A2", "2"),
            ("A3", "3"),
            ("B1", "=SUM(OFFSET($A$1,0,0,3,1))"),
        ],
    );
    assert_eq!(wb.value_at(0, 1, 0), Value::Number(6.0));
}

#[test]
fn offset_shifted_to_a_single_cell_reads_that_cell() {
    // The omitted height and width default to the base's own 1x1 extent.
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1:C1", "1\t2\t3"),
            ("A2:C2", "4\t5\t6"),
            ("D1", "=OFFSET($A$1,1,1)"),
        ],
    );
    assert_eq!(wb.value_at(0, 3, 0), Value::Number(5.0)); // D1 -> B2 = 5
}

#[test]
fn indirect_resolves_a1_text_built_by_concat() {
    let wb = load_one_tab("Sheet1", &[("A1", "42"), ("B1", "=INDIRECT(\"A\"&1)")]);
    assert_eq!(wb.value_at(0, 1, 0), Value::Number(42.0));
}

#[test]
fn indirect_resolves_a_sheet_qualified_text() {
    // A cross-sheet target, resolved through the ordinary Resolver.
    let wb = Workbook::from_tabs(&[
        ("Data", &[("B2", "77")]),
        ("Main", &[("A1", "=INDIRECT(\"Data!B2\")")]),
    ])
    .expect("loads clean");
    assert_eq!(wb.value_at(1, 0, 0), Value::Number(77.0)); // Main!A1
}

#[test]
fn nested_forging_is_a_located_ref_refusal() {
    // Never a wrong dynamic guess, and never a panic.
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1", "C1"),
            ("C1", "5"),
            ("B1", "=INDIRECT(INDIRECT(\"A1\"))"),
        ],
    );
    assert_eq!(wb.value_at(0, 1, 0), Value::Error(ErrKind::Ref)); // B1
    let diags = wb.eval_diagnostics();
    assert!(
        diags.iter().any(|d| d.code == Code::ForgeRefusal),
        "a located ForgeRefusal diagnostic: {diags:?}"
    );
}

#[test]
fn a_forged_over_large_range_is_a_num_refusal() {
    // 2,000,000 cells: over the shared materialization bound, so SUM propagates its refusal.
    let wb = load_one_tab(
        "Sheet1",
        &[("A1", "1"), ("B1", "=SUM(OFFSET($A$1,0,0,2000000,1))")],
    );
    assert_eq!(wb.value_at(0, 1, 0), Value::Error(ErrKind::Num));
    let diags = wb.eval_diagnostics();
    assert!(
        diags.iter().any(|d| d.code == Code::RangeTooLarge),
        "the over-large forged range is a RangeTooLarge refusal: {diags:?}"
    );
}

#[test]
fn a_forger_arg_cycle_is_a_located_ref_refusal() {
    // The row offset reads A1 itself, so the fixpoint makes no progress and must refuse, not hang.
    let wb = load_one_tab("Sheet1", &[("A5", "9"), ("A1", "=OFFSET($A$5,A1,0)")]);
    assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Ref)); // A1
    let diags = wb.eval_diagnostics();
    assert!(
        diags.iter().any(|d| d.code == Code::ForgeRefusal),
        "a located ForgeRefusal for the forger-arg cycle: {diags:?}"
    );
}

#[test]
fn offset_off_grid_is_a_located_ref_refusal() {
    // Shifted above the grid: a refusal, never a wrong wrap.
    let wb = load_one_tab("Sheet1", &[("A1", "5"), ("B1", "=OFFSET($A$1,-1,0)")]);
    assert_eq!(wb.value_at(0, 1, 0), Value::Error(ErrKind::Ref));
}

#[test]
fn differential_forging_two_pass_equals_naive() {
    // The oracle applies the SAME rewrite, so this grades the graph over the effective form.
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1", "10"),
            ("A2", "20"),
            ("A3", "30"),
            ("A4", "40"),
            ("B1", "=SUM(OFFSET($A$1,0,0,3,1))"), // -> SUM($A$1:$A$3) = 60
            ("B2", "=OFFSET($A$1,3,0)"),          // -> A4 = 40
            ("B3", "=INDIRECT(\"A\"&2)"),         // -> A2 = 20
            ("C1", "=B1+B2+B3"),                  // consumes three forger cells -> 120
            ("D1", "=SUM(OFFSET($A$1,0,0,COUNT($A$1:$A$4),1))"), // -> SUM($A$1:$A$4) = 100
        ],
    );
    assert_agrees(
        &wb,
        &[
            (0, 0, 0),
            (0, 0, 1),
            (0, 0, 2),
            (0, 0, 3),
            (0, 1, 0),
            (0, 1, 1),
            (0, 1, 2),
            (0, 2, 0),
            (0, 3, 0),
        ],
    );
    // Anchored, so the differential above cannot be vacuously self-agreeing.
    assert_eq!(wb.value_at(0, 2, 0), Value::Number(120.0)); // C1
    assert_eq!(wb.value_at(0, 3, 0), Value::Number(100.0)); // D1
}

#[test]
fn trace_shows_a_forger_resolved_dependencies() {
    // The RESOLVED range's three cells, not the un-forged call's lone base ref.
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1", "1"),
            ("A2", "2"),
            ("A3", "3"),
            ("B1", "=SUM(OFFSET($A$1,0,0,3,1))"),
        ],
    );
    let root = wb.trace(0, 1, 0, Direction::Upstream, None).unwrap();
    assert_eq!(
        root.children.len(),
        3,
        "B1 resolves to a 3-cell range: {:?}",
        root.children.iter().map(|c| &c.cell).collect::<Vec<_>>()
    );
}

#[test]
fn a_forger_inside_another_forgers_forged_range_resolves() {
    // A2 is reachable only through B1's just-rewritten range, so the ORIGINAL grid cone never lists it: without the fixpoint's re-collection it evaluates to a silent, memoized backstop `#REF!`.
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1", "10"),
            ("A2", "=OFFSET($A$4,0,0)"), // forges A4 = 20; sits inside B1's forged range
            ("A3", "30"),
            ("A4", "20"),
            ("B1", "=SUM(OFFSET($A$1,0,0,3,1))"),
        ],
    );
    assert_eq!(wb.value_at(0, 1, 0), Value::Number(60.0)); // B1 -> SUM($A$1:$A$3) = 60
    assert_eq!(wb.value_at(0, 0, 1), Value::Number(20.0)); // A2 -> A4 = 20, NOT a #REF!
    assert!(
        wb.eval_diagnostics().is_empty(),
        "the chained forger resolves cleanly, no refusal: {:?}",
        wb.eval_diagnostics()
    );
    assert_eq!(
        wb.forge.len(),
        2,
        "both B1 and the range-reached A2 are rewritten"
    );
}

#[test]
fn indirect_with_numeric_zero_a1_is_refused_as_r1c1() {
    // Excel coerces the flag to a LOGICAL, so a numeric 0 is FALSE and selects R1C1.
    let wb = load_one_tab("Sheet1", &[("A1", "5"), ("B1", "=INDIRECT(\"A1\",0)")]);
    assert_eq!(wb.value_at(0, 1, 0), Value::Error(ErrKind::Ref)); // B1
    let diags = wb.eval_diagnostics();
    assert!(
        diags.iter().any(|d| d.code == Code::ForgeRefusal),
        "a numeric-zero a1 flag is the R1C1 ForgeRefusal: {diags:?}"
    );
}

#[test]
fn a_non_forging_workbook_never_enters_the_forge_pass() {
    let wb = load_one_tab(
        "Sheet1",
        &[("A1", "1"), ("B1", "=A1+1"), ("C1", "=SUM(A1:B1)")],
    );
    assert!(!wb.has_forgers, "no forger -> the gate is off");
    let _ = wb.value_at(0, 2, 0);
    let _ = wb.value_at(0, 1, 0);
    assert_eq!(wb.forge.len(), 0, "the forge store stays empty");
}

#[test]
fn a_forging_workbook_records_rewrites_only_for_its_forgers() {
    let wb = load_one_tab("Sheet1", &[("A1", "5"), ("B1", "=OFFSET($A$1,0,0)")]);
    assert!(wb.has_forgers, "a forger -> the gate is on");
    assert_eq!(wb.value_at(0, 1, 0), Value::Number(5.0)); // B1 -> A1
    assert_eq!(wb.forge.len(), 1, "only B1 is rewritten");
}

#[test]
fn indirect_argument_no_arg_row_anchors_at_the_forger_cell() {
    // Anchoring at A1 instead would resolve "A1" -> 99, a silent wrong value.
    let wb = load_one_tab(
        "Sheet1",
        &[("A1", "99"), ("A5", "42"), ("C5", "=INDIRECT(\"A\"&ROW())")],
    );
    assert_eq!(wb.value_at(0, 2, 4), Value::Number(42.0)); // C5 -> A5
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn offset_argument_no_arg_column_anchors_at_the_forger_cell() {
    // Anchoring at A1 instead would shift by 1 and read B1 -> 99, a silent wrong value.
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1", "1"),
            ("B1", "99"),
            ("D1", "42"),
            ("C1", "=OFFSET($A$1,0,COLUMN())"),
        ],
    );
    assert_eq!(wb.value_at(0, 2, 0), Value::Number(42.0)); // C1 -> D1
    assert!(wb.eval_diagnostics().is_empty());
}
