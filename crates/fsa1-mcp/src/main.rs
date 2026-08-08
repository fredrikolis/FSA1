// Concern: answers a flag, or serves the verb layer over MCP on stdio | Non-concern: what a verb computes, exit codes | IO: (argv; stdin frames) -> flag text or frames on stdout, a hint on stderr

mod tools;

use std::io::{BufRead, IsTerminal, Write};

use serde_json::{Value, json};

/// The one version of the protocol this server implements. A client asking for it gets it back; a
/// client asking for anything else is told what we speak instead, which is what the handshake is for
/// — echoing an unknown string back would claim support for a protocol we have never seen.
const PROTOCOL_VERSION: &str = "2025-06-18";

/// Plain text, not JSON: a machine reads the version from `initialize`'s `serverInfo`, so these two
/// flags exist for a person at a terminal.
const HELP: &str = "\
fsa1-mcp — the FSA1 MCP server: a spreadsheet as a filesystem, driven by an agent.

It speaks JSON-RPC 2.0 over stdio, one frame per line: requests on stdin, responses on stdout.
There is nothing to run by hand — a host launches it, and the Claude plugin does so as

    npx -y fsa1-mcp

Options:
  --version    print the version and exit
  --help       print this text and exit

Any other argument is ignored and the server starts.
";

/// Stderr, never stdout: stdout is the JSON-RPC channel, and one byte on it that is not a frame
/// corrupts the session. The server runs on afterwards — refusing a tty is not this binary's job.
const TTY_HINT: &str =
    "fsa1-mcp is an MCP server; it reads JSON-RPC on stdin. A host launches it. Try --help.";

/// Answers a flag and says whether it did, before stdin is ever touched: a flag must return with the
/// pipe untouched, or a host holding it open waits forever for a frame it has already been given.
/// An unrecognized argument is neither an error nor a refusal — a host may pass one this build has
/// never seen, and failing to start is the worse failure.
fn answer_argv() -> bool {
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--version" => println!("{}", env!("CARGO_PKG_VERSION")),
            "--help" => print!("{HELP}"),
            _ => continue,
        }
        return true;
    }
    false
}

fn main() {
    if answer_argv() {
        return;
    }
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    if stdin.is_terminal() {
        eprintln!("{TTY_HINT}");
    }
    // A bad frame is answered, not fatal: exiting on one takes the whole session with it.
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Some(response) = handle(&line) else {
            continue;
        };
        if writeln!(stdout, "{response}").is_err() || stdout.flush().is_err() {
            break;
        }
    }
}

fn handle(line: &str) -> Option<Value> {
    let req: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return Some(error(Value::Null, -32700, &format!("parse error: {e}"))),
    };
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(json!({}));

    match method {
        // A notification carries no id and is answered by silence, per JSON-RPC.
        m if req.get("id").is_none() => {
            let _ = m;
            None
        }
        "initialize" => Some(result(id, initialize(&params))),
        "tools/list" => Some(result(id, json!({ "tools": tools::list() }))),
        "tools/call" => Some(result(id, tools::call(&params))),
        other => Some(error(id, -32601, &format!("method not found: {other}"))),
    }
}

fn initialize(_params: &Value) -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "fsa1", "version": env!("CARGO_PKG_VERSION") }
    })
}

fn result(id: Value, value: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": value })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
