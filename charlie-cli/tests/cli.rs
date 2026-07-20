// Concern: the CLI CONTRACT integration test — drive the built `charlie-cli` binary end-to-end against temp workbooks and lock the observable surface the model's unit tests cannot: the argv dispatch, the human text output (the ASCII table / scalar / prose on stdout, the located diagnostics, the eval value), the `--version` JSON handshake, the on-disk `sample` workbook + its never-clobber refusal, the `import` of a real `.ods`/`.xlsx` into a renderable workbook (+ its unsupported-extension/conflict/not-found refusals), the `--guide` text, that a removed `--format json` is now an unknown flag, and the EXIT CODE an agent branches on (0 clean render/check/eval/sample/import/guide · 2 bad args · 3 error-severity diagnostics or error-valued eval or an unsupported import format · 4 sample/import target-dir conflict · 24 not found) | Non-concern: the render/lint/eval LOGIC (charlie-model's own tests own value spelling, demand-driven eval, array broadcasting, diagnostic detection, and the sample CONTENT), the ODS/xlsx conversion LOGIC (charlie-ingest's own tests own it), the text emitters (main.rs `output` owns them), and comfy-table's internals | IO: spawns `$CARGO_BIN_EXE_charlie-cli`, writes temp workbook dirs, reads committed `.ods`/`.xlsx` fixtures, asserts on stdout + exit status
//! End-to-end tests of the `charlie-cli` binary: exit codes and stdout for `render`, `check`, `eval`,
//! `sample`, `--guide`, `--version`, and misuse. The spreadsheet logic is tested in `charlie-model`;
//! this locks the thin shell's own contract.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A unique temp directory for one test's workbook, removed by [`Fixture`]'s drop.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Fixture {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root =
            std::env::temp_dir().join(format!("charlie-cli-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create fixture root");
        Fixture { root }
    }

    /// Write one cell/range file `name` with `body` into tab `tab`. A file's content is exactly its
    /// grid (GRID1) — no annotation line, so the body is written verbatim.
    fn file(&self, tab: &str, name: &str, body: &str) -> &Fixture {
        let dir = self.root.join(tab);
        std::fs::create_dir_all(&dir).expect("create tab dir");
        std::fs::write(dir.join(name), body).expect("write file");
        self
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

/// Run the binary with `args`, returning `(exit_code, stdout)`.
fn run(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_charlie-cli"))
        .args(args)
        .output()
        .expect("spawn charlie-cli");
    let code = out.status.code().expect("exit code");
    (code, String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn render_default_draws_the_computed_cone() {
    // No mode flag → the COMBINED default (not --values): the demand-driven value B1 still computes.
    let fx = Fixture::new("render");
    fx.file("Sheet1", "A1", "20000")
        .file("Sheet1", "B1", "=A1*2");
    let (code, out) = run(&["render", fx.path().to_str().unwrap()]);
    assert_eq!(code, 0, "clean render exits 0; got:\n{out}");
    // The header row, the gutter, and the demand-driven value B1 = 40000.
    assert!(out.contains("| A "), "column-letter header:\n{out}");
    assert!(out.contains("40000"), "B1 should compute to 40000:\n{out}");
}

#[test]
fn render_functions_shows_formula_text() {
    let fx = Fixture::new("funcs");
    fx.file("Sheet1", "A1", "2").file("Sheet1", "B1", "=A1*2");
    let (code, out) = run(&["render", fx.path().to_str().unwrap(), "--functions"]);
    assert_eq!(code, 0);
    assert!(out.contains("=A1*2"), "formula text in --functions:\n{out}");
}

#[test]
fn render_default_is_combined_value_then_source() {
    // With no mode flag the default is COMBINED: a literal shows its value plain; a formula shows
    // `<value> ← =<formula>` (value AND authored source in one cell). --values/--functions still narrow.
    let fx = Fixture::new("combined");
    fx.file("Sheet1", "A1", "2").file("Sheet1", "B1", "=A1*2");
    let (code, out) = run(&["render", fx.path().to_str().unwrap()]);
    assert_eq!(code, 0, "combined render exits 0:\n{out}");
    // The formula cell carries value AND source, joined by the U+2190 arrow.
    assert!(
        out.contains("4 ← =A1*2"),
        "a formula shows `<value> ← =<formula>` in the combined default:\n{out}"
    );

    // --values narrows to the computed value only (no arrow, no source).
    let (vc, vals) = run(&["render", fx.path().to_str().unwrap(), "--values"]);
    assert_eq!(vc, 0);
    assert!(
        vals.contains("| 4 ") && !vals.contains("←"),
        "--values shows the value only (no arrow):\n{vals}"
    );

    // --functions narrows to the authored source only.
    let (fc, funcs) = run(&["render", fx.path().to_str().unwrap(), "--functions"]);
    assert_eq!(fc, 0);
    assert!(
        funcs.contains("=A1*2") && !funcs.contains("←"),
        "--functions shows the source only (no arrow):\n{funcs}"
    );
}

#[test]
fn render_missing_tab_is_not_found() {
    let fx = Fixture::new("notab");
    fx.file("Sheet1", "A1", "1");
    let (code, _) = run(&["render", fx.path().to_str().unwrap(), "--tab", "Nope"]);
    assert_eq!(code, 24, "an unknown tab is exit 24 (not found)");
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
    let (code, _) = run(&["check", "/no/such/charlie/workbook/xyz"]);
    assert_eq!(code, 24);
}

#[test]
fn render_a_grid5_array_formula_region_fills_its_range() {
    // A1:A3 = {3;1;2} (three literals) and C1:C3 = `=SORT(A1:A3)` (one array formula) — the GRID5
    // smoke case. --values shows each coordinate its sorted element; --functions shows the formula at
    // the anchor and the `^` continuation marker below it.
    let fx = Fixture::new("grid5");
    fx.file("Sheet1", "A1:A3", "3\n1\n2")
        .file("Sheet1", "C1:C3", "=SORT(A1:A3)");
    let (code, out) = run(&["render", fx.path().to_str().unwrap(), "--range", "C1:C3"]);
    assert_eq!(code, 0, "clean region render exits 0:\n{out}");
    for v in ["| 1 ", "| 2 ", "| 3 "] {
        assert!(out.contains(v), "sorted element {v}:\n{out}");
    }
    let (code, out) = run(&[
        "render",
        fx.path().to_str().unwrap(),
        "--range",
        "C1:C3",
        "--functions",
    ]);
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
    // C1:C2 (2x1) holds `=SORT(A1:A3)` whose value is 3x1 — a shape mismatch is a located dimension
    // error detected at evaluation (GRID5), exit 3.
    let fx = Fixture::new("grid5mismatch");
    fx.file("Sheet1", "A1:A3", "3\n1\n2")
        .file("Sheet1", "C1:C2", "=SORT(A1:A3)");
    let (code, out) = run(&["check", fx.path().to_str().unwrap()]);
    assert_eq!(code, 3, "a region shape mismatch is exit 3:\n{out}");
    assert!(out.contains("dimension-mismatch"), "the code:\n{out}");
    assert!(out.contains("array formula"), "the located message:\n{out}");
}

/// Recursively collect the AUTHORITATIVE files under `root` (relative paths), excluding the reserved
/// `.cache/` — the only place `check` (like the other read commands) may write derived data (FS3). A
/// read-only command must leave this set unchanged.
fn authoritative_files(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n == ".cache") {
                    continue;
                }
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
    // D1 carries a pre-existing GRID6 load error (an unparseable `=SUM(`); H3 is a valid formula the
    // agent authored. A scope over H3's neighbourhood is CLEAN (exit 0) even though the wider workbook
    // has the D1 error, while a scope over D1 reports it (exit 3) and the unscoped check reports it too.
    let fx = Fixture::new("scope-range");
    fx.file("Sheet1", "D1", "=SUM(") // unrelated pre-existing error cell (out of scope below)
        .file("Sheet1", "A1", "1")
        .file("Sheet1", "A2", "2")
        .file("Sheet1", "H3", "=SUM(A1:A2)"); // the cell the agent authored (clean)

    // In scope over H3's neighbourhood: clean, exit 0, and D1's error is NOT reported.
    let (code, out) = run(&["check", fx.path().to_str().unwrap(), "--range", "G1:H5"]);
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

    // Scoped over the faulty D1: the error IS reported, exit 3.
    let (code, out) = run(&["check", fx.path().to_str().unwrap(), "--range", "D1:D1"]);
    assert_eq!(code, 3, "the in-scope D1 error is exit 3:\n{out}");
    assert!(
        out.contains("formula-syntax"),
        "D1's error is in scope:\n{out}"
    );

    // Unscoped: unchanged — the D1 error is reported, exit 3.
    let (code, out) = run(&["check", fx.path().to_str().unwrap()]);
    assert_eq!(code, 3, "the unscoped check is unchanged:\n{out}");
    assert!(
        out.contains("formula-syntax"),
        "unscoped reports D1:\n{out}"
    );
}

#[test]
fn check_cell_scopes_to_a_single_cell() {
    // A single-cell scope: the clean authored cell exits 0; the faulty cell exits 3.
    let fx = Fixture::new("scope-cell");
    fx.file("Sheet1", "D1", "=SUM(")
        .file("Sheet1", "H3", "=1+1");

    let (code, out) = run(&["check", fx.path().to_str().unwrap(), "--cell", "H3"]);
    assert_eq!(code, 0, "the clean authored cell exits 0:\n{out}");
    assert!(!out.contains("formula-syntax"), "D1 out of scope:\n{out}");

    let (code, out) = run(&["check", fx.path().to_str().unwrap(), "--cell", "D1"]);
    assert_eq!(code, 3, "the faulty cell in scope exits 3:\n{out}");
    assert!(out.contains("formula-syntax"), "D1 in scope:\n{out}");

    // A range value passed to --cell is refused (bad args).
    let (code, _) = run(&["check", fx.path().to_str().unwrap(), "--cell", "A1:B2"]);
    assert_eq!(code, 2, "--cell rejects a range");
}

#[test]
fn check_sheet_qualified_cell_selects_the_named_tab() {
    // The SAME address `D1` exists on two tabs: clean on Beta, faulty on Alpha. A sheet-qualified
    // `--cell Beta!D1` must resolve the TRUE tab (a bare-filename GRID6 loc is ambiguous across tabs)
    // and exit 0, while `Alpha!D1` exits 3.
    let fx = Fixture::new("scope-qualified");
    fx.file("Alpha", "D1", "=SUM(") // faulty D1 on Alpha
        .file("Beta", "D1", "=1+1"); // clean D1 on Beta

    let (code, out) = run(&["check", fx.path().to_str().unwrap(), "--cell", "Beta!D1"]);
    assert_eq!(
        code, 0,
        "Beta!D1 is clean; Alpha's error is out of scope:\n{out}"
    );
    assert!(
        !out.contains("formula-syntax"),
        "Alpha out of scope:\n{out}"
    );

    let (code, out) = run(&["check", fx.path().to_str().unwrap(), "--cell", "Alpha!D1"]);
    assert_eq!(code, 3, "Alpha!D1 is the faulty cell:\n{out}");
    assert!(out.contains("formula-syntax"), "Alpha!D1 in scope:\n{out}");

    // A tab-only scope narrows to the whole tab.
    let (code, _) = run(&["check", fx.path().to_str().unwrap(), "--tab", "Beta"]);
    assert_eq!(code, 0, "the clean Beta tab exits 0");
    let (code, _) = run(&["check", fx.path().to_str().unwrap(), "--tab", "Alpha"]);
    assert_eq!(code, 3, "the faulty Alpha tab exits 3");

    // A scope tab absent from a loaded workbook is a not-found refusal (exit 24).
    let (code, _) = run(&["check", fx.path().to_str().unwrap(), "--tab", "Nope"]);
    assert_eq!(code, 24, "an unknown scope tab is not-found");
}

#[test]
fn check_scoped_on_an_unloadable_workbook_freezes_the_best_effort_region_filter() {
    // FREEZE the load-failed + scoped decision (cmd_check's `Ok(Err(load_diags))` arm). A GRID4 literal
    // dimension mismatch on the A1:D9 file ABORTS the load (the workbook won't load at ALL) — unlike a
    // per-cell GRID6 error, which loads with an error cell (covered above). The aborting refusal's loc
    // parses to the region A1:D9 from its filename, so a scope that MISSES that region suppresses it: a
    // scoped `check` reads green (exit 0) on a globally-unloadable import. This is the documented
    // best-effort "only my cells" relaxation of the unscoped "a workbook that won't load is itself the
    // failure" contract — the branch carries real filter logic, so pin the decided behavior either way.
    let fx = Fixture::new("scope-loadfail");
    fx.file("Sheet1", "A1:D9", "one literal in a 9x4 range") // GRID4 dimension mismatch -> load aborts
        .file("Sheet1", "H3", "=1+1"); // the cell "the agent authored" (never reached: load aborts)

    // Out of scope (the sharp edge): the aborting fault sits in A1:D9; a scope elsewhere hides it.
    for scope in [["--range", "Z1:Z2"], ["--cell", "H3"]] {
        let (code, out) = run(&["check", fx.path().to_str().unwrap(), scope[0], scope[1]]);
        assert_eq!(code, 0, "an out-of-scope load failure reads green:\n{out}");
        assert!(
            !out.contains("dimension-mismatch"),
            "the fault is out of scope:\n{out}"
        );
    }

    // In scope (A1:B2 intersects A1:D9): the same aborting fault is reported, exit 3.
    let (code, out) = run(&["check", fx.path().to_str().unwrap(), "--range", "A1:B2"]);
    assert_eq!(code, 3, "an in-scope load failure is exit 3:\n{out}");
    assert!(
        out.contains("dimension-mismatch"),
        "the in-scope fault is reported:\n{out}"
    );

    // Unscoped: unchanged — a workbook that won't load is itself the failure, exit 3.
    let (code, out) = run(&["check", fx.path().to_str().unwrap()]);
    assert_eq!(code, 3, "the unscoped load failure is unchanged:\n{out}");
    assert!(
        out.contains("dimension-mismatch"),
        "unscoped reports the fault:\n{out}"
    );
}

#[test]
fn check_scope_never_suppresses_a_region_less_load_refusal() {
    // The load-failed filter's tab axis is asymmetric (the counterpart to the region suppression above):
    // a refusal whose loc carries NO parseable region — a malformed filename yields a `Loc::File` whose
    // name does not parse, so `loc_target` returns (None, None) — rides through EVERY scope's rect and
    // tab filter. So a truly structural refusal is ALWAYS surfaced, even under a scope that would hide a
    // region-bearing fault. Pin that guarantee: the bad-filename load abort reports under any scope.
    let fx = Fixture::new("scope-loadfail-noregion");
    fx.file("Sheet1", "A1:B2:C3", "1"); // more than one `:` -> malformed-filename -> load aborts

    for args in [
        vec!["check", fx.path().to_str().unwrap()],
        vec!["check", fx.path().to_str().unwrap(), "--range", "Z1:Z2"],
        vec!["check", fx.path().to_str().unwrap(), "--cell", "Y9"],
    ] {
        let (code, out) = run(&args);
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
fn check_is_read_only_and_writes_no_authoritative_file() {
    // check (scoped or not) writes NO authoritative cell/tab/file (CORE3): the authoritative file set
    // is identical before and after. (It may still populate the derived `.cache/`, FS3 — excluded.)
    let fx = Fixture::new("scope-readonly");
    fx.file("Sheet1", "D1", "=SUM(")
        .file("Sheet1", "A1", "1")
        .file("Sheet1", "H3", "=A1+1");

    let before = authoritative_files(fx.path());
    for args in [
        vec!["check", fx.path().to_str().unwrap()],
        vec!["check", fx.path().to_str().unwrap(), "--cell", "H3"],
        vec!["check", fx.path().to_str().unwrap(), "--range", "A1:H3"],
        vec!["check", fx.path().to_str().unwrap(), "--tab", "Sheet1"],
    ] {
        let _ = run(&args);
    }
    let after = authoritative_files(fx.path());
    assert_eq!(
        before, after,
        "check must not create, modify, or delete any authoritative file"
    );
}

#[test]
fn eval_computes_a_sum_against_the_workbook() {
    // A range SUM over literal cells: the ad-hoc formula pulls A1:A3 through the model.
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
    // ONE discriminating case that the eval value stream spells numbers through Excel's General
    // formatter and not Rust's raw `Display`: `1e20` prints scientific `1E+20` (raw `Display` would
    // leak a 21-digit integer). The exhaustive General spelling table (scientific thresholds, 15-sig
    // rounding, `-0.0`→`0`) is frozen once at its leaf home, charlie-ast::eval
    // `num_to_text_matches_excel_general_format`; this e2e case only proves the shell routes through it.
    let fx = Fixture::new("eval-general");
    fx.file("Sheet1", "A1", "0");
    let (code, out) = run(&["eval", fx.path().to_str().unwrap(), "--formula", "=1e20"]);
    assert_eq!(code, 0, "=1e20 evaluates cleanly:\n{out}");
    assert_eq!(out.trim(), "1E+20", "General-formats to 1E+20:\n{out}");
}

#[test]
fn eval_resolves_a_cross_tab_reference_against_the_named_tab() {
    // `--tab Summary` binds unqualified refs to Summary, and an explicit `Inputs!A1` reaches the
    // other tab: Inputs!A1 (10) * A1 (Summary!A1 = 4) = 40.
    let fx = Fixture::new("eval-cross");
    fx.file("Inputs", "A1", "10").file("Summary", "A1", "4");
    let (code, out) = run(&[
        "eval",
        fx.path().to_str().unwrap(),
        "--tab",
        "Summary",
        "--formula",
        "=Inputs!A1*A1",
    ]);
    assert_eq!(code, 0, "cross-tab eval exits 0:\n{out}");
    assert_eq!(out.trim(), "40", "Inputs!A1 * Summary!A1 = 40:\n{out}");
}

#[test]
fn eval_a_bad_formula_exits_non_zero() {
    // An unparseable formula is a located diagnostic (on stderr) and a non-zero exit.
    let fx = Fixture::new("eval-bad");
    fx.file("Sheet1", "A1", "1");
    let (code, _) = run(&["eval", fx.path().to_str().unwrap(), "--formula", "=SUM("]);
    assert_eq!(code, 3, "a parse error is a validation exit (3)");
}

#[test]
fn eval_an_error_value_exits_non_zero() {
    // A well-formed formula that evaluates to a spreadsheet error prints the error and exits non-zero.
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
fn version_prints_a_json_envelope() {
    let (code, out) = run(&["--version"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("\"status\":\"success\""),
        "version envelope:\n{out}"
    );
    assert!(
        out.contains("\"name\":\"charlie-cli\""),
        "version envelope:\n{out}"
    );
}

#[test]
fn unknown_command_is_bad_args() {
    let (code, _) = run(&["frobnicate"]);
    assert_eq!(code, 2, "an unknown command is exit 2 (bad args)");
}

#[test]
fn bad_range_is_bad_args() {
    let fx = Fixture::new("badrange");
    fx.file("Sheet1", "A1", "1");
    let (code, _) = run(&["render", fx.path().to_str().unwrap(), "--range", "a1"]);
    assert_eq!(code, 2, "a non-canonical --range is exit 2");
}

#[test]
fn an_enormous_range_is_a_located_refusal_not_a_crash() {
    // `--range A1:A4294967295` is a syntactically-valid address pair; without a viewport cap it
    // aborts the process on allocation. It must instead be a clean bad-args refusal (exit 2).
    let fx = Fixture::new("hugerange");
    fx.file("Sheet1", "A1", "1");
    let (code, _) = run(&[
        "render",
        fx.path().to_str().unwrap(),
        "--range",
        "A1:A4294967295",
    ]);
    assert_eq!(
        code, 2,
        "an oversized --range is a located refusal, not a crash"
    );
}

#[test]
fn sample_writes_a_renderable_workbook_and_prints_next_steps() {
    // `sample <dir>` on a fresh (not-yet-created) directory writes the model's tutorial workbook and
    // exits 0, printing the next-steps hints; the written tree is a REAL workbook the same binary can
    // then render (proving the sample is live, not prose).
    let fx = Fixture::new("sample");
    let target = fx.path().join("wb");
    let target_s = target.to_str().unwrap().to_string();

    let (code, out) = run(&["sample", &target_s]);
    assert_eq!(code, 0, "a fresh sample exits 0; got:\n{out}");
    // The two tabs and a couple of representative cell/range files exist on disk.
    assert!(target.join("Orders").is_dir(), "Orders tab written:\n{out}");
    assert!(
        target.join("Summary").is_dir(),
        "Summary tab written:\n{out}"
    );
    assert!(
        target.join("Orders/A1:D1").is_file(),
        "the header range file exists:\n{out}"
    );
    assert!(
        target.join("Orders/D5").is_file(),
        "the SUM total cell exists:\n{out}"
    );
    // The next-steps text prints and names the renamed binary.
    assert!(out.contains("next:"), "next-steps hint printed:\n{out}");
    assert!(
        out.contains("charlie-cli render"),
        "next-steps names charlie-cli:\n{out}"
    );
    // The hint quotes the sample's real grand total; pinning it here means a change to the sample's
    // total fails this test instead of leaving the tutorial hint silently stale.
    assert!(
        out.contains("110"),
        "next-steps hint quotes the sample total (110):\n{out}"
    );

    // The sample is a genuine workbook: rendering it succeeds and shows a computed total (D5 = 110).
    let (rcode, rout) = run(&["render", &target_s]);
    assert_eq!(rcode, 0, "the written sample renders cleanly:\n{rout}");
    assert!(
        rout.contains("110"),
        "D5 grand total renders as 110:\n{rout}"
    );
}

#[test]
fn sample_writes_into_an_existing_empty_dir() {
    // The never-clobber gate has three target states: absent, exists-and-empty, exists-and-non-empty.
    // The success tests above use an absent dir and the refusal test uses a non-empty dir; this pins
    // the third branch — an EXISTING but EMPTY directory PROCEEDS (an empty dir is not a clobber),
    // writes the workbook into it, and exits 0.
    let fx = Fixture::new("sample-empty");
    let target = fx.path().join("wb");
    std::fs::create_dir_all(&target).expect("pre-create an empty target dir");
    let target_s = target.to_str().unwrap().to_string();

    let (code, out) = run(&["sample", &target_s]);
    assert_eq!(
        code, 0,
        "an existing EMPTY dir is not a clobber -> sample proceeds and exits 0; got:\n{out}"
    );
    assert!(
        target.join("Orders/D5").is_file(),
        "the workbook was written into the pre-existing empty dir:\n{out}"
    );
}

#[test]
fn sample_refuses_to_clobber_a_nonempty_dir_and_writes_nothing() {
    // The never-clobber guarantee: `sample <dir>` on an EXISTING, NON-EMPTY directory refuses,
    // writes nothing, and exits 4 (CONFLICT — the argv is valid, the target state is the problem;
    // it is NOT a usage error / exit 2). The pre-existing content is left untouched.
    let fx = Fixture::new("sample-clobber");
    // A sentinel file makes the target directory non-empty (and is NOT a workbook tab).
    fx.file("keep", "A1", "42");
    let target_s = fx.path().to_str().unwrap().to_string();

    let (code, _out) = run(&["sample", &target_s]);
    assert_eq!(
        code, 4,
        "clobber refusal is a CONFLICT (exit 4), not bad-args (exit 2)"
    );
    // Nothing was written: no tutorial tab appeared, and the sentinel is intact.
    assert!(
        !fx.path().join("Orders").exists(),
        "refusal must write no Orders tab"
    );
    assert!(
        !fx.path().join("Summary").exists(),
        "refusal must write no Summary tab"
    );
    let kept = std::fs::read_to_string(fx.path().join("keep/A1")).expect("sentinel survives");
    assert!(kept.contains("42"), "the pre-existing file is untouched");
}

#[test]
fn guide_prints_and_exits_zero() {
    // `--guide` prints the terse on-disk-model tour and exits 0.
    let (code, out) = run(&["--guide"]);
    assert_eq!(code, 0, "--guide exits 0:\n{out}");
    assert!(
        out.contains("charlie-cli"),
        "the guide names the binary:\n{out}"
    );
    assert!(
        out.contains("STRUCTURE"),
        "the guide has its structure section:\n{out}"
    );
}

// ---- `--format` is gone: text is the sole output form; the flag is now an unknown flag. ----

#[test]
fn format_json_is_now_an_unknown_flag() {
    // The global `--format text|json` selector was removed (text is the only output form). `--format`
    // is no longer recognized, so `--format json` is a plain unknown flag → bad args (exit 2).
    let fx = Fixture::new("format-gone");
    fx.file("Sheet1", "A1", "1");
    let (code, _) = run(&["render", fx.path().to_str().unwrap(), "--format", "json"]);
    assert_eq!(code, 2, "`--format json` is now an unknown flag (exit 2)");
}

/// A committed `.ods` fixture from the sibling `charlie-ingest` crate (the CLI test tree has none of
/// its own; import's job is to CONVERT that real file into a workbook this binary can then render).
fn ingest_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../charlie-ingest/tests/fixtures")
        .join(name)
}

#[test]
fn import_ods_then_render_and_eval_the_converted_workbook() {
    let fx = Fixture::new("import");
    let dest = fx.path().join("wb"); // absent -> import creates it
    let src = ingest_fixture("smoke.ods");
    let (code, out) = run(&["import", src.to_str().unwrap(), dest.to_str().unwrap()]);
    assert_eq!(code, 0, "import should succeed:\n{out}");
    assert!(
        out.contains("5 cell file(s)"),
        "five per-cell files written (Sheet1: A1,A2,A3,B1; Sheet2: A1):\n{out}"
    );

    // The converted workbook computes the Excel values through the format-blind engine.
    let a3 = run(&[
        "eval",
        dest.to_str().unwrap(),
        "--tab",
        "Sheet1",
        "--formula",
        "=A3",
    ]);
    assert_eq!((a3.0, a3.1.trim()), (0, "30"));
    let b1 = run(&[
        "eval",
        dest.to_str().unwrap(),
        "--tab",
        "Sheet1",
        "--formula",
        "=B1",
    ]);
    assert_eq!((b1.0, b1.1.trim()), (0, "60"));
    let cross = run(&[
        "eval",
        dest.to_str().unwrap(),
        "--tab",
        "Sheet2",
        "--formula",
        "=A1",
    ]);
    assert_eq!((cross.0, cross.1.trim()), (0, "30"));
}

#[test]
fn import_into_a_non_empty_dir_is_a_conflict() {
    let fx = Fixture::new("import-conflict");
    fx.file("Existing", "A1", "1"); // makes the dest non-empty
    let src = ingest_fixture("smoke.ods");
    let (code, _) = run(&["import", src.to_str().unwrap(), fx.path().to_str().unwrap()]);
    assert_eq!(code, 4, "a non-empty destination is a conflict (exit 4)");
}

#[test]
fn import_a_missing_source_is_not_found() {
    let fx = Fixture::new("import-missing");
    let dest = fx.path().join("wb");
    let (code, _) = run(&["import", "/no/such/file.ods", dest.to_str().unwrap()]);
    assert_eq!(code, 24, "a missing source is not found (exit 24)");
}

#[test]
fn import_xlsx_then_eval_the_converted_workbook() {
    // The CLI import path is format-agnostic (it dispatches to charlie-ingest by extension), so an
    // .xlsx flows through the binary exactly as the .ods does — same converted values.
    let fx = Fixture::new("import-xlsx");
    let dest = fx.path().join("wb");
    let src = ingest_fixture("smoke.xlsx");
    let (code, out) = run(&["import", src.to_str().unwrap(), dest.to_str().unwrap()]);
    assert_eq!(code, 0, "xlsx import should succeed:\n{out}");
    assert!(
        out.contains("5 cell file(s)"),
        "five per-cell files written (Sheet1: A1,A2,A3,B1; Sheet2: A1):\n{out}"
    );

    let a3 = run(&[
        "eval",
        dest.to_str().unwrap(),
        "--tab",
        "Sheet1",
        "--formula",
        "=A3",
    ]);
    assert_eq!((a3.0, a3.1.trim()), (0, "30"));
    let cross = run(&[
        "eval",
        dest.to_str().unwrap(),
        "--tab",
        "Sheet2",
        "--formula",
        "=A1",
    ]);
    assert_eq!((cross.0, cross.1.trim()), (0, "30"));
}

#[test]
fn import_an_unsupported_extension_is_a_validation_refusal() {
    // A .csv (or any non-.ods/.xlsx) is a located CORE2 refusal (exit 3), never format-sniffed.
    let fx = Fixture::new("import-badext");
    let src = fx.path().join("data.csv");
    std::fs::write(&src, "a,b\n1,2\n").unwrap();
    let dest = fx.path().join("wb");
    let (code, _) = run(&["import", src.to_str().unwrap(), dest.to_str().unwrap()]);
    assert_eq!(
        code, 3,
        "an unsupported extension is a validation error (exit 3)"
    );
}

// ---- tree (CLI3): the workbook's complete structure as a read-only nested view ----

/// Create a POSIX symlink `link` -> `target` inside tab `tab` (the FS4 name representation the tree
/// resolves). Unix-only, matching the model's symlink name form.
fn symlink(fx: &Fixture, tab: &str, link: &str, target: &str) {
    let dir = fx.path().join(tab);
    std::fs::create_dir_all(&dir).expect("create tab dir");
    std::os::unix::fs::symlink(target, dir.join(link)).expect("create symlink");
}

/// A stable snapshot of every AUTHORITATIVE entry under `root` (recursively), EXCLUDING the derived
/// `.cache/` (FS3): each regular file as `path=bytes` and each symlink as `path->target`. Used to prove
/// `tree` leaves the workbook byte-identical (CORE3).
fn snapshot(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap())
            .collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for e in entries {
            let path = e.path();
            let rel = path
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let ft = e.file_type().unwrap();
            if ft.is_symlink() {
                let target = std::fs::read_link(&path).unwrap();
                out.push(format!("{rel}->{}", target.display()));
            } else if ft.is_dir() {
                if rel == ".cache" {
                    continue; // derived, non-authoritative (FS3)
                }
                walk(&path, base, out);
            } else {
                out.push(format!("{rel}={}", std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out
}

/// Build the canonical tree fixture: a formula cell, a multi-row range file, a named-range symlink, a
/// named-formula ref-file, and a derived `.cache/` that must never appear.
fn tree_fixture(tag: &str) -> Fixture {
    let fx = Fixture::new(tag);
    fx.file("Sheet1", "A1", "Product") // a literal cell
        .file("Sheet1", "B1", "10") // a literal the formula reads
        .file("Sheet1", "C1", "=B1*2") // a FORMULA cell
        .file("Sheet1", "A2:A5", "1\n2\n3\n4") // a MULTI-ROW range file (4 cells)
        .file("Sheet1", "Rate", "=B1*1.05"); // a named-FORMULA ref-file
    symlink(&fx, "Sheet1", "Days.begin", "A2"); // a named-RANGE symlink (corner pair)
    symlink(&fx, "Sheet1", "Days.end", "A5");
    // A derived .cache/ that tree must exclude (FS3) — a standalone workbook is not a git repo.
    std::fs::create_dir_all(fx.path().join(".cache")).unwrap();
    std::fs::write(fx.path().join(".cache").join("junk"), "regenerable").unwrap();
    fx
}

#[test]
fn tree_presents_every_authored_cell_and_name_and_excludes_cache() {
    // CLI3 completeness + exclusion: every authored cell and name is present; the derived .cache/ never is.
    let fx = tree_fixture("tree-complete");
    let (code, out) = run(&["tree", fx.path().to_str().unwrap()]);
    assert_eq!(code, 0, "clean tree exits 0:\n{out}");
    // Every authored cell of every file.
    for cell in ["A1", "B1", "C1", "A2", "A3", "A4", "A5"] {
        assert!(out.contains(cell), "authored cell {cell} present:\n{out}");
    }
    // Both names, each shown by what it resolves to (FS4).
    assert!(out.contains("Days"), "the named range is present:\n{out}");
    assert!(out.contains("Rate"), "the named formula is present:\n{out}");
    assert!(
        out.contains("→ Sheet1!A2:A5"),
        "the symlinked range resolves to its target A1 ref:\n{out}"
    );
    // The tab itself.
    assert!(
        out.contains("Sheet1/"),
        "the tab is a directory node:\n{out}"
    );
    // The derived .cache/ is NEVER shown (FS3) — neither the dir nor its content.
    assert!(
        !out.contains(".cache") && !out.contains("junk"),
        "the derived .cache/ must be excluded:\n{out}"
    );
}

#[test]
fn tree_functions_shows_source_and_values_shows_computed() {
    // --functions shows authored formula text; --values shows the computed value (mirroring render).
    let fx = tree_fixture("tree-modes");
    let (fc, funcs) = run(&["tree", fx.path().to_str().unwrap(), "--functions"]);
    assert_eq!(fc, 0, "{funcs}");
    assert!(
        funcs.contains("C1  # =B1*2"),
        "--functions shows the authored formula, not its value:\n{funcs}"
    );
    assert!(
        funcs.contains("Rate  # =B1*1.05"),
        "--functions shows the named formula's definition:\n{funcs}"
    );

    let (vc, vals) = run(&["tree", fx.path().to_str().unwrap(), "--values"]);
    assert_eq!(vc, 0, "{vals}");
    assert!(
        vals.contains("C1  # 20"),
        "--values shows the computed value 10*2=20:\n{vals}"
    );
    assert!(
        vals.contains("Rate  # 10.5"),
        "--values shows the named formula's computed value 10*1.05=10.5:\n{vals}"
    );
    // Default mode is COMBINED: a formula cell shows `<value> ← =<formula>` (value AND source),
    // matching render's default; the named formula likewise carries both.
    let (_, dflt) = run(&["tree", fx.path().to_str().unwrap()]);
    assert!(
        dflt.contains("C1  # 20 ← =B1*2"),
        "the default mode is combined (value ← source):\n{dflt}"
    );
    assert!(
        dflt.contains("Rate  # 10.5 ← =B1*1.05"),
        "a named formula in combined shows value ← source:\n{dflt}"
    );
    // A literal renders plain in combined (its value IS its provenance — no arrow).
    assert!(
        dflt.contains("B1  # 10") && !dflt.contains("B1  # 10 ←"),
        "a literal renders plain (no arrow) in combined:\n{dflt}"
    );
}

#[test]
fn tree_range_expands_a1_ordered_capped_and_elided() {
    // A multi-cell range file expands to one A1-ordered node per coordinate, capped at ~50 with the
    // remainder shown as an elided count (a large range never floods the view).
    let fx = Fixture::new("tree-cap");
    let body: String = (1..=60)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    fx.file("T", "A1:A60", &body); // 60 authored cells
    let (code, out) = run(&["tree", fx.path().to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    // A1-ordered: A1 comes before A50 in the output.
    let a1 = out.find("A1 ").expect("A1 present");
    let a50 = out.find("A50 ").expect("A50 present");
    assert!(a1 < a50, "cells are A1-ordered (A1 before A50):\n{out}");
    // Capped at 50: the 51st coordinate is NOT a node.
    assert!(
        !out.contains("A51 "),
        "past the cap, a coordinate is elided, not shown:\n{out}"
    );
    // The remainder (60 - 50 = 10) is an elided count marker.
    assert!(
        out.contains("[+10"),
        "the 10 over-cap cells are shown as an elided count:\n{out}"
    );
}

#[test]
fn tree_full_lifts_the_cap_so_the_elided_markers_hint_is_honest() {
    // The elided-count marker borrows annotated-tree's "use --full to expand" text; --full must be a
    // real, accepted flag that lifts the per-range cap so nothing is elided (the affordance the marker
    // advertises actually works — an agent following the hint is not refused).
    let fx = Fixture::new("tree-full");
    let body: String = (1..=60)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    fx.file("T", "A1:A60", &body); // 60 authored cells — past the default cap of 50
    let (code, out) = run(&["tree", fx.path().to_str().unwrap(), "--full"]);
    assert_eq!(code, 0, "--full is an accepted flag (exit 0):\n{out}");
    // Every coordinate now shows, including ones past the default cap.
    assert!(
        out.contains("A51 ") && out.contains("A60 "),
        "--full expands past the default cap (A51 and A60 present):\n{out}"
    );
    // Nothing is elided, so the "use --full to expand" marker is absent.
    assert!(
        !out.contains("use --full to expand"),
        "--full elides nothing, so the expand hint does not appear:\n{out}"
    );
}

#[test]
fn tree_range_shows_every_cell_uncapped_while_the_default_view_caps() {
    // An explicit --range OVERRIDES the per-range cap: `tree <wb>/<Tab> --range A1:A60` shows ALL 60
    // cells (nothing elided), whereas the implicit whole-structure view of the same file caps at 50.
    let fx = Fixture::new("tree-range");
    let body: String = (1..=60)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    fx.file("T", "A1:A60", &body); // 60 authored cells in one range file

    // The default (no --range) whole-structure view caps at 50 with the 10-cell elision marker.
    let (dc, dflt) = run(&["tree", fx.path().to_str().unwrap()]);
    assert_eq!(dc, 0, "{dflt}");
    assert!(
        !dflt.contains("A51 ") && dflt.contains("[+10"),
        "the implicit view keeps the cap (A51 elided, +10 marker):\n{dflt}"
    );

    // --range on the tab scope shows EVERY coordinate, uncapped — no elision marker.
    let scope = fx.path().join("T");
    let (rc, ranged) = run(&["tree", scope.to_str().unwrap(), "--range", "A1:A60"]);
    assert_eq!(rc, 0, "a tab-scoped --range exits 0:\n{ranged}");
    for cell in ["A1 ", "A50 ", "A51 ", "A60 "] {
        assert!(
            ranged.contains(cell),
            "the explicit range shows every cell incl. {cell} (uncapped):\n{ranged}"
        );
    }
    assert!(
        !ranged.contains("use --full to expand") && !ranged.contains("[+"),
        "an explicit range elides nothing:\n{ranged}"
    );

    // --range without a tab scope (a whole-workbook path) is a bad-args refusal: the range is ambiguous
    // across tabs, so it needs a <workbook>/<Tab> scope.
    let (bc, _) = run(&["tree", fx.path().to_str().unwrap(), "--range", "A1:A60"]);
    assert_eq!(bc, 2, "--range without a tab scope is bad args (exit 2)");
}

#[test]
fn tree_collapses_a_grid5_array_formula_under_functions_and_expands_it_under_values() {
    // GRID5 mode-conditional collapse (the `fe.array_formula && mode == Functions` branch in
    // tree.rs cell_nodes): a range file that is a SINGLE array formula is ONE node under --functions
    // (its formula lives once, at the anchor — no per-cell authored source to show), but expands to
    // one node per computed A1 coordinate under --values, exactly like any range. A1:A3 = {3;1;2}
    // and C1:C3 = `=SORT(A1:A3)`, whose computed value is the sorted {1;2;3}.
    let fx = Fixture::new("tree-grid5");
    fx.file("Sheet1", "A1:A3", "3\n1\n2")
        .file("Sheet1", "C1:C3", "=SORT(A1:A3)");

    // --functions: ONE node carrying the array formula at the range's anchor; it does NOT expand.
    let (fc, funcs) = run(&["tree", fx.path().to_str().unwrap(), "--functions"]);
    assert_eq!(fc, 0, "{funcs}");
    assert!(
        funcs.contains("C1:C3  # =SORT(A1:A3)"),
        "the array formula is ONE node at the range anchor:\n{funcs}"
    );
    assert!(
        !funcs.contains("C2"),
        "under --functions the array formula does not expand per coordinate:\n{funcs}"
    );

    // --values: the array formula expands to one node per computed coordinate (the sorted elements),
    // and the collapsed range-file node is gone.
    let (vc, vals) = run(&["tree", fx.path().to_str().unwrap(), "--values"]);
    assert_eq!(vc, 0, "{vals}");
    assert!(
        !vals.contains("C1:C3"),
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
    // A <workbook>/<Tab> scope roots the view at that tab: the tab's cells show directly (no tab-dir
    // header) and the OTHER tab is absent.
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
    // CORE3: tree opens nothing for write; the authoritative workbook is byte-identical after (in both
    // modes, --values included — the cache is disabled so no derived bytes are written either).
    let fx = tree_fixture("tree-readonly");
    let before = snapshot(fx.path());
    let (fc, _) = run(&["tree", fx.path().to_str().unwrap(), "--functions"]);
    let (vc, _) = run(&["tree", fx.path().to_str().unwrap(), "--values"]);
    assert_eq!((fc, vc), (0, 0));
    let after = snapshot(fx.path());
    assert_eq!(
        before, after,
        "tree must leave every authoritative cell/tab/name byte-identical (CORE3)"
    );
}

#[test]
fn tree_rejects_the_removed_format_flag_as_an_unknown_flag() {
    // Text is the sole output form; `--format` no longer exists, so `tree ... --format json` is a plain
    // unknown-flag refusal (exit 2) — there is no second machine-readable form to re-derive from.
    let fx = tree_fixture("tree-nojson");
    let (code, _) = run(&["tree", fx.path().to_str().unwrap(), "--format", "json"]);
    assert_eq!(code, 2, "`--format json` is now an unknown flag (exit 2)");
}
