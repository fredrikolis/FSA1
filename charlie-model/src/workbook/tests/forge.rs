// Concern: the ENG6 reference-FORGING fitness pins — over in-memory workbooks whose cells contain INDIRECT/OFFSET: (a) a dynamic OFFSET range driven by a COUNT sums the resolved rectangle; (b) a static SUM(OFFSET(...)) rewrites to a range the existing SUM reducer consumes; (c) INDIRECT resolves an A1 text (built by concat) and a sheet-qualified text; (d) nested forging (a forger whose own argument forges) is a located #REF! ForgeRefusal (restricted v1); (e) a forged OVER-LARGE range is left for the resolver's #NUM! (MAX_RANGE_CELLS); (f) a forger-arg cycle (an argument depending on the forger's own output) is a located #REF!; (g) the ENG3 two-pass==naive differential holds over forger cones (the naive oracle applies the SAME rewrite); (h) trace (CLI2) shows a forger's RESOLVED dependencies; (i) ZERO-OVERHEAD — a non-forging workbook has `has_forgers=false` and never records a forge rewrite, while a forging one does; (j) a forger reachable ONLY through another forger's forged range is discovered by the fixpoint's per-round effective-cone re-collection and resolved (no false-reject, not a silent backstop #REF!); (k) a numeric-zero `a1` flag is coerced to FALSE -> the R1C1 ForgeRefusal | Non-concern: the ENG4 forger-cone non-caching pin (the `cache` submodule owns the temp-dir `.cache/` instrument) and the charlie-ast parse-accept of the forgers (charlie-ast owns that) — this grades the model's forge pass on VALUES | IO: in-memory `Workbook`s -> asserted `Value`s / `Diagnostic` codes / `TraceNode`s
use charlie_ast::{ErrKind, Value};

use super::{assert_agrees, load_one_tab};
use crate::diagnostic::Code;
use crate::workbook::{Direction, Workbook};

#[test]
fn offset_with_a_dynamic_height_sums_the_resolved_range() {
    // B1 = SUM(OFFSET($A$1,0,0,COUNT($A$1:$A$5),1)): COUNT sees 3 numbers, so OFFSET forges a 3-tall
    // range $A$1:$A$3 the SUM reduces -> 10+20+30 = 60 (the dynamic-named-range workhorse).
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
    // SUM(OFFSET($A$1,0,0,3,1)) -> SUM($A$1:$A$3) = 1+2+3 = 6.
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
    // OFFSET($A$1,1,1) with default 1x1 extent -> the single cell B2. A1..C2 fill a 3x2 block.
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
    // INDIRECT("A"&1) -> the reference A1 -> 42.
    let wb = load_one_tab("Sheet1", &[("A1", "42"), ("B1", "=INDIRECT(\"A\"&1)")]);
    assert_eq!(wb.value_at(0, 1, 0), Value::Number(42.0));
}

#[test]
fn indirect_resolves_a_sheet_qualified_text() {
    // INDIRECT("Data!B2") forges the cross-sheet reference Data!B2, resolved through the Resolver.
    let wb = Workbook::from_tabs(&[
        ("Data", &[("B2", "77")]),
        ("Main", &[("A1", "=INDIRECT(\"Data!B2\")")]),
    ])
    .expect("loads clean");
    assert_eq!(wb.value_at(1, 0, 0), Value::Number(77.0)); // Main!A1
}

#[test]
fn nested_forging_is_a_located_ref_refusal() {
    // A forger whose OWN argument forges (INDIRECT(INDIRECT(...))) is out of restricted v1 -> a located
    // #REF! ForgeRefusal, never a wrong dynamic guess and never a panic (CORE2).
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
    // OFFSET forges a range of 2,000,000 cells (> MAX_RANGE_CELLS): the plan leaves it unexpanded and
    // Resolver::range refuses it as #NUM! (the shared materialization bound), which SUM propagates.
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
    // A1 = OFFSET($A$5, A1, 0): the row offset reads A1 itself, so the forger's argument cone depends on
    // its own output -> the fixpoint makes no progress -> a located #REF! (never a hang).
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
    // OFFSET($A$1,-1,0) shifts above the grid -> a located #REF! (Excel), never a wrong wrap.
    let wb = load_one_tab("Sheet1", &[("A1", "5"), ("B1", "=OFFSET($A$1,-1,0)")]);
    assert_eq!(wb.value_at(0, 1, 0), Value::Error(ErrKind::Ref));
}

#[test]
fn differential_forging_two_pass_equals_naive() {
    // ENG3: the graph EQUALS a per-cell eval OVER the forge-rewritten effective form (the naive oracle
    // applies the SAME rewrite). A web of forgers + their consumers, all clean shapes.
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
    // Anchor the expected values so the differential is not vacuously self-agreeing.
    assert_eq!(wb.value_at(0, 2, 0), Value::Number(120.0)); // C1
    assert_eq!(wb.value_at(0, 3, 0), Value::Number(100.0)); // D1
}

#[test]
fn trace_shows_a_forger_resolved_dependencies() {
    // CLI2/ENG6: tracing B1 = SUM(OFFSET($A$1,0,0,3,1)) shows it depending on the RESOLVED range's
    // cells A1,A2,A3 (the effective expr), not the un-forged OFFSET's lone base ref.
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
    // COMPLETENESS (no false-reject): B1 = SUM(OFFSET($A$1,0,0,3,1)) rewrites to SUM($A$1:$A$3); A2 —
    // a cell INSIDE that forged range — is ITSELF a forger (=OFFSET($A$4,0,0) -> A4 = 20). A2 is
    // reachable only through B1's just-rewritten range, so the ORIGINAL grid cone never lists it; the
    // fixpoint's per-round re-collection against the EFFECTIVE cone must discover and resolve it, or A2's
    // un-rewritten OFFSET evaluates to a silent (memoized) backstop #REF!. Correct: A1+A2+A3 = 10+20+30.
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
    // MINOR: Excel coerces the a1 flag to a LOGICAL, so INDIRECT("A1", 0) is R1C1 style (numeric 0 ==
    // FALSE), refused in restricted v1 as a located #REF! — not mistaken for A1 style.
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
    // ZERO-OVERHEAD: a workbook with no INDIRECT/OFFSET has `has_forgers == false`, so `demand` skips
    // Pass 0 and `effective_expr` short-circuits; no forge rewrite is ever recorded.
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
    // The gate is on and exactly the forger cells are rewritten (B1 forges; A1 does not).
    let wb = load_one_tab("Sheet1", &[("A1", "5"), ("B1", "=OFFSET($A$1,0,0)")]);
    assert!(wb.has_forgers, "a forger -> the gate is on");
    assert_eq!(wb.value_at(0, 1, 0), Value::Number(5.0)); // B1 -> A1
    assert_eq!(wb.forge.len(), 1, "only B1 is rewritten");
}
