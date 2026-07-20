// Concern: the two-pass engine's BEHAVIORAL pins — demand-driven chains, cycle/self-reference/cross-sheet #REF! refusals, the explicit-grid VAL1 rule, diamond/deep-DAG compute-once, memoization stability, the pull-depth and range-materialization #NUM! bounds and their order-independence (depth-tainted values/ranges never poison a shallower demand), ad-hoc `eval_formula`, batch `values_at` sharing, the computation-hash (ENG4) determinism/sensitivity/cycle=None/VAL1/GRID5-anchor pins, the trace (CLI2) upstream/downstream/shared-dep-repeated/cycle/depth-cap/GRID5-region + out-of-range pins, and the NAIVE-oracle differential test proving the graph EQUALS a per-cell evaluation (over scalar chains AND GRID5 array-formula regions — the dep_key sharing that collapses region coordinates onto one anchor node) | Non-concern: the engine's internal graph shape/node-count/traversal order (asserted nowhere — only VALUES are graded, so a future parallel-execution refactor stays free) and the formula language itself (charlie-ast owns it) | IO: in-memory (and one temp-dir) `Workbook`s -> asserted `Value`s / `Diagnostic` codes / `FormulaOutcome`s
use super::*;

use charlie_ast::{ArrayView, ErrKind, RangeRef, Shape, eval_at};

// The ENG4 persistent-cache + FS3 fitness pins live in their own concern-scoped submodule (they need
// a real temp-dir workbook and the eval-counter instrument), keeping this behavioral file well under
// the per-file line budget.
mod cache;
// The ENG6 reference-FORGING fitness pins (INDIRECT/OFFSET source-rewrite, refusals, the differential,
// and the zero-overhead gate) live in their own submodule, keeping this file under the line budget.
mod forge;
// The FS4 NAME fitness pins (symlink + ref-file representations, scope, refusals, write-through) live
// in their own submodule — they need a real temp-dir workbook with symlinks, so `load_dir`.
mod names;

/// A file's content is exactly its grid (GRID1) — no annotation line. This helper owns the body
/// string so the `&file("…")` call sites hand an owned contents to the loader.
fn file(body: &str) -> String {
    body.to_string()
}

/// Load a single-tab workbook from `(filename, body)` pairs, asserting a clean load.
fn load_one_tab(tab: &str, files: &[(&str, &str)]) -> Workbook {
    let owned: Vec<(String, String)> = files
        .iter()
        .map(|(n, b)| ((*n).to_string(), file(b)))
        .collect();
    let refs: Vec<(&str, &str)> = owned
        .iter()
        .map(|(n, c)| (n.as_str(), c.as_str()))
        .collect();
    Workbook::from_tabs(&[(tab, &refs)])
        .unwrap_or_else(|d| panic!("workbook should load clean: {d:?}"))
}

#[test]
fn chain_a_to_b_to_c_pulls_through_the_model() {
    // A1 = 1 (literal); B1 = A1 + 1 (formula); C1 = B1 * 10 (formula). Requesting C1 pulls B1,
    // which pulls A1 — the demand-driven chain.
    let wb = load_one_tab("Sheet1", &[("A1", "1"), ("B1", "=A1+1"), ("C1", "=B1*10")]);
    assert_eq!(wb.value_at(0, 2, 0), Value::Number(20.0)); // C1
    assert_eq!(wb.value_at(0, 1, 0), Value::Number(2.0)); // B1
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn a_direct_cycle_is_a_ref_refusal_not_a_hang() {
    // A1 = B1; B1 = A1 — a two-cell cycle. Must refuse with #REF!, never overflow the stack.
    let wb = load_one_tab("Sheet1", &[("A1", "=B1"), ("B1", "=A1")]);
    assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Ref)); // A1
    let diags = wb.eval_diagnostics();
    assert!(diags.iter().any(|d| d.code == Code::Cycle), "{diags:?}");
}

#[test]
fn a_self_reference_is_a_cycle() {
    // A1 = A1 + 1 references its own cell.
    let wb = load_one_tab("Sheet1", &[("A1", "=A1+1")]);
    assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Ref));
    assert!(wb.eval_diagnostics().iter().any(|d| d.code == Code::Cycle));
}

#[test]
fn cross_sheet_reference_resolves_the_named_tab() {
    // Inputs!A1 = 10; Summary!A1 = Inputs!A1 * 2 -> 20. Also proves an UNQUALIFIED ref inside a
    // Summary formula resolves against Summary, not tab 0.
    let wb = Workbook::from_tabs(&[
        ("Inputs", &[("A1", &file("10"))]),
        (
            "Summary",
            &[
                ("A1", &file("=Inputs!A1*2")),
                ("A2", &file("100")),
                ("A3", &file("=A2+1")), // unqualified A2 must mean Summary!A2
            ],
        ),
    ])
    .expect("loads clean");
    assert_eq!(wb.value_at(1, 0, 0), Value::Number(20.0)); // Summary!A1
    assert_eq!(wb.value_at(1, 0, 2), Value::Number(101.0)); // Summary!A3 = Summary!A2 + 1
}

#[test]
fn an_explicit_grid_gives_each_cell_its_own_formula() {
    // VAL1: a range file's content is the EXPLICIT grid — no drag-fill. A1:A3 is a literal column
    // vector 1,2,3. B1:B3 is a 3x1 grid of THREE explicit formulas `=A1`, `=A2`, `=A3` (one per
    // cell, written out), so B1=A1=1, B2=A2=2, B3=A3=3. D1 = SUM(A1:A3) pulls the whole range.
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1:A3", "1\n2\n3"),
            ("D1", "=SUM(A1:A3)"),
            ("B1:B3", "=A1\n=A2\n=A3"),
        ],
    );
    assert_eq!(wb.value_at(0, 3, 0), Value::Number(6.0)); // D1
    assert_eq!(wb.value_at(0, 1, 0), Value::Number(1.0)); // B1 = A1
    assert_eq!(wb.value_at(0, 1, 1), Value::Number(2.0)); // B2 = A2
    assert_eq!(wb.value_at(0, 1, 2), Value::Number(3.0)); // B3 = A3
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn an_explicit_grid_evaluates_absolute_and_relative_refs_as_written() {
    // The explicit-grid replacement for the old drag-fill: C2:C4 is a 3x1 grid whose three cells
    // are written out `=A2*B$1`, `=A3*B$1`, `=A4*B$1`. A is 1,2,3 down; B1 (the `$`-pinned row) is
    // 10. Each cell evaluates its OWN formula as written: C2=A2*B1=10, C3=A3*B1=20, C4=A4*B1=30.
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A2:A4", "1\n2\n3"),
            ("B1", "10"),
            ("C2:C4", "=A2*B$1\n=A3*B$1\n=A4*B$1"),
        ],
    );
    assert_eq!(wb.value_at(0, 2, 1), Value::Number(10.0)); // C2
    assert_eq!(wb.value_at(0, 2, 2), Value::Number(20.0)); // C3
    assert_eq!(wb.value_at(0, 2, 3), Value::Number(30.0)); // C4
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn a_bare_range_formula_in_a_single_cell_keeps_the_top_left_element() {
    // A formula that evaluates to a genuinely multi-cell array (`=A1:A3`) written into a SINGLE cell
    // keeps only the array's TOP-LEFT element (GRID5/ENG6: no dynamic spill beyond a declared range,
    // so a one-cell array formula is its implicit-intersection top-left, never `#VALUE!`).
    let wb = load_one_tab("Sheet1", &[("A1:A3", "1\n2\n3"), ("C1", "=A1:A3")]);
    assert_eq!(wb.value_at(0, 2, 0), Value::Number(1.0)); // C1 -> A1
}

#[test]
fn a_diamond_dag_evaluates_each_cell_once_never_exponentially() {
    // A diamond that, WITHOUT single-node sharing, re-evaluates the shared base exponentially:
    // each level references the one below TWICE, so a naive re-eval is 2^depth. The two-pass graph
    // merges the shared node so it is linear and returns instantly. A1=1; each A{n}=A{n+1}+A{n+1}
    // down a long column, so A1 = 2^(len-1). Reaching the assert at all proves no exponential hang.
    let len = 40usize; // 2^39 ~ 5.5e11 re-evals if exponential; instant if shared
    let owned: Vec<(String, String)> = (0..len)
        .map(|i| {
            let name = format!("A{}", i + 1);
            let body = if i + 1 < len {
                format!("=A{n}+A{n}", n = i + 2)
            } else {
                "1".to_string()
            };
            (name, body)
        })
        .collect();
    let refs: Vec<(&str, &str)> = owned
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_str()))
        .collect();
    let wb = load_one_tab("Sheet1", &refs);
    assert_eq!(
        wb.value_at(0, 0, 0),
        Value::Number(2f64.powi((len - 1) as i32)) // A1 = 2^(len-1), computed linearly
    );
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn memoization_gives_a_stable_answer_on_repeated_pulls() {
    // Re-requesting the same formula cell (and its dependents) yields the same value — the memo
    // does not corrupt state across pulls.
    let wb = load_one_tab("Sheet1", &[("A1", "5"), ("B1", "=A1*A1")]);
    assert_eq!(wb.value_at(0, 1, 0), Value::Number(25.0));
    assert_eq!(wb.value_at(0, 1, 0), Value::Number(25.0));
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn a_gap_cell_reads_blank() {
    let wb = load_one_tab("Sheet1", &[("A1", "1")]);
    // Z9 is claimed by no file.
    assert_eq!(wb.value_at(0, 25, 8), Value::Blank);
}

#[test]
fn load_surfaces_overlap_and_bad_files() {
    // Two files claiming intersecting cells -> a load-time overlap refusal. A1:C3 declares 3x3, so
    // its body is a full 3x3 grid; B2 is a single cell inside it.
    let err = Workbook::from_tabs(&[(
        "Sheet1",
        &[
            ("A1:C3", &file("1\t2\t3\n4\t5\t6\n7\t8\t9")),
            ("B2", &file("x")),
        ],
    )])
    .unwrap_err();
    assert!(err.iter().any(|d| d.code == Code::Overlap), "{err:?}");
}

#[test]
fn an_unparseable_formula_is_a_located_error_cell_not_a_whole_file_refusal() {
    // GRID6: an unparseable formula is a per-cell LOCATED ERROR VALUE, not a whole-file failure. The
    // workbook LOADS; A1 resolves to `#NAME?`; B1 (which references A1) propagates the error (CORE2);
    // C1 (unrelated) still evaluates; and `lint`/`check` reports A1 with a non-zero (error) severity.
    let wb = load_one_tab(
        "Sheet1",
        &[("A1", "=SUM("), ("B1", "=A1+1"), ("C1", "=7*6")],
    );
    assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Name)); // A1 the located error
    assert_eq!(wb.value_at(0, 1, 0), Value::Error(ErrKind::Name)); // B1 propagates it (no crash)
    assert_eq!(wb.value_at(0, 2, 0), Value::Number(42.0)); // C1 unrelated, still evaluates
    let diags = wb.lint();
    assert!(
        diags
            .iter()
            .any(|d| d.code == Code::FormulaSyntax && d.code.severity() == crate::Severity::Error),
        "check must report the load-error cell with an error severity: {diags:?}"
    );
}

#[test]
fn load_dir_reads_folders_as_tabs() {
    // Round-trip through the filesystem loader: two tabs, a cross-sheet pull.
    let base = std::env::temp_dir().join(format!(
        "charlie-wb-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let inputs = base.join("Inputs");
    let summary = base.join("Summary");
    std::fs::create_dir_all(&inputs).unwrap();
    std::fs::create_dir_all(&summary).unwrap();
    std::fs::write(inputs.join("A1"), file("7")).unwrap();
    std::fs::write(summary.join("A1"), file("=Inputs!A1*6")).unwrap();

    let wb = Workbook::load_dir(&base)
        .expect("fs read ok")
        .expect("loads clean");
    assert_eq!(wb.sheet_names(), vec!["Inputs", "Summary"]);
    // Summary is tab index 1 (sorted: Inputs, Summary).
    assert_eq!(wb.value_at(1, 0, 0), Value::Number(42.0));

    std::fs::remove_dir_all(&base).ok();
}

/// Build a single-column chain `A1=A2(+1), A2=A3(+1), ..., A{len-1}=A{len}(+1)` with the bottom
/// cell `A{len}` a literal `0`. Each `+1` makes the top cell's value the chain length minus one
/// when it fully evaluates, so a computed answer proves the whole chain was walked.
fn chain_files(len: usize) -> Vec<(String, String)> {
    (0..len)
        .map(|i| {
            let name = format!("A{}", i + 1);
            let body = if i + 1 < len {
                format!("=A{}+1", i + 2)
            } else {
                "0".to_string()
            };
            (name, body)
        })
        .collect()
}

#[test]
fn a_legal_deep_chain_under_the_bound_computes_fully() {
    // A chain well within [`MAX_PULL_DEPTH`] evaluates end-to-end: the depth guard never fires on
    // a legal sheet, only on a pathologically deep one.
    let len = (MAX_PULL_DEPTH / 2) as usize; // comfortably under the bound
    let owned = chain_files(len);
    let refs: Vec<(&str, &str)> = owned
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_str()))
        .collect();
    let wb = load_one_tab("Sheet1", &refs);
    assert_eq!(wb.value_at(0, 0, 0), Value::Number((len - 1) as f64)); // A1
    assert!(
        wb.eval_diagnostics().is_empty(),
        "{:?}",
        wb.eval_diagnostics()
    );
}

#[test]
fn a_deep_acyclic_chain_refuses_instead_of_overflowing_the_stack() {
    // A finite, entirely acyclic chain deeper than the bound. The cycle detector never trips
    // (nothing is re-entered), so ONLY the pull-depth guard stands between the plan DFS and a
    // native stack overflow: reaching the assertions at all proves no SIGABRT. The deepest link is
    // a located #NUM!-class refusal that propagates up to the requested top cell.
    let len = (MAX_PULL_DEPTH as usize) + 64;
    let owned = chain_files(len);
    let refs: Vec<(&str, &str)> = owned
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_str()))
        .collect();
    let wb = load_one_tab("Sheet1", &refs);
    assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Num)); // A1
    let diags = wb.eval_diagnostics();
    assert!(
        diags.iter().any(|d| d.code == Code::DepthLimit),
        "{diags:?}"
    );
    // Never misclassified as a cycle: this chain has no cycle.
    assert!(!diags.iter().any(|d| d.code == Code::Cycle), "{diags:?}");
}

#[test]
fn check_over_a_workbook_containing_a_deep_chain_does_not_crash() {
    // `lint` drives EVERY cell, so a workbook that merely CONTAINS an over-deep chain must lint to
    // a located refusal rather than aborting the process.
    let len = (MAX_PULL_DEPTH as usize) + 8;
    let owned = chain_files(len);
    let refs: Vec<(&str, &str)> = owned
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_str()))
        .collect();
    let wb = load_one_tab("Sheet1", &refs);
    let diags = wb.lint();
    assert!(
        diags.iter().any(|d| d.code == Code::DepthLimit),
        "{diags:?}"
    );
}

#[test]
fn a_depth_refused_pull_does_not_poison_a_later_shallower_pull() {
    // Order-independence (never falsely reject a computable cell). One chain A1->A2->...->A320.
    // Pulling A1 FIRST refuses at depth 256 and propagates #NUM! up through A1..A256 -- but those
    // ancestor outcomes are depth-tainted and must NOT be memoized, so a LATER direct pull of A256
    // (whose own chain A256..A320 is only 65 links deep, legally computable) returns its real
    // value, not a cached #NUM!.
    let len = (MAX_PULL_DEPTH as usize) + 64; // 320
    let owned = chain_files(len);
    let refs: Vec<(&str, &str)> = owned
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_str()))
        .collect();
    let wb = load_one_tab("Sheet1", &refs);

    // Pull the deep top first: it refuses (its chain is 320 links).
    assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Num)); // A1

    // A256's own chain is short enough to compute: A256 = A257+1 = ... = A320(0) + 64 = 64.
    let a256 = wb.value_at(0, 0, 255); // A256 is column A (0), zero-based row 255
    assert_eq!(
        a256,
        Value::Number((len - 256) as f64),
        "call order poisoned a computable cell -- a depth-tainted outcome was memoized"
    );
}

#[test]
fn a_depth_tainted_range_is_not_frozen_into_the_arena() {
    // Order-independence for RANGE materialization -- the arena analogue of the per-cell memo
    // depth guard. An H-chain H1->..->H99->0 forwards to 0 and is read by `SUM(H1:H1)`. That range
    // is FIRST demanded from the bottom of a 200-deep A-chain (A1->..->A200 = `=SUM(H1:H1)`):
    // pulling A1 descends ~200 links, so materializing H1:H1 there pushes past MAX_PULL_DEPTH (256)
    // and would freeze a depth-tainted #NUM! into the H1:H1 rectangle. A LATER shallow
    // `B1 = SUM(H1:H1)` (H1:H1 reached only 99 links deep, legally computable) must recompute to 0.
    let mut owned: Vec<(String, String)> = Vec::new();
    let h_len = 99usize; // H-chain: forwarding, bottom literal 0 => H1 == 0, reached 99 links deep.
    for i in 0..h_len {
        let name = format!("H{}", i + 1);
        let body = if i + 1 < h_len {
            format!("=H{}", i + 2)
        } else {
            "0".to_string()
        };
        owned.push((name, body));
    }
    let a_len = 200usize; // A-chain: forwarding, bottom cell reads SUM(H1:H1) at ~200 links deep.
    for i in 0..a_len {
        let name = format!("A{}", i + 1);
        let body = if i + 1 < a_len {
            format!("=A{}", i + 2)
        } else {
            "=SUM(H1:H1)".to_string()
        };
        owned.push((name, body));
    }
    owned.push(("B1".to_string(), "=SUM(H1:H1)".to_string())); // the later SHALLOW demand.
    let refs: Vec<(&str, &str)> = owned
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_str()))
        .collect();
    let wb = load_one_tab("Sheet1", &refs);

    // Pull the DEEP A-chain first: H1:H1 is reached past the depth bound, tainting the range.
    assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Num)); // A1 (deep -> #NUM!)

    // The later SHALLOW pull: H1:H1 is only 99 links deep here, so SUM(H1:H1) computes to 0.
    assert_eq!(
        wb.value_at(0, 1, 0), // B1 = col 1, row 0
        Value::Number(0.0),
        "a depth-tainted range buffer was frozen into the arena and poisoned a shallow demand"
    );
}

#[test]
fn an_inverted_range_yields_the_same_rectangle() {
    // Corner order is not observable: a reversed spelling (`B2:A1`) resolves to the SAME rectangle
    // as its canonical form (`A1:B2`). This is the public contract (`RangeRef::normalized` owns the
    // corner-order rule); how the arena dedups the two keys is an internal the test does not pin.
    let wb = load_one_tab("Sheet1", &[("A1", "1")]);
    let normalized = RangeRef {
        start: CellRef {
            col: 0,
            row: 0,
            sheet: None,
        },
        end: CellRef {
            col: 1,
            row: 1,
            sheet: None,
        },
    };
    let inverted = RangeRef {
        start: CellRef {
            col: 1,
            row: 1,
            sheet: None,
        },
        end: CellRef {
            col: 0,
            row: 0,
            sheet: None,
        },
    };
    let normalized_cells = wb.range(normalized).cells.to_vec();
    // Same rectangle regardless of corner order.
    assert_eq!(wb.range(inverted).cells, normalized_cells.as_slice());
}

#[test]
fn a_reference_to_a_pathologically_large_range_refuses_instead_of_oom() {
    // =SUM(A2:ZZ100000) references ~70M empty cells. Materializing a Value per cell would drive an
    // OOM abort; the model caps the range, so the reference resolves to a located #NUM! rather
    // than allocating.
    let wb = load_one_tab("Sheet1", &[("A1", "=SUM(A2:ZZ100000)")]);
    assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Num)); // A1
    let diags = wb.eval_diagnostics();
    assert!(
        diags.iter().any(|d| d.code == Code::RangeTooLarge),
        "{diags:?}"
    );
    // The refusal is sheet-qualified to the offending formula file.
    assert!(
        diags.iter().any(|d| matches!(
            &d.loc,
            Loc::TabFile { tab, name } if tab == "Sheet1" && name == "A1"
        )),
        "range-too-large refusal must anchor on Sheet1/A1: {diags:?}"
    );
}

#[test]
fn a_range_at_the_materialization_bound_still_computes() {
    // A merely-large but valid range materializes: A1:A5 holds 1..5; =SUM(A1:A5) over 5 cells is
    // well under the bound -> 15.
    let wb = load_one_tab(
        "Sheet1",
        &[("A1:A5", "1\n2\n3\n4\n5"), ("C1", "=SUM(A1:A5)")],
    );
    assert_eq!(wb.value_at(0, 2, 0), Value::Number(15.0)); // C1
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn a_cross_sheet_cycle_is_located_to_the_sheet_qualified_file() {
    // Sheet1!A1 = Sheet2!A1 and Sheet2!A1 = Sheet1!A1 -- a cross-sheet cycle. The refusal must
    // name the TAB, not a bare `A1` (which exists on BOTH sheets and is otherwise untraceable).
    let wb = Workbook::from_tabs(&[
        ("Sheet1", &[("A1", &file("=Sheet2!A1"))]),
        ("Sheet2", &[("A1", &file("=Sheet1!A1"))]),
    ])
    .expect("loads clean");
    assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Ref)); // Sheet1!A1
    let diags = wb.eval_diagnostics();
    let cyc = diags
        .iter()
        .find(|d| d.code == Code::Cycle)
        .expect("a cycle diagnostic must fire");
    match &cyc.loc {
        Loc::TabFile { tab, name } => {
            assert!(tab == "Sheet1" || tab == "Sheet2", "unexpected tab {tab:?}");
            assert_eq!(name, "A1");
        }
        other => panic!("cross-sheet cycle must be sheet-qualified, got {other:?}"),
    }
}

#[test]
fn eval_formula_evaluates_an_ad_hoc_string_against_the_workbook() {
    // The `charlie-cli eval` entry: an ad-hoc formula string evaluates against the loaded workbook,
    // referencing stored cells. A clean value is `Value`; a spreadsheet error value is `Error`.
    let wb = load_one_tab("Sheet1", &[("A1", "6"), ("A2", "7")]);
    assert_eq!(
        wb.eval_formula(0, "A1*A2").unwrap(),
        FormulaOutcome::Value("42".to_string())
    );
    assert_eq!(
        wb.eval_formula(0, "1/0").unwrap(),
        FormulaOutcome::Error("#DIV/0!".to_string())
    );
    // A parse failure is a located refusal.
    assert!(wb.eval_formula(0, "SUM(").is_err());
}

#[test]
fn isformula_reads_the_grid_content_kind_over_the_real_resolver() {
    // A1 is a literal, B1 a formula (whose value errors), C1 a per-cell formula. ISFORMULA reads the
    // cell's CONTENT KIND through the model's `Resolver::is_formula`, never its value — so B1 reports
    // TRUE even though it evaluates to #DIV/0!, and a gap (Z9) reports FALSE.
    let wb = load_one_tab("Sheet1", &[("A1", "10"), ("B1", "=1/0"), ("C1", "=A1+1")]);
    assert_eq!(
        wb.eval_formula(0, "ISFORMULA(A1)").unwrap(),
        FormulaOutcome::Value("FALSE".to_string())
    );
    assert_eq!(
        wb.eval_formula(0, "ISFORMULA(B1)").unwrap(),
        FormulaOutcome::Value("TRUE".to_string())
    );
    assert_eq!(
        wb.eval_formula(0, "ISFORMULA(C1)").unwrap(),
        FormulaOutcome::Value("TRUE".to_string())
    );
    // A gap (no file claims Z9) is not a formula.
    assert_eq!(
        wb.eval_formula(0, "ISFORMULA(Z9)").unwrap(),
        FormulaOutcome::Value("FALSE".to_string())
    );
}

#[test]
fn isformula_reports_true_across_a_grid5_array_formula_region() {
    // A GRID5 array-formula region: the whole A1:A3 file is one `=SEQUENCE(3)`. Every coordinate of
    // the region reports as a formula (VAL1: one array-formula cell spanning its range).
    let wb = load_one_tab("Sheet1", &[("A1:A3", "=SEQUENCE(3)")]);
    assert_eq!(
        wb.eval_formula(0, "ISFORMULA(A1)").unwrap(),
        FormulaOutcome::Value("TRUE".to_string())
    );
    assert_eq!(
        wb.eval_formula(0, "ISFORMULA(A3)").unwrap(),
        FormulaOutcome::Value("TRUE".to_string())
    );
}

#[test]
fn a_shared_dependency_computes_once_across_a_batch_render() {
    // ENG3 sharing: a viewport demanded via `values_at` builds ONE merged graph, so a dependency
    // referenced by several viewport cells is computed once. A1=2 (shared base); B1=A1+1, C1=A1+1
    // (both read A1); the batch returns all three from one pass.
    let wb = load_one_tab("Sheet1", &[("A1", "2"), ("B1", "=A1+1"), ("C1", "=A1+1")]);
    let vals = wb.values_at(&[(0, 0, 0), (0, 1, 0), (0, 2, 0)]);
    assert_eq!(
        vals,
        vec![Value::Number(2.0), Value::Number(3.0), Value::Number(3.0)]
    );
    assert!(wb.eval_diagnostics().is_empty());
}

// ------------------------------------------------------------------------------------------
// NAIVE reference oracle + the ENG3 differential test (the graph EQUALS a naive per-cell eval).
// ------------------------------------------------------------------------------------------

/// An independent, dead-simple per-cell evaluator — the TEST-ONLY reference the differential test
/// grades the two-pass engine against. It evaluates a demanded cell straight from the grids by
/// native recursion through `charlie_ast::eval`, with a `visiting` set for basic cycle protection
/// and a tiny memo so a shared/diamond ancestor does not re-evaluate exponentially. It shares NONE
/// of the two-pass algorithm (no `DepGraph`, no `PlanNode`, no plan/`array_region_anchor` redirect,
/// no `topo_order`, no `fill_array_region`) — only the workbook's structural cell-location plumbing
/// (`covering` + the `LoadedFile`'s `region`/`array_formula`) and the `Arena` range-materialization
/// helper, which are not evaluation logic. Its verdict is VALUES; the differential test asserts
/// nothing about how either side reaches them.
///
/// REGION-AWARE (GRID5): a coordinate inside an array-formula region is NOT read from the region's
/// lone `1x1` grid (indexing it at a continuation offset would be out of bounds, and scalarizing the
/// anchor would demote its array to `#VALUE!`). Instead [`NaiveOracle::region_element`] re-derives the
/// engine's `fill_array_region` rules INDEPENDENTLY — evaluate the region's ONE formula and take its
/// element `(row-min_row, col-min_col)` row-major — so the dep_key sharing that collapses many region
/// coordinates onto one anchor node (exactly what the two-pass==naive test exists to prove) is graded
/// on VALUE, not merely on direct pins. A stored SINGLE-cell formula keeps its array's TOP-LEFT
/// element (`cell_top_left`, the engine's `cell_scalar` rule), NOT the in-expression `scalarize`.
///
/// NAMED COVERAGE BOUNDARY: this oracle has no pull-depth guard and unconditionally memoizes every
/// result, so it structurally cannot model the two-pass path's depth-tainted `#NUM!` — that value is
/// deliberately NOT memoized and is root-relative/order-dependent (see `MAX_PULL_DEPTH`,
/// `finish_pass`, and the range-materialization `#NUM!` bound). The differential cases here therefore
/// exercise only CLEAN-value shapes (diamond, deep-but-under-bound chain, cross-tab, shared range,
/// and GRID5 regions — shared upstream, multi-dependent, region-over-a-chain, and shape/scalar
/// `#SPILL!`). The memo/taint interactions the oracle can't represent — including a CYCLIC or
/// DEPTH-REFUSED region — are frozen separately by single-path `assert_eq` tests (`a_legal_deep_chain…`,
/// the `#NUM!` depth tests, the range-too-large test, `a_cyclic_region…`, `a_depth_refused_region…`) —
/// they are not graded against this oracle.
struct NaiveOracle<'w> {
    wb: &'w Workbook,
    cur: Cell<u32>,
    memo: RefCell<HashMap<CellKey, Value>>,
    visiting: RefCell<HashSet<CellKey>>,
    arena: Arena,
}

impl<'w> NaiveOracle<'w> {
    fn new(wb: &'w Workbook) -> NaiveOracle<'w> {
        NaiveOracle {
            wb,
            cur: Cell::new(0),
            memo: RefCell::new(HashMap::new()),
            visiting: RefCell::new(HashSet::new()),
            arena: Arena::default(),
        }
    }

    fn eval_cell(&self, sheet: u32, col: u32, row: u32) -> Value {
        self.value(CellRef {
            col,
            row,
            sheet: Some(SheetId(sheet)),
        })
    }

    /// Collapse a stored SINGLE-cell formula result to its scalar: a genuinely multi-cell array keeps
    /// only its TOP-LEFT element (the engine's `cell_scalar` GRID5/ENG6 implicit-intersection rule),
    /// re-derived here so the oracle shares no code with the engine's EVALUATE pass. A 1x1 array yields
    /// its single cell; an empty array is `Blank`; a scalar passes through. This is the CELL-position
    /// rule (never `#VALUE!`), deliberately NOT `charlie_ast::scalarize` (the in-expression rule).
    fn cell_top_left(v: Value) -> Value {
        match v {
            Value::Array(_, cells) => cells.into_iter().next().unwrap_or(Value::Blank),
            other => other,
        }
    }

    /// The value at one coordinate `key` of a GRID5 array-formula region — the INDEPENDENT reference
    /// model of the engine's `fill_array_region`. It evaluates the region's ONE formula (the covering
    /// file's lone grid cell) against the region's sheet and then applies the same three TOTAL rules the
    /// engine does, WITHOUT touching any engine plan/eval type (`DepGraph`/`PlanNode`/`array_region_anchor`/
    /// `fill_array_region`):
    /// * an array whose shape AND orientation match the region -> element `(row-min_row, col-min_col)`
    ///   row-major (a 1x1 region can't occur — a 1x1 file is never an array region, so its formula rides
    ///   the `cell_top_left` single-cell path above);
    /// * an error value -> that error at every coordinate;
    /// * anything else (a scalar, or a wrong-shaped/wrong-oriented array) -> a located `#SPILL!`.
    ///
    /// The `visiting` guard makes a self-referential region terminate as `#REF!` rather than loop
    /// (parity with the engine's terminal handling; cyclic/depth regions are graded by single-path
    /// tests, never this oracle).
    fn region_element(&self, id: FileId, file: &LoadedFile, key: CellKey) -> Value {
        let region = file.region;
        let rows = region.max_row - region.min_row + 1;
        let cols = region.max_col - region.min_col + 1;
        self.visiting.borrow_mut().insert(key);
        let prev = self.cur.replace(id.0);
        let value = match file.grid.cell_at(0, 0) {
            // Anchor no-arg ROW()/COLUMN() at the region's top-left, exactly as the engine's
            // `fill_array_region` does, so the differential stays faithful for a region formula.
            GridCell::Formula { expr, .. } => eval_at(expr, self, region.min_row, region.min_col),
            GridCell::Value(v) => v.clone(),
            GridCell::LoadError { diag, .. } => crate::grid::load_error_value(diag),
        };
        self.cur.set(prev);
        self.visiting.borrow_mut().remove(&key);

        let r_off = key.2 - region.min_row;
        let c_off = key.1 - region.min_col;
        match value {
            Value::Array(shape, cells) if shape.rows == rows && shape.cols == cols => {
                let idx = (r_off * cols + c_off) as usize;
                cells.into_iter().nth(idx).unwrap_or(Value::Blank)
            }
            Value::Error(k) => Value::Error(k),
            _ => Value::Error(ErrKind::Spill),
        }
    }
}

impl Resolver for NaiveOracle<'_> {
    fn value(&self, cell: CellRef) -> Value {
        let sheet = cell.sheet.map_or_else(|| self.cur.get(), |SheetId(i)| i);
        let key = (sheet, cell.col, cell.row);
        if let Some(v) = self.memo.borrow().get(&key) {
            return v.clone();
        }
        if self.visiting.borrow().contains(&key) {
            return Value::Error(ErrKind::Ref); // basic cycle protection
        }
        let Some((id, file)) = self.wb.covering(sheet, cell.col, cell.row) else {
            return Value::Blank;
        };
        // GRID5: a coordinate inside an array-formula region takes the region's ONE formula's element
        // (row-min_row, col-min_col), re-derived INDEPENDENTLY (never the engine's fill_array_region /
        // plan redirect). Every non-region cell reads its own grid cell as before.
        let v = if file.array_formula {
            self.region_element(id, file, key)
        } else {
            let dr = cell.row - file.region.min_row;
            let dc = cell.col - file.region.min_col;
            match file.grid.cell_at(dr, dc) {
                GridCell::Value(v) => v.clone(),
                // GRID6: a load-error cell resolves to its located error value, re-derived
                // independently (parity with the engine's resolver arm).
                GridCell::LoadError { diag, .. } => crate::grid::load_error_value(diag),
                GridCell::Formula { expr, .. } => {
                    self.visiting.borrow_mut().insert(key);
                    let prev = self.cur.replace(id.0);
                    // ENG3: the naive oracle evaluates the SAME effective (forge-rewritten) expr the
                    // two-pass engine does, so the differential proves the graph equals a per-cell eval
                    // OVER the resolved references. The forge Pass 0 already ran (the caller's
                    // `values_at` demanded these cells before the oracle re-evaluates), so a forger cell
                    // reads its static rewrite here too. Zero effect when the workbook has no forgers.
                    let eff = self.wb.effective_expr(key, expr);
                    // A stored single-cell formula keeps its array's TOP-LEFT element (the engine's
                    // `cell_scalar` rule), re-derived here — NOT the in-expression `scalarize` (#VALUE!).
                    // Anchor no-arg ROW()/COLUMN() at this cell (parity with `compute_formula`).
                    let r = Self::cell_top_left(eval_at(eff, self, cell.row, cell.col));
                    self.cur.set(prev);
                    self.visiting.borrow_mut().remove(&key);
                    r
                }
            }
        };
        self.memo.borrow_mut().insert(key, v.clone());
        v
    }

    fn range(&self, range: RangeRef) -> ArrayView<'_> {
        let eff = SheetId(cell_sheet(range.start.sheet, self.cur.get()));
        let norm = range.normalized();
        let (c0, c1) = (norm.start.col, norm.end.col);
        let (r0, r1) = (norm.start.row, norm.end.row);
        let (rows, cols) = (r1 - r0 + 1, c1 - c0 + 1);
        let key = RangeRef {
            start: CellRef {
                col: c0,
                row: r0,
                sheet: Some(eff),
            },
            end: CellRef {
                col: c1,
                row: r1,
                sheet: Some(eff),
            },
        };
        if let Some(view) = self.arena.get(key) {
            return view;
        }
        let mut buf = Vec::with_capacity((rows as usize) * (cols as usize));
        for r in r0..=r1 {
            for c in c0..=c1 {
                buf.push(self.value(CellRef {
                    col: c,
                    row: r,
                    sheet: Some(eff),
                }));
            }
        }
        self.arena.insert(key, Shape { rows, cols }, buf)
    }

    fn sheet_id(&self, name: &str) -> Option<SheetId> {
        self.wb.sheet_id(name)
    }

    fn now_serial(&self) -> f64 {
        self.wb.now
    }
}

fn cell_sheet(sheet: Option<SheetId>, home: u32) -> u32 {
    sheet.map_or(home, |SheetId(i)| i)
}

/// Assert the naive oracle and the two-pass engine agree on the VALUE of every demanded cell —
/// values only, never the graph's shape/node-count/traversal order (asserting internals would
/// freeze "how" and block a future parallel-execution refactor).
fn assert_agrees(wb: &Workbook, cells: &[(u32, u32, u32)]) {
    let oracle = NaiveOracle::new(wb);
    // Interleave demands so the two-pass memo/arena is exercised across cells, and evaluate the
    // batch through the merged-graph path too.
    let batch = wb.values_at(cells);
    for (&(s, c, r), two_pass) in cells.iter().zip(batch) {
        let naive = oracle.eval_cell(s, c, r);
        assert_eq!(
            naive, two_pass,
            "naive vs two-pass diverge at (sheet {s}, col {c}, row {r}): \
             naive={naive:?} two_pass={two_pass:?}"
        );
    }
}

#[test]
fn differential_diamond_shared_ancestor() {
    // A diamond: one shared base A1 reached by many cells and by a two-path top. B1,C1 both read
    // A1; D1 reads B1 and C1 (A1 via two paths); wide fan-out E1,F1 also read A1.
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1", "3"),
            ("B1", "=A1+1"),
            ("C1", "=A1*2"),
            ("D1", "=B1+C1"),
            ("E1", "=A1*10"),
            ("F1", "=A1-1+E1"),
        ],
    );
    assert_agrees(
        &wb,
        &[
            (0, 0, 0),
            (0, 1, 0),
            (0, 2, 0),
            (0, 3, 0),
            (0, 4, 0),
            (0, 5, 0),
        ],
    );
}

#[test]
fn differential_deep_linear_chain() {
    // A deep (but under-bound) linear chain shared by a top demand and interior demands.
    let len = 60usize;
    let owned = chain_files(len); // A1=A2+1, ..., A60=0
    let refs: Vec<(&str, &str)> = owned
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_str()))
        .collect();
    let wb = load_one_tab("Sheet1", &refs);
    assert_agrees(&wb, &[(0, 0, 0), (0, 0, 29), (0, 0, 59), (0, 0, 10)]);
}

#[test]
fn differential_cross_tab_shared_ancestor() {
    // A cross-tab shared ancestor: Base!A1 feeds several formulas on two other tabs, and a cell
    // that combines both tabs (a cross-tab diamond bottoming out at Base!A1).
    let wb = Workbook::from_tabs(&[
        ("Base", &[("A1", &file("100"))]),
        (
            "R1",
            &[("A1", &file("=Base!A1*2")), ("A2", &file("=Base!A1+A1"))],
        ),
        (
            "R2",
            &[
                ("A1", &file("=Base!A1-10")),
                ("A2", &file("=R1!A1+R2!A1+Base!A1")),
            ],
        ),
    ])
    .expect("loads clean");
    assert_agrees(
        &wb,
        &[
            (0, 0, 0), // Base!A1
            (1, 0, 0), // R1!A1
            (1, 0, 1), // R1!A2
            (2, 0, 0), // R2!A1
            (2, 0, 1), // R2!A2 (combines both tabs + Base)
        ],
    );
}

#[test]
fn differential_one_large_range_aggregated_by_several_cells() {
    // One large shared range (a 100-cell column) aggregated by several cells: SUM, an offset SUM,
    // and a formula that reads two aggregates. Every aggregate shares the SAME 100 ancestor cells.
    let column: String = (1..=100)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1:A100", column.as_str()),
            ("C1", "=SUM(A1:A100)"),
            ("C2", "=SUM(A1:A100)+1"),
            ("C3", "=C1+C2"),
            ("C4", "=AVERAGE(A1:A100)"),
        ],
    );
    assert_agrees(
        &wb,
        &[(0, 2, 0), (0, 2, 1), (0, 2, 2), (0, 2, 3), (0, 0, 49)],
    );
}

// ------------------------------------------------------------------------------------------
// GRID5 — array-formula regions (a range file whose whole content is one =formula).
// ------------------------------------------------------------------------------------------

#[test]
fn sort_region_fills_its_range_sorted() {
    // A1:A3 = {3;1;2} (three literal cells); C1:C3 is a SINGLE `=SORT(A1:A3)` filling the 3x1 range.
    let wb = load_one_tab("Sheet1", &[("A1:A3", "3\n1\n2"), ("C1:C3", "=SORT(A1:A3)")]);
    assert_eq!(wb.value_at(0, 2, 0), Value::Number(1.0)); // C1
    assert_eq!(wb.value_at(0, 2, 1), Value::Number(2.0)); // C2
    assert_eq!(wb.value_at(0, 2, 2), Value::Number(3.0)); // C3
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn unique_region_over_a_column_with_dups() {
    // A1:A5 = {5;5;7;5;7}; C1:C2 = UNIQUE(A1:A5) -> the two distinct values {5;7} in first-seen order.
    let wb = load_one_tab(
        "Sheet1",
        &[("A1:A5", "5\n5\n7\n5\n7"), ("C1:C2", "=UNIQUE(A1:A5)")],
    );
    assert_eq!(wb.value_at(0, 2, 0), Value::Number(5.0)); // C1
    assert_eq!(wb.value_at(0, 2, 1), Value::Number(7.0)); // C2
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn sequence_region_generates_its_counter() {
    // C1:C3 = SEQUENCE(3) -> {1;2;3}, a region with no external dependency.
    let wb = load_one_tab("Sheet1", &[("C1:C3", "=SEQUENCE(3)")]);
    assert_eq!(wb.value_at(0, 2, 0), Value::Number(1.0));
    assert_eq!(wb.value_at(0, 2, 1), Value::Number(2.0));
    assert_eq!(wb.value_at(0, 2, 2), Value::Number(3.0));
}

#[test]
fn transpose_region_fills_the_transposed_orientation() {
    // A1:C1 = {1,2,3} (a 1x3 row); C3:C5... actually transpose to a 3x1 column region E1:E3.
    let wb = load_one_tab(
        "Sheet1",
        &[("A1:C1", "1\t2\t3"), ("E1:E3", "=TRANSPOSE(A1:C1)")],
    );
    assert_eq!(wb.value_at(0, 4, 0), Value::Number(1.0)); // E1
    assert_eq!(wb.value_at(0, 4, 1), Value::Number(2.0)); // E2
    assert_eq!(wb.value_at(0, 4, 2), Value::Number(3.0)); // E3
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn a_shape_mismatch_region_is_a_located_dimension_error() {
    // C1:C2 (2x1) holds `=SORT(A1:A3)` whose value is 3x1 — wrong shape. Every coordinate is #SPILL!
    // and a located dimension error (GRID4's code) is recorded (GRID5, detected AT EVALUATION).
    let wb = load_one_tab("Sheet1", &[("A1:A3", "3\n1\n2"), ("C1:C2", "=SORT(A1:A3)")]);
    assert_eq!(wb.value_at(0, 2, 0), Value::Error(ErrKind::Spill)); // C1
    assert_eq!(wb.value_at(0, 2, 1), Value::Error(ErrKind::Spill)); // C2
    let diags = wb.eval_diagnostics();
    assert!(
        diags.iter().any(|d| d.code == Code::DimensionMismatch),
        "{diags:?}"
    );
}

#[test]
fn a_scalar_in_a_range_region_is_a_located_dimension_error() {
    // C1:C3 holds `=SUM(A1:A3)` — a SCALAR in a >1 range. A scalar cannot fill a range: #SPILL!.
    let wb = load_one_tab("Sheet1", &[("A1:A3", "3\n1\n2"), ("C1:C3", "=SUM(A1:A3)")]);
    assert_eq!(wb.value_at(0, 2, 0), Value::Error(ErrKind::Spill));
    assert!(
        wb.eval_diagnostics()
            .iter()
            .any(|d| d.code == Code::DimensionMismatch)
    );
}

#[test]
fn a_one_cell_array_formula_keeps_the_top_left_element() {
    // A 1x1 file holding an array formula is NOT a region (its range spans one coordinate); it keeps
    // only the array's TOP-LEFT element (implicit intersection, GRID5): =SORT(A1:A3) -> 1.
    let wb = load_one_tab("Sheet1", &[("A1:A3", "3\n1\n2"), ("E1", "=SORT(A1:A3)")]);
    assert_eq!(wb.value_at(0, 4, 0), Value::Number(1.0)); // E1 -> top-left of the sorted array
}

#[test]
fn a_coordinate_reference_into_a_region_resolves_to_its_element() {
    // C1:C3 = SORT(A1:A3) = {1;2;3}; D1 = `=C2` resolves to the region's (1,0) element = 2 (ENG2/ENG3:
    // the region computes once and a reference into it reads the filled coordinate).
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1:A3", "3\n1\n2"),
            ("C1:C3", "=SORT(A1:A3)"),
            ("D1", "=C2"),
        ],
    );
    assert_eq!(wb.value_at(0, 3, 0), Value::Number(2.0)); // D1 -> C2
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn a_region_whose_input_is_a_formula_chain_computes_once_and_correctly() {
    // A1:A3 = {3;1;2}; B1:B3 is a per-cell formula grid (=A1+10 ...) = {13;11;12}; C1:C3 = SORT(B1:B3)
    // = {11;12;13}. The region's input is itself computed formulas — the two-pass engine computes each
    // once (ENG2) and the sorted region is correct; a reference into the region also reads it.
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1:A3", "3\n1\n2"),
            ("B1:B3", "=A1+10\n=A2+10\n=A3+10"),
            ("C1:C3", "=SORT(B1:B3)"),
            ("D1", "=C1+C3"),
        ],
    );
    assert_eq!(wb.value_at(0, 2, 0), Value::Number(11.0)); // C1
    assert_eq!(wb.value_at(0, 2, 1), Value::Number(12.0)); // C2
    assert_eq!(wb.value_at(0, 2, 2), Value::Number(13.0)); // C3
    assert_eq!(wb.value_at(0, 3, 0), Value::Number(24.0)); // D1 = 11 + 13
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn a_single_literal_in_a_multi_cell_range_stays_a_grid4_dimension_error() {
    // Disambiguation: a lone LITERAL (`5`) in a >1 range is NOT a GRID5 region — only a lone =formula
    // triggers GRID5. It stays a GRID4 dimension error at LOAD.
    let err = Workbook::from_tabs(&[("Sheet1", &[("C1:C3", "5")])]).unwrap_err();
    assert!(
        err.iter().any(|d| d.code == Code::DimensionMismatch),
        "{err:?}"
    );
}

#[test]
fn a_cyclic_region_is_a_located_ref_at_every_coordinate() {
    // GRID5 region on a REFERENCE CYCLE: C1:C3 = `=SORT(C1:C3)` references itself. The region's ONE
    // formula never runs (its anchor is a cycle terminal), so every coordinate — INCLUDING the
    // continuation cells the anchor's array would have filled — must resolve to a located #REF! (ENG2),
    // never an out-of-bounds read of the region's 1x1 grid (the CORE2 major this pins). Demand ONLY the
    // continuation cells in a single batched pass, so the resolver must read them from the pass results
    // rather than a memo hit seeded by an earlier anchor demand.
    let wb = load_one_tab("Sheet1", &[("C1:C3", "=SORT(C1:C3)")]);
    let continuations = wb.values_at(&[(0, 2, 1), (0, 2, 2)]); // C2, C3 — continuation cells only
    assert_eq!(
        continuations,
        vec![Value::Error(ErrKind::Ref), Value::Error(ErrKind::Ref)]
    );
    assert_eq!(wb.value_at(0, 2, 0), Value::Error(ErrKind::Ref)); // C1 (the anchor)
    let diags = wb.eval_diagnostics();
    assert!(diags.iter().any(|d| d.code == Code::Cycle), "{diags:?}");
}

#[test]
fn a_cross_reference_into_a_cyclic_region_propagates_the_ref() {
    // A dependent formula (D1 = `=C3`) referencing a CONTINUATION coordinate of a cyclic region reads
    // the located #REF! that fills every region coordinate, rather than tripping the grid fall-through.
    let wb = load_one_tab("Sheet1", &[("C1:C3", "=SORT(C1:C3)"), ("D1", "=C3")]);
    assert_eq!(wb.value_at(0, 3, 0), Value::Error(ErrKind::Ref)); // D1 -> C3 (continuation) -> #REF!
    assert!(wb.eval_diagnostics().iter().any(|d| d.code == Code::Cycle));
}

#[test]
fn a_depth_refused_region_is_a_located_num_at_every_coordinate() {
    // GRID5 region reached past the pull-depth bound AS A DEPENDENCY: a chain X1->..->X256 whose
    // deepest link references a CONTINUATION coordinate (C3) of the region C1:C3 = SEQUENCE(3). Planning
    // reaches the region's anchor at depth MAX_PULL_DEPTH and refuses it (DepthRefused) BEFORE its
    // formula runs, so every region coordinate must be filled with a located #NUM! — reading C3 deep
    // must not fall through to an out-of-bounds grid read (CORE2) — and the refusal propagates up the
    // chain. `len == MAX_PULL_DEPTH` places the deepest chain cell (X256) at depth 255 (itself
    // computable) and the region anchor it references at depth 256 (refused).
    let len = MAX_PULL_DEPTH as usize;
    let mut owned: Vec<(String, String)> = vec![("C1:C3".to_string(), "=SEQUENCE(3)".to_string())];
    for i in 0..len {
        let name = format!("X{}", i + 1);
        let body = if i + 1 < len {
            format!("=X{}", i + 2)
        } else {
            "=C3".to_string() // the deepest link references the region's continuation coordinate
        };
        owned.push((name, body));
    }
    let refs: Vec<(&str, &str)> = owned
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_str()))
        .collect();
    let wb = load_one_tab("Sheet1", &refs);
    // X1 is column X (index 23), row 1 (zero-based row 0); the deep chain propagates the region's #NUM!.
    assert_eq!(wb.value_at(0, 23, 0), Value::Error(ErrKind::Num));
    let diags = wb.eval_diagnostics();
    assert!(
        diags.iter().any(|d| d.code == Code::DepthLimit),
        "{diags:?}"
    );
    // The region was refused as a depth limit, never misclassified as a cycle.
    assert!(!diags.iter().any(|d| d.code == Code::Cycle), "{diags:?}");
}

#[test]
fn render_values_and_functions_over_a_region() {
    use crate::render::{RenderMode, parse_viewport, render};
    let wb = load_one_tab("Sheet1", &[("A1:A3", "3\n1\n2"), ("C1:C3", "=SORT(A1:A3)")]);
    let vp = parse_viewport("C1:C3").unwrap();
    // --values: each coordinate its own element.
    let vals = render(&wb, 0, vp, RenderMode::Values);
    let col: Vec<&str> = vals.rows.iter().map(|r| r.cells[0].as_str()).collect();
    assert_eq!(col, vec!["1", "2", "3"]);
    // --functions: the anchor shows the array formula; continuation cells show the caret marker.
    let fns = render(&wb, 0, vp, RenderMode::Functions);
    let col: Vec<&str> = fns.rows.iter().map(|r| r.cells[0].as_str()).collect();
    assert_eq!(col, vec!["=SORT(A1:A3)", "^", "^"]);
}

// ------------------------------------------------------------------------------------------
// GRID5 region DIFFERENTIAL cases (naive == two-pass over the dep_key region sharing) — these
// route through the REGION-AWARE `NaiveOracle`, so the sharing that collapses many region
// coordinates onto one anchor node is graded on VALUE, not merely on direct pins.
// ------------------------------------------------------------------------------------------

#[test]
fn differential_region_shares_its_input_range_with_another_cell() {
    // (a) SHARED UPSTREAM: the region's input range A1:A3 is ALSO read by E1=SUM(A1:A3). The plan
    // merges the region node's deps and E1's deps onto the same A-cells; naive==two-pass proves the
    // shared input is evaluated identically whether pulled by the region or by the ordinary formula.
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1:A3", "3\n1\n2"),
            ("C1:C3", "=SORT(A1:A3)"), // region over the shared range
            ("E1", "=SUM(A1:A3)"),     // another cell reading the SAME range
        ],
    );
    assert_agrees(
        &wb,
        &[
            (0, 0, 0), // A1
            (0, 0, 1), // A2
            (0, 0, 2), // A3
            (0, 2, 0), // C1 (region anchor)
            (0, 2, 1), // C2 (continuation)
            (0, 2, 2), // C3 (continuation)
            (0, 4, 0), // E1 = SUM(A1:A3), shares the region's input range
        ],
    );
}

#[test]
fn differential_region_read_by_multiple_dependents() {
    // (b) MULTIPLE DEPENDENTS reading INTO one region: D1=C1, D2=C3, D3=C1+C3 all reference region
    // coordinates of C1:C3=SORT(A1:A3)={1;2;3}. `dep_key` redirects EACH referenced coordinate onto the
    // ONE anchor node (C1, C3 and both-in-C3+C1 collapse to the C1 anchor), so the region computes once
    // and each dependent must see the RIGHT element (D1->1, D2->3, D3->4). Grading D1/D2/D3 on VALUE is
    // exactly the anchor-collapse the two-pass==naive test exists to prove.
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1:A3", "3\n1\n2"),
            ("C1:C3", "=SORT(A1:A3)"),
            ("D1", "=C1"),    // reads the anchor coordinate
            ("D2", "=C3"),    // reads a continuation coordinate
            ("D3", "=C1+C3"), // reads two coordinates of the same region
        ],
    );
    assert_agrees(
        &wb,
        &[
            (0, 2, 0), // C1
            (0, 2, 1), // C2
            (0, 2, 2), // C3
            (0, 3, 0), // D1 -> C1 = 1
            (0, 3, 1), // D2 -> C3 = 3
            (0, 3, 2), // D3 -> C1 + C3 = 4
        ],
    );
}

#[test]
fn differential_region_over_a_formula_chain() {
    // (c) REGION OVER A FORMULA CHAIN: the region's input B1:B3 is itself a per-cell formula grid
    // reading a further range A1:A3 (a chain A -> B -> the region), and D1 reads two region coordinates.
    // The two-pass engine computes each B once (ENG2) and fills the sorted region; naive==two-pass proves
    // the chained-input region agrees element for element.
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1:A3", "3\n1\n2"),
            ("B1:B3", "=A1+10\n=A2+10\n=A3+10"), // {13;11;12}
            ("C1:C3", "=SORT(B1:B3)"),           // {11;12;13}
            ("D1", "=C1+C3"),                    // 24
        ],
    );
    assert_agrees(
        &wb,
        &[
            (0, 0, 0), // A1
            (0, 1, 0), // B1
            (0, 1, 2), // B3
            (0, 2, 0), // C1
            (0, 2, 1), // C2
            (0, 2, 2), // C3
            (0, 3, 0), // D1 = C1 + C3
        ],
    );
}

#[test]
fn differential_shape_mismatch_and_scalar_regions_agree_on_the_spill() {
    // (d) SHAPE-MISMATCH and SCALAR regions: C1:C2 (2x1) holds a 3x1 SORT (wrong shape) and G1:G3 holds
    // a SUM (a scalar) — both fill every coordinate with a located `#SPILL!`. The region-aware oracle
    // re-derives the SAME located error, so naive==two-pass on the refusal shape too (not only clean
    // values).
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1:A3", "3\n1\n2"),
            ("C1:C2", "=SORT(A1:A3)"), // 3x1 array into a 2x1 range -> #SPILL!
            ("G1:G3", "=SUM(A1:A3)"),  // a scalar into a 3x1 range -> #SPILL!
        ],
    );
    assert_agrees(
        &wb,
        &[
            (0, 2, 0), // C1 -> #SPILL!
            (0, 2, 1), // C2 -> #SPILL!
            (0, 6, 0), // G1 -> #SPILL!
            (0, 6, 1), // G2 -> #SPILL!
            (0, 6, 2), // G3 -> #SPILL!
        ],
    );
    // Both paths also surface the located dimension error (GRID5, detected at evaluation).
    assert!(
        wb.eval_diagnostics()
            .iter()
            .any(|d| d.code == Code::DimensionMismatch)
    );
}

// ------------------------------------------------------------------------------------------
// Computation hash (the ENG4 primitive) — determinism, sensitivity, cycle=None, VAL1, GRID5.
// ------------------------------------------------------------------------------------------

#[test]
fn computation_hash_is_deterministic() {
    // Two identical workbooks (and repeated calls) yield the SAME opaque digest — a deterministic,
    // content-only function (ENG4). A clean cell has a hash.
    let build = || load_one_tab("Sheet1", &[("A1", "1"), ("B1", "=A1+1"), ("C1", "=B1*10")]);
    let a = build();
    let b = build();
    assert!(a.computation_hash(0, 2, 0).is_some()); // C1 has a hash
    assert_eq!(a.computation_hash(0, 2, 0), b.computation_hash(0, 2, 0)); // across workbooks
    assert_eq!(a.computation_hash(0, 2, 0), a.computation_hash(0, 2, 0)); // repeated call, stable
}

#[test]
fn computation_hash_is_sensitive_upstream_and_isolates_the_unrelated() {
    // Editing an UPSTREAM cell changes the dependent's hash; an UNRELATED cell's hash is unchanged.
    let wb1 = load_one_tab("Sheet1", &[("A1", "1"), ("B1", "=A1+1"), ("Z1", "99")]);
    let wb2 = load_one_tab("Sheet1", &[("A1", "2"), ("B1", "=A1+1"), ("Z1", "99")]);
    // The literal A1 itself changed.
    assert_ne!(wb1.computation_hash(0, 0, 0), wb2.computation_hash(0, 0, 0));
    // B1 depends on A1 -> its hash changes even though B1's own text is identical.
    assert_ne!(wb1.computation_hash(0, 1, 0), wb2.computation_hash(0, 1, 0));
    // Z1 is unrelated -> unchanged.
    assert_eq!(
        wb1.computation_hash(0, 25, 0),
        wb2.computation_hash(0, 25, 0)
    );
}

#[test]
fn a_cyclic_cell_has_no_computation_hash() {
    // A cell on a reference cycle has NO computation hash (ENG4), mirroring the plan's Cycle terminal;
    // a clean literal alongside still hashes.
    let wb = load_one_tab("Sheet1", &[("A1", "=B1"), ("B1", "=A1"), ("C1", "7")]);
    assert_eq!(wb.computation_hash(0, 0, 0), None); // A1 (on the cycle)
    assert_eq!(wb.computation_hash(0, 1, 0), None); // B1 (on the cycle)
    assert!(wb.computation_hash(0, 2, 0).is_some()); // C1 (clean literal)
}

#[test]
fn same_formula_over_same_refs_hashes_the_same_regardless_of_position() {
    // VAL1: the hash is over CONTENT, never the cell's own address. `=A1+A2` in B5 and in Z9 read the
    // SAME refs, so they share one digest.
    let wb = load_one_tab(
        "Sheet1",
        &[("A1", "3"), ("A2", "4"), ("B5", "=A1+A2"), ("Z9", "=A1+A2")],
    );
    let hb = wb.computation_hash(0, 1, 4); // B5 (col B=1, row 5 -> zero-based 4)
    let hz = wb.computation_hash(0, 25, 8); // Z9 (col Z=25, row 9 -> zero-based 8)
    assert!(hb.is_some());
    assert_eq!(hb, hz);
}

#[test]
fn a_region_members_hash_is_its_anchors() {
    // GRID5: a region member's hash is its anchor's (ONE computation, VAL1/ENG3).
    let wb = load_one_tab("Sheet1", &[("A1:A3", "3\n1\n2"), ("C1:C3", "=SORT(A1:A3)")]);
    let anchor = wb.computation_hash(0, 2, 0); // C1 (anchor)
    assert!(anchor.is_some());
    assert_eq!(wb.computation_hash(0, 2, 1), anchor); // C2 (member)
    assert_eq!(wb.computation_hash(0, 2, 2), anchor); // C3 (member)
}

#[test]
fn a_cell_downstream_of_a_cycle_has_no_computation_hash() {
    // A `None` propagates upward (ENG4): a clean formula D1 = C1 whose dependency C1 is on a reference
    // cycle (A1=B1, B1=A1, C1=A1) inherits the cycle's missing digest -> `None`, even though D1's own
    // text is a plain reference. An unrelated literal alongside still hashes.
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1", "=B1"),
            ("B1", "=A1"),
            ("C1", "=A1"), // reads a cycle member -> no digest
            ("D1", "=C1"), // transitively downstream of the cycle -> no digest either
            ("E1", "42"),
        ],
    );
    assert_eq!(wb.computation_hash(0, 2, 0), None); // C1
    assert_eq!(wb.computation_hash(0, 3, 0), None); // D1 (downstream of the cycle)
    assert!(wb.computation_hash(0, 4, 0).is_some()); // E1 (unrelated literal)
}

#[test]
fn a_depth_tainted_cell_has_no_computation_hash() {
    // A chain deeper than [`MAX_PULL_DEPTH`] is depth-tainted: the digest walk hits the pull-depth
    // bound and yields `None`, which propagates up to the requested top cell (ENG4, mirroring the
    // plan's DepthRefused terminal). The bottom literal, reached far shallower, still hashes.
    let len = (MAX_PULL_DEPTH as usize) + 64;
    let owned = chain_files(len);
    let refs: Vec<(&str, &str)> = owned
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_str()))
        .collect();
    let wb = load_one_tab("Sheet1", &refs);
    assert_eq!(wb.computation_hash(0, 0, 0), None); // A1 (top of an over-deep chain)
    assert!(wb.computation_hash(0, 0, (len - 1) as u32).is_some()); // A{len} (the bottom literal)
}

// ------------------------------------------------------------------------------------------
// trace (CLI2) — upstream/downstream, shared-dep (repeated), cycle, depth cap, GRID5 region.
// ------------------------------------------------------------------------------------------

#[test]
fn trace_upstream_shows_a_shared_dependency_once_as_repeated() {
    // A diamond: D1 reads B1 and C1, both read A1. A1 appears fully once and `repeated` once (ENG3
    // sharing; no diamond blow-up).
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1", "2"),
            ("B1", "=A1+1"),
            ("C1", "=A1+1"),
            ("D1", "=B1+C1"),
        ],
    );
    let root = wb.trace(0, 3, 0, Direction::Upstream, None).unwrap();
    assert_eq!(root.cell, "Sheet1!D1");
    assert_eq!(root.status, TraceStatus::Ok);
    assert!(root.hash.is_some());
    // Children B1, C1 (sorted by key), each with one A1 child.
    assert_eq!(root.children.len(), 2);
    for c in &root.children {
        assert_eq!(c.children[0].cell, "Sheet1!A1");
    }
    // Exactly one of the two A1 nodes is the repeated (shared) one.
    let repeated = root
        .children
        .iter()
        .filter(|c| c.children[0].repeated)
        .count();
    assert_eq!(repeated, 1);
}

#[test]
fn trace_upstream_reports_a_cycle_without_looping() {
    // A1 = B1, B1 = A1. The walk reports the cycle (status Cycle, no hash), never loops (ENG2).
    let wb = load_one_tab("Sheet1", &[("A1", "=B1"), ("B1", "=A1")]);
    let root = wb.trace(0, 0, 0, Direction::Upstream, None).unwrap();
    assert_eq!(root.cell, "Sheet1!A1");
    assert_eq!(root.status, TraceStatus::Cycle);
    assert_eq!(root.hash, None);
    let b1 = &root.children[0];
    assert_eq!(b1.cell, "Sheet1!B1");
    assert_eq!(b1.status, TraceStatus::Cycle);
    let back = &b1.children[0];
    assert_eq!(back.cell, "Sheet1!A1");
    assert_eq!(back.status, TraceStatus::Cycle);
    assert!(back.children.is_empty()); // the back-edge is not re-descended
}

#[test]
fn trace_downstream_is_the_relation_transposed() {
    // A1 is read by B1 and C1; B1 is read by D1. Downstream from A1 is the transposed dependency map.
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1", "2"),
            ("B1", "=A1+1"),
            ("C1", "=A1*3"),
            ("D1", "=B1+1"),
        ],
    );
    let root = wb.trace(0, 0, 0, Direction::Downstream, None).unwrap();
    assert_eq!(root.cell, "Sheet1!A1");
    assert_eq!(root.status, TraceStatus::Literal);
    let consumers: Vec<&str> = root.children.iter().map(|c| c.cell.as_str()).collect();
    assert_eq!(consumers, vec!["Sheet1!B1", "Sheet1!C1"]);
    // B1's consumer is D1; C1 has none.
    assert_eq!(root.children[0].children[0].cell, "Sheet1!D1");
    assert!(root.children[1].children.is_empty());
}

#[test]
fn trace_respects_a_depth_cap() {
    // A chain C1 -> B1 -> A1 capped at depth 1: C1 (depth 0), B1 (depth 1, not descended).
    let wb = load_one_tab("Sheet1", &[("A1", "1"), ("B1", "=A1+1"), ("C1", "=B1+1")]);
    let root = wb.trace(0, 2, 0, Direction::Upstream, Some(1)).unwrap();
    assert_eq!(root.cell, "Sheet1!C1");
    assert_eq!(root.children.len(), 1);
    assert_eq!(root.children[0].cell, "Sheet1!B1");
    assert!(root.children[0].children.is_empty());
}

#[test]
fn trace_into_and_out_of_a_grid5_region() {
    // C1:C3 = SORT(A1:A3) = {1;2;3}; D1 = C1+C3 references two region coordinates that COLLAPSE onto
    // the single anchor node (dep_key). Upstream from D1 reaches the region anchor once, then A1:A3;
    // downstream from A1 reaches the region then D1.
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1:A3", "3\n1\n2"),
            ("C1:C3", "=SORT(A1:A3)"),
            ("D1", "=C1+C3"),
        ],
    );
    let up = wb.trace(0, 3, 0, Direction::Upstream, None).unwrap();
    assert_eq!(up.cell, "Sheet1!D1");
    assert_eq!(up.value, "4"); // C1 + C3 = 1 + 3
    assert_eq!(up.children.len(), 1); // both C-coordinates collapse to the one anchor
    let region = &up.children[0];
    assert_eq!(region.cell, "Sheet1!C1");
    assert_eq!(region.formula.as_deref(), Some("=SORT(A1:A3)"));
    let deps: Vec<&str> = region.children.iter().map(|c| c.cell.as_str()).collect();
    assert_eq!(deps, vec!["Sheet1!A1", "Sheet1!A2", "Sheet1!A3"]);

    let down = wb.trace(0, 0, 0, Direction::Downstream, None).unwrap();
    let consumers: Vec<&str> = down.children.iter().map(|c| c.cell.as_str()).collect();
    assert_eq!(consumers, vec!["Sheet1!C1"]); // the region consumes A1
    assert_eq!(down.children[0].children[0].cell, "Sheet1!D1"); // and D1 consumes the region
}

#[test]
fn trace_classifies_an_error_valued_formula_as_error() {
    // A formula that computes to a spreadsheet error value (`=1/0` -> `#DIV/0!`) is `TraceStatus::Error`
    // (not `Ok`), and — being a clean, computable value (no cycle, no depth taint) — still carries a hash.
    let wb = load_one_tab("Sheet1", &[("A1", "=1/0")]);
    let root = wb.trace(0, 0, 0, Direction::Upstream, None).unwrap();
    assert_eq!(root.cell, "Sheet1!A1");
    assert_eq!(root.status, TraceStatus::Error);
    assert_eq!(root.value, "#DIV/0!");
    assert!(root.hash.is_some());
}

/// Does any node in the trace tree carry `status`? (Recursive search for a status coverage pin.)
fn tree_has_status(node: &TraceNode, status: TraceStatus) -> bool {
    node.status == status || node.children.iter().any(|c| tree_has_status(c, status))
}

#[test]
fn trace_classifies_a_depth_limit_terminal() {
    // A chain deeper than [`MAX_PULL_DEPTH`] traced with no user cap: the walk descends to the
    // engine's pull-depth bound and stops there, classifying that terminal `TraceStatus::DepthLimit`
    // (never a stack overflow — CORE2). The bound is reached at a formula node deep in the chain.
    let len = (MAX_PULL_DEPTH as usize) + 8;
    let owned = chain_files(len);
    let refs: Vec<(&str, &str)> = owned
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_str()))
        .collect();
    let wb = load_one_tab("Sheet1", &refs);
    let root = wb.trace(0, 0, 0, Direction::Upstream, None).unwrap();
    assert!(
        tree_has_status(&root, TraceStatus::DepthLimit),
        "an over-deep chain must produce a depth-limit terminal"
    );
}

#[test]
fn trace_of_an_out_of_range_tab_is_a_located_refusal() {
    // CORE2: a bad tab index is a located refusal, never a panic.
    let wb = load_one_tab("Sheet1", &[("A1", "1")]);
    let err = wb.trace(5, 0, 0, Direction::Upstream, None).unwrap_err();
    assert_eq!(err.code, Code::CellOutOfRange);
}
