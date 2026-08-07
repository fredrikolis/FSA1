// Concern: the one way a verb says no, carrying the kind a front end maps to its own vocabulary | Non-concern: printing it, picking an exit code | IO: (a kind + message, or diagnostics) -> Refusal

use fsa1_model::Diagnostic;

/// The five ways an operation refuses. A front end maps these onto whatever it answers in — an exit
/// code, a JSON-RPC error — and nothing here knows which.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    InvalidArguments,
    Validation,
    Conflict,
    NotFound,
    Io,
}

impl Kind {
    /// The wire spelling, for a front end that names a kind rather than numbering it.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::InvalidArguments => "invalid-arguments",
            Kind::Validation => "validation",
            Kind::Conflict => "conflict",
            Kind::NotFound => "not-found",
            Kind::Io => "io",
        }
    }
}

/// A refusal carries its `diagnostics` when the workbook itself is what was refused, so a front end
/// can render them the way it renders any other finding rather than flattening them into the message.
#[derive(Clone, Debug)]
pub struct Refusal {
    pub kind: Kind,
    pub message: String,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn fail(kind: Kind, message: &str) -> Refusal {
    Refusal {
        kind,
        message: message.to_string(),
        diagnostics: Vec::new(),
    }
}

pub fn bad_arg(message: &str) -> Refusal {
    fail(Kind::InvalidArguments, message)
}

/// A load that refused: the diagnostics ARE the message, so the text is left to whoever draws them.
pub fn refused(diagnostics: Vec<Diagnostic>) -> Refusal {
    Refusal {
        kind: Kind::Validation,
        message: String::new(),
        diagnostics,
    }
}
