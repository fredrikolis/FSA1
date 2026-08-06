// Concern: declares the refusal-code registry and the located Diag | Non-concern: raising a refusal, eval failures (an ErrKind value) | IO: (DiagCode, Span, message) -> Diag

use std::fmt;

/// A half-open byte span into the formula string. The lexer cuts only at char boundaries, so
/// `&formula[span.start..span.end]` never panics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Span {
        Span { start, end }
    }

    pub const fn at(offset: usize) -> Span {
        Span {
            start: offset,
            end: offset,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// A refusal code — the stable key a consumer switches on. Message wording is not frozen; see
/// [`DiagCode::summary`] for what each code means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagCode {
    UnexpectedChar,
    UnterminatedString,
    InvalidNumber,
    EmptyFormula,
    UnexpectedToken,
    UnexpectedEof,
    UnclosedParen,
    UnbalancedParen,
    UnknownFunction,
    BadArity,
    MalformedArray,
    ReservedUnion,
    ReservedIntersection,
    ReservedDynamicRange,
    ReservedName,
    UnsupportedFunction,
    ReservedCrossSheet,
    UnsupportedFormat,
    MalformedSheetName,
    RecursionLimit,
}

impl DiagCode {
    pub const ALL: &'static [DiagCode] = &[
        DiagCode::UnexpectedChar,
        DiagCode::UnterminatedString,
        DiagCode::InvalidNumber,
        DiagCode::EmptyFormula,
        DiagCode::UnexpectedToken,
        DiagCode::UnexpectedEof,
        DiagCode::UnclosedParen,
        DiagCode::UnbalancedParen,
        DiagCode::UnknownFunction,
        DiagCode::BadArity,
        DiagCode::MalformedArray,
        DiagCode::ReservedUnion,
        DiagCode::ReservedIntersection,
        DiagCode::ReservedDynamicRange,
        DiagCode::ReservedName,
        DiagCode::UnsupportedFunction,
        DiagCode::ReservedCrossSheet,
        DiagCode::UnsupportedFormat,
        DiagCode::MalformedSheetName,
        DiagCode::RecursionLimit,
    ];

    pub fn code_str(self) -> &'static str {
        match self {
            DiagCode::UnexpectedChar => "unexpected-char",
            DiagCode::UnterminatedString => "unterminated-string",
            DiagCode::InvalidNumber => "invalid-number",
            DiagCode::EmptyFormula => "empty-formula",
            DiagCode::UnexpectedToken => "unexpected-token",
            DiagCode::UnexpectedEof => "unexpected-eof",
            DiagCode::UnclosedParen => "unclosed-paren",
            DiagCode::UnbalancedParen => "unbalanced-paren",
            DiagCode::UnknownFunction => "unknown-function",
            DiagCode::BadArity => "bad-arity",
            DiagCode::MalformedArray => "malformed-array",
            DiagCode::ReservedUnion => "reserved-union",
            DiagCode::ReservedIntersection => "reserved-intersection",
            DiagCode::ReservedDynamicRange => "reserved-dynamic-range",
            DiagCode::ReservedName => "reserved-name",
            DiagCode::UnsupportedFunction => "unsupported-function",
            DiagCode::ReservedCrossSheet => "reserved-cross-sheet",
            DiagCode::UnsupportedFormat => "unsupported-format",
            DiagCode::MalformedSheetName => "malformed-sheet-name",
            DiagCode::RecursionLimit => "recursion-limit",
        }
    }

    /// The rule this code enforces, in one line — the single description of each code.
    pub fn summary(self) -> &'static str {
        match self {
            DiagCode::UnexpectedChar => "a byte that cannot begin any token",
            DiagCode::UnterminatedString => "a \"-string was not closed",
            DiagCode::InvalidNumber => "a numeric literal is not a finite number",
            DiagCode::EmptyFormula => "the formula is empty",
            DiagCode::UnexpectedToken => "a token appeared where a value was expected",
            DiagCode::UnexpectedEof => "input ended mid-expression",
            DiagCode::UnclosedParen => "a ( was never closed",
            DiagCode::UnbalancedParen => "a ) has no matching (",
            DiagCode::UnknownFunction => "the function name is not recognized",
            DiagCode::BadArity => "wrong number of arguments for this function",
            DiagCode::MalformedArray => {
                "an array literal is ragged or holds a non-constant element"
            }
            DiagCode::ReservedUnion => "the , union operator is reserved (not v1)",
            DiagCode::ReservedIntersection => {
                "the space intersection operator is reserved (not v1)"
            }
            DiagCode::ReservedDynamicRange => "a dynamic : range is reserved (not v1)",
            DiagCode::ReservedName => "a bare defined-name is reserved (not v1)",
            DiagCode::UnsupportedFunction => "the function is recognized but not implemented",
            DiagCode::ReservedCrossSheet => {
                "a 3D / multi-sheet range reference is reserved (not v1)"
            }
            DiagCode::UnsupportedFormat => "the TEXT format code is not in the supported v1 subset",
            DiagCode::MalformedSheetName => "a quoted sheet name is not well-formed",
            DiagCode::RecursionLimit => "the formula nests deeper than the parser's bound",
        }
    }

    pub fn severity(self) -> Severity {
        Severity::Error
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diag {
    pub code: DiagCode,
    pub span: Span,
    pub message: String,
}

impl Diag {
    pub fn new(code: DiagCode, span: Span, message: impl Into<String>) -> Diag {
        Diag {
            code,
            span,
            message: message.into(),
        }
    }
}

impl fmt::Display for Diag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "error[{}]: {} (at bytes {}..{})",
            self.code.code_str(),
            self.message,
            self.span.start,
            self.span.end,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_self_consistent() {
        let mut codes: Vec<&str> = DiagCode::ALL.iter().map(|c| c.code_str()).collect();
        let before = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(before, codes.len(), "code strings must be unique");
        for c in DiagCode::ALL {
            assert!(!c.summary().is_empty());
            let s = c.code_str();
            assert!(!s.is_empty() && !s.contains(' '), "kebab, no spaces: {s:?}");
            assert!(
                s.chars().all(|ch| ch.is_ascii_lowercase() || ch == '-'),
                "kebab-case only: {s:?}"
            );
            assert_eq!(c.severity(), Severity::Error);
        }
    }

    #[test]
    fn all_lists_every_variant_exactly_once() {
        // The exhaustive match is the drift guard: a new variant fails to compile until it is here.
        for c in DiagCode::ALL {
            let _: &str = match c {
                DiagCode::UnexpectedChar
                | DiagCode::UnterminatedString
                | DiagCode::InvalidNumber
                | DiagCode::EmptyFormula
                | DiagCode::UnexpectedToken
                | DiagCode::UnexpectedEof
                | DiagCode::UnclosedParen
                | DiagCode::UnbalancedParen
                | DiagCode::UnknownFunction
                | DiagCode::BadArity
                | DiagCode::MalformedArray
                | DiagCode::ReservedUnion
                | DiagCode::ReservedIntersection
                | DiagCode::ReservedDynamicRange
                | DiagCode::ReservedName
                | DiagCode::UnsupportedFunction
                | DiagCode::ReservedCrossSheet
                | DiagCode::UnsupportedFormat
                | DiagCode::MalformedSheetName
                | DiagCode::RecursionLimit => c.code_str(),
            };
        }
        assert_eq!(
            DiagCode::ALL.len(),
            20,
            "ALL must list every DiagCode variant exactly once"
        );
    }

    #[test]
    fn display_is_ascii_and_located() {
        let d = Diag::new(
            DiagCode::UnknownFunction,
            Span::new(1, 4),
            "no such function BAR",
        );
        let s = d.to_string();
        assert!(s.is_ascii());
        assert!(s.contains("error[unknown-function]"));
        assert!(s.contains("at bytes 1..4"));
    }
}
