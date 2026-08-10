// Concern: locks the render/check/eval/trace/tree argv dispatch, stdout and exit codes | Non-concern: the spreadsheet logic beneath it | IO: spawns the binary -> stdout + exit status

mod common;

use common::{Fixture, at, run, run_err, run_in, snapshot};
use std::path::Path;

#[test]
fn render_default_draws_the_computed_cone() {
    let fx = Fixture::new("render");
    fx.file("Sheet1", "A1", "20000")
        .file("Sheet1", "B1", "=A1*2");
    let (code, out) = run(&["render", fx.path().to_str().unwrap()]);
    assert_eq!(code, 0, "clean render exits 0; got:\n{out}");
    assert!(out.contains("| A "), "column-letter header:\n{out}");
    assert!(out.contains("40000"), "B1 should compute to 40000:\n{out}");
}

#[test]
fn render_functions_shows_formula_text() {
    let fx = Fixture::new("funcs");
    fx.file("Sheet1", "A1", "2").file("Sheet1", "B1", "=A1*2");
    let (code, out) = run(&["render", fx.path().to_str().unwrap(), "--mode", "functions"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("=A1*2"),
        "formula text in --mode functions:\n{out}"
    );
}

#[test]
fn render_default_is_combined_value_then_source() {
    let fx = Fixture::new("combined");
    fx.file("Sheet1", "A1", "2").file("Sheet1", "B1", "=A1*2");
    let (code, out) = run(&["render", fx.path().to_str().unwrap()]);
    assert_eq!(code, 0, "combined render exits 0:\n{out}");
    assert!(
        out.contains("4 ← =A1*2"),
        "a formula shows `<value> ← =<formula>` in the combined default:\n{out}"
    );

    let (vc, vals) = run(&["render", fx.path().to_str().unwrap(), "--mode", "values"]);
    assert_eq!(vc, 0);
    assert!(
        vals.contains("| 4 ") && !vals.contains("←"),
        "--mode values shows the value only (no arrow):\n{vals}"
    );

    let (fc, funcs) = run(&["render", fx.path().to_str().unwrap(), "--mode", "functions"]);
    assert_eq!(fc, 0);
    assert!(
        funcs.contains("=A1*2") && !funcs.contains("←"),
        "--mode functions shows the source only (no arrow):\n{funcs}"
    );
}

#[test]
fn render_missing_tab_in_tab_position_is_not_found() {
    let fx = Fixture::new("notab");
    fx.file("Sheet1", "A1", "1");
    let (code, _, err) = run_err(&["render", &at(&fx, "Nope/A1")]);
    assert_eq!(code, 24, "a non-final missing tab is exit 24 (not found)");
    assert!(
        err.contains("no tab named") && err.contains("Nope"),
        "the located refusal names the missing tab:\n{err}"
    );
}

#[test]
fn check_clean_workbook_exits_zero() {
    let fx = Fixture::new("clean");
    fx.file("Sheet1", "A1", "1").file("Sheet1", "B1", "=A1+1");
    let (code, out) = run(&["check", fx.path().to_str().unwrap()]);
    assert_eq!(code, 0, "clean check exits 0:\n{out}");
    assert!(out.contains("no diagnostics"), "clean report:\n{out}");
}

#[test]
fn check_cycle_reports_and_exits_three() {
    let fx = Fixture::new("cycle");
    fx.file("Sheet1", "A1", "=B1").file("Sheet1", "B1", "=A1");
    let (code, out) = run(&["check", fx.path().to_str().unwrap()]);
    assert_eq!(
        code, 3,
        "a cycle is an error-severity diagnostic -> exit 3:\n{out}"
    );
    assert!(out.contains("cycle"), "the cycle code:\n{out}");
    assert!(
        out.contains("circular reference"),
        "the located message:\n{out}"
    );
}

#[test]
fn check_overlap_reports_and_exits_three() {
    let fx = Fixture::new("overlap");
    fx.file("Sheet1", "A1:C3", "1\t2\t3\n4\t5\t6\n7\t8\t9")
        .file("Sheet1", "B2", "x");
    let (code, out) = run(&["check", fx.path().to_str().unwrap()]);
    assert_eq!(code, 3, "an overlap is exit 3:\n{out}");
    assert!(out.contains("overlap"), "the overlap code:\n{out}");
}

#[test]
fn check_missing_path_is_not_found() {
    let (code, _) = run(&["check", "/no/such/FSA1/workbook/xyz"]);
    assert_eq!(code, 24);
}

#[test]
fn render_a_grid5_array_formula_region_fills_its_range() {
    let fx = Fixture::new("grid5");
    fx.file("Sheet1", "A1:A3", "3\n1\n2")
        .file("Sheet1", "C1:C3", "=SORT(A1:A3)");
    let (code, out) = run(&["render", &at(&fx, "Sheet1/C1:C3")]);
    assert_eq!(code, 0, "clean region render exits 0:\n{out}");
    for v in ["| 1 ", "| 2 ", "| 3 "] {
        assert!(out.contains(v), "sorted element {v}:\n{out}");
    }
    let (code, out) = run(&["render", &at(&fx, "Sheet1/C1:C3"), "--mode", "functions"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("=SORT(A1:A3)"),
        "anchor shows the formula:\n{out}"
    );
    assert!(
        out.contains("| ^ "),
        "continuation cells show the caret marker:\n{out}"
    );
}

#[test]
fn check_a_grid5_shape_mismatch_reports_a_dimension_error() {
    let fx = Fixture::new("grid5mismatch");
    fx.file("Sheet1", "A1:A3", "3\n1\n2")
        .file("Sheet1", "C1:C2", "=SORT(A1:A3)");
    let (code, out) = run(&["check", fx.path().to_str().unwrap()]);
    assert_eq!(code, 3, "a region shape mismatch is exit 3:\n{out}");
    assert!(out.contains("dimension-mismatch"), "the code:\n{out}");
    assert!(out.contains("array formula"), "the located message:\n{out}");
}

/// Nothing is excluded — `.cache/` included — so a derived write anywhere under the workbook shows.
fn every_file(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, base, out);
            } else {
                out.push(p.strip_prefix(base).unwrap().to_string_lossy().into_owned());
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

#[test]
fn check_range_reports_only_in_scope_and_exits_zero_when_scope_is_clean() {
    let fx = Fixture::new("scope-range");
    fx.file("Sheet1", "D1", "=SUM(")
        .file("Sheet1", "A1", "1")
        .file("Sheet1", "A2", "2")
        .file("Sheet1", "H3", "=SUM(A1:A2)");

    let (code, out) = run(&["check", &at(&fx, "Sheet1/G1:H5")]);
    assert_eq!(
        code, 0,
        "a clean scope exits 0 despite the D1 error:\n{out}"
    );
    assert!(
        !out.contains("formula-syntax"),
        "D1 is out of scope:\n{out}"
    );
    assert!(
        out.contains("no diagnostics"),
        "clean in-scope report:\n{out}"
    );

    let (code, out) = run(&["check", &at(&fx, "Sheet1/D1:D1")]);
    assert_eq!(code, 3, "the in-scope D1 error is exit 3:\n{out}");
    assert!(
        out.contains("formula-syntax"),
        "D1's error is in scope:\n{out}"
    );

    let (code, out) = run(&["check", fx.path().to_str().unwrap()]);
    assert_eq!(code, 3, "the unscoped check is unchanged:\n{out}");
    assert!(
        out.contains("formula-syntax"),
        "unscoped reports D1:\n{out}"
    );
}

#[test]
fn check_cell_scopes_to_a_single_cell() {
    let fx = Fixture::new("scope-cell");
    fx.file("Sheet1", "D1", "=SUM(")
        .file("Sheet1", "H3", "=1+1");

    let (code, out) = run(&["check", &at(&fx, "Sheet1/H3")]);
    assert_eq!(code, 0, "the clean authored cell exits 0:\n{out}");
    assert!(!out.contains("formula-syntax"), "D1 out of scope:\n{out}");

    let (code, out) = run(&["check", &at(&fx, "Sheet1/D1")]);
    assert_eq!(code, 3, "the faulty cell in scope exits 3:\n{out}");
    assert!(out.contains("formula-syntax"), "D1 in scope:\n{out}");

    let (code, out) = run(&["check", &at(&fx, "Sheet1/A1:B2")]);
    assert_eq!(
        code, 0,
        "a region scope is accepted (D1/H3 out of it):\n{out}"
    );
}

#[test]
fn path_tab_selects_the_named_tab_for_check() {
    let fx = Fixture::new("scope-qualified");
    fx.file("Alpha", "D1", "=SUM(").file("Beta", "D1", "=1+1");

    let (code, out) = run(&["check", &at(&fx, "Beta/D1")]);
    assert_eq!(
        code, 0,
        "Beta/D1 is clean; Alpha's error is out of scope:\n{out}"
    );
    assert!(
        !out.contains("formula-syntax"),
        "Alpha out of scope:\n{out}"
    );

    let (code, out) = run(&["check", &at(&fx, "Alpha/D1")]);
    assert_eq!(code, 3, "Alpha/D1 is the faulty cell:\n{out}");
    assert!(out.contains("formula-syntax"), "Alpha/D1 in scope:\n{out}");

    let (code, _) = run(&["check", &at(&fx, "Beta")]);
    assert_eq!(code, 0, "the clean Beta tab exits 0");
    let (code, _) = run(&["check", &at(&fx, "Alpha")]);
    assert_eq!(code, 3, "the faulty Alpha tab exits 3");

    let (code, _) = run(&["check", &at(&fx, "Nope/A1")]);
    assert_eq!(code, 24, "an unknown scope tab is not-found");
}

#[test]
fn check_scoped_on_an_unloadable_workbook_freezes_the_best_effort_region_filter() {
    let fx = Fixture::new("scope-loadfail");
    fx.file("Sheet1", "A1:D9", "one literal in a 9x4 range")
        .file("Sheet1", "H3", "=1+1");

    for scope in ["Sheet1/Z1:Z2", "Sheet1/H3"] {
        let (code, out) = run(&["check", &at(&fx, scope)]);
        assert_eq!(code, 0, "an out-of-scope load failure reads green:\n{out}");
        assert!(
            !out.contains("dimension-mismatch"),
            "the fault is out of scope:\n{out}"
        );
    }

    let (code, out) = run(&["check", &at(&fx, "Sheet1/A1:B2")]);
    assert_eq!(code, 3, "an in-scope load failure is exit 3:\n{out}");
    assert!(
        out.contains("dimension-mismatch"),
        "the in-scope fault is reported:\n{out}"
    );

    let (code, out) = run(&["check", fx.path().to_str().unwrap()]);
    assert_eq!(code, 3, "the unscoped load failure is unchanged:\n{out}");
    assert!(
        out.contains("dimension-mismatch"),
        "unscoped reports the fault:\n{out}"
    );
}

#[test]
fn check_scope_never_suppresses_a_region_less_load_refusal() {
    let fx = Fixture::new("scope-loadfail-noregion");
    fx.file("Sheet1", "A1:B2:C3", "1");

    for args in [
        vec!["check".to_string(), fx.path().to_str().unwrap().to_string()],
        vec!["check".to_string(), at(&fx, "Sheet1/Z1:Z2")],
        vec!["check".to_string(), at(&fx, "Sheet1/Y9")],
    ] {
        let argv: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, out) = run(&argv);
        assert_eq!(
            code, 3,
            "a region-less load refusal is never suppressed:\n{out}"
        );
        assert!(
            out.contains("malformed-filename"),
            "the refusal surfaces:\n{out}"
        );
    }
}

#[test]
fn check_is_read_only_and_writes_no_file_at_all() {
    let fx = Fixture::new("scope-readonly");
    fx.file("Sheet1", "D1", "=SUM(")
        .file("Sheet1", "A1", "1")
        .file("Sheet1", "H3", "=A1+1");

    let before = every_file(fx.path());
    for path in [
        fx.path().to_str().unwrap().to_string(),
        at(&fx, "Sheet1/H3"),
        at(&fx, "Sheet1/A1:H3"),
        at(&fx, "Sheet1"),
    ] {
        let _ = run(&["check", &path]);
    }
    let after = every_file(fx.path());
    assert_eq!(
        before, after,
        "check must not create, modify, or delete any authoritative file"
    );
}

#[test]
fn eval_computes_a_sum_against_the_workbook() {
    let fx = Fixture::new("eval-sum");
    fx.file("Sheet1", "A1:A3", "1\n2\n3");
    let (code, out) = run(&[
        "eval",
        fx.path().to_str().unwrap(),
        "--formula",
        "=SUM(A1:A3)",
    ]);
    assert_eq!(code, 0, "clean eval exits 0; got:\n{out}");
    assert_eq!(out.trim(), "6", "SUM(A1:A3) = 6:\n{out}");
}

#[test]
fn eval_number_routes_through_the_general_formatter() {
    // fsa1-ast::eval `num_to_text_matches_excel_general_format` freezes the exhaustive General spelling table; one case here proves the shell routes through it.
    let fx = Fixture::new("eval-general");
    fx.file("Sheet1", "A1", "0");
    let (code, out) = run(&["eval", fx.path().to_str().unwrap(), "--formula", "=1e20"]);
    assert_eq!(code, 0, "=1e20 evaluates cleanly:\n{out}");
    assert_eq!(out.trim(), "1E+20", "General-formats to 1E+20:\n{out}");
}

#[test]
fn eval_resolves_a_cross_tab_reference_against_the_named_tab() {
    let fx = Fixture::new("eval-cross");
    fx.file("Inputs", "A1", "10").file("Summary", "A1", "4");
    let (code, out) = run(&["eval", &at(&fx, "Summary"), "--formula", "=Inputs!A1*A1"]);
    assert_eq!(code, 0, "cross-tab eval exits 0:\n{out}");
    assert_eq!(out.trim(), "40", "Inputs!A1 * Summary!A1 = 40:\n{out}");
}

#[test]
fn eval_a_bad_formula_exits_non_zero() {
    let fx = Fixture::new("eval-bad");
    fx.file("Sheet1", "A1", "1");
    let (code, _) = run(&["eval", fx.path().to_str().unwrap(), "--formula", "=SUM("]);
    assert_eq!(code, 3, "a parse error is a validation exit (3)");
}

#[test]
fn eval_an_error_value_exits_non_zero() {
    let fx = Fixture::new("eval-err");
    fx.file("Sheet1", "A1", "1");
    let (code, out) = run(&["eval", fx.path().to_str().unwrap(), "--formula", "=1/0"]);
    assert_eq!(code, 3, "an error-valued result exits 3:\n{out}");
    assert_eq!(out.trim(), "#DIV/0!", "the error value is printed:\n{out}");
}

#[test]
fn eval_missing_formula_is_bad_args() {
    let fx = Fixture::new("eval-noformula");
    fx.file("Sheet1", "A1", "1");
    let (code, _) = run(&["eval", fx.path().to_str().unwrap()]);
    assert_eq!(code, 2, "eval with no formula is bad args (2)");
}

#[test]
fn bad_range_is_bad_args() {
    let fx = Fixture::new("badrange");
    fx.file("Sheet1", "A1", "1");
    let (code, _, err) = run_err(&["render", &at(&fx, "Sheet1/a1")]);
    assert_eq!(code, 2, "a non-canonical A1 selector is exit 2");
    assert!(
        err.contains("not a canonical A1 cell or range") && err.contains("no defined name"),
        "the refusal names both possibilities:\n{err}"
    );
}

#[test]
fn an_enormous_range_is_a_located_refusal_not_a_crash() {
    let fx = Fixture::new("hugerange");
    fx.file("Sheet1", "A1", "1");
    let (code, _) = run(&["render", &at(&fx, "Sheet1/A1:A4294967295")]);
    assert_eq!(
        code, 2,
        "an oversized region is a located refusal, not a crash"
    );
}

#[test]
fn format_json_is_now_an_unknown_flag() {
    let fx = Fixture::new("format-gone");
    fx.file("Sheet1", "A1", "1");
    let (code, _) = run(&["render", fx.path().to_str().unwrap(), "--format", "json"]);
    assert_eq!(code, 2, "`--format json` is now an unknown flag (exit 2)");
}

fn alias(fx: &Fixture, tab: &str, link: &str, target: &str) {
    let dir = fx.path().join(tab);
    std::fs::create_dir_all(&dir).expect("create tab dir");
    fsa1_model::write_name_alias(target, &dir.join(link)).expect("create name alias");
}

fn tree_fixture(tag: &str) -> Fixture {
    let fx = Fixture::new(tag);
    fx.file("Sheet1", "A1", "Product")
        .file("Sheet1", "B1", "10")
        .file("Sheet1", "C1", "=B1*2")
        .file("Sheet1", "A2:A5", "1\n2\n3\n4")
        .file("Sheet1", "Rate", "=B1*1.05");
    alias(&fx, "Sheet1", "Days.begin", "A2");
    alias(&fx, "Sheet1", "Days.end", "A5");
    std::fs::create_dir_all(fx.path().join(".cache")).unwrap();
    std::fs::write(fx.path().join(".cache").join("junk"), "regenerable").unwrap();
    fx
}

#[test]
fn tree_presents_every_authored_cell_and_name_and_excludes_cache() {
    let fx = tree_fixture("tree-complete");
    let (code, out) = run(&["tree", fx.path().to_str().unwrap()]);
    assert_eq!(code, 0, "clean tree exits 0:\n{out}");
    for cell in ["A1", "B1", "C1", "A2", "A3", "A4", "A5"] {
        assert!(out.contains(cell), "authored cell {cell} present:\n{out}");
    }
    assert!(out.contains("Days"), "the named range is present:\n{out}");
    assert!(out.contains("Rate"), "the named formula is present:\n{out}");
    assert!(
        out.contains("→ Sheet1!A2:A5"),
        "the symlinked range resolves to its target A1 ref:\n{out}"
    );
    assert!(
        out.contains("Sheet1/"),
        "the tab is a directory node:\n{out}"
    );
    assert!(
        !out.contains(".cache") && !out.contains("junk"),
        "the derived .cache/ must be excluded:\n{out}"
    );
}

#[test]
fn tree_functions_shows_source_and_values_shows_computed() {
    let fx = tree_fixture("tree-modes");
    let (fc, funcs) = run(&["tree", fx.path().to_str().unwrap(), "--mode", "functions"]);
    assert_eq!(fc, 0, "{funcs}");
    assert!(
        funcs.contains("C1  # =B1*2"),
        "--functions shows the authored formula, not its value:\n{funcs}"
    );
    assert!(
        funcs.contains("Rate  # =B1*1.05"),
        "--functions shows the named formula's definition:\n{funcs}"
    );

    let (vc, vals) = run(&["tree", fx.path().to_str().unwrap(), "--mode", "values"]);
    assert_eq!(vc, 0, "{vals}");
    assert!(
        vals.contains("C1  # 20"),
        "--values shows the computed value 10*2=20:\n{vals}"
    );
    assert!(
        vals.contains("Rate  # 10.5"),
        "--values shows the named formula's computed value 10*1.05=10.5:\n{vals}"
    );
    let (_, dflt) = run(&["tree", fx.path().to_str().unwrap()]);
    assert!(
        dflt.contains("C1  # 20 ← =B1*2"),
        "the default mode is combined (value ← source):\n{dflt}"
    );
    assert!(
        dflt.contains("Rate  # 10.5 ← =B1*1.05"),
        "a named formula in combined shows value ← source:\n{dflt}"
    );
    assert!(
        dflt.contains("B1  # 10") && !dflt.contains("B1  # 10 ←"),
        "a literal renders plain (no arrow) in combined:\n{dflt}"
    );
}

#[test]
fn tree_range_expands_a1_ordered_capped_and_elided() {
    let fx = Fixture::new("tree-cap");
    let body: String = (1..=60)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    fx.file("T", "A1:A60", &body);
    let (code, out) = run(&["tree", fx.path().to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    let a1 = out.find("A1 ").expect("A1 present");
    let a50 = out.find("A50 ").expect("A50 present");
    assert!(a1 < a50, "cells are A1-ordered (A1 before A50):\n{out}");
    assert!(
        !out.contains("A51 "),
        "past the cap, a coordinate is elided, not shown:\n{out}"
    );
    assert!(
        out.contains("[+10"),
        "the 10 over-cap cells are shown as an elided count:\n{out}"
    );
}

#[test]
fn tree_full_lifts_the_cap_so_the_elided_markers_hint_is_honest() {
    let fx = Fixture::new("tree-full");
    let body: String = (1..=60)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    fx.file("T", "A1:A60", &body);
    let (code, out) = run(&["tree", fx.path().to_str().unwrap(), "--full"]);
    assert_eq!(code, 0, "--full is an accepted flag (exit 0):\n{out}");
    assert!(
        out.contains("A51 ") && out.contains("A60 "),
        "--full expands past the default cap (A51 and A60 present):\n{out}"
    );
    assert!(
        !out.contains("use --full to expand"),
        "--full elides nothing, so the expand hint does not appear:\n{out}"
    );
}

#[test]
fn tree_region_path_shows_every_cell_uncapped_while_the_default_view_caps() {
    let fx = Fixture::new("tree-range");
    let body: String = (1..=60)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    fx.file("T", "A1:A60", &body);

    let (dc, dflt) = run(&["tree", fx.path().to_str().unwrap()]);
    assert_eq!(dc, 0, "{dflt}");
    assert!(
        !dflt.contains("A51 ") && dflt.contains("[+10"),
        "the implicit view keeps the cap (A51 elided, +10 marker):\n{dflt}"
    );

    let (rc, ranged) = run(&["tree", &at(&fx, "T/A1:A60")]);
    assert_eq!(rc, 0, "a tab-region path exits 0:\n{ranged}");
    for cell in ["A1 ", "A50 ", "A51 ", "A60 "] {
        assert!(
            ranged.contains(cell),
            "the explicit region shows every cell incl. {cell} (uncapped):\n{ranged}"
        );
    }
    assert!(
        !ranged.contains("use --full to expand") && !ranged.contains("[+"),
        "an explicit region elides nothing:\n{ranged}"
    );

    let (bc, _) = run(&["tree", fx.path().to_str().unwrap(), "--range", "A1:A60"]);
    assert_eq!(bc, 2, "--range is now an unknown flag (exit 2)");
}

#[test]
fn tree_collapses_a_grid5_array_formula_under_functions_and_expands_it_under_values() {
    let fx = Fixture::new("tree-grid5");
    fx.file("Sheet1", "A1:A3", "3\n1\n2")
        .file("Sheet1", "C1:C3", "=SORT(A1:A3)");

    let (fc, funcs) = run(&["tree", fx.path().to_str().unwrap(), "--mode", "functions"]);
    assert_eq!(fc, 0, "{funcs}");
    assert!(
        funcs.contains(&format!(
            "{}  # =SORT(A1:A3)",
            fsa1_model::range_file_name("C1:C3")
        )),
        "the array formula is ONE node at the range anchor:\n{funcs}"
    );
    assert!(
        !funcs.contains("C2"),
        "under --functions the array formula does not expand per coordinate:\n{funcs}"
    );

    let (vc, vals) = run(&["tree", fx.path().to_str().unwrap(), "--mode", "values"]);
    assert_eq!(vc, 0, "{vals}");
    assert!(
        !vals.contains(&fsa1_model::range_file_name("C1:C3")),
        "under --values the range file is expanded, not shown collapsed:\n{vals}"
    );
    for (label, elem) in [("C1", "1"), ("C2", "2"), ("C3", "3")] {
        assert!(
            vals.contains(&format!("{label}  # {elem}")),
            "the computed coordinate {label} = {elem} expands under --values:\n{vals}"
        );
    }
}

#[test]
fn tree_scope_roots_the_view_at_a_tab() {
    let fx = Fixture::new("tree-scope");
    fx.file("Alpha", "A1", "in-alpha")
        .file("Beta", "A1", "in-beta");
    let scope = fx.path().join("Alpha");
    let (code, out) = run(&["tree", scope.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("in-alpha"),
        "the scoped tab's cell is shown:\n{out}"
    );
    assert!(
        !out.contains("in-beta") && !out.contains("Beta/"),
        "a tab outside the scope is absent:\n{out}"
    );
    assert!(
        !out.contains("Alpha/"),
        "the scoped tab is the ROOT (its name is not printed as a dir):\n{out}"
    );
}

#[test]
fn tree_is_read_only_leaving_the_workbook_byte_identical() {
    let fx = tree_fixture("tree-readonly");
    let before = snapshot(fx.path());
    let (fc, _) = run(&["tree", fx.path().to_str().unwrap(), "--mode", "functions"]);
    let (vc, _) = run(&["tree", fx.path().to_str().unwrap(), "--mode", "values"]);
    assert_eq!((fc, vc), (0, 0));
    let after = snapshot(fx.path());
    assert_eq!(
        before, after,
        "tree must leave every authoritative cell/tab/name byte-identical (CORE3)"
    );
}

#[test]
fn tree_rejects_the_removed_format_flag_as_an_unknown_flag() {
    let fx = tree_fixture("tree-nojson");
    let (code, _) = run(&["tree", fx.path().to_str().unwrap(), "--format", "json"]);
    assert_eq!(code, 2, "`--format json` is now an unknown flag (exit 2)");
}

/// `tree` has no coordinate plane to mark, so it NAMES each figure beside the tab's cells and names:
/// the entry, then the mark it draws and the ranges it binds. A range-form figure keeps its own name,
/// a layer reports its children, and a figure binding nothing shows the mark and no arrow — there is
/// no range for one to point at.
#[test]
fn tree_names_each_figure_with_the_mark_it_draws_and_the_ranges_it_binds() {
    let fx = Fixture::new("tree-figures");
    fx.file("Sheet1", "A1:B3", "1\t2\n3\t4\n5\t6")
        .file("Sheet2", "A1:A3", "7\n8\n9")
        .file(
            "Sheet1",
            "D2:K17.json",
            "{\"data\":{\"name\":\"A1:B3\"},\"mark\":\"bar\"}",
        )
        .file(
            "Sheet1",
            "Chart1.json",
            "{\"layer\":[{\"data\":{\"name\":\"A1:B3\"},\"mark\":\"line\"},\
             {\"data\":{\"name\":\"Sheet2!A1:A3\"},\"mark\":\"point\"}]}",
        )
        .file("Sheet1", "Bare.json", "{\"mark\":\"point\"}");

    let (code, out) = run(&["tree", fx.path().to_str().unwrap()]);
    assert_eq!(code, 0, "a workbook holding figures trees clean:\n{out}");
    assert!(
        out.contains(&format!(
            "{}  # bar ← A1:B3",
            fsa1_model::range_file_name("D2:K17.json")
        )),
        "a range-form figure is named by its entry, its mark and its one binding:\n{out}"
    );
    assert!(
        out.contains("Chart1.json  # layer(line, point) ← A1:B3, Sheet2!A1:A3"),
        "a layer names its child marks and every range it binds, in order:\n{out}"
    );
    assert!(
        out.contains("Bare.json  # point\n"),
        "a figure that binds nothing shows the mark and no arrow:\n{out}"
    );
}

#[test]
fn accept01_path_tab_renders_the_named_tab() {
    let fx = Fixture::new("a1-pathtab");
    fx.file("Orders", "A1", "111").file("Summary", "A1", "999");
    let (code, out) = run(&["render", &at(&fx, "Summary")]);
    assert_eq!(code, 0, "a path tab renders:\n{out}");
    assert!(out.contains("999"), "the Summary tab is drawn:\n{out}");
    assert!(!out.contains("111"), "not the Orders tab:\n{out}");
}

#[test]
fn accept02_permissive_region_and_single_cell() {
    let fx = Fixture::new("a2-region");
    fx.file("Sheet1", "A1:C1", "10\t20\t30")
        .file("Sheet1", "B3", "77");
    let (code, out) = run(&["render", &at(&fx, "Sheet1/A1:C1")]);
    assert_eq!(code, 0, "a region renders:\n{out}");
    for v in ["10", "20", "30"] {
        assert!(out.contains(v), "the region cell {v} is drawn:\n{out}");
    }
    let (code, out) = run(&["render", &at(&fx, "Sheet1/B3")]);
    assert_eq!(code, 0, "a single-cell region renders:\n{out}");
    assert!(out.contains("77"), "the single cell B3 is drawn:\n{out}");
}

#[test]
fn accept03_no_clipping_pads_blank() {
    let fx = Fixture::new("a3-noclip");
    fx.file("Sheet1", "A1:D9", "x\tx\tx\tx\n".repeat(9).trim_end());
    let (code, out) = run(&["render", &at(&fx, "Sheet1/A1:F20")]);
    assert_eq!(
        code, 0,
        "an out-of-used-region rect renders (padded):\n{out}"
    );
    assert!(
        out.contains("| E ") && out.contains("| F "),
        "cols E,F present:\n{out}"
    );
    assert!(
        out.contains(" 20 "),
        "row 20 present (not clipped to row 9):\n{out}"
    );
}

#[test]
fn accept04_cross_tab_resolves_under_path_selection() {
    let fx = Fixture::new("a4-crosstab");
    fx.file("Assumptions", "B6", "42")
        .file("Model", "A1", "=Assumptions!B6");
    let (code, out) = run(&["render", &at(&fx, "Model"), "--mode", "values"]);
    assert_eq!(code, 0, "the cross-tab ref resolves:\n{out}");
    assert!(out.contains("42"), "=Assumptions!B6 resolves to 42:\n{out}");
}

#[test]
fn accept05_mode_enum_all_values_and_unknown() {
    let fx = Fixture::new("a5-mode");
    fx.file("Sheet1", "A1", "2").file("Sheet1", "B1", "=A1*2");
    for m in ["combined", "values", "functions"] {
        let (code, out) = run(&["render", fx.path().to_str().unwrap(), "--mode", m]);
        assert_eq!(code, 0, "--mode {m} exits 0:\n{out}");
    }
    let (code, _, err) = run_err(&["render", fx.path().to_str().unwrap(), "--mode", "raw"]);
    assert_eq!(code, 2, "an unknown --mode is bad args");
    assert!(
        err.contains("combined") && err.contains("values") && err.contains("functions"),
        "the refusal lists the three valid values:\n{err}"
    );
    let (vc, _) = run(&["render", fx.path().to_str().unwrap(), "--values"]);
    let (fc, _) = run(&["render", fx.path().to_str().unwrap(), "--functions"]);
    assert_eq!(
        (vc, fc),
        (2, 2),
        "--values/--functions are now unknown flags"
    );
}

/// The carrier an agent redirects to a file: stdout is the whole document and nothing else, and it
/// is a grid of elements — no `<table>`, whose one stylesheet had nowhere to put a scoped rule.
#[test]
fn render_format_html_writes_one_document_to_stdout() {
    let fx = Fixture::new("html");
    fx.file("Sheet1", "A1", "2").file("Sheet1", "B1", "=A1*2");
    let (code, out, err) = run_err(&["render", fx.path().to_str().unwrap(), "--format", "html"]);
    assert_eq!(code, 0, "a clean html render exits 0:\n{out}{err}");
    assert!(
        out.starts_with("<!doctype html>") && out.trim_end().ends_with("</html>"),
        "stdout is one whole document:\n{out}"
    );
    assert!(
        !out.contains("<td") && !out.contains("<table"),
        "the grid is elements the sidecars address, never a table:\n{out}"
    );
    assert!(
        out.contains(">A</fsa1-head>") && out.contains(">1</fsa1-head>"),
        "column letters and row numbers ride as <fsa1-head>:\n{out}"
    );

    // A page draws values and shows a formula in its bar, so a `--mode` has nothing left to pick.
    for mode in ["values", "functions", "combined"] {
        let (code, _, err) = run_err(&[
            "render",
            fx.path().to_str().unwrap(),
            "--format",
            "html",
            "--mode",
            mode,
        ]);
        assert_eq!(code, 3, "--mode {mode} with html is refused");
        assert!(err.contains("formula bar"), "{mode}: {err}");
    }
    assert!(
        out.contains("<fsa1-cell data-ref=\"B1\" data-formula=\"=A1*2\""),
        "a formula rides as an attribute, not as cell text:\n{out}"
    );
    assert!(
        !out.contains('←'),
        "no cell carries the combined arrow:\n{out}"
    );
}

/// What a sidecar's carrier cannot hold is text that ends open: an unclosed comment or bracket
/// swallows every later rule, and `</style` ends the raw-text element the bytes ride in outright.
/// The verb that DRAWS one refuses before writing a document; `check` grades the model, which none
/// of these is anything to, and still passes.
#[test]
fn render_format_html_refuses_text_its_carrier_cannot_hold() {
    for spelling in ["</style>", "</STYLE>", "/*evil", "[evil"] {
        let fx = Fixture::new("html-carrier");
        fx.file("Sheet1", "A1:A2", "x\ny").file(
            "Sheet1",
            "A1:A2.css",
            &format!(
                "  fsa1-cell {{ color: #ff0000 }}\n  fsa1-row:first-child fsa1-cell {{ \
                 font-family: Ar{spelling}ial }}\n"
            ),
        );
        let root = fx.path().to_str().unwrap();
        let (code, _out, err) = run_err(&["render", root, "--format", "html"]);
        assert_eq!(code, 3, "{spelling}: the carrier refuses:\n{err}");
        assert!(
            err.contains("Sheet1/A1:A2.css:2:"),
            "{spelling}: the refusal locates the sidecar:\n{err}"
        );
        let (code, out, err) = run_err(&["check", root]);
        assert_eq!(code, 0, "{spelling}: check grades the MODEL:\n{out}{err}");
    }
}

#[test]
fn render_unknown_format_is_invalid_arguments() {
    let fx = Fixture::new("badformat");
    fx.file("Sheet1", "A1", "1");
    let (code, _, err) = run_err(&["render", fx.path().to_str().unwrap(), "--format", "pdf"]);
    assert_eq!(code, 2, "an unknown --format is bad args");
    assert!(
        err.contains("ascii") && err.contains("html"),
        "the refusal lists the two valid values:\n{err}"
    );
}

#[test]
fn accept06_disjoint_region_note_but_still_exit_zero() {
    let fx = Fixture::new("a6-disjoint");
    fx.file("Sheet1", "A1:D9", "x\tx\tx\tx\n".repeat(9).trim_end());
    let (code, out, err) = run_err(&["render", &at(&fx, "Sheet1/Q99:Q200")]);
    assert_eq!(code, 0, "a disjoint region still exits 0:\n{out}");
    assert!(
        err.contains("lies entirely outside the tab's used region"),
        "the disjoint note names both rectangles:\n{err}"
    );
    assert!(
        err.contains("Q99:Q200") && err.contains("A1:D9"),
        "note names both rects:\n{err}"
    );

    let (code, _out, err) = run_err(&["render", &at(&fx, "Sheet1/C8:F12")]);
    assert_eq!(code, 0, "a partial overlap exits 0");
    assert!(
        !err.contains("lies entirely outside"),
        "a partial overlap emits no note:\n{err}"
    );
}

#[test]
fn accept07_and_20_position_based_trailing_rule() {
    let fx = Fixture::new("a7-position");
    fx.file("Model", "A1", "1");

    let (code, _, err) = run_err(&["render", &at(&fx, "Model/total")]);
    assert_eq!(code, 2, "a final unknown non-A1 segment is bad args");
    assert!(
        err.contains("not a canonical A1 cell or range") && err.contains("no defined name"),
        "the refusal names both possibilities:\n{err}"
    );

    let (code, _, err) = run_err(&["render", &at(&fx, "Nope")]);
    assert_eq!(code, 2, "a final non-folder non-A1 is a bad-args refusal");
    assert!(
        err.contains("no defined name") && !err.contains("no tab named"),
        "a FINAL segment is NOT 'no tab named':\n{err}"
    );

    let (code, _, err) = run_err(&["render", &at(&fx, "Nope/A1")]);
    assert_eq!(code, 24, "a non-final missing tab is not-found");
    assert!(
        err.contains("no tab named"),
        "the tab-position refusal:\n{err}"
    );

    let (code, _, err) = run_err(&["render", &at(&fx, "Model/b2")]);
    assert_eq!(code, 2, "a lowercase selector is bad args");
    assert!(
        err.contains("not a canonical A1 cell or range") && err.contains("no defined name"),
        "the combined-wording refusal:\n{err}"
    );
}

/// Tabs sort alphabetically (Assumptions, Data, Model), so the default first tab is Assumptions.
fn name_fixture(tag: &str) -> Fixture {
    let fx = Fixture::new(tag);
    fx.file("Model", "A1", "header")
        .file("Model", "B5", "55")
        .file("Model", "B6", "11")
        .file("Model", "A2:A4", "10\n20\n30")
        .file("Model", "total", "=B5")
        .file("Model", "Days", "=A2:A4")
        .file("Model", "Rate", "=Base*1.05")
        .file("Model", "elsewhere", "=Assumptions!B6")
        .file("Assumptions", "B6", "99")
        .file("Data", "A1", "77");
    std::fs::write(fx.path().join("anchor"), "=Data!A1").expect("write workbook-scoped name");
    alias(&fx, "Model", "blk_total", "B5");
    fx
}

#[test]
fn accept02_named_cell_and_range_address_a_region_both_forms() {
    let fx = name_fixture("a2-name-forms");

    let (code, out) = run(&["render", &at(&fx, "Model/total")]);
    assert_eq!(code, 0, "a named cell renders:\n{out}");
    assert!(out.contains("55"), "total resolves to B5=55:\n{out}");

    let (scode, sout) = run(&["render", &at(&fx, "Model/blk_total")]);
    assert_eq!(scode, 0, "the symlink-form name renders:\n{sout}");
    assert_eq!(
        out, sout,
        "symlink and ref-file forms of the same target render the same grid"
    );

    let (rcode, rout) = run(&["render", &at(&fx, "Model/Days")]);
    assert_eq!(rcode, 0, "a named range renders:\n{rout}");
    for v in ["10", "20", "30"] {
        assert!(rout.contains(v), "Days range value {v} present:\n{rout}");
    }
}

#[test]
fn accept03_bare_workbook_scoped_name_uses_the_default_tab() {
    let fx = name_fixture("a3-bare-name");
    let (code, out) = run(&["render", &at(&fx, "anchor")]);
    assert_eq!(code, 0, "a bare workbook-scoped name renders:\n{out}");
    assert!(out.contains("77"), "anchor resolves to Data!A1=77:\n{out}");
}

#[test]
fn accept04_trace_named_single_cell_and_named_range_refusal() {
    let fx = name_fixture("a4-trace-name");

    let (code, out) = run(&["trace", &at(&fx, "Model/total")]);
    let (acode, aout) = run(&["trace", &at(&fx, "Model/B5")]);
    assert_eq!(code, 0, "a named single cell traces:\n{out}");
    assert_eq!(
        (code, &out),
        (acode, &aout),
        "a named cell traces identically to its A1 cell"
    );

    let (rcode, _, rerr) = run_err(&["trace", &at(&fx, "Model/Days")]);
    assert_eq!(rcode, 2, "a named range is refused by trace");
    assert!(
        rerr.contains("trace targets one cell") && rerr.contains("Days"),
        "the range refusal names the name:\n{rerr}"
    );
}

#[test]
fn accept05_cross_tab_named_ref_targets_the_other_tab() {
    let fx = name_fixture("a5-crosstab");

    let (code, out) = run(&["render", &at(&fx, "Model/elsewhere")]);
    assert_eq!(code, 0, "a cross-tab name renders:\n{out}");
    assert!(out.contains("99"), "elsewhere -> Assumptions!B6=99:\n{out}");
    assert!(!out.contains("11"), "NOT Model's own B6=11:\n{out}");

    let (ccode, _) = run(&["check", &at(&fx, "Model/elsewhere")]);
    assert_eq!(ccode, 0, "check scopes cleanly to the cross-tab region");
}

#[test]
fn accept06_named_formula_is_a_located_refusal() {
    let fx = name_fixture("a6-expr");
    let (code, _, err) = run_err(&["render", &at(&fx, "Model/Rate")]);
    assert_eq!(
        code, 2,
        "a named formula/constant is refused (exit 2):\n{err}"
    );
    assert!(
        err.contains("is a named formula/constant, not a cell or range"),
        "the Expr refusal message:\n{err}"
    );
}

#[test]
fn accept07_unknown_and_botched_final_segment_name_both_possibilities() {
    let fx = name_fixture("a7-unknown");

    let (code, _, err) = run_err(&["render", &at(&fx, "Model/nope")]);
    assert_eq!(code, 2, "an unknown name is bad args");
    assert!(
        err.contains("not a canonical A1 cell or range") && err.contains("no defined name"),
        "the combined refusal:\n{err}"
    );

    let (bcode, _, berr) = run_err(&["render", &at(&fx, "Model/b2")]);
    assert_eq!(bcode, 2, "a lowercase A1 is the same refusal");
    assert!(
        berr.contains("not a canonical A1 cell or range") && berr.contains("no defined name"),
        "the same combined refusal for a botched A1:\n{berr}"
    );
}

#[test]
fn accept08_check_name_scoping_and_the_non_loading_edge() {
    let fx = Fixture::new("a8-scope");
    fx.file("Model", "A1", "header")
        .file("Model", "A2:A4", "10\n20\n30")
        .file("Model", "G1", "=bad(")
        .file("Model", "Days", "=A2:A4")
        .file("Model", "broken", "=G1");

    let (bcode, bout) = run(&["check", &at(&fx, "Model/broken")]);
    assert_eq!(
        bcode, 3,
        "a name scoping the error cell reports it (exit 3):\n{bout}"
    );

    let (ccode, cout) = run(&["check", &at(&fx, "Model/Days")]);
    assert_eq!(
        ccode, 0,
        "a name scoping a clean region passes despite the error outside it:\n{cout}"
    );

    let broken = Fixture::new("a8-noload");
    broken
        .file("Model", "A1:D9", "one literal in a 9x4 range")
        .file("Model", "total", "=B5");
    let (bcode, _) = run(&["check", &at(&broken, "Model/total")]);
    assert_eq!(
        bcode, 3,
        "the load failure dominates a name lookup on a broken workbook"
    );
}

#[test]
fn accept09_tree_gains_name_addressing_uniformly() {
    let fx = name_fixture("a9-tree-name");
    let (code, out) = run(&["tree", &at(&fx, "Model/Days")]);
    assert_eq!(code, 0, "tree of a named range exits 0:\n{out}");
    assert!(
        out.contains("10"),
        "the named range's cells are in the tree:\n{out}"
    );
    assert!(
        !out.contains("55"),
        "an out-of-scope cell (B5) is not in the scoped tree:\n{out}"
    );
}

#[test]
fn accept10_scope_shadowing_sheet_over_workbook() {
    let fx = Fixture::new("a10-shadow");
    fx.file("Model", "A1", "h")
        .file("Model", "B5", "55")
        .file("Model", "Anchor", "=B5")
        .file("Other", "A1", "1")
        .file("Data", "A1", "77");
    std::fs::write(fx.path().join("Anchor"), "=Data!A1").expect("write workbook-scoped Anchor");

    let (c1, o1) = run(&["render", &at(&fx, "Model/Anchor")]);
    assert_eq!(c1, 0, "{o1}");
    assert!(
        o1.contains("55"),
        "sheet-scoped Anchor -> Model!B5=55:\n{o1}"
    );

    let (c2, o2) = run(&["render", &at(&fx, "Other/Anchor")]);
    assert_eq!(c2, 0, "{o2}");
    assert!(
        o2.contains("77"),
        "workbook-scoped Anchor -> Data!A1=77 on Other:\n{o2}"
    );
}

#[test]
fn accept11_eval_refuses_a_name_but_a_name_inside_a_formula_resolves() {
    let fx = name_fixture("a11-eval");

    let (code, _, err) = run_err(&["eval", &at(&fx, "Model/total"), "--formula", "=1"]);
    assert_eq!(code, 2, "eval refuses a name-in-path:\n{err}");
    assert!(
        err.contains("eval takes <wb> or <wb>/<tab>"),
        "the eval region refusal:\n{err}"
    );

    let (ecode, eout) = run(&["eval", &at(&fx, "Model"), "--formula", "=SUM(Days)"]);
    assert_eq!(ecode, 0, "a name inside a formula evaluates:\n{eout}");
    assert!(eout.contains("60"), "=SUM(Days)=10+20+30=60:\n{eout}");
}

#[test]
fn name_target_on_a_spaced_tab_resolves_via_the_quoted_sheet_split() {
    // A target on a spaced tab is stored quoted (`'Cash Flows'!B2`), so this reaches the cross-tab split and the quote-strip branch of `name_ref_to_region`.
    let fx = Fixture::new("name-quoted-sheet");
    fx.file("Model", "A1", "header")
        .file("Cash Flows", "B2", "888");
    alias(&fx, "Model", "flow", "../Cash Flows/B2");

    let (code, out) = run(&["render", &at(&fx, "Model/flow")]);
    assert_eq!(code, 0, "a name into a spaced tab resolves:\n{out}");
    assert!(
        out.contains("888"),
        "flow -> 'Cash Flows'!B2 = 888 (quoted-sheet split + unquote):\n{out}"
    );
}

#[test]
fn unqualified_workbook_scoped_name_resolves_against_the_scope_tab() {
    let fx = Fixture::new("name-unqualified-wb");
    fx.file("Model", "A1", "header").file("Model", "B5", "55");
    std::fs::write(fx.path().join("localref"), "=B5")
        .expect("write an unqualified workbook-scoped name");

    let (code, out) = run(&["render", &at(&fx, "Model/localref")]);
    assert_eq!(
        code, 0,
        "an unqualified workbook-scoped name resolves against the scope tab:\n{out}"
    );
    assert!(
        out.contains("55"),
        "localref -> Model!B5 = 55 via the scope tab:\n{out}"
    );
}

#[test]
fn accept09_trace_single_cell_and_range_refusal() {
    let fx = Fixture::new("a9-trace");
    fx.file("Sheet1", "A1", "10").file("Sheet1", "B7", "=A1*2");
    let (code, out) = run(&["trace", &at(&fx, "Sheet1/B7")]);
    assert_eq!(code, 0, "a single-cell trace exits 0:\n{out}");
    assert!(out.contains("B7"), "the traced cell is the root:\n{out}");
    assert!(
        out.contains("A1"),
        "its upstream dependency A1 is shown:\n{out}"
    );

    let (code, _, err) = run_err(&["trace", &at(&fx, "Sheet1/B2:D9")]);
    assert_eq!(code, 2, "a range selector is refused (exit 2)");
    assert!(
        err.contains("range") && err.contains("one cell"),
        "the range refusal is located:\n{err}"
    );

    let (code, _) = run(&["trace", &at(&fx, "Sheet1"), "--cell", "B7"]);
    assert_eq!(code, 2, "--cell is now an unknown flag");
}

#[test]
fn accept10_eval_refuses_a_region_selector() {
    let fx = Fixture::new("a10-eval");
    fx.file("Inputs", "A1", "5").file("Model", "A1", "7");
    let (code, out) = run(&["eval", &at(&fx, "Model"), "--formula", "=A1"]);
    assert_eq!(
        (code, out.trim()),
        (0, "7"),
        "eval binds to the path tab Model"
    );

    let (code, _, err) = run_err(&["eval", &at(&fx, "Model/A1"), "--formula", "=A1"]);
    assert_eq!(code, 2, "a region selector on eval is refused");
    assert!(err.contains("not a region"), "the located refusal:\n{err}");
}

#[test]
fn accept11_cli_tab_qualifier_removed_but_formula_tab_ref_still_evaluates() {
    let fx = Fixture::new("a11-tabref");
    fx.file("Sheet2", "A1", "41").file("Model", "A1", "1");
    let (code, out) = run(&["eval", &at(&fx, "Model"), "--formula", "=Sheet2!A1+1"]);
    assert_eq!(
        (code, out.trim()),
        (0, "42"),
        "the formula Tab!A1 ref evaluates:\n{out}"
    );
}

#[test]
fn accept13_absolute_relative_and_bare_roots_resolve() {
    let fx = Fixture::new("a13-roots");
    fx.file("demo", "A1", "123");

    let (code, out) = run(&["render", &at(&fx, "demo")]);
    assert_eq!(code, 0, "an absolute path tab renders:\n{out}");
    assert!(out.contains("123"), "the demo tab draws:\n{out}");

    let parent = fx.path().parent().unwrap();
    let bare = fx.path().file_name().unwrap().to_str().unwrap();
    let (code, out) = run_in(parent, &["render", bare]);
    assert_eq!(code, 0, "a bare workbook name resolves against cwd:\n{out}");
    assert!(out.contains("123"), "the bare-name workbook draws:\n{out}");
    let rel = format!("./{bare}");
    let (code, out) = run_in(parent, &["render", &rel]);
    assert_eq!(code, 0, "a ./-relative workbook resolves:\n{out}");
    assert!(
        out.contains("123"),
        "the ./-relative workbook draws:\n{out}"
    );
}

#[test]
fn accept14_filesystem_type_classification() {
    let fx = Fixture::new("a14-fstype");
    fx.file("Model", "A1", "500").file("B2", "A1", "888");

    let (code, out) = run(&["render", &at(&fx, "B2")]);
    assert_eq!(code, 0, "a folder named B2 renders as a tab:\n{out}");
    assert!(out.contains("888"), "tab B2's cell A1 draws:\n{out}");

    let (code, out) = run(&["render", &at(&fx, "D4")]);
    assert_eq!(
        code, 0,
        "a non-folder A1 is a selector on the default tab:\n{out}"
    );
    assert!(
        out.contains("| D "),
        "the selector D4 draws on the default tab:\n{out}"
    );

    let (code, out) = run(&["render", &at(&fx, "Model/A1")]);
    assert_eq!(
        code, 0,
        "an A1 on an explicit tab is a region there:\n{out}"
    );
    assert!(out.contains("500"), "Model!A1 draws:\n{out}");
}

#[test]
fn accept19_broken_empty_singletab_and_broken_sibling() {
    let broken = Fixture::new("a19-broken");
    broken.file("Sheet1", "A1:D9", "one literal in a 9x4 range");
    let (code, _) = run(&["render", broken.path().to_str().unwrap()]);
    assert_eq!(code, 3, "a broken root refuses at exit 3");
    let (code, out) = run(&["check", broken.path().to_str().unwrap()]);
    assert_eq!(
        code, 3,
        "an unscoped check on a broken root is exit 3:\n{out}"
    );
    assert!(
        out.contains("dimension-mismatch"),
        "the load fault is surfaced:\n{out}"
    );

    let empty = Fixture::new("a19-empty");
    std::fs::create_dir_all(empty.path().join("E")).unwrap();
    for cmd in [
        vec!["render", &*at(&empty, "E")],
        vec!["eval", &*at(&empty, "E"), "--formula", "=1"],
    ] {
        let (code, _, err) = run_err(&cmd);
        assert_eq!(code, 3, "an empty root is exit 3");
        assert!(
            err.contains("has no tabs"),
            "the has-no-tabs guard fires:\n{err}"
        );
    }

    let single = Fixture::new("a19-single");
    single.file("Model", "A1", "7");
    let (code, out) = run(&["render", &at(&single, "Model")]);
    assert_eq!(code, 0, "the single-tab workbook renders:\n{out}");
    assert!(out.contains("7"), "Model draws:\n{out}");

    let et = Fixture::new("a19-emptytab");
    et.file("Model", "A1", "1");
    std::fs::create_dir_all(et.path().join("EmptyTab")).unwrap();
    let (code, out) = run(&["render", &at(&et, "EmptyTab")]);
    assert_eq!(
        code, 0,
        "an empty tab of a workbook renders at exit 0 (not 'has no tabs'):\n{out}"
    );

    let sib = Fixture::new("a19-sibling");
    sib.file("Good", "A1", "1")
        .file("Bad", "A1:D9", "one literal in a 9x4 range");
    let (code, out) = run(&["check", &at(&sib, "Good/Z1:Z2")]);
    assert_eq!(
        code, 0,
        "a Good-tab region missing the Bad fault reads green:\n{out}"
    );
    assert!(
        !out.contains("dimension-mismatch"),
        "the Bad-tab fault is out of the region scope:\n{out}"
    );
    let (code, out) = run(&["check", &at(&sib, "Good")]);
    assert_eq!(
        code, 3,
        "a bare tab scope surfaces the load fault (best-effort):\n{out}"
    );
    assert!(
        !out.contains("no tab named") && !out.contains("no such workbook"),
        "Good resolved as a tab — not misrouted:\n{out}"
    );
}

#[test]
fn accept18_help_leads_with_the_path_form() {
    let (code, out) = run(&["render", "--help"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("--mode") && out.contains("--format"),
        "render help documents --mode and --format:\n{out}"
    );
    assert!(
        !out.contains("--tab") && !out.contains("--range"),
        "no removed flags in render help:\n{out}"
    );
    let (_, out) = run(&["check", "--help"]);
    assert!(
        !out.contains("--tab") && !out.contains("--cell"),
        "no removed flags in check help:\n{out}"
    );
    let (_, out) = run(&["trace", "--help"]);
    assert!(!out.contains("--cell"), "no --cell in trace help:\n{out}");
    let empty = Fixture::new("a18-guide");
    std::fs::create_dir_all(empty.path().join("E")).unwrap();
    let (_, _, err) = run_err(&["render", &at(&empty, "E")]);
    assert!(
        err.contains("<workbook>/<tab>"),
        "the has-no-tabs refusal teaches the path form:\n{err}"
    );
}

fn tree_cells(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|l| {
            let (head, text) = l.split_once("  # ")?;
            let cell = head.rsplit(' ').next()?;
            Some((cell.to_string(), text.trim_end().to_string()))
        })
        .collect()
}

fn render_column_cells(out: &str, col: &str) -> Vec<(String, String)> {
    out.lines()
        .filter(|l| l.starts_with('|'))
        .filter_map(|l| {
            let mut parts = l.split('|').map(str::trim);
            parts.next()?;
            let row = parts.next()?;
            let text = parts.next()?;
            row.parse::<u32>().ok()?; // skips the header row, whose gutter is empty
            Some((format!("{col}{row}"), text.to_string()))
        })
        .collect()
}

#[test]
fn tree_and_render_report_the_same_value_for_every_cell_of_a_deep_chain() {
    // A cell's value derives from its content, never from the chain length or the filename order a walk happened to visit in (`A1, A10, A100, A1000, …`).
    let fx = Fixture::new("chain-1200");
    let n = 1200usize;
    for i in 1..n {
        fx.file("S", &format!("A{i}"), &format!("=A{}+1", i + 1));
    }
    fx.file("S", &format!("A{n}"), "1");

    let (tc, tout) = run(&["tree", fx.path().to_str().unwrap(), "--mode", "values"]);
    assert_eq!(tc, 0, "tree exits 0:\n{tout}");
    let (rc, rout) = run(&[
        "render",
        &at(&fx, &format!("S/A1:A{n}")),
        "--mode",
        "values",
    ]);
    assert_eq!(rc, 0, "render exits 0:\n{rout}");

    let mut from_tree = tree_cells(&tout);
    let mut from_render = render_column_cells(&rout, "A");
    assert_eq!(from_tree.len(), n, "tree shows every chain cell");
    assert_eq!(from_render.len(), n, "render shows every chain cell");
    from_tree.sort();
    from_render.sort();
    assert_eq!(
        from_tree, from_render,
        "tree and render must agree on every cell of the chain"
    );
    for (cell, text) in &from_tree {
        let i: usize = cell[1..].parse().unwrap();
        assert_eq!(
            *text,
            (n - i + 1).to_string(),
            "{cell} computes from its content"
        );
    }
}

#[test]
fn scope_and_output_form_are_independent_axes() {
    let fx = Fixture::new("six-ways");
    fx.file("Alpha", "A1", "2")
        .file("Alpha", "A2", "=A1*3")
        .file("Beta", "A1", "=Alpha!A2+1");

    for verb in ["render", "tree"] {
        let (code, out) = run(&[verb, fx.path().to_str().unwrap(), "--mode", "values"]);
        assert_eq!(code, 0, "{verb} <wb> exits 0:\n{out}");
        assert!(
            out.contains('6') && out.contains('7'),
            "{verb} <wb>:\n{out}"
        );
        assert!(
            out.contains("Alpha") && out.contains("Beta"),
            "{verb} <wb> names both sheets:\n{out}"
        );

        let (code, out) = run(&[verb, &at(&fx, "Beta"), "--mode", "values"]);
        assert_eq!(code, 0, "{verb} <wb>/<tab> exits 0:\n{out}");
        assert!(out.contains('7'), "{verb} <wb>/<tab>:\n{out}");

        let (code, out) = run(&[verb, &at(&fx, "Alpha/A2"), "--mode", "values"]);
        assert_eq!(code, 0, "{verb} <wb>/<tab>/<A1> exits 0:\n{out}");
        assert!(out.contains('6'), "{verb} <wb>/<tab>/<A1>:\n{out}");
    }
}

#[test]
fn trace_prints_a_100k_link_cone_instead_of_overflowing_the_stack() {
    // The walk, the print and the tree's own DROP each have to be iterative: any one recursing per link aborts the process, which is neither a value nor a located refusal.
    const LINKS: usize = 100_000;
    let fx = Fixture::new("trace-100k");
    let mut grid = String::with_capacity(LINKS * 10);
    for i in 1..LINKS {
        grid.push_str(&format!("=A{}+1\n", i + 1));
    }
    grid.push_str("0\n");
    fx.file("S", &format!("A1:A{LINKS}"), &grid);

    let (code, out) = run(&["trace", &at(&fx, "S/A1")]);
    assert_eq!(
        code, 0,
        "a {LINKS}-link trace exits 0, never a stack overflow"
    );
    assert_eq!(
        out.lines().count(),
        LINKS,
        "one line per link, none dropped"
    );
    assert!(
        out.starts_with(&format!("S!A1  =A2+1  -> {}  [", LINKS - 1)),
        "the root computes through the whole chain: {}",
        out.lines().next().unwrap_or_default()
    );
    let deepest = out.lines().next_back().expect("a last line");
    assert!(
        deepest
            .trim_start()
            .starts_with(&format!("S!A{LINKS}  -> 0  [")),
        "the last line is the literal at the bottom of the chain: {deepest}"
    );
    let widest = out.lines().map(str::len).max().unwrap_or(0);
    assert!(
        widest < 512,
        "no line may carry a depth-proportional left margin; widest was {widest}"
    );

    let (code, capped) = run(&["trace", &at(&fx, "S/A1"), "--depth", "2"]);
    assert_eq!(code, 0, "a capped trace exits 0:\n{capped}");
    assert_eq!(
        capped.lines().count(),
        3,
        "the root plus two levels:\n{capped}"
    );
}

/// The routing a workbook and its presentation loading SEPARATELY creates: a sidecar that will not
/// parse is a fault of the verbs that draw or lint presentation, and invisible to the ones that do
/// not open one. Deleting either half of `check`'s fold leaves this the only witness.
#[test]
fn a_malformed_sidecar_reaches_only_the_verbs_that_read_presentation() {
    let fx = Fixture::new("sidecar-routing");
    fx.file("Sheet1", "A1", "20000")
        .file("Sheet1", "B1", "=A1*2")
        .file("Sheet1", "A1:B1.css", "  fsa1-cell { color: crimson }\n");
    let root = fx.path().to_str().unwrap();

    let (code, out) = run(&["check", root]);
    assert_eq!(code, 3, "check reports the sidecar located; got:\n{out}");
    assert!(
        out.contains("presentation-value") && out.contains("Sheet1/A1:B1.css"),
        "check names the fault and where it is:\n{out}"
    );

    for verb in [vec!["render", root, "--format", "html"], vec!["pack", root]] {
        let (code, out) = run(&verb);
        assert_eq!(
            code, 3,
            "{verb:?} draws presentation, so it refuses:\n{out}"
        );
    }

    // The ASCII table opens the sidecar for AXIS RUNS alone, and this fixture holds no figure whose cover needs measuring in them -- so it opens none.
    let cell = at(&fx, "Sheet1/B1");
    for verb in [
        vec!["render", root],
        vec!["eval", root, "--formula", "=B1"],
        vec!["tree", root],
        vec!["trace", cell.as_str()],
    ] {
        let (code, out) = run(&verb);
        assert_eq!(code, 0, "{verb:?} opens no sidecar, so it answers:\n{out}");
    }
}

/// A figure is a TAB's, and the workbook root holds no coordinates for one to bind. Without the
/// refusal the entry falls into the defined-name arm and is claimed as a name.
#[test]
fn a_root_level_figure_is_a_located_refusal() {
    let fx = Fixture::new("figure-root");
    fx.file("Sheet1", "A1", "1");
    std::fs::write(fx.path().join("stray.json"), "{\"mark\":\"bar\"}").expect("write");
    let (code, out) = run(&["check", &fx.path().display().to_string()]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("figure-in-root"), "{out}");
    assert!(out.contains("stray.json"), "{out}");
}

/// The one tree the range form turns from ACCEPTED into a refusal: `Q4.json` states its own
/// placement in its name, so `Q4.css` beside it is a second, contradicting answer. A name-form
/// figure takes the same sidecar and is clean, so the refusal is the FORM's and not the pairing's.
#[test]
fn a_sidecar_over_a_range_named_figure_is_a_located_clash() {
    let spec = "{\"data\":{\"name\":\"A1:B1\"},\"mark\":\"bar\"}";
    let placement = "  figure { anchor: A1:B2 }\n";

    let fx = Fixture::new("figure-sidecar-clash");
    fx.file("Sheet1", "A1", "1")
        .file("Sheet1", "B1", "2")
        .file("Sheet1", "Q4.json", spec)
        .file("Sheet1", "Q4.css", placement);
    let (code, out) = run(&["check", fx.path().to_str().unwrap()]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("figure-sidecar-clash"), "{out}");
    assert!(out.contains("Sheet1/Q4.css"), "{out}");
    assert!(
        out.contains("Q4") && out.contains("Q4.json"),
        "the message names the range the figure fills and the figure that fills it:\n{out}"
    );

    // The same sidecar over a NAME-form figure is the accepted pairing.
    let ok = Fixture::new("figure-sidecar-named");
    ok.file("Sheet1", "A1", "1")
        .file("Sheet1", "B1", "2")
        .file("Sheet1", "Chart1.json", spec)
        .file("Sheet1", "Chart1.css", placement);
    let (ok_code, ok_out) = run(&["check", ok.path().to_str().unwrap()]);
    assert_eq!(ok_code, 0, "a name-form figure takes a sidecar:\n{ok_out}");
}

/// `check` LINTS a figure: its refusals are findings, and the workbook's own values still load.
#[test]
fn check_lints_a_figure_without_denying_the_workbook_its_values() {
    let fx = Fixture::new("figure-lint");
    fx.file("Sheet1", "A1:B2", "x\ty\n1\t2");
    std::fs::write(
        fx.path().join("Sheet1/broken.json"),
        "{\"data\":{\"name\":\"A1:B9\"},\"mark\":\"bar\"}",
    )
    .expect("write");
    let root = fx.path().display().to_string();
    let (code, out) = run(&["check", &root]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("figure-binding"), "{out}");
    assert!(out.contains("Sheet1/broken.json"), "{out}");
    // The values are untouched by a figure's fault: the grid still renders.
    let (rcode, rout) = run(&["render", &format!("{root}/Sheet1/A1")]);
    assert_eq!(rcode, 0, "{rout}");
    assert!(rout.contains('1'), "{rout}");
}

/// A figure with a placement sidecar beside it. The stem is shared: `<name>.json` IS the figure and
/// `<name>.css` is where it sits.
fn figured(tag: &str, placement: &str) -> Fixture {
    let fx = Fixture::new(tag);
    fx.file("Sheet1", "A1", "1")
        .file("Sheet1", "B1", "2")
        .file("Sheet1", "C3", "=A1+B1")
        .file(
            "Sheet1",
            "Chart1.json",
            "{\"data\":{\"name\":\"A1:B1\"},\"mark\":\"bar\"}",
        )
        .file("Sheet1", "Chart1.css", placement);
    fx
}

/// The ASCII grid is the only surface an agent laying out a sheet ever sees, so a covered cell is
/// marked IN it: `fig` alone where the cell is empty, `fig! ` before the cell's own text where it is
/// not, and nothing at all where no figure reaches. Every `--mode` marks, because a figure occludes
/// a coordinate whatever the coordinate is spelled as.
#[test]
fn the_ascii_grid_marks_every_cell_a_figure_covers_and_no_other() {
    let fx = figured("figure-cover-marks", "  figure { anchor: A1:B2 }\n");
    let root = fx.path().to_str().unwrap();

    for mode in [None, Some("values"), Some("functions")] {
        let mut argv = vec!["render", root];
        if let Some(m) = mode {
            argv.extend(["--mode", m]);
        }
        let (code, out, err) = run_err(&argv);
        assert_eq!(code, 0, "{argv:?} exits 0:\n{out}{err}");
        assert!(
            out.contains("fig! 1") && out.contains("fig! 2"),
            "{argv:?}: a covered cell HOLDING a value keeps its text behind the `fig! ` prefix:\n{out}"
        );
        assert_eq!(
            out.matches("| fig ").count(),
            2,
            "{argv:?}: A2 and B2 are covered and empty, so each is the bare mark:\n{out}"
        );
        assert!(
            !out.contains("fig! 3") && !out.contains("fig! ="),
            "{argv:?}: C3 is outside the cover, so it is unmarked:\n{out}"
        );
    }

    // Combined keeps BOTH halves of an occluded formula cell, prefix and all.
    let wide = figured("figure-cover-combined", "  figure { anchor: A1:C3 }\n");
    let (code, out) = run(&["render", wide.path().to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("fig! 3 ← =A1+B1"),
        "the mark PREFIXES the combined spelling, it does not replace it:\n{out}"
    );
}

/// One note per figure, in one of exactly two spellings: a placed figure names the rectangle it
/// covers, and an unplaced one says so rather than naming a position no author wrote.
#[test]
fn each_figure_gets_one_note_naming_its_cover_or_the_lack_of_one() {
    let placed = figured("figure-note-placed", "  figure { anchor: A1:B2 }\n");
    let (code, _out, err) = run_err(&["render", placed.path().to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    assert!(
        err.contains("figure Sheet1/Chart1.json covers A1:B2 and binds A1:B1"),
        "the placed spelling names the cover and the bindings:\n{err}"
    );

    let bare = Fixture::new("figure-note-unplaced");
    bare.file("Sheet1", "A1", "1")
        .file("Sheet1", "B1", "2")
        .file(
            "Sheet1",
            "Chart1.json",
            "{\"data\":{\"name\":\"A1:B1\"},\"mark\":\"bar\"}",
        );
    let (code, out, err) = run_err(&["render", bare.path().to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    assert!(
        err.contains("figure Sheet1/Chart1.json has no placement and binds A1:B1"),
        "with no sidecar there is no authored position to name:\n{err}"
    );
    assert!(!out.contains("fig"), "and nothing is marked:\n{out}");
}

/// Marking is the ASCII TABLE's answer to a figure it cannot draw. `--format html` draws the figure
/// itself, and `tree` has no coordinate plane to occlude — neither may mark.
#[test]
fn only_the_ascii_table_marks_a_cover() {
    let fx = figured("figure-cover-carriers", "  figure { anchor: A1:B2 }\n");
    let root = fx.path().to_str().unwrap();

    let (code, html) = run(&["render", root, "--format", "html"]);
    assert_eq!(code, 0, "{html}");
    assert!(
        !html.contains("fig!") && !html.contains(">fig<"),
        "html DRAWS the figure, so it marks no cell:\n{html}"
    );

    let (code, tree) = run(&["tree", root]);
    assert_eq!(code, 0, "{tree}");
    assert!(!tree.contains("fig"), "tree marks nothing:\n{tree}");
}

/// A `length` is any finite non-negative literal an author can write, so a sidecar that grades CLEAN
/// hands the cover arithmetic EMU near `i64::MAX`. Input never panics the binary, and it never wraps
/// into a quietly wrong rectangle either — the figure simply lands off the drawn grid.
#[test]
fn an_absurd_placement_length_renders_instead_of_crashing() {
    let fx = figured(
        "figure-cover-overflow",
        "  figure { anchor: A1; height: 1cm; left: 99999999999999999999cm; width: 1cm }\n",
    );
    let root = fx.path().to_str().unwrap();

    let (code, out) = run(&["check", root]);
    assert_eq!(
        code, 0,
        "the sidecar grades clean, so render must cope:\n{out}"
    );

    let (code, out, err) = run_err(&["render", root]);
    assert_eq!(code, 0, "no panic, no refusal:\n{out}{err}");
    assert!(
        out.contains("| 1 ") && out.contains("3 ← =A1+B1"),
        "the sheet is drawn on its own content:\n{out}"
    );
    assert!(
        !out.contains("fig"),
        "and nothing near A1 is marked:\n{out}"
    );
    assert!(err.contains("figure Sheet1/Chart1.json covers"), "{err}");
}

/// A figure cannot cost the caller the grid. A cover far past the content would widen the viewport
/// over the render bound, so the widening is dropped — the sheet still draws, and the covered cells
/// that fall inside it are still marked.
#[test]
fn a_cover_over_the_render_bound_does_not_take_the_grid_with_it() {
    let fx = figured("figure-cover-bound", "  figure { anchor: A1:Z50000 }\n");
    let root = fx.path().to_str().unwrap();

    let (code, out, err) = run_err(&["render", root]);
    assert_eq!(
        code, 0,
        "the caller gave no region, so there is nothing to narrow:\n{out}{err}"
    );
    assert!(
        !err.contains("render bound"),
        "and no refusal prescribing a fix the caller has no argv for:\n{err}"
    );
    assert!(
        out.contains("fig! 1") && out.contains("fig! 2"),
        "the content is drawn AND the cover reaches it:\n{out}"
    );
    assert!(
        err.contains("figure Sheet1/Chart1.json covers A1:Z50000"),
        "the note still names the whole cover:\n{err}"
    );

    // A REGION the caller did state is still bounded, and still says so.
    let (code, _out, err) = run_err(&["render", &format!("{root}/Sheet1/A1:Z50000")]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("over the render bound"), "{err}");
}

/// "The tab's used region" has ONE value per carrier: ascii draws no style, so its used region is
/// CONTENT, and html draws one, so its used region takes in every stated region. The note and the
/// grid read the same value, so a note can never describe a region the grid does not span.
#[test]
fn the_used_region_a_note_names_is_the_one_the_grid_spans() {
    let fx = Fixture::new("used-region-carriers");
    fx.file("Sheet1", "A1", "1")
        .file("Sheet1", "Z1:Z5.css", "  fsa1-cell { color: #3f0421 }\n");
    let root = fx.path().to_str().unwrap();

    let (code, out, err) = run_err(&["render", root]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains("| A |") && !out.contains("| Z "),
        "ascii's grid spans the CONTENT alone:\n{out}"
    );

    for region in ["Z1:Z5", "AA1:AA2"] {
        let (code, _out, err) = run_err(&["render", &format!("{root}/Sheet1/{region}")]);
        assert_eq!(code, 0, "{err}");
        assert!(
            err.contains(&format!(
                "region {region} lies entirely outside the tab's used region A1"
            )),
            "ascii's note reads the same CONTENT region its grid does:\n{err}"
        );
    }

    // html DRAWS the style, so the styled region is part of its used region — and its note says so.
    let (code, _out, err) = run_err(&[
        "render",
        &format!("{root}/Sheet1/AA1:AA2"),
        "--format",
        "html",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(
        err.contains("outside the tab's used region A1:Z5"),
        "html's used region takes in the stated one:\n{err}"
    );
}

/// A cover that saturates to the whole coordinate plane spans `2^32` cells on EACH axis, and the
/// bound test multiplies the two. The count is the only thing standing between a pathological
/// sidecar and a string per cell, so it must read as OVER the bound -- not wrap to something under
/// it. Both spellings reach the same arithmetic: one saturates the extent, the other the anchor.
#[test]
fn a_cover_spanning_the_whole_plane_is_still_bounded_out_of_the_grid() {
    for (tag, placement) in [
        (
            "figure-cover-plane-extent",
            "  figure { anchor: A1; height: 99999999999999999999cm; width: 99999999999999999999cm }\n",
        ),
        (
            "figure-cover-plane-anchor",
            "  figure { anchor: XFD1048576; height: 99999999999cm; width: 99999999999cm }\n",
        ),
    ] {
        let fx = figured(tag, placement);
        let root = fx.path().to_str().unwrap();

        let (code, out) = run(&["check", root]);
        assert_eq!(code, 0, "{tag}: the sidecar grades clean:\n{out}");

        let (code, out, err) = run_err(&["render", root]);
        assert_eq!(code, 0, "{tag}: no panic and no abort:\n{out}{err}");
        assert!(
            out.contains("| 1 ") && out.contains("3 ← =A1+B1"),
            "{tag}: the sheet is drawn on its own content:\n{out}"
        );
        assert!(err.contains("figure Sheet1/Chart1.json covers"), "{err}");
    }
}
