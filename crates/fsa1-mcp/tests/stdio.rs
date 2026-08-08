// Concern: drives the built binary over real argv, stdin, stdout and stderr | Non-concern: what a verb computes (fsa1-verbs) | IO: (argv; request lines; fsa1-ingest fixtures) -> assertions + a temp dir

use std::io::{Read, Write};
use std::process::{Command, Stdio};

/// Writes every line to the server's stdin and returns one parsed value per response line. The
/// process is driven end to end rather than calling a handler directly: a framing bug that a unit
/// test cannot see is exactly what this is for.
fn talk(lines: &[&str]) -> Vec<serde_json::Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fsa1-mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn fsa1-mcp");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        for l in lines {
            writeln!(stdin, "{l}").expect("write a request");
        }
    }
    let out = child.wait_with_output().expect("wait");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).expect("a JSON response line"))
        .collect()
}

fn text_of(v: &serde_json::Value) -> String {
    v["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string()
}

fn is_error(v: &serde_json::Value) -> bool {
    v["result"]["isError"].as_bool().unwrap()
}

/// A workbook on disk, built from the model's own sample so no test spawns the other front end.
struct Fixture(std::path::PathBuf);

impl Fixture {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("fsa1-mcp-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        for (rel, body) in fsa1_model::sample_workbook() {
            let full = dir.join(fsa1_model::range_file_path(&rel));
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, body).unwrap();
        }
        Fixture(dir)
    }
    fn path(&self) -> String {
        self.0.display().to_string()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn call(name: &str, args: serde_json::Value) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"{name}","arguments":{args}}}}}"#
    )
}

#[test]
fn initialize_answers_with_the_version_this_server_speaks() {
    let r = talk(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
    ]);
    assert_eq!(r[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(r[0]["result"]["serverInfo"]["name"], "fsa1");
    // Where a MACHINE reads the build's version: it must be the value `--version` prints for a person.
    assert_eq!(
        r[0]["result"]["serverInfo"]["version"],
        env!("CARGO_PKG_VERSION")
    );
}

/// A client asking for a protocol we do not implement is told what we DO implement — echoing its
/// string back would claim support for a version this server has never seen.
#[test]
fn an_unknown_protocol_version_is_answered_with_ours_not_echoed() {
    let r = talk(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
    ]);
    assert_eq!(r[0]["result"]["protocolVersion"], "2025-06-18");
}

#[test]
fn a_notification_is_answered_by_silence() {
    let r = talk(&[
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":9,"method":"tools/list"}"#,
    ]);
    assert_eq!(r.len(), 1, "only the request is answered: {r:?}");
    assert_eq!(r[0]["id"], 9);
}

#[test]
fn tools_list_names_exactly_the_six_verbs_that_need_the_engine() {
    let r = talk(&[r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#]);
    let names: Vec<&str> = r[0]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["unpack", "pack", "render", "check", "eval", "trace"]
    );
}

/// The one thing a spreadsheet server must not invite is a request to write a cell, so every tool
/// carries the same closing note. The INVARIANT is that they all share it — not its wording, which
/// is ours on both sides and free to change.
#[test]
fn every_tool_closes_with_the_same_note_about_writing_cells() {
    let r = talk(&[r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#]);
    let tools = r[0]["result"]["tools"].as_array().unwrap();
    let note = |t: &serde_json::Value| {
        let d = t["description"].as_str().unwrap();
        d[d.rfind(". ").map(|i| i + 2).unwrap_or(0)..].to_string()
    };
    let first = note(&tools[0]);
    assert!(
        first.to_lowercase().contains("cell"),
        "the shared note should be about cells: {first}"
    );
    for t in tools {
        assert_eq!(note(t), first, "{} closes differently", t["name"]);
    }
}

#[test]
fn render_and_check_answer_over_the_wire() {
    let fx = Fixture::new("happy");
    let r = talk(&[
        &call("check", serde_json::json!({ "target": fx.path() })),
        &call(
            "render",
            serde_json::json!({ "target": format!("{}/Orders", fx.path()), "mode": "values" }),
        ),
    ]);
    assert!(!is_error(&r[0]), "check: {}", text_of(&r[0]));
    assert!(!is_error(&r[1]), "render: {}", text_of(&r[1]));
    assert!(text_of(&r[1]).contains("Product"), "{}", text_of(&r[1]));
}

/// A formula that yields an error value EVALUATED. Scoring that as a failed call would tell a model
/// the tool broke when what it actually got was the answer.
#[test]
fn an_error_value_from_eval_is_an_answer_not_a_refusal() {
    let fx = Fixture::new("errval");
    let r = talk(&[&call(
        "eval",
        serde_json::json!({ "target": fx.path(), "formula": "=1/0" }),
    )]);
    assert!(!is_error(&r[0]), "{}", text_of(&r[0]));
    assert_eq!(text_of(&r[0]).trim(), "#DIV/0!");
}

#[test]
fn a_missing_workbook_is_a_refusal_naming_its_kind() {
    // Three deep, so the resolver reaches an absent ROOT rather than a parent it can probe.
    let root = std::env::temp_dir().join(format!("fsa1-mcp-absent-{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(&root).unwrap();
    let missing = root.join("wb").join("Tab").join("A1");
    let r = talk(&[&call(
        "check",
        serde_json::json!({ "target": missing.display().to_string() }),
    )]);
    std::fs::remove_dir_all(&root).ok();
    assert!(is_error(&r[0]));
    assert!(
        text_of(&r[0]).starts_with("fsa1: not-found:"),
        "{}",
        text_of(&r[0])
    );
}

#[test]
fn a_missing_required_argument_is_invalid_arguments() {
    let r = talk(&[&call("render", serde_json::json!({}))]);
    assert!(is_error(&r[0]));
    assert!(
        text_of(&r[0]).starts_with("fsa1: invalid-arguments:"),
        "{}",
        text_of(&r[0])
    );
}

#[test]
fn an_unknown_tool_is_invalid_arguments_not_a_protocol_error() {
    let r = talk(&[&call("nope", serde_json::json!({}))]);
    assert!(is_error(&r[0]));
    assert!(
        text_of(&r[0]).contains("no such tool"),
        "{}",
        text_of(&r[0])
    );
}

/// A bad frame must not take the session with it: each of these is followed by a request that is
/// still answered.
#[test]
fn a_malformed_line_and_an_unknown_method_are_survivable() {
    let r = talk(&[
        "not json at all",
        r#"{"jsonrpc":"2.0","id":2,"method":"foo/bar"}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#,
    ]);
    assert_eq!(r[0]["error"]["code"], -32700);
    assert_eq!(r[1]["error"]["code"], -32601);
    assert_eq!(r[2]["id"], 3, "the server kept serving: {r:?}");
}

/// Runs the binary with `args` while HOLDING ITS STDIN OPEN and writing nothing, then waits with a
/// deadline. A flag that fell through to the read loop would block forever on that open pipe, so
/// returning at all is the proof that argv was answered before stdin was ever touched.
fn run_flag(args: &[&str]) -> (String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fsa1-mcp"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn fsa1-mcp");
    // Taken out of the child handle so nothing drops it: the write end stays open for the whole wait.
    let _held_open = child.stdin.take().expect("stdin");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let status = loop {
        match child.try_wait().expect("try_wait") {
            Some(s) => break s,
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                panic!("fsa1-mcp {args:?} did not return with stdin held open — it read stdin");
            }
            None => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    };
    assert!(status.success(), "fsa1-mcp {args:?}: {status}");

    let mut out = String::new();
    let mut err = String::new();
    child
        .stdout
        .take()
        .expect("stdout")
        .read_to_string(&mut out)
        .expect("read stdout");
    child
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut err)
        .expect("read stderr");
    (out, err)
}

#[test]
fn version_prints_the_workspace_version_without_reading_stdin() {
    let (out, err) = run_flag(&["--version"]);
    assert_eq!(out.trim(), env!("CARGO_PKG_VERSION"));
    assert!(err.is_empty(), "stderr: {err}");
}

#[test]
fn help_names_the_stdio_json_rpc_surface_without_reading_stdin() {
    let (out, err) = run_flag(&["--help"]);
    assert!(out.contains("JSON-RPC"), "{out}");
    assert!(out.contains("stdio"), "{out}");
    assert!(err.is_empty(), "stderr: {err}");
}

/// A host may pass a flag this build has never seen. Starting is the right answer: refusing to start
/// is the worse failure, and the session proves the server really did come up.
#[test]
fn an_unrecognized_argument_still_starts_the_server() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fsa1-mcp"))
        .arg("--some-future-flag")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn fsa1-mcp");
    writeln!(
        child.stdin.as_mut().expect("stdin"),
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list"}}"#
    )
    .expect("write a request");
    let out = child.wait_with_output().expect("wait");
    let v: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).expect("a JSON response");
    assert_eq!(v["id"], 1);
}

/// With stdin a PIPE rather than a terminal there is no hint, and every stdout line is a frame:
/// one byte on that channel that is not JSON-RPC corrupts the session. This proves only the
/// NEGATIVE: the hint needs a pty no case here allocates, so the branch writing it is covered by
/// no test — a known blind spot, not an oversight.
#[test]
fn a_piped_stdin_gets_no_hint_and_stdout_carries_only_frames() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fsa1-mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn fsa1-mcp");
    writeln!(
        child.stdin.as_mut().expect("stdin"),
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18"}}}}"#
    )
    .expect("write a request");
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let v: serde_json::Value =
            serde_json::from_str(line).expect("every stdout line is a frame");
        assert_eq!(v["jsonrpc"], "2.0", "{line}");
    }
}

/// A scratch directory this test file owns, so a `dest` that is taken VERBATIM has a parent to land
/// in. Named per test, because two of them run at once.
fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("fsa1-mcp-{tag}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).expect("create the scratch dir");
    dir
}

/// The file `crates/fsa1-cli/tests/cli_convert.rs` proves a strict unpack refuses, reached by path
/// rather than by a dependency edge: this crate does not read xlsx, it asks the verb layer to.
fn ingest_fixture(name: &str) -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fsa1-ingest/tests/fixtures")
        .join(name)
        .display()
        .to_string()
}

/// `strict` is a capability an agent can only reach if the schema offers it AND the handler reads it.
/// The same source that the strict call must refuse is imported by the call without it, so a `strict`
/// accepted and then dropped fails here rather than silently converting lossily.
#[test]
fn unpack_reads_strict_and_refuses_what_a_lossy_unpack_accepts() {
    let root = scratch("unpack-strict");
    let src = ingest_fixture("literals.xlsx");
    let r = talk(&[
        &call(
            "unpack",
            serde_json::json!({
                "source": src,
                "dest": root.join("strict").display().to_string(),
                "strict": true
            }),
        ),
        &call(
            "unpack",
            serde_json::json!({
                "source": src,
                "dest": root.join("lossy").display().to_string()
            }),
        ),
    ]);
    let (strict, lossy) = (text_of(&r[0]), text_of(&r[1]));
    std::fs::remove_dir_all(&root).ok();
    assert!(
        is_error(&r[0]),
        "a strict unpack of literals.xlsx refuses: {strict}"
    );
    assert!(
        !is_error(&r[1]),
        "the same source unpacks without strict: {lossy}"
    );
}

/// `format` names the one thing `pack` writes, so naming it changes nothing and naming anything else
/// is refused with the choices — never a silently different file.
#[test]
fn packs_format_key_defaults_to_xlsx_and_refuses_any_other() {
    let fx = Fixture::new("pack-format");
    let root = scratch("pack-format-out");
    let dest = |name: &str| root.join(name).display().to_string();
    let r = talk(&[
        &call(
            "pack",
            serde_json::json!({ "source": fx.path(), "dest": dest("with.xlsx"), "format": "xlsx" }),
        ),
        &call(
            "pack",
            serde_json::json!({ "source": fx.path(), "dest": dest("without.xlsx") }),
        ),
        &call(
            "pack",
            serde_json::json!({ "source": fx.path(), "dest": dest("other.ods"), "format": "ods" }),
        ),
    ]);
    let (with, without, other) = (text_of(&r[0]), text_of(&r[1]), text_of(&r[2]));
    let other_written = root.join("other.ods").exists();
    std::fs::remove_dir_all(&root).ok();

    assert!(!is_error(&r[0]) && !is_error(&r[1]), "{with}\n{without}");
    assert_eq!(
        with.replace("with.xlsx", "OUT"),
        without.replace("without.xlsx", "OUT"),
        "an explicit format: xlsx is what an omitted one already means"
    );
    assert!(is_error(&r[2]), "format: ods is refused: {other}");
    assert!(
        other.contains("unknown format \"ods\"") && other.contains("xlsx"),
        "the refusal names what was asked for and what is accepted: {other}"
    );
    assert!(!other_written, "a refused format writes no file");
}
