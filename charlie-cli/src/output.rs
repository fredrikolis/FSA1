// Concern: the OUTPUT ENVELOPE layer — the single home for the CLI's machine surface: the `--format` selector, the stable `ErrorCode` (a machine dispatch key + its exit code), and the dual renderer that lays ONE structured outcome (a render grid, a diagnostics report, an eval value, a sample result, an import result, or an operational error) into EITHER a `{status,data|error}` JSON envelope on stdout (`--format json`) OR the human ASCII/prose form (`--format text`, the default); errors and diagnostics are DATA — in JSON they are enveloped on stdout, never scraped from a bordered table (cli-interface-standards Part 2) | Non-concern: WHAT to show (charlie-model's `render`/`lint`/`eval_formula` own the grid, the diagnostics, and the value, charlie-ingest owns the import; this only serializes their output), argv parsing and exit-code dispatch (main.rs), and comfy-table drawing (ascii.rs, which this delegates to for the text form) | IO: (a `Format` + a structured outcome) -> a printed JSON envelope on stdout OR a human table/prose on stdout/stderr
//! The output envelope: [`Format`] selects the rendering, [`ErrorCode`] is the stable machine error
//! key, and the `emit_*` functions dual-render one outcome as either a JSON envelope (stdout) or the
//! human ASCII/prose form. The JSON form is the machine-parseable surface an agent branches on; the
//! text form is the terminal-friendly default.

use std::time::{SystemTime, UNIX_EPOCH};

use charlie_model::{Applicability, Diagnostic, Fix, Loc, RenderGrid, Severity};

use crate::ascii::{diagnostics_table, grid_table};

/// How the CLI renders its outcome: the human ASCII/prose form (the default) or a machine-parseable
/// JSON envelope on stdout. Selected by `--format`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Json,
}

/// A stable machine error code for an OPERATIONAL failure of the invocation (bad args, not found,
/// conflict, I/O, a validation refusal). The `code` string is the agent's dispatch key and each maps
/// to the exit code an agent also branches on (cli-interface-standards Part 2 "Standard Error Codes").
#[derive(Clone, Copy)]
pub enum ErrorCode {
    /// Invalid CLI usage (unknown command/flag, missing/duplicate argument) — exit 2.
    InvalidArguments,
    /// The input failed validation (a workbook won't load, an error-severity diagnostic) — exit 3.
    Validation,
    /// The target state conflicts with the operation (the never-clobber refusal) — exit 4.
    Conflict,
    /// A path (workbook directory) does not exist — exit 24.
    NotFound,
    /// An unexpected I/O failure — exit 1.
    Io,
}

impl ErrorCode {
    /// The stable code string an agent switches on.
    fn code_str(self) -> &'static str {
        match self {
            ErrorCode::InvalidArguments => "invalid_arguments",
            ErrorCode::Validation => "validation_error",
            ErrorCode::Conflict => "conflict",
            ErrorCode::NotFound => "not_found",
            ErrorCode::Io => "internal_error",
        }
    }

    /// The process exit code paired with this error (kept consistent with the code per the standard).
    pub fn exit(self) -> u8 {
        match self {
            ErrorCode::InvalidArguments => 2,
            ErrorCode::Validation => 3,
            ErrorCode::Conflict => 4,
            ErrorCode::Io => 1,
            ErrorCode::NotFound => 24,
        }
    }
}

// ----------------------------------------------------------------------------------------------
// JSON encoding — a minimal hand-rolled serializer (the crate's only external dep is comfy-table;
// the payloads here are strings and string arrays, so a proper string escaper is all that is needed).
// ----------------------------------------------------------------------------------------------

/// Encode `s` as a JSON string literal (surrounding quotes included), escaping per RFC 8259.
fn jstr(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A JSON array of already-encoded element strings.
fn jarray(elems: &[String]) -> String {
    format!("[{}]", elems.join(","))
}

/// The located pointer of a diagnostic as a JSON object, faithful to the heterogeneous [`Loc`]
/// (never flattened to a single scraped string), carrying both halves of a location whenever the
/// diagnostic knows them (cli-interface-standards Part 2 "Diagnostics": byte `span` `{offset,length}`
/// AND/OR `start`/`end` `{line,column}` — "provide both when you can"): a filename anchor emits its
/// byte `span`, a body anchor its 1-based `start`+`end` line/column, a tab its name, a
/// sheet-qualified file both tab and name.
fn location_json(loc: &Loc) -> String {
    match loc {
        Loc::File {
            name,
            span: Some(s),
        } => format!(
            "{{\"file\":{},\"span\":{{\"offset\":{},\"length\":{}}}}}",
            jstr(name),
            s.offset,
            s.len
        ),
        Loc::File { name, span: None } => format!("{{\"file\":{}}}", jstr(name)),
        Loc::Body {
            file,
            line,
            col,
            end_line,
            end_col,
        } => format!(
            "{{\"file\":{},\"start\":{{\"line\":{line},\"column\":{col}}},\
             \"end\":{{\"line\":{end_line},\"column\":{end_col}}}}}",
            jstr(file)
        ),
        Loc::Tab { tab } => format!("{{\"tab\":{}}}", jstr(tab)),
        Loc::TabFile { tab, name } => {
            format!("{{\"tab\":{},\"file\":{}}}", jstr(tab), jstr(name))
        }
    }
}

/// The file a diagnostic (and its [`Fix`] edit) is anchored on — the filename for a name/sheet-file
/// anchor, the body file for a body anchor, the tab for a whole-tab anchor.
fn loc_file(loc: &Loc) -> &str {
    match loc {
        Loc::File { name, .. } => name,
        Loc::Body { file, .. } => file,
        Loc::TabFile { name, .. } => name,
        Loc::Tab { tab } => tab,
    }
}

/// A structured `fix` as a JSON object: the `applicability` gate (whether an agent may apply it
/// unattended) plus the `edits[]` — each `{file, span, replacement}` — that carry it out
/// (cli-interface-standards Part 2 "Diagnostics"). Emitted only for a diagnostic that carries a known
/// machine edit; omitted otherwise (never fabricated).
fn fix_json(loc: &Loc, fix: &Fix) -> String {
    let applicability = match fix.applicability {
        Applicability::MachineApplicable => "machine_applicable",
        Applicability::MaybeIncorrect => "maybe_incorrect",
    };
    let edit = format!(
        "{{\"file\":{},\"span\":{{\"offset\":{},\"length\":{}}},\"replacement\":{}}}",
        jstr(loc_file(loc)),
        fix.span.offset,
        fix.span.len,
        jstr(&fix.replacement)
    );
    format!(
        "{{\"applicability\":{},\"edits\":[{edit}]}}",
        jstr(applicability)
    )
}

/// One diagnostic as a JSON object: the stable `code` dispatch key, `severity`, one-line `message`,
/// the structured `help` remediation (what to change — read from a dedicated field, not parsed out of
/// `message`), the located `location`, and — when the remediation is a known machine edit — a
/// structured `fix` (cli-interface-standards Part 2 "Diagnostics"; `fix` omitted when unknown).
fn diagnostic_json(d: &Diagnostic) -> String {
    let severity = match d.code.severity() {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };
    let mut out = format!(
        "{{\"code\":{},\"severity\":{},\"message\":{},\"help\":{},\"location\":{}",
        jstr(d.code.code_str()),
        jstr(severity),
        jstr(&d.message),
        jstr(d.code.help()),
        location_json(&d.loc)
    );
    if let Some(fix) = &d.fix {
        out.push_str(",\"fix\":");
        out.push_str(&fix_json(&d.loc, fix));
    }
    out.push('}');
    out
}

/// The envelope `meta` block: the `timestamp` (Unix seconds from [`SystemTime`]) the standard's
/// envelope shape carries (cli-interface-standards Part 2 "Unified Output Envelope"). A pre-epoch
/// clock (never expected) degrades to `0` rather than panicking.
fn meta_json() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{{\"timestamp\":{timestamp}}}")
}

/// Print a `{"status":"success","data":<data>,"meta":{...}}` envelope on stdout. `data` is a
/// pre-encoded JSON value.
fn print_success(data: &str) {
    println!(
        "{{\"status\":\"success\",\"data\":{data},\"meta\":{}}}",
        meta_json()
    );
}

/// Print a `{"status":"error","error":{code,message},"data":<data>,"meta":{...}}` envelope on stdout.
/// `data` is a pre-encoded JSON value (or `null` when the error carries no payload).
fn print_error(code: ErrorCode, message: &str, data: &str) {
    println!(
        "{{\"status\":\"error\",\"error\":{{\"code\":{},\"message\":{}}},\"data\":{data},\"meta\":{}}}",
        jstr(code.code_str()),
        jstr(message),
        meta_json()
    );
}

// ----------------------------------------------------------------------------------------------
// Emitters — each dual-renders one outcome and returns nothing (the caller owns the exit code).
// ----------------------------------------------------------------------------------------------

/// Emit an OPERATIONAL error. JSON: a `{status:error,error:{code,message}}` envelope on stdout (errors
/// are data). Text: the human prose on stderr, with a `--help` pointer for a usage error.
pub fn emit_error(fmt: Format, code: ErrorCode, message: &str) {
    match fmt {
        Format::Json => print_error(code, message, "null"),
        Format::Text => {
            if matches!(code, ErrorCode::InvalidArguments) {
                eprintln!("charlie-cli: {message}\n\nrun `charlie-cli --help` for usage");
            } else {
                eprintln!("charlie-cli: {message}");
            }
        }
    }
}

/// Emit a rendered viewport. JSON: `data` = `{columns:[...],rows:[{label,cells:[...]}]}`. Text: the
/// comfy-table ASCII grid on stdout.
pub fn emit_grid(fmt: Format, grid: &RenderGrid) {
    match fmt {
        Format::Text => println!("{}", grid_table(grid)),
        Format::Json => {
            let columns: Vec<String> = grid.col_labels.iter().map(|c| jstr(c)).collect();
            let rows: Vec<String> = grid
                .rows
                .iter()
                .map(|r| {
                    let cells: Vec<String> = r.cells.iter().map(|c| jstr(c)).collect();
                    format!(
                        "{{\"label\":{},\"cells\":{}}}",
                        jstr(&r.row_label),
                        jarray(&cells)
                    )
                })
                .collect();
            let data = format!(
                "{{\"columns\":{},\"rows\":{}}}",
                jarray(&columns),
                jarray(&rows)
            );
            print_success(&data);
        }
    }
}

/// Emit a lint report. The verdict drives `status`/exit (an error-severity diagnostic rejects); the
/// `diagnostics[]` array rides in `data` on success OR error (cli-interface-standards Part 2). JSON:
/// the envelope on stdout. Text: the comfy-table ASCII report on stdout. Returns the exit code.
pub fn emit_diagnostics(fmt: Format, diags: &[Diagnostic]) -> u8 {
    let error_count = diags
        .iter()
        .filter(|d| matches!(d.code.severity(), Severity::Error))
        .count();
    match fmt {
        Format::Text => println!("{}", diagnostics_table(diags)),
        Format::Json => {
            let arr = jarray(&diags.iter().map(diagnostic_json).collect::<Vec<_>>());
            let data = format!("{{\"diagnostics\":{arr}}}");
            if error_count > 0 {
                let msg = format!(
                    "{error_count} error-severity diagnostic{}",
                    if error_count == 1 { "" } else { "s" }
                );
                print_error(ErrorCode::Validation, &msg, &data);
            } else {
                print_success(&data);
            }
        }
    }
    if error_count > 0 {
        ErrorCode::Validation.exit()
    } else {
        0
    }
}

/// Emit an ad-hoc formula's plain value. JSON: `data` = `{value:"..."}`. Text: the bare value on stdout
/// (the same spelling `render` uses).
pub fn emit_eval_value(fmt: Format, value: &str) {
    match fmt {
        Format::Text => println!("{value}"),
        Format::Json => print_success(&format!("{{\"value\":{}}}", jstr(value))),
    }
}

/// Emit an ad-hoc formula that produced a spreadsheet ERROR value (`#DIV/0!`, `#REF!`, …) — a
/// validation refusal that still carries the error value. JSON: a `validation_error` envelope whose
/// `data.value` is the error text. Text: the error value on stdout (uniform with a plain value — one
/// stream for every eval outcome). Returns the exit code.
pub fn emit_eval_error_value(fmt: Format, value: &str) -> u8 {
    match fmt {
        Format::Text => println!("{value}"),
        Format::Json => print_error(
            ErrorCode::Validation,
            "the formula evaluated to an error value",
            &format!("{{\"value\":{}}}", jstr(value)),
        ),
    }
    ErrorCode::Validation.exit()
}

/// Emit a set of located diagnostics as a VALIDATION refusal (an unparseable ad-hoc formula, or a
/// workbook that would not load). JSON: a `validation_error` envelope carrying `data.diagnostics[]`.
/// Text: the located diagnostics on stdout (one stream for every evaluation outcome). Returns the
/// exit code.
pub fn emit_validation_diagnostics(fmt: Format, diags: &[Diagnostic]) -> u8 {
    match fmt {
        Format::Text => {
            for d in diags {
                println!("{d}");
            }
        }
        Format::Json => {
            let arr = jarray(&diags.iter().map(diagnostic_json).collect::<Vec<_>>());
            print_error(
                ErrorCode::Validation,
                "the input was refused",
                &format!("{{\"diagnostics\":{arr}}}"),
            );
        }
    }
    ErrorCode::Validation.exit()
}

/// Emit `--version` as a JSON success envelope. Version is always the machine handshake surface (a
/// JSON envelope regardless of `--format`, per cli-interface-standards.md); `data` = `{name,version}`
/// from the crate's compile-time metadata. Routes through [`print_success`] so the `{status,data}`
/// envelope shape stays single-homed in this module (never hand-rolled at the call site).
pub fn emit_version() {
    let data = format!(
        "{{\"name\":{},\"version\":{}}}",
        jstr("charlie-cli"),
        jstr(env!("CARGO_PKG_VERSION"))
    );
    print_success(&data);
}

/// Emit the result of importing a spreadsheet into a charlie workbook. JSON: `data` =
/// `{path,tabs:[...],files:n}`. Text: the terse next-steps hint (`text_lines`, composed by the caller
/// from the [`charlie_ingest::ImportReport`]).
pub fn emit_import(fmt: Format, path: &str, tabs: &[String], files: usize, text_lines: &str) {
    match fmt {
        Format::Text => print!("{text_lines}"),
        Format::Json => {
            let tabs_json = jarray(&tabs.iter().map(|t| jstr(t)).collect::<Vec<_>>());
            print_success(&format!(
                "{{\"path\":{},\"tabs\":{},\"files\":{files}}}",
                jstr(path),
                tabs_json
            ));
        }
    }
}

/// Emit the result of writing a sample workbook. JSON: `data` = `{path,tabs:[...]}`. Text: the terse
/// next-steps hint (`lines`, already composed by the caller from the fixed sample content).
pub fn emit_sample(fmt: Format, path: &str, tabs: &[String], text_lines: &str) {
    match fmt {
        Format::Text => print!("{text_lines}"),
        Format::Json => {
            let tabs_json = jarray(&tabs.iter().map(|t| jstr(t)).collect::<Vec<_>>());
            print_success(&format!(
                "{{\"path\":{},\"tabs\":{}}}",
                jstr(path),
                tabs_json
            ));
        }
    }
}
