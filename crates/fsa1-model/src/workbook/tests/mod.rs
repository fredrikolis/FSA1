// Concern: pins the engine's demand-driven behaviour end to end | Non-concern: graph shape and traversal order | IO: workbooks -> asserted values and diagnostics
use std::collections::HashSet;

use super::*;

use fsa1_ast::{ArrayView, ErrKind, RangeRef, Shape, eval_at};

mod forge;
mod names;
mod scale;

/// Owns the body string, so a `&file("…")` call site hands the loader owned contents.
fn file(body: &str) -> String {
    body.to_string()
}

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
    let wb = load_one_tab("Sheet1", &[("A1", "1"), ("B1", "=A1+1"), ("C1", "=B1*10")]);
    assert_eq!(wb.value_at(0, 2, 0), Value::Number(20.0)); // C1
    assert_eq!(wb.value_at(0, 1, 0), Value::Number(2.0)); // B1
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn a_direct_cycle_is_a_ref_refusal_not_a_hang() {
    let wb = load_one_tab("Sheet1", &[("A1", "=B1"), ("B1", "=A1")]);
    assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Ref)); // A1
    let diags = wb.eval_diagnostics();
    assert!(diags.iter().any(|d| d.code == Code::Cycle), "{diags:?}");
}

#[test]
fn a_self_reference_is_a_cycle() {
    let wb = load_one_tab("Sheet1", &[("A1", "=A1+1")]);
    assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Ref));
    assert!(wb.eval_diagnostics().iter().any(|d| d.code == Code::Cycle));
}

#[test]
fn cross_sheet_reference_resolves_the_named_tab() {
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
    let wb = load_one_tab("Sheet1", &[("A1:A3", "1\n2\n3"), ("C1", "=A1:A3")]);
    assert_eq!(wb.value_at(0, 2, 0), Value::Number(1.0)); // C1 -> A1
}

#[test]
fn a_diamond_dag_evaluates_each_cell_once_never_exponentially() {
    // Each level references the one below TWICE, so reaching the assert at all proves the sharing.
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
    // A matching value would not prove reuse, so `eval_count` is what actually pins it.
    let wb = load_one_tab("Sheet1", &[("A1", "5"), ("B1", "=A1*A1")]);
    assert_eq!(wb.value_at(0, 1, 0), Value::Number(25.0));
    let after_first = wb.eval_count();
    assert_eq!(wb.value_at(0, 1, 0), Value::Number(25.0));
    assert_eq!(
        wb.eval_count(),
        after_first,
        "the repeat demand of B1 must be served from the memo, evaluating nothing"
    );
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn a_gap_cell_reads_blank() {
    let wb = load_one_tab("Sheet1", &[("A1", "1")]);
    assert_eq!(wb.value_at(0, 25, 8), Value::Blank); // Z9, claimed by no file
}

#[test]
fn load_surfaces_overlap_and_bad_files() {
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
fn a_range_named_figure_occupies_its_range_and_a_name_named_one_occupies_nothing() {
    let overlaps = |files: &[(&str, &str)]| -> Vec<Diagnostic> {
        match Workbook::from_tabs(&[("Sheet1", files)]) {
            Ok(_) => Vec::new(),
            Err(d) => d.into_iter().filter(|d| d.code == Code::Overlap).collect(),
        }
    };

    let cell_and_figure = overlaps(&[("E4", "x"), ("D2:K17.json", "{}")]);
    assert_eq!(cell_and_figure.len(), 1, "{cell_and_figure:?}");
    assert!(
        cell_and_figure[0].message.contains("E4")
            && cell_and_figure[0].message.contains("D2:K17.json"),
        "{cell_and_figure:?}"
    );

    // The name form floats, so the same cell is clean beside it.
    assert!(
        overlaps(&[("E4", "x"), ("Chart1.json", "{}")]).is_empty(),
        "a name-form figure occupies nothing"
    );

    let two_figures = overlaps(&[("D2:K17.json", "{}"), ("K17:L20.json", "{}")]);
    assert_eq!(two_figures.len(), 1, "{two_figures:?}");
}

#[test]
fn an_unparseable_formula_is_a_located_error_cell_not_a_whole_file_refusal() {
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
    let base = std::env::temp_dir().join(format!(
        "FSA1-wb-{}-{}",
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
    assert_eq!(wb.value_at(1, 0, 0), Value::Number(42.0)); // tab 1, tabs being sorted

    std::fs::remove_dir_all(&base).ok();
}

fn temp_dir_for(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "FSA1-wb-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ))
}

#[test]
fn a_cell_file_round_trips_whole_at_and_past_the_read_buffer_boundary() {
    // Biggest file FIRST in filename order, so every later one is read through a scratch buffer the first grew past `READ_BUF` — the reuse hazard, pinned in the same test.
    let sizes = [READ_BUF * 3 + 7, READ_BUF, READ_BUF + 1, 10, READ_BUF - 1];
    let base = temp_dir_for("bigfile");
    let tab = base.join("Wide");
    std::fs::create_dir_all(&tab).unwrap();
    let bodies: Vec<String> = sizes
        .iter()
        .map(|&size| {
            // The trailing marker is what a truncated read cannot reproduce.
            let mut body = "x".repeat(size - 4);
            body.push_str("$end");
            assert_eq!(body.len(), size);
            body
        })
        .collect();
    for (i, body) in bodies.iter().enumerate() {
        let name = format!("{}1", fsa1_ast::a1::format_column(i as u32));
        std::fs::write(tab.join(crate::range_file_name(&name)), body).unwrap();
    }

    let wb = Workbook::load_dir(&base)
        .expect("fs read ok")
        .expect("loads clean");
    for (i, body) in bodies.iter().enumerate() {
        assert_eq!(
            wb.value_at(0, i as u32, 0),
            Value::Text(body.clone()),
            "the {}-byte cell file must round-trip whole",
            sizes[i]
        );
    }

    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn a_cell_file_that_is_not_utf8_is_a_located_refusal() {
    // CORE2: the loader refuses, naming the fault, rather than loading a lossy value.
    let base = temp_dir_for("badutf8");
    let tab = base.join("Sheet1");
    std::fs::create_dir_all(&tab).unwrap();
    std::fs::write(tab.join("A1"), [0x68, 0x69, 0xff, 0xfe]).unwrap();

    let err = Workbook::load_dir(&base).expect_err("a non-UTF-8 cell file must refuse");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(err.to_string(), "stream did not contain valid UTF-8");

    std::fs::remove_dir_all(&base).ok();
}

/// Each `+1` makes the top cell's value the chain length minus one, so a computed answer proves
/// the whole chain was walked.
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

/// On its own this proves nothing about stack safety: a recursive per-cell walk survives 1200 links
/// on the default 2 MiB test stack, measured. [`SMALL_STACK`] is what makes it a bound.
const DEEP_CHAIN: usize = 1200;

/// Deliberately TINY: [`DEEP_CHAIN`] frames cannot fit in it at any plausible size — 128 KiB / 1200
/// is 109 bytes a frame — so a walk that went back to native recursion ABORTS rather than passing.
/// An iterative walk keeps its state on the heap and needs a few frames, so it is unaffected.
const SMALL_STACK: usize = 128 * 1024;

/// Re-raises the body's panic, so a failed assertion inside still fails the test.
fn on_a_small_stack(body: impl FnOnce() + Send + 'static) {
    let handle = std::thread::Builder::new()
        .stack_size(SMALL_STACK)
        .spawn(body)
        .expect("spawn the small-stack pin thread");
    if let Err(panic) = handle.join() {
        std::panic::resume_unwind(panic);
    }
}

#[test]
fn a_very_deep_chain_computes_at_every_link() {
    on_a_small_stack(|| {
        let owned = chain_files(DEEP_CHAIN);
        let refs: Vec<(&str, &str)> = owned
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_str()))
            .collect();
        let wb = load_one_tab("Sheet1", &refs);
        assert_eq!(
            wb.value_at(0, 0, 0),
            Value::Number((DEEP_CHAIN - 1) as f64) // A1 = A2+1 = ... = A1200(0) + 1199
        );
        assert!(
            wb.eval_diagnostics().is_empty(),
            "a long chain is not a fault: {:?}",
            wb.eval_diagnostics()
        );
    });
}

#[test]
fn every_cell_of_a_deep_chain_has_one_value_whatever_was_demanded() {
    // A value that varied with the demand set would not be derived from content alone.
    on_a_small_stack(|| {
        let owned = chain_files(DEEP_CHAIN);
        let refs: Vec<(&str, &str)> = owned
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_str()))
            .collect();
        let expected: Vec<Value> = (0..DEEP_CHAIN)
            .map(|i| Value::Number((DEEP_CHAIN - 1 - i) as f64))
            .collect();
        let keys: Vec<CellKey> = (0..DEEP_CHAIN).map(|i| (0, 0, i as u32)).collect();

        // (a) one cell at a time, top-down, so the deepest root is demanded first.
        let per_cell = load_one_tab("Sheet1", &refs);
        let one_at_a_time: Vec<Value> = keys
            .iter()
            .map(|k| per_cell.value_at(k.0, k.1, k.2))
            .collect();
        assert_eq!(one_at_a_time, expected, "a per-cell demand");

        // (b) one batch over the whole chain.
        let batched = load_one_tab("Sheet1", &refs);
        assert_eq!(batched.values_at(&keys), expected, "a batched demand");

        // (c) bottom-up, shallowest first, so no cell is ever reached through a long chain.
        let bottom_up = load_one_tab("Sheet1", &refs);
        for k in keys.iter().rev() {
            bottom_up.value_at(k.0, k.1, k.2);
        }
        let after: Vec<Value> = keys
            .iter()
            .map(|k| bottom_up.value_at(k.0, k.1, k.2))
            .collect();
        assert_eq!(after, expected, "a bottom-up demand");

        assert!(batched.lint().is_empty(), "{:?}", batched.lint());
    });
}

#[test]
fn a_range_reached_through_a_deep_chain_materializes_the_same_values() {
    // The arena caches a materialized rectangle, which is sound only because its cells derive from their own content, not from how deep the demand that first materialized it happened to be.
    let mut owned: Vec<(String, String)> = Vec::new();
    let h_len = 99usize;
    for i in 0..h_len {
        let name = format!("H{}", i + 1);
        let body = if i + 1 < h_len {
            format!("=H{}", i + 2)
        } else {
            "0".to_string()
        };
        owned.push((name, body));
    }
    let a_len = 200usize;
    for i in 0..a_len {
        let name = format!("A{}", i + 1);
        let body = if i + 1 < a_len {
            format!("=A{}", i + 2)
        } else {
            "=SUM(H1:H1)".to_string()
        };
        owned.push((name, body));
    }
    owned.push(("B1".to_string(), "=SUM(H1:H1)".to_string())); // the later SHALLOW demand
    let refs: Vec<(&str, &str)> = owned
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_str()))
        .collect();
    let wb = load_one_tab("Sheet1", &refs);
    assert_eq!(wb.value_at(0, 0, 0), Value::Number(0.0)); // A1, through the deep chain
    assert_eq!(wb.value_at(0, 1, 0), Value::Number(0.0)); // B1, the shallow demand: the same value
}

#[test]
fn an_inverted_range_yields_the_same_rectangle() {
    // How the arena dedups the two keys is an internal this deliberately does not pin.
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
    assert_eq!(wb.range(inverted).cells, normalized_cells.as_slice());
}

#[test]
fn a_reference_to_a_pathologically_large_range_refuses_instead_of_oom() {
    // ~70M empty cells: a `Value` per cell would be an OOM abort.
    let wb = load_one_tab("Sheet1", &[("A1", "=SUM(A2:ZZ100000)")]);
    assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Num)); // A1
    let diags = wb.eval_diagnostics();
    assert!(
        diags.iter().any(|d| d.code == Code::RangeTooLarge),
        "{diags:?}"
    );
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
    let wb = load_one_tab(
        "Sheet1",
        &[("A1:A5", "1\n2\n3\n4\n5"), ("C1", "=SUM(A1:A5)")],
    );
    assert_eq!(wb.value_at(0, 2, 0), Value::Number(15.0)); // C1
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn a_cross_sheet_cycle_is_located_to_the_sheet_qualified_file() {
    // A bare `A1` exists on BOTH sheets here, so a refusal naming only it is untraceable.
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
    let wb = load_one_tab("Sheet1", &[("A1", "6"), ("A2", "7")]);
    assert_eq!(
        wb.eval_formula(0, "A1*A2").unwrap(),
        FormulaOutcome::Value("42".to_string())
    );
    assert_eq!(
        wb.eval_formula(0, "1/0").unwrap(),
        FormulaOutcome::Error("#DIV/0!".to_string())
    );
    assert!(wb.eval_formula(0, "SUM(").is_err());
}

#[test]
fn isformula_reads_the_grid_content_kind_over_the_real_resolver() {
    // B1 must report TRUE even though it evaluates to `#DIV/0!`: the kind is not the value.
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
    assert_eq!(
        wb.eval_formula(0, "ISFORMULA(Z9)").unwrap(), // a gap
        FormulaOutcome::Value("FALSE".to_string())
    );
}

#[test]
fn isformula_reports_true_across_a_grid5_array_formula_region() {
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
    let wb = load_one_tab("Sheet1", &[("A1", "2"), ("B1", "=A1+1"), ("C1", "=A1+1")]);
    let vals = wb.values_at(&[(0, 0, 0), (0, 1, 0), (0, 2, 0)]);
    assert_eq!(
        vals,
        vec![Value::Number(2.0), Value::Number(3.0), Value::Number(3.0)]
    );
    assert!(wb.eval_diagnostics().is_empty());
}

/// The TEST-ONLY reference the differential grades the two-pass engine against. It shares NONE of
/// that algorithm — only the workbook's structural cell-location plumbing and the `Arena` — and
/// recurses natively, terminating a re-entered cell by a `visiting` guard, so it can only grade
/// ACYCLIC shapes of a depth it survives. Single-path `assert_eq` tests cover what it cannot.
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

    /// The engine's `cell_scalar` rule, re-derived so the oracle shares no code with it. The
    /// CELL-position rule, deliberately not `fsa1_ast::scalarize`'s in-expression one.
    fn cell_top_left(v: Value) -> Value {
        match v {
            Value::Array(_, cells) => cells.into_iter().next().unwrap_or(Value::Blank),
            other => other,
        }
    }

    /// The INDEPENDENT reference model of `fill_array_region`, touching no engine plan or eval type.
    /// Reading a region coordinate off the lone `1x1` grid instead would index out of bounds, and
    /// scalarizing the anchor would demote its array — so the element is re-derived here.
    fn region_element(&self, id: FileId, file: &LoadedFile, key: CellKey) -> Value {
        let region = file.region;
        let rows = region.max_row - region.min_row + 1;
        let cols = region.max_col - region.min_col + 1;
        self.visiting.borrow_mut().insert(key);
        let prev = self.cur.replace(id.0);
        let value = match file.grid.cell_at(0, 0) {
            // Anchored at the region's top-left, as `fill_array_region` does.
            GridCell::Formula { expr, .. } => eval_at(expr, self, region.min_row, region.min_col),
            GridCell::Value { value, .. } => value.clone(),
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
        let v = if file.array_formula {
            self.region_element(id, file, key)
        } else {
            let dr = cell.row - file.region.min_row;
            let dc = cell.col - file.region.min_col;
            match file.grid.cell_at(dr, dc) {
                GridCell::Value { value, .. } => value.clone(),
                GridCell::LoadError { diag, .. } => crate::grid::load_error_value(diag),
                GridCell::Formula { expr, .. } => {
                    self.visiting.borrow_mut().insert(key);
                    let prev = self.cur.replace(id.0);
                    // The SAME effective expr the engine reads, so the differential proves the graph equals a per-cell eval OVER the resolved references. Pass 0 has already run, the caller having demanded these cells first.
                    let eff = self.wb.effective_expr(key, expr);
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

/// VALUES only, never the graph's shape, node count, or traversal order: asserting an internal
/// would freeze "how" and block a future parallel-execution refactor.
fn assert_agrees(wb: &Workbook, cells: &[(u32, u32, u32)]) {
    let oracle = NaiveOracle::new(wb);
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

#[test]
fn sort_region_fills_its_range_sorted() {
    let wb = load_one_tab("Sheet1", &[("A1:A3", "3\n1\n2"), ("C1:C3", "=SORT(A1:A3)")]);
    assert_eq!(wb.value_at(0, 2, 0), Value::Number(1.0)); // C1
    assert_eq!(wb.value_at(0, 2, 1), Value::Number(2.0)); // C2
    assert_eq!(wb.value_at(0, 2, 2), Value::Number(3.0)); // C3
    assert!(wb.eval_diagnostics().is_empty());
}

#[test]
fn unique_region_over_a_column_with_dups() {
    // First-seen order.
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
    // A region with no external dependency at all.
    let wb = load_one_tab("Sheet1", &[("C1:C3", "=SEQUENCE(3)")]);
    assert_eq!(wb.value_at(0, 2, 0), Value::Number(1.0));
    assert_eq!(wb.value_at(0, 2, 1), Value::Number(2.0));
    assert_eq!(wb.value_at(0, 2, 2), Value::Number(3.0));
}

#[test]
fn transpose_region_fills_the_transposed_orientation() {
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
    // A 2x1 region holding a 3x1 value: detected AT EVALUATION, not at load.
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
    // A 1x1 file is never a region, its range spanning one coordinate.
    let wb = load_one_tab("Sheet1", &[("A1:A3", "3\n1\n2"), ("E1", "=SORT(A1:A3)")]);
    assert_eq!(wb.value_at(0, 4, 0), Value::Number(1.0)); // E1 -> top-left of the sorted array
}

#[test]
fn a_coordinate_reference_into_a_region_resolves_to_its_element() {
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
    // The region's input is itself computed formulas.
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

/// A sidecar is CLASSIFIED and then dropped: it states no cell, so no value can derive from one and
/// the tab it styles reaches no further for having been styled (VAL1).
#[test]
fn a_styled_tab_loads_and_its_sidecar_contributes_no_cell() {
    let wb = load_one_tab(
        "Sheet1",
        &[
            ("A1:A2", "3\n4"),
            ("C1", "=SUM(A1:A2)"),
            ("E1:G5.css", "  fsa1-cell { text-align: right }\n"),
        ],
    );
    assert_eq!(wb.value_at(0, 2, 0), Value::Number(7.0));
    assert!(wb.eval_diagnostics().is_empty());
    assert!(
        wb.source_at(0, 4, 0).is_none(),
        "E1 is under the root alone"
    );
    assert_eq!(
        wb.content_region(0),
        Some(Rect {
            min_col: 0,
            min_row: 0,
            max_col: 2,
            max_row: 1
        }),
        "a block's root is no content of the tab's",
    );
}

/// The engine cannot be refused by a file it never opens. Every fault these two earn is still
/// earned, on the overlay, which `check` loads and folds in; `eval` answers over the same tree.
#[test]
fn a_sidecar_that_will_not_parse_refuses_no_verb_reading_values() {
    for sidecar in ["  th { color: red }\n", "  fsa1-cell { color: #\n"] {
        let wb = load_one_tab(
            "Sheet1",
            &[
                ("A1:A2", "3\n4"),
                ("C1", "=SUM(A1:A2)"),
                ("A1:A2.css", sidecar),
            ],
        );
        assert_eq!(wb.value_at(0, 2, 0), Value::Number(7.0), "{sidecar:?}");
        assert!(wb.lint().is_empty(), "{sidecar:?}: {:?}", wb.lint());
    }
}

/// A sidecar is read from DISK by the same classifier the in-memory loader uses, ahead of both the
/// cell arm — whose parser refuses `A1:C3.css` as a malformed range — and the defined-name arm.
#[test]
fn a_sidecar_on_disk_is_classified_before_the_cell_and_name_arms() {
    let dir = std::env::temp_dir().join(format!("fsa1-wb-{}-sidecar", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Sheet1")).expect("a tab");
    std::fs::write(dir.join("Sheet1").join("A1"), "1").expect("a cell");
    std::fs::write(
        dir.join("Sheet1").join("A1.css"),
        "  fsa1-cell { font-weight: bold }\n",
    )
    .expect("a sidecar");
    let wb = Workbook::load_dir(&dir)
        .expect("the tree is readable")
        .unwrap_or_else(|d| panic!("a sidecar tree loads clean: {d:?}"));
    let overlay = crate::overlay::Overlay::load_dir(&dir)
        .expect("the tree is readable")
        .unwrap_or_else(|d| panic!("a sidecar tree overlays clean: {d:?}"));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        overlay
            .cell_style(&wb, 0, 0, 0)
            .expect("A1 is styled")
            .font_weight,
        Some(crate::FontWeight::Bold),
    );
    assert!(wb.name_table().names().is_empty(), "and declares no name");
    assert_eq!(
        wb.tab_files(0).expect("Sheet1").len(),
        1,
        "and is no range file of the tab's",
    );
}

#[test]
fn a_single_literal_in_a_multi_cell_range_stays_a_grid4_dimension_error() {
    // Only a lone `=formula` makes a region, so this stays a dimension error at LOAD.
    let err = Workbook::from_tabs(&[("Sheet1", &[("C1:C3", "5")])]).unwrap_err();
    assert!(
        err.iter().any(|d| d.code == Code::DimensionMismatch),
        "{err:?}"
    );
}

#[test]
fn a_cyclic_region_is_a_located_ref_at_every_coordinate() {
    // The region's ONE formula never runs, so a CONTINUATION coordinate must still resolve rather than fall through to an out-of-bounds read of the region's 1x1 grid. Demanding only those, in one batch, is what forces the resolver to read them from the pass results.
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
    let wb = load_one_tab("Sheet1", &[("C1:C3", "=SORT(C1:C3)"), ("D1", "=C3")]);
    assert_eq!(wb.value_at(0, 3, 0), Value::Error(ErrKind::Ref)); // D1 -> C3 (continuation) -> #REF!
    assert!(wb.eval_diagnostics().iter().any(|d| d.code == Code::Cycle));
}

#[test]
fn a_region_reached_through_a_deep_chain_yields_its_elements() {
    // How far down a chain a region was reached is not a property of its value.
    let len = 1200usize;
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
    assert_eq!(wb.value_at(0, 23, 0), Value::Number(3.0)); // X1, column X being index 23
    assert!(
        wb.eval_diagnostics().is_empty(),
        "{:?}",
        wb.eval_diagnostics()
    );
}

#[test]
fn render_values_and_functions_over_a_region() {
    use crate::render::{RenderMode, parse_viewport, render};
    let wb = load_one_tab("Sheet1", &[("A1:A3", "3\n1\n2"), ("C1:C3", "=SORT(A1:A3)")]);
    let vp = parse_viewport("C1:C3").unwrap();
    let vals = render(&wb, 0, vp, RenderMode::Values);
    let col: Vec<&str> = vals.rows.iter().map(|r| r.cells[0].as_str()).collect();
    assert_eq!(col, vec!["1", "2", "3"]);
    let fns = render(&wb, 0, vp, RenderMode::Functions);
    let col: Vec<&str> = fns.rows.iter().map(|r| r.cells[0].as_str()).collect();
    assert_eq!(col, vec!["=SORT(A1:A3)", "^", "^"]);
}

#[test]
fn differential_region_shares_its_input_range_with_another_cell() {
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
    // `dep_key` collapses each referenced coordinate onto ONE anchor node, so each dependent must still see the RIGHT element.
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
    // The oracle re-derives the same refusal, so the differential grades that shape too.
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
    assert!(
        wb.eval_diagnostics()
            .iter()
            .any(|d| d.code == Code::DimensionMismatch)
    );
}

#[test]
fn computation_hash_is_deterministic() {
    let build = || load_one_tab("Sheet1", &[("A1", "1"), ("B1", "=A1+1"), ("C1", "=B1*10")]);
    let a = build();
    let b = build();
    assert!(a.computation_hash(0, 2, 0).is_some()); // C1 has a hash
    assert_eq!(a.computation_hash(0, 2, 0), b.computation_hash(0, 2, 0)); // across workbooks
    assert_eq!(a.computation_hash(0, 2, 0), a.computation_hash(0, 2, 0)); // repeated call, stable
}

#[test]
fn computation_hash_is_sensitive_upstream_and_isolates_the_unrelated() {
    let wb1 = load_one_tab("Sheet1", &[("A1", "1"), ("B1", "=A1+1"), ("Z1", "99")]);
    let wb2 = load_one_tab("Sheet1", &[("A1", "2"), ("B1", "=A1+1"), ("Z1", "99")]);
    assert_ne!(wb1.computation_hash(0, 0, 0), wb2.computation_hash(0, 0, 0)); // A1, edited
    // B1's own text is identical, yet its hash must change.
    assert_ne!(wb1.computation_hash(0, 1, 0), wb2.computation_hash(0, 1, 0));
    assert_eq!(
        // Z1, unrelated
        wb1.computation_hash(0, 25, 0),
        wb2.computation_hash(0, 25, 0)
    );
}

#[test]
fn a_cyclic_cell_has_no_computation_hash() {
    let wb = load_one_tab("Sheet1", &[("A1", "=B1"), ("B1", "=A1"), ("C1", "7")]);
    assert_eq!(wb.computation_hash(0, 0, 0), None); // A1 (on the cycle)
    assert_eq!(wb.computation_hash(0, 1, 0), None); // B1 (on the cycle)
    assert!(wb.computation_hash(0, 2, 0).is_some()); // C1 (clean literal)
}

#[test]
fn same_formula_over_same_refs_hashes_the_same_regardless_of_position() {
    // The hash is over CONTENT, never over the cell's own address.
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
    let wb = load_one_tab("Sheet1", &[("A1:A3", "3\n1\n2"), ("C1:C3", "=SORT(A1:A3)")]);
    let anchor = wb.computation_hash(0, 2, 0); // C1 (anchor)
    assert!(anchor.is_some());
    assert_eq!(wb.computation_hash(0, 2, 1), anchor); // C2 (member)
    assert_eq!(wb.computation_hash(0, 2, 2), anchor); // C3 (member)
}

#[test]
fn a_cell_downstream_of_a_cycle_has_no_computation_hash() {
    // A missing digest propagates upward, whatever the dependent's own text says.
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
fn every_cell_of_a_deep_chain_has_a_computation_hash() {
    // Chain length is NOT one of the two hashless terminals.
    on_a_small_stack(|| {
        let owned = chain_files(DEEP_CHAIN);
        let refs: Vec<(&str, &str)> = owned
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_str()))
            .collect();
        let wb = load_one_tab("Sheet1", &refs);
        assert!(wb.computation_hash(0, 0, 0).is_some()); // A1 (top of the chain)
        assert!(wb.computation_hash(0, 0, (DEEP_CHAIN / 2) as u32).is_some()); // the middle
        assert!(wb.computation_hash(0, 0, (DEEP_CHAIN - 1) as u32).is_some()); // the bottom literal
    });
}

#[test]
fn trace_upstream_shows_a_shared_dependency_once_as_repeated() {
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
    assert_eq!(root.children.len(), 2); // B1 and C1, sorted by key
    for c in &root.children {
        assert_eq!(c.children[0].cell, "Sheet1!A1");
    }
    let repeated = root
        .children
        .iter()
        .filter(|c| c.children[0].repeated)
        .count();
    assert_eq!(repeated, 1);
}

#[test]
fn trace_upstream_reports_a_cycle_without_looping() {
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
    assert_eq!(root.children[0].children[0].cell, "Sheet1!D1");
    assert!(root.children[1].children.is_empty());
}

#[test]
fn trace_respects_a_depth_cap() {
    let wb = load_one_tab("Sheet1", &[("A1", "1"), ("B1", "=A1+1"), ("C1", "=B1+1")]);
    let root = wb.trace(0, 2, 0, Direction::Upstream, Some(1)).unwrap();
    assert_eq!(root.cell, "Sheet1!C1");
    assert_eq!(root.children.len(), 1);
    assert_eq!(root.children[0].cell, "Sheet1!B1");
    assert!(root.children[0].children.is_empty());
}

#[test]
fn trace_builds_and_drops_a_deep_cone_without_recursing() {
    // BUILDING, INSPECTING, and DROPPING this tree are three traversals of a linked structure, any one of which aborts if it recurses per link — hence the explicit stack below and the drop inside the pin.
    on_a_small_stack(|| {
        let owned = chain_files(DEEP_CHAIN);
        let refs: Vec<(&str, &str)> = owned
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_str()))
            .collect();
        let wb = load_one_tab("Sheet1", &refs);
        let root = wb.trace(0, 0, 0, Direction::Upstream, None).unwrap();
        assert_eq!(root.cell, "Sheet1!A1");

        let (mut nodes, mut leaf) = (0usize, String::new());
        let mut stack: Vec<&TraceNode> = vec![&root];
        while let Some(node) = stack.pop() {
            nodes += 1;
            if node.children.is_empty() {
                leaf = node.cell.clone();
            }
            stack.extend(node.children.iter());
        }
        assert_eq!(nodes, DEEP_CHAIN, "one node per link, none elided");
        assert_eq!(leaf, format!("Sheet1!A{DEEP_CHAIN}"), "the bottom literal");

        drop(root); // the tree's own drop, still on the small stack
    });
}

#[test]
fn trace_into_and_out_of_a_grid5_region() {
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
    // On no cycle, so it still carries a hash.
    let wb = load_one_tab("Sheet1", &[("A1", "=1/0")]);
    let root = wb.trace(0, 0, 0, Direction::Upstream, None).unwrap();
    assert_eq!(root.cell, "Sheet1!A1");
    assert_eq!(root.status, TraceStatus::Error);
    assert_eq!(root.value, "#DIV/0!");
    assert!(root.hash.is_some());
}

#[test]
fn trace_of_an_out_of_range_tab_is_a_located_refusal() {
    let wb = load_one_tab("Sheet1", &[("A1", "1")]);
    let err = wb.trace(5, 0, 0, Direction::Upstream, None).unwrap_err();
    assert_eq!(err.code, Code::CellOutOfRange);
}

#[test]
fn a_workbook_that_is_its_own_git_repo_loads_clean() {
    let dir = std::env::temp_dir().join(format!(
        "FSA1-gitwb-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(dir.join("Data")).unwrap();
    std::fs::write(dir.join("Data/A1"), "1").unwrap();
    std::fs::write(dir.join("Data/A2"), "=A1*2").unwrap();
    std::fs::create_dir_all(dir.join(".git/refs")).unwrap();
    std::fs::write(dir.join(".git/HEAD"), "ref: refs/heads/master").unwrap();
    std::fs::write(dir.join(".gitignore"), ".cache/\n").unwrap();
    std::fs::write(dir.join(".gitattributes"), "* text\n").unwrap();

    let wb = Workbook::load_dir(&dir).unwrap().expect("must load");
    assert_eq!(wb.sheet_names(), vec!["Data"], ".git must not become a tab");
    assert!(
        wb.lint()
            .iter()
            .all(|d| d.code.severity() != crate::diagnostic::Severity::Error),
        "a git-rooted workbook must lint clean: {:?}",
        wb.lint()
    );
    assert_eq!(wb.value_at(0, 0, 1), Value::Number(2.0));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_stale_cache_directory_is_inert_neither_a_tab_nor_a_value_source() {
    // What reserving a name nothing writes to buys: deleting a workbook's `.cache/` changes no value.
    let mk = |suffix: &str| {
        let dir = std::env::temp_dir().join(format!(
            "FSA1-stalecache-{suffix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(dir.join("Data")).unwrap();
        std::fs::write(dir.join("Data/A1"), "1").unwrap();
        std::fs::write(dir.join("Data/A2"), "=A1*2").unwrap();
        dir
    };
    let plain = mk("plain");
    let stale = mk("stale");
    // `.cache/A1` is A1-shaped, so admitting the directory as a tab would contribute a value.
    std::fs::create_dir_all(stale.join(".cache")).unwrap();
    std::fs::write(stale.join(".cache/v2-0123456789abcdef"), [0u8; 17]).unwrap();
    std::fs::write(stale.join(".cache/A1"), "999").unwrap();
    std::fs::write(stale.join(".cache/.tmp-v2-dead-1"), b"torn").unwrap();

    let a = Workbook::load_dir(&plain).unwrap().expect("must load");
    let b = Workbook::load_dir(&stale).unwrap().expect("must load");
    assert_eq!(a.sheet_names(), vec!["Data"]);
    assert_eq!(
        b.sheet_names(),
        vec!["Data"],
        "a pre-existing .cache/ must not become a tab (FS3)"
    );
    assert_eq!(a.value_at(0, 0, 1), Value::Number(2.0));
    assert_eq!(
        b.value_at(0, 0, 1),
        a.value_at(0, 0, 1),
        "a stale .cache/ must change no value"
    );
    assert!(
        b.lint()
            .iter()
            .all(|d| d.code.severity() != crate::diagnostic::Severity::Error),
        "a stale .cache/ must raise no refusal: {:?}",
        b.lint()
    );
    std::fs::remove_dir_all(&plain).ok();
    std::fs::remove_dir_all(&stale).ok();
}

#[test]
fn an_open_axis_binds_to_the_used_bounds_not_to_a_fixed_sheet_height() {
    // The sheet is infinite and sparse, so `A:A` is bound by what EXISTS: three cells, not a million.
    let wb = Workbook::from_tabs(&[("S", &[("A1", "1"), ("A2", "2"), ("A3", "3"), ("C1", "9")])])
        .unwrap();
    assert_eq!(
        wb.eval_formula(0, "SUM(A:A)").unwrap(),
        FormulaOutcome::Value("6".to_string())
    );
    assert_eq!(
        wb.eval_formula(0, "COUNT(A:A)").unwrap(),
        FormulaOutcome::Value("3".to_string())
    );
    assert_eq!(
        wb.eval_formula(0, "SUM(1:1)").unwrap(), // the used WIDTH: A1 and C1
        FormulaOutcome::Value("10".to_string())
    );
    // ROWS answers "how many exist", a declared divergence: the model has no fixed height.
    assert_eq!(
        wb.eval_formula(0, "ROWS(A:A)").unwrap(),
        FormulaOutcome::Value("3".to_string())
    );
}

#[test]
fn an_open_axis_plans_its_formula_cells_not_just_its_literals() {
    // With LITERALS an unplanned open range is invisible; with formula cells it is a wrong value.
    let wb =
        Workbook::from_tabs(&[("S", &[("A1", "1"), ("A2", "=A1+1"), ("A3", "=A2+1")])]).unwrap();
    assert_eq!(
        wb.eval_formula(0, "SUM(A:A)").unwrap(),
        FormulaOutcome::Value("6".to_string()),
        "every cell of the open axis must be planned, formulas included"
    );
}

#[test]
fn an_open_axis_never_overflows_the_positional_and_forging_functions() {
    // `ROW`/`COLUMN` read the syntactic node, never the resolver, so the clamp cannot reach them.
    let wb = Workbook::from_tabs(&[("S", &[("A1", "1")])]).unwrap();
    for f in ["ROW(A:A)", "COLUMN(1:1)", "SUM(OFFSET(A:A,0,0))"] {
        let out = wb.eval_formula(0, f).unwrap();
        assert_eq!(
            out,
            FormulaOutcome::Error("#REF!".to_string()),
            "{f} must refuse, never overflow or abort"
        );
    }
}
