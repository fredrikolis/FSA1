// Concern: the OUTPUT layer — the single home for the CLI's TEXT rendering surface: the stable `ErrorCode` (a machine dispatch key mapped to the process exit code an agent branches on) and the `emit_*` functions that lay ONE structured outcome (a render grid, a diagnostics report, an eval value, a dependency trace, or an operational error) into its human ASCII/prose form on stdout/stderr; the ONE machine handshake that remains is `--version`, emitted as the fixed `{status,data}` envelope cli-interface-standards mandates for the version surface | Non-concern: WHAT to show (charlie-model's `render`/`lint`/`eval_formula` own the grid, the diagnostics, and the value; this only serializes their output), argv parsing and exit-code dispatch (main.rs), and comfy-table drawing (ascii.rs, which this delegates to for the grid/diagnostics tables) | IO: (a structured outcome) -> printed human text on stdout (a table/scalar/tree) or stderr (an operational error) + the paired exit code for a diagnostics/eval/validation outcome
//! The output layer: [`ErrorCode`] is the stable machine error key (and its exit code), and the
//! `emit_*` functions render one outcome as its human ASCII/prose text form. Text is the sole output
//! form; `--version` alone stays a fixed JSON handshake (the standard's machine version surface).

use std::time::{SystemTime, UNIX_EPOCH};

use charlie_model::{Diagnostic, RenderGrid, Severity, TraceNode};

use crate::ascii::{diagnostics_table, grid_table};

/// A stable machine error code for an OPERATIONAL failure of the invocation (bad args, not found,
/// conflict, I/O, a validation refusal). Each maps to the exit code an agent branches on
/// (cli-interface-standards Part 2 "Standard Error Codes").
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
    /// The process exit code paired with this error (the machine dispatch key an agent branches on).
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
// Emitters — each renders one outcome as human text and (where an outcome carries a verdict) returns
// the paired exit code; the caller owns the exit code otherwise.
// ----------------------------------------------------------------------------------------------

/// Emit an OPERATIONAL error as human prose on stderr, with a `--help` pointer for a usage error.
pub fn emit_error(code: ErrorCode, message: &str) {
    if matches!(code, ErrorCode::InvalidArguments) {
        eprintln!("charlie-cli: {message}\n\nrun `charlie-cli --help` for usage");
    } else {
        eprintln!("charlie-cli: {message}");
    }
}

/// Emit a rendered viewport as the comfy-table ASCII grid on stdout.
pub fn emit_grid(grid: &RenderGrid) {
    println!("{}", grid_table(grid));
}

/// Emit a lint report as the comfy-table ASCII report on stdout, and return the exit code. The verdict
/// drives it: an error-severity diagnostic rejects the workbook (validation exit); otherwise 0 (any
/// warnings/advice ride along without failing — cli-interface-standards Part 2).
pub fn emit_diagnostics(diags: &[Diagnostic]) -> u8 {
    let error_count = diags
        .iter()
        .filter(|d| matches!(d.code.severity(), Severity::Error))
        .count();
    println!("{}", diagnostics_table(diags));
    if error_count > 0 {
        ErrorCode::Validation.exit()
    } else {
        0
    }
}

/// Emit an ad-hoc formula's plain value on stdout (the same spelling `render` uses).
pub fn emit_eval_value(value: &str) {
    println!("{value}");
}

/// Emit an ad-hoc formula that produced a spreadsheet ERROR value (`#DIV/0!`, `#REF!`, …) on stdout —
/// a validation refusal that still carries the error value (uniform with a plain value: one stream for
/// every eval outcome). Returns the exit code.
pub fn emit_eval_error_value(value: &str) -> u8 {
    println!("{value}");
    ErrorCode::Validation.exit()
}

/// Emit a set of located diagnostics as a VALIDATION refusal (an unparseable ad-hoc formula, or a
/// workbook that would not load) on stdout — one stream for every evaluation outcome. Returns the exit
/// code.
pub fn emit_validation_diagnostics(diags: &[Diagnostic]) -> u8 {
    for d in diags {
        println!("{d}");
    }
    ErrorCode::Validation.exit()
}

/// Emit `--version` as the fixed JSON handshake the standard mandates for the version surface (the one
/// machine envelope the CLI keeps): `{status:success, data:{name,version}, meta:{timestamp}}` on
/// stdout. The name is a constant and the version is compile-time semver, so both are safe to embed
/// verbatim (no escaping needed).
pub fn emit_version() {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!(
        "{{\"status\":\"success\",\"data\":{{\"name\":\"charlie-cli\",\"version\":\"{}\"}},\"meta\":{{\"timestamp\":{timestamp}}}}}",
        env!("CARGO_PKG_VERSION")
    );
}

/// Emit a dependency trace (CLI2) as an indented text tree on stdout — one line per node,
/// `<cell>  <formula>  -> <value>  [<hash|status>]`, `(repeated)` on a shared node. A produced trace is
/// always a success (the walk is total, CORE2); errors/diagnostics remain the caller's job.
pub fn emit_trace(node: &TraceNode) {
    let mut out = String::new();
    trace_text(node, 0, &mut out);
    print!("{out}");
}

/// Render one trace node (and its subtree) as an indented text line.
fn trace_text(node: &TraceNode, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    // A hash names an ordinary node; a hashless node (a cycle / depth-limit / blank) shows its status.
    let tag = match &node.hash {
        Some(h) => h.clone(),
        None => node.status.as_str().to_string(),
    };
    let formula = match &node.formula {
        Some(f) => format!("  {f}"),
        None => String::new(),
    };
    let repeated = if node.repeated { "  (repeated)" } else { "" };
    out.push_str(&format!(
        "{indent}{}{formula}  -> {}  [{tag}]{repeated}\n",
        node.cell, node.value
    ));
    for child in &node.children {
        trace_text(child, depth + 1, out);
    }
}
