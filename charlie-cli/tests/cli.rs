// Concern: the CLI CONTRACT integration test — drive the built `charlie` binary end-to-end against temp workbooks and lock the observable surface the model's unit tests cannot: the argv dispatch, the ASCII table on stdout, and the EXIT CODE an agent branches on (0 clean render/check · 2 bad args · 3 error-severity diagnostics · 24 not found) | Non-concern: the render/lint LOGIC (charlie-model's own tests own value spelling, demand-driven eval, diagnostic detection) and comfy-table's internals | IO: spawns `$CARGO_BIN_EXE_charlie`, writes temp workbook dirs, asserts on stdout + exit status
//! End-to-end tests of the `charlie` binary: exit codes and stdout for `render`, `check`,
//! `--version`, and misuse. The spreadsheet logic is tested in `charlie-model`; this locks the
//! thin shell's own contract.

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
    let out = Command::new(env!("CARGO_BIN_EXE_charlie"))
        .args(args)
        .output()
        .expect("spawn charlie");
    let code = out.status.code().expect("exit code");
    (code, String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn render_values_draws_the_computed_cone() {
    let fx = Fixture::new("render");
    fx.file("Sheet1", "A1.cell", "20000")
        .file("Sheet1", "B1.cell", "=A1*2");
    let (code, out) = run(&["render", fx.path().to_str().unwrap()]);
    assert_eq!(code, 0, "clean render exits 0; got:\n{out}");
    // The header row, the gutter, and the demand-driven value B1 = 40000.
    assert!(out.contains("| A "), "column-letter header:\n{out}");
    assert!(out.contains("40000"), "B1 should compute to 40000:\n{out}");
}

#[test]
fn render_functions_shows_formula_text() {
    let fx = Fixture::new("funcs");
    fx.file("Sheet1", "A1.cell", "2")
        .file("Sheet1", "B1.cell", "=A1*2");
    let (code, out) = run(&["render", fx.path().to_str().unwrap(), "--functions"]);
    assert_eq!(code, 0);
    assert!(out.contains("=A1*2"), "formula text in --functions:\n{out}");
}

#[test]
fn render_missing_tab_is_not_found() {
    let fx = Fixture::new("notab");
    fx.file("Sheet1", "A1.cell", "1");
    let (code, _) = run(&["render", fx.path().to_str().unwrap(), "--tab", "Nope"]);
    assert_eq!(code, 24, "an unknown tab is exit 24 (not found)");
}

#[test]
fn check_clean_workbook_exits_zero() {
    let fx = Fixture::new("clean");
    fx.file("Sheet1", "A1.cell", "1")
        .file("Sheet1", "B1.cell", "=A1+1");
    let (code, out) = run(&["check", fx.path().to_str().unwrap()]);
    assert_eq!(code, 0, "clean check exits 0:\n{out}");
    assert!(out.contains("no diagnostics"), "clean report:\n{out}");
}

#[test]
fn check_cycle_reports_and_exits_three() {
    let fx = Fixture::new("cycle");
    fx.file("Sheet1", "A1.cell", "=B1")
        .file("Sheet1", "B1.cell", "=A1");
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
    fx.file("Sheet1", "A1:C3.range", "1\t2\t3\n4\t5\t6\n7\t8\t9")
        .file("Sheet1", "B2.cell", "x");
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
fn version_prints_a_json_envelope() {
    let (code, out) = run(&["--version"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("\"status\":\"success\""),
        "version envelope:\n{out}"
    );
    assert!(
        out.contains("\"name\":\"charlie\""),
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
    fx.file("Sheet1", "A1.cell", "1");
    let (code, _) = run(&["render", fx.path().to_str().unwrap(), "--range", "a1"]);
    assert_eq!(code, 2, "a non-canonical --range is exit 2");
}

#[test]
fn an_enormous_range_is_a_located_refusal_not_a_crash() {
    // `--range A1:A4294967295` is a syntactically-valid address pair; without a viewport cap it
    // aborts the process on allocation. It must instead be a clean bad-args refusal (exit 2).
    let fx = Fixture::new("hugerange");
    fx.file("Sheet1", "A1.cell", "1");
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
