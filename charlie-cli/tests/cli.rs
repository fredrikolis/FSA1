// Concern: the CLI CONTRACT integration test — drive the built `charlie-cli` binary end-to-end against temp workbooks and lock the observable surface the model's unit tests cannot: the argv dispatch, the ASCII table on stdout (render/check), the scalar VALUE on stdout (eval — e.g. `6`, `4`, `#DIV/0!`), the on-disk `sample` workbook + its never-clobber refusal, the `--guide` text, and the EXIT CODE an agent branches on (0 clean render/check/eval/sample/guide · 2 bad args · 3 error-severity diagnostics or error-valued eval · 4 sample target-dir conflict · 24 not found) | Non-concern: the render/lint/eval LOGIC (charlie-model's own tests own value spelling, demand-driven eval, array broadcasting, diagnostic detection, and the sample CONTENT) and comfy-table's internals | IO: spawns `$CARGO_BIN_EXE_charlie-cli`, writes temp workbook dirs, asserts on stdout + exit status
//! End-to-end tests of the `charlie-cli` binary: exit codes and stdout for `render`, `check`, `eval`,
//! `sample`, `--guide`, `--version`, and misuse. The spreadsheet logic is tested in `charlie-model`;
//! this locks the thin shell's own contract.

use std::path::{Path, PathBuf};
use std::process::Command;

const ANN: &str = "# Concern: c | Non-concern: n | IO: none\n";

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

    /// Write one cell/range file `name` with `body` into tab `tab`.
    fn file(&self, tab: &str, name: &str, body: &str) -> &Fixture {
        let dir = self.root.join(tab);
        std::fs::create_dir_all(&dir).expect("create tab dir");
        std::fs::write(dir.join(name), format!("{ANN}{body}")).expect("write file");
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
fn render_values_draws_the_computed_cone() {
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
fn eval_computes_a_sum_against_the_workbook() {
    // A range SUM over literal cells: the ad-hoc formula pulls A1:A3 through the model.
    let fx = Fixture::new("eval-sum");
    fx.file("Sheet1", "A1:A3", "1\n2\n3");
    let (code, out) = run(&["eval", fx.path().to_str().unwrap(), "=SUM(A1:A3)"]);
    assert_eq!(code, 0, "clean eval exits 0; got:\n{out}");
    assert_eq!(out.trim(), "6", "SUM(A1:A3) = 6:\n{out}");
}

#[test]
fn eval_number_uses_excel_general_format() {
    // The bare-value display path spells a number in Excel's General format (the SAME formatter the
    // `&`/TEXT text form uses): an extreme magnitude goes scientific instead of leaking Rust's
    // full-precision Display, a 16-integer-digit value rounds to 15 sig digits, and a computed -0.0
    // canonicalizes to an unsigned 0 (Excel never shows -0). Oracle: Excel's General (%.15g) rule.
    let fx = Fixture::new("eval-general");
    fx.file("Sheet1", "A1", "0");
    let big = fx.path().to_str().unwrap();
    for (formula, want) in [
        ("=1e20", "1E+20"),
        ("=1e-9", "1E-09"),
        ("=1234567890123456", "1.23456789012346E+15"),
        ("=-A1", "0"),
        ("=0*-1", "0"),
    ] {
        let (code, out) = run(&["eval", big, formula]);
        assert_eq!(code, 0, "{formula} evaluates cleanly:\n{out}");
        assert_eq!(
            out.trim(),
            want,
            "{formula} General-formats to {want}:\n{out}"
        );
    }
}

#[test]
fn eval_sumproduct_boolean_coercion_is_not_a_value_error() {
    // The `--(cond)` idiom: a boolean array coerces to 1/0 under the double-unary, so SUMPRODUCT
    // counts the cells > 7 (15, 25, 10, 30 => 4 of 5) rather than refusing #VALUE!.
    let fx = Fixture::new("eval-sumproduct");
    fx.file("Sheet1", "A1:A5", "5\n15\n25\n10\n30");
    let (code, out) = run(&[
        "eval",
        fx.path().to_str().unwrap(),
        "=SUMPRODUCT(--(A1:A5>7))",
    ]);
    assert_eq!(code, 0, "the SUMPRODUCT idiom evaluates cleanly:\n{out}");
    assert_ne!(out.trim(), "#VALUE!", "must NOT be #VALUE!:\n{out}");
    assert_eq!(out.trim(), "4", "cells > 7 are 15,25,10,30 => 4:\n{out}");
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
    let (code, _) = run(&["eval", fx.path().to_str().unwrap(), "=SUM("]);
    assert_eq!(code, 3, "a parse error is a validation exit (3)");
}

#[test]
fn eval_an_error_value_exits_non_zero() {
    // A well-formed formula that evaluates to a spreadsheet error prints the error and exits non-zero.
    let fx = Fixture::new("eval-err");
    fx.file("Sheet1", "A1", "1");
    let (code, out) = run(&["eval", fx.path().to_str().unwrap(), "=1/0"]);
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
