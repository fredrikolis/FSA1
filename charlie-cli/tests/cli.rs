// Concern: the CLI CONTRACT integration test — drive the built `charlie-cli` binary end-to-end against temp workbooks and lock the observable surface the model's unit tests cannot: the argv dispatch, BOTH output forms selected by `--format` (the human ASCII table / scalar on stdout in text mode, and the `{status,data|error}` JSON envelope on stdout in json mode — success data, the diagnostics[] array, the eval value, and error/not-found/validation envelopes), the on-disk `sample` workbook + its never-clobber refusal, the `import` of a real `.ods`/`.xlsx` into a renderable workbook (+ its unsupported-extension/conflict/not-found refusals), the `--guide` text, and the EXIT CODE an agent branches on (0 clean render/check/eval/sample/import/guide · 2 bad args · 3 error-severity diagnostics or error-valued eval or an unsupported import format · 4 sample/import target-dir conflict · 24 not found) | Non-concern: the render/lint/eval LOGIC (charlie-model's own tests own value spelling, demand-driven eval, array broadcasting, diagnostic detection, and the sample CONTENT), the ODS/xlsx conversion LOGIC (charlie-ingest's own tests own it), the envelope serialization (main.rs `output` owns it), and comfy-table's internals | IO: spawns `$CARGO_BIN_EXE_charlie-cli`, writes temp workbook dirs, reads committed `.ods`/`.xlsx` fixtures, asserts on stdout + exit status
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

// ---- `--format json`: the machine envelope surface (cli-interface-standards Part 2). ----

#[test]
fn render_json_emits_a_success_envelope_with_columns_and_rows() {
    let fx = Fixture::new("render-json");
    fx.file("Sheet1", "A1", "20000")
        .file("Sheet1", "B1", "=A1*2");
    let (code, out) = run(&["render", fx.path().to_str().unwrap(), "--format", "json"]);
    assert_eq!(code, 0, "clean render exits 0; got:\n{out}");
    assert!(
        out.contains("\"status\":\"success\""),
        "success envelope on stdout:\n{out}"
    );
    assert!(out.contains("\"columns\""), "grid columns:\n{out}");
    assert!(out.contains("\"rows\""), "grid rows:\n{out}");
    // The computed value rides in the structured data, not a scraped ASCII cell.
    assert!(out.contains("40000"), "B1 computes to 40000:\n{out}");
}

#[test]
fn check_clean_json_is_a_success_envelope_with_empty_diagnostics() {
    let fx = Fixture::new("check-clean-json");
    fx.file("Sheet1", "A1", "1").file("Sheet1", "B1", "=A1+1");
    let (code, out) = run(&["check", fx.path().to_str().unwrap(), "--format", "json"]);
    assert_eq!(code, 0, "clean check exits 0:\n{out}");
    assert!(out.contains("\"status\":\"success\""), "success:\n{out}");
    assert!(
        out.contains("\"diagnostics\":[]"),
        "empty diagnostics array:\n{out}"
    );
}

#[test]
fn check_cycle_json_is_an_error_envelope_with_a_located_diagnostic() {
    let fx = Fixture::new("check-cycle-json");
    fx.file("Sheet1", "A1", "=B1").file("Sheet1", "B1", "=A1");
    let (code, out) = run(&["check", fx.path().to_str().unwrap(), "--format", "json"]);
    assert_eq!(code, 3, "a cycle is a validation error -> exit 3:\n{out}");
    assert!(
        out.contains("\"status\":\"error\""),
        "error envelope:\n{out}"
    );
    assert!(
        out.contains("\"code\":\"validation_error\""),
        "validation_error code:\n{out}"
    );
    // The diagnostic carries the stable dispatch code and a machine location (never a scraped table).
    assert!(out.contains("\"code\":\"cycle\""), "the cycle code:\n{out}");
    assert!(out.contains("\"location\""), "a located diagnostic:\n{out}");
}

#[test]
fn check_json_completes_the_location_and_carries_a_fix_for_a_non_canonical_filename() {
    // A lowercase filename is a load-time refusal with a DETERMINISTIC canonical rename: the JSON
    // diagnostic completes its byte `span` {offset,length} AND carries a machine-applicable `fix`
    // (cli-interface-standards Part 2 "Diagnostics"), so an agent can apply the rename unattended.
    let fx = Fixture::new("check-noncanon-json");
    fx.file("Sheet1", "a1", "42");
    let (code, out) = run(&["check", fx.path().to_str().unwrap(), "--format", "json"]);
    assert_eq!(
        code, 3,
        "a non-canonical filename rejects -> exit 3:\n{out}"
    );
    assert!(
        out.contains("\"code\":\"lowercase-column\""),
        "the lowercase code:\n{out}"
    );
    // The location is a byte span {offset,length}, never a bare `byte`.
    assert!(
        out.contains("\"span\":{\"offset\":0,\"length\":2}"),
        "completed byte span:\n{out}"
    );
    // A machine-applicable fix with the deterministic canonical replacement.
    assert!(
        out.contains("\"fix\":{\"applicability\":\"machine_applicable\""),
        "a machine-applicable fix:\n{out}"
    );
    assert!(
        out.contains("\"replacement\":\"A1\""),
        "the canonical rename:\n{out}"
    );
    // The envelope carries the standard `meta` block.
    assert!(
        out.contains("\"meta\":{\"timestamp\":"),
        "envelope meta block:\n{out}"
    );
}

#[test]
fn eval_parse_error_json_locates_a_body_span_with_start_and_end() {
    // An unparseable ad-hoc formula is a located Body diagnostic: the JSON carries BOTH `start` and
    // `end` {line,column} for the offending token (cli-interface-standards Part 2 "Diagnostics").
    let fx = Fixture::new("eval-parse-json");
    fx.file("Sheet1", "A1", "1");
    let (code, out) = run(&[
        "eval",
        fx.path().to_str().unwrap(),
        "--formula",
        "=1+*2",
        "--format",
        "json",
    ]);
    assert_eq!(code, 3, "a parse error rejects -> exit 3:\n{out}");
    assert!(
        out.contains("\"code\":\"formula-syntax\""),
        "the syntax code:\n{out}"
    );
    assert!(
        out.contains("\"start\":{\"line\":1,\"column\":4}") && out.contains("\"end\":{"),
        "a body span with start AND end:\n{out}"
    );
}

#[test]
fn eval_json_wraps_the_value_in_a_success_envelope() {
    let fx = Fixture::new("eval-json");
    fx.file("Sheet1", "A1:A3", "1\n2\n3");
    let (code, out) = run(&[
        "eval",
        fx.path().to_str().unwrap(),
        "--formula",
        "=SUM(A1:A3)",
        "--format",
        "json",
    ]);
    assert_eq!(code, 0, "clean eval exits 0:\n{out}");
    assert!(
        out.contains("\"status\":\"success\""),
        "success envelope:\n{out}"
    );
    assert!(out.contains("\"value\":\"6\""), "value in data:\n{out}");
}

#[test]
fn eval_error_value_json_is_an_error_envelope_carrying_the_value() {
    let fx = Fixture::new("eval-errval-json");
    fx.file("Sheet1", "A1", "1");
    let (code, out) = run(&[
        "eval",
        fx.path().to_str().unwrap(),
        "--formula",
        "=1/0",
        "--format",
        "json",
    ]);
    assert_eq!(code, 3, "an error-valued result exits 3:\n{out}");
    assert!(
        out.contains("\"status\":\"error\""),
        "error envelope:\n{out}"
    );
    assert!(
        out.contains("\"value\":\"#DIV/0!\""),
        "the error value in data:\n{out}"
    );
}

#[test]
fn a_bad_arg_json_error_envelope_is_on_stdout() {
    // An operational error is DATA in json mode: the envelope prints to STDOUT (not prose to stderr),
    // so an agent can parse the failure. Exit code stays 2 (bad args).
    let (code, out) = run(&["frobnicate", "--format", "json"]);
    assert_eq!(code, 2, "an unknown command is exit 2:\n{out}");
    assert!(
        out.contains("\"status\":\"error\""),
        "error on stdout:\n{out}"
    );
    assert!(
        out.contains("\"code\":\"invalid_arguments\""),
        "the invalid_arguments code:\n{out}"
    );
}

#[test]
fn a_not_found_json_error_envelope_is_on_stdout() {
    let (code, out) = run(&["check", "/no/such/charlie/workbook/xyz", "--format", "json"]);
    assert_eq!(code, 24, "not found is exit 24:\n{out}");
    assert!(
        out.contains("\"status\":\"error\""),
        "error on stdout:\n{out}"
    );
    assert!(
        out.contains("\"code\":\"not_found\""),
        "the not_found code:\n{out}"
    );
}

#[test]
fn an_invalid_format_value_is_bad_args() {
    let fx = Fixture::new("badformat");
    fx.file("Sheet1", "A1", "1");
    let (code, _) = run(&["render", fx.path().to_str().unwrap(), "--format", "yaml"]);
    assert_eq!(code, 2, "an unknown --format value is exit 2 (bad args)");
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
    let (code, out) = run(&[
        "import",
        src.to_str().unwrap(),
        dest.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(code, 0, "import should succeed:\n{out}");
    assert!(
        out.contains("\"status\":\"success\""),
        "success envelope:\n{out}"
    );
    assert!(
        out.contains("\"files\":2"),
        "two range files written:\n{out}"
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
    let (code, out) = run(&[
        "import",
        "/no/such/file.ods",
        dest.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(code, 24, "a missing source is not found (exit 24):\n{out}");
    assert!(
        out.contains("\"code\":\"not_found\""),
        "not_found code:\n{out}"
    );
}

#[test]
fn import_xlsx_then_eval_the_converted_workbook() {
    // The CLI import path is format-agnostic (it dispatches to charlie-ingest by extension), so an
    // .xlsx flows through the binary exactly as the .ods does — same converted values.
    let fx = Fixture::new("import-xlsx");
    let dest = fx.path().join("wb");
    let src = ingest_fixture("smoke.xlsx");
    let (code, out) = run(&[
        "import",
        src.to_str().unwrap(),
        dest.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(code, 0, "xlsx import should succeed:\n{out}");
    assert!(
        out.contains("\"files\":2"),
        "two range files written:\n{out}"
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
    let (code, out) = run(&[
        "import",
        src.to_str().unwrap(),
        dest.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(
        code, 3,
        "an unsupported extension is a validation error (exit 3):\n{out}"
    );
    assert!(
        out.contains("\"code\":\"validation_error\"") && out.contains("unsupported source format"),
        "validation refusal naming the format:\n{out}"
    );
}
