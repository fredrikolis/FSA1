// Concern: serves the verb layer over MCP on stdio | Non-concern: what a verb computes, argv, exit codes | IO: (JSON-RPC on stdin) -> JSON-RPC on stdout

mod tools;

use std::io::{BufRead, Write};

use serde_json::{Value, json};

/// The one version of the protocol this server implements. A client asking for it gets it back; a
/// client asking for anything else is told what we speak instead, which is what the handshake is for
/// — echoing an unknown string back would claim support for a protocol we have never seen.
const PROTOCOL_VERSION: &str = "2025-06-18";

fn main() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
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
