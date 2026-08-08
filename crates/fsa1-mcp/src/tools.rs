// Concern: declares each tool with its schema and runs the one a call names | Non-concern: the JSON-RPC envelope, what a verb computes | IO: (a tools/call params) -> a tool result

use fsa1_model::{Direction, FormulaOutcome, RenderMode};
use fsa1_verbs::ops;
use fsa1_verbs::present;
use fsa1_verbs::refusal::{Kind, Refusal, bad_arg};
use serde_json::{Value, json};
use std::path::Path;

/// Said on every tool, because the one thing a model must not infer from a spreadsheet server is
/// that it should ask the server to write cells. The filesystem is the write surface.
const FS_NOTE: &str = "Cells are files: read and edit them with your own file tools — this server has no write command.";

fn schema(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required })
}

pub fn list() -> Vec<Value> {
    let target = json!({ "type": "string", "description": "<workbook>[/<tab>[/<A1 cell or range, or a defined name>]]" });
    vec![
        json!({
            "name": "unpack",
            "description": format!("Convert an .xlsx or .ods file into an FSA1 workbook directory — tabs become folders, each file's name is the A1 range it fills. {FS_NOTE}"),
            "inputSchema": schema(json!({
                "source": { "type": "string", "description": "the .xlsx or .ods file to read" },
                "dest": { "type": "string", "description": "workbook directory to create; derived from the source name when omitted" },
                "decomposition": { "type": "string", "description": "how a sheet is split into range files" },
                "strict": { "type": "boolean", "description": "refuse rather than unpack a file the workbook cannot carry back identically" }
            }), &["source"])
        }),
        json!({
            "name": "pack",
            "description": format!("Serialize an FSA1 workbook directory back into a single .xlsx file. {FS_NOTE}"),
            "inputSchema": schema(json!({
                "source": { "type": "string", "description": "the workbook directory to pack" },
                "dest": { "type": "string", "description": "output .xlsx path; derived from the directory name when omitted" },
                "strict": { "type": "boolean", "description": "refuse rather than write where a figure reaches no Excel chart" },
                // Listed from the vocabulary, never by hand: a client should not need a refused call to learn the set.
                "format": { "type": "string", "enum": fsa1_verbs::PackFormat::choices(), "description": "the format written; xlsx when omitted, and the extension a derived dest takes" }
            }), &["source"])
        }),
        json!({
            "name": "render",
            "description": format!("Draw a workbook, a tab or a range as an ASCII grid, or as one standalone HTML document. {FS_NOTE}"),
            "inputSchema": schema(json!({
                "target": target,
                "mode": { "type": "string", "enum": ["combined", "values", "functions"], "description": "combined shows a value and the formula behind it; ascii only, since html draws values and shows each formula in its formula bar" },
                "format": { "type": "string", "enum": ["ascii", "html"], "description": "html takes no mode" }
            }), &["target"])
        }),
        json!({
            "name": "check",
            "description": format!("Lint a workbook, a tab or a range: broken references, overlapping ranges, malformed filenames, cycles. {FS_NOTE}"),
            "inputSchema": schema(json!({ "target": target }), &["target"])
        }),
        json!({
            "name": "eval",
            "description": format!("Evaluate an ad-hoc formula against a workbook without writing it anywhere. {FS_NOTE}"),
            "inputSchema": schema(json!({
                "target": target,
                "formula": { "type": "string", "description": "an =formula, evaluated in the target's scope" }
            }), &["target", "formula"])
        }),
        json!({
            "name": "trace",
            "description": format!("Walk one cell's dependency chain, upstream to its inputs or downstream to what it feeds. {FS_NOTE}"),
            "inputSchema": schema(json!({
                "target": json!({ "type": "string", "description": "<workbook>/<tab>/<A1> — exactly one cell" }),
                "direction": { "type": "string", "enum": ["upstream", "downstream"] },
                "depth": { "type": "integer", "description": "how many levels to walk; unbounded when omitted" }
            }), &["target"])
        }),
    ]
}

pub fn call(params: &Value) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    match run(name, &args) {
        Ok(text) => json!({ "content": [{ "type": "text", "text": text }], "isError": false }),
        Err(r) => {
            let text = if r.diagnostics.is_empty() {
                format!("fsa1: {}: {}", r.kind.as_str(), r.message)
            } else {
                let lines: Vec<String> = r.diagnostics.iter().map(|d| d.to_string()).collect();
                format!("fsa1: {}: {}", r.kind.as_str(), lines.join("\n"))
            };
            json!({ "content": [{ "type": "text", "text": text }], "isError": true })
        }
    }
}

fn str_arg(args: &Value, key: &str) -> Result<String, Refusal> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| bad_arg(&format!("{key} is required and must be a string")))
}

fn run(name: &str, args: &Value) -> Result<String, Refusal> {
    match name {
        "render" => {
            let mode = match args.get("mode").and_then(Value::as_str) {
                None => None,
                Some("combined") => Some(RenderMode::Combined),
                Some("values") => Some(RenderMode::Values),
                Some("functions") => Some(RenderMode::Functions),
                Some(v) => {
                    return Err(bad_arg(&format!(
                        "mode must be combined, values or functions (got {v:?})"
                    )));
                }
            };
            let format = match args
                .get("format")
                .and_then(Value::as_str)
                .unwrap_or("ascii")
            {
                "ascii" => ops::Format::Ascii,
                "html" => ops::Format::Html,
                v => {
                    return Err(bad_arg(&format!(
                        "format must be ascii or html (got {v:?})"
                    )));
                }
            };
            let r = ops::render(ops::RenderArgs {
                target: &str_arg(args, "target")?,
                mode,
                format,
            })?;
            Ok(join_notes(r.text, &r.notes))
        }
        "check" => {
            let diags = ops::check(ops::CheckArgs {
                target: &str_arg(args, "target")?,
            })?;
            if diags.is_empty() {
                return Ok("verdict: clean (0 error, 0 warning)".to_string());
            }
            // The verdict leads: the CLI answers this with an exit code, a table would not.
            let errors = diags
                .iter()
                .filter(|d| matches!(d.code.severity(), fsa1_model::Severity::Error))
                .count();
            let verdict = if errors > 0 { "rejected" } else { "clean" };
            Ok(format!(
                "verdict: {verdict} ({errors} error, {} warning)\n\n{}",
                diags.len() - errors,
                present::diagnostics_table(&diags)
            ))
        }
        "eval" => {
            let target = str_arg(args, "target")?;
            let formula = str_arg(args, "formula")?;
            // An error VALUE is an answer, not a refusal: the formula evaluated and this is what it says.
            Ok(
                match ops::eval(ops::EvalArgs {
                    target: &target,
                    formula: &formula,
                })? {
                    FormulaOutcome::Value(v) | FormulaOutcome::Error(v) => v,
                },
            )
        }
        "trace" => {
            let dir = match args
                .get("direction")
                .and_then(Value::as_str)
                .unwrap_or("upstream")
            {
                "upstream" => Direction::Upstream,
                "downstream" => Direction::Downstream,
                v => {
                    return Err(bad_arg(&format!(
                        "direction must be upstream or downstream (got {v:?})"
                    )));
                }
            };
            let depth = args.get("depth").and_then(Value::as_u64).map(|d| d as u32);
            let node = ops::trace(ops::TraceArgs {
                target: &str_arg(args, "target")?,
                dir,
                depth,
            })?;
            Ok(present::trace(&node))
        }
        "unpack" => {
            let source = str_arg(args, "source")?;
            let dest = args.get("dest").and_then(Value::as_str).map(Path::new);
            let decomposition = match args.get("decomposition").and_then(Value::as_str) {
                // The enum's own FromStr is each policy's one spelling, so the surfaces cannot drift.
                Some(v) => Some(v.parse::<fsa1_ingest::Decomposition>().map_err(|()| {
                    let all: Vec<&str> = fsa1_ingest::Decomposition::ALL
                        .iter()
                        .map(|d| d.name())
                        .collect();
                    bad_arg(&format!(
                        "unknown decomposition {v:?}; choose one of: {}",
                        all.join(", ")
                    ))
                })?),
                None => None,
            };
            let strict = args.get("strict").and_then(Value::as_bool).unwrap_or(false);
            let u = ops::unpack(ops::UnpackArgs {
                src: Path::new(&source),
                dest,
                decomposition,
                strict,
            })?;
            Ok(format!(
                "unpacked {source} -> {} ({} tab(s), {} range file(s), decomposed by {})",
                u.dest.display(),
                u.report.tabs.len(),
                u.report.files,
                u.report.decomposition.name(),
            ))
        }
        "pack" => {
            let source = str_arg(args, "source")?;
            let dest = args.get("dest").and_then(Value::as_str).map(Path::new);
            let strict = args.get("strict").and_then(Value::as_bool).unwrap_or(false);
            let format = match args.get("format").and_then(Value::as_str) {
                // The enum's own FromStr is each format's one spelling, so the surfaces cannot drift.
                Some(v) => v.parse::<fsa1_verbs::PackFormat>().map_err(|()| {
                    bad_arg(&format!(
                        "unknown format {v:?}; choose one of: {}",
                        fsa1_verbs::PackFormat::choices().join(", ")
                    ))
                })?,
                None => fsa1_verbs::PackFormat::Xlsx,
            };
            let p = ops::pack(ops::PackArgs {
                folder: Path::new(&source),
                dest,
                format,
                strict,
            })?;
            let text = format!(
                "packed {source} -> {} ({} sheet(s), {} chart(s) written)",
                p.dest.display(),
                p.sheets,
                p.charts
            );
            // A figure Excel draws no chart for is one note per figure, which is what a client acts on.
            Ok(join_notes(
                text,
                &p.not_drawn
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<String>>(),
            ))
        }
        other => Err(Refusal {
            kind: Kind::InvalidArguments,
            message: format!("no such tool {other:?}"),
            diagnostics: Vec::new(),
        }),
    }
}

/// A note is about the answer, so it rides with it rather than going to a stream no client reads.
fn join_notes(text: String, notes: &[String]) -> String {
    if notes.is_empty() {
        return text;
    }
    format!("{}\n\n{}", notes.join("\n"), text)
}
