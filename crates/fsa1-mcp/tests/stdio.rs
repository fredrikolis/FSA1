// Concern: drives the server over real stdin/stdout | Non-concern: what a verb computes (fsa1-verbs owns it) | IO: (request lines) -> assertions

use std::io::Write;
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
