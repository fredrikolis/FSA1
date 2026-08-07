// Concern: writes an outcome as text, pairing a failure with its exit code | Non-concern: choosing the outcome, table layout | IO: (an outcome) -> stdout/stderr + a code

use std::time::{SystemTime, UNIX_EPOCH};

use fsa1_model::{Diagnostic, Severity, TraceNode};

use fsa1_verbs::present::diagnostics_table;

#[derive(Clone, Copy)]
pub enum ErrorCode {
    InvalidArguments,
    Validation,
    Conflict,
    NotFound,
    Io,
}

impl ErrorCode {
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

pub fn emit_error(code: ErrorCode, message: &str) {
    if matches!(code, ErrorCode::InvalidArguments) {
        eprintln!("fsa1-cli: {message}\n\nrun `fsa1-cli --help` for usage");
    } else {
        eprintln!("fsa1-cli: {message}");
    }
}

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

pub fn emit_eval_value(value: &str) {
    println!("{value}");
}

pub fn emit_eval_error_value(value: &str) -> u8 {
    println!("{value}");
    ErrorCode::Validation.exit()
}

pub fn emit_validation_diagnostics(diags: &[Diagnostic]) -> u8 {
    for d in diags {
        println!("{d}");
    }
    ErrorCode::Validation.exit()
}

pub fn emit_version() {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!(
        "{{\"status\":\"success\",\"data\":{{\"name\":\"fsa1-cli\",\"version\":\"{}\"}},\"meta\":{{\"timestamp\":{timestamp}}}}}",
        env!("CARGO_PKG_VERSION")
    );
}

pub fn emit_trace(node: &TraceNode) {
    print!("{}", fsa1_verbs::present::trace(node));
}
