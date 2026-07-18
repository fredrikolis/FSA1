// Concern: the formula-engine LOCATED-REFUSAL registry — the stable `DiagCode` enum (one kebab code + severity + summary per parse/lex refusal), the byte-offset `Span` a refusal points at, and the located `Diag` value; a lex/parse hole is one of these BESIDE the (absent) tree, never a panic, an `Expr::Error` poison node, or a silent drop (ast-standards PART 5) | Non-concern: DETECTING a violation (lexer/parser raise these) and the eval-time error taxonomy (`value::ErrKind` owns first-class error VALUES; a refusal is a parse-time verdict, an `ErrKind` is a runtime value) | IO: none — (`DiagCode`, `Span`, message) -> a rendered ASCII refusal
//! Located refusals for the lexer/parser: [`DiagCode`] (the single-sourced registry), [`Span`],
//! [`Severity`], [`Diag`].
//!
//! A formula is a single parse *unit*: the lexer/parser return `Result<_, Diag>` and stop at the
//! first refusal (recovery-per-unit, where the unit is the whole formula — ast-standards PART 5).
//! The refusal stands *beside* the tree: on the error path there is no `Expr` at all, so the engine
//! never grows a poison `Expr::Error` node consumers must special-case. Eval, by contrast, never
//! produces a `Diag` — an evaluation failure is a first-class [`crate::ErrKind`] *value*.

use std::fmt;

/// A byte span into the formula string, `start..end` (half-open). Locations are byte offsets, never
/// line/col: a formula is one line, and `&formula[span.start..span.end]` lands on char boundaries by
/// construction (the lexer only cuts at ASCII boundaries or whole multi-byte chars). ast-standards
/// PART 3: provenance is a located span, kept out of any node's structural equality.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Span {
        Span { start, end }
    }

    /// A zero-width span at a single offset (for a refusal that points *between* bytes, e.g. an
    /// unexpected end-of-input).
    pub const fn at(offset: usize) -> Span {
        Span {
            start: offset,
            end: offset,
        }
    }
}

/// Severity of a refusal. Orthogonal to the verdict (ast-standards PART 5). Every refusal the v1
/// lexer/parser raises is an [`Severity::Error`] (it rejects the formula); [`Severity::Warning`] is
/// reserved for advisory diagnostics that will ride along on an *accepted* formula in a later phase
/// (e.g. a deprecation notice), and so is carried in the type but unused today.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    /// RESERVED — no v1 refusal uses this yet.
    Warning,
}

/// A stable refusal code — the API a consumer switches on. The *wording* of a `Diag`'s message is
/// deliberately not frozen; the code is (ast-standards PART 5, "single-sourced code registry:
/// stable switch key, flexible message"). Codes split three refusal *categories* that must never be
/// conflated: a malformed lexeme, a malformed structure, and a construct that is recognized but
/// **reserved/unimplemented** in scalar-only v1 (parse-and-preserve is impossible for these because
/// there is no node to carry them — cf. the `@`/`#` nodes, which *do* parse).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagCode {
    // --- lexical (a byte run is not a well-formed token) ---
    /// A byte that cannot begin any token (e.g. `\`, a bare `.`, a stray control char).
    UnexpectedChar,
    /// A `"`-string with no closing quote before end-of-input.
    UnterminatedString,
    /// A numeric lexeme that does not parse to a *finite* `f64` (e.g. `1e999`, `1.2.3`).
    InvalidNumber,

    // --- structural (tokens are individually fine, their arrangement is not) ---
    /// The formula was empty (or only `=` / whitespace) — nothing to parse.
    EmptyFormula,
    /// A token appeared where a value/operand was expected.
    UnexpectedToken,
    /// Input ended mid-expression (a dangling operator, an unclosed call).
    UnexpectedEof,
    /// A `(` was never closed by a matching `)`.
    UnclosedParen,
    /// A `)` with no matching `(`.
    UnbalancedParen,
    /// A function name that is not in the registry — recognized-but-unimplemented (ast-standards
    /// PART 5). Distinct from `#NAME?`: that is an eval-time *value*; this is a parse verdict,
    /// because an unknown name has no valid `FuncId` to build a `Call` node with.
    UnknownFunction,
    /// A known function called with a wrong argument count (checked at parse so eval can trust the
    /// arity — DbC: the parser is the one defended boundary).
    BadArity,
    /// A `{…}` array literal that is not well-formed: a RAGGED literal whose rows differ in width
    /// (`{1,2;3}`), a non-constant element (`{A1}`, `{SUM(1)}` — Excel array constants hold only
    /// numeric/text/logical/error constants), or an empty/dangling separator (`{}`, `{1,}`). An
    /// unterminated literal (`{1,2`) is instead an [`DiagCode::UnexpectedEof`] (input ended
    /// mid-construct). Distinct from the reserved codes: an array literal is a v1 construct that
    /// *parses* — this names a malformed one, not a deferred feature.
    MalformedArray,

    // --- reserved constructs (recognized, but no v1 node can carry them) ---
    /// The union operator `,` outside a function-argument list — reserved (scope.md).
    ReservedUnion,
    /// The intersection operator (whitespace between two references) — reserved (scope.md).
    ReservedIntersection,
    /// A dynamic `:` range whose endpoints are not both static cell references — reserved: a static
    /// range folds to [`crate::Expr::Range`], the dynamic form has no v1 node (scope.md).
    ReservedDynamicRange,
    /// A bare defined-name (an identifier that is neither a cell reference, `TRUE`/`FALSE`, nor a
    /// function call) — named ranges are reserved in v1 (scope.md).
    ReservedName,
    /// A 3D / multi-sheet range whose two endpoints name *different* sheets (`Sheet1!A1:Sheet2!B2`),
    /// or whose sheet qualifier sits on the wrong endpoint (`A1:Sheet2!B2`) — reserved in v1. A
    /// *single-sheet* cross-sheet reference (`Sheet1!A1`, `Sheet1!A1:B2`) now PARSES and resolves at
    /// eval via the [`crate::Resolver`] (a [`crate::RefNode`]/[`crate::RangeNode`] carries the parsed
    /// sheet name); only the multi-sheet form stays reserved.
    ReservedCrossSheet,
    /// A recognized-but-RESERVED reference-returning function (`INDIRECT`, `OFFSET`) — parsed as a
    /// call name, then refused at parse. These functions return a *reference* (not a value) and forge
    /// a DYNAMIC dependency edge the v1 scalar engine has no node for (scope.md items 5, and `OFFSET`
    /// deferred), so a call is refused up front with a located verdict on the name rather than a wrong
    /// value guess or the generic `unknown-function` path (the name IS recognized — it is reserved,
    /// not unknown). The refusal is emitted by the row's always-refuse `validate` seam, so it stays
    /// registry data, not a hand-fork in the parser.
    ReservedRefFunction,
    /// A `TEXT(value, format)` call whose format is a *literal* string naming no supported v1 code
    /// (the subset `func::text::classify_format` accepts). This is a PARSE verdict, not an eval-time value: a wrong-format guess is
    /// refused up front rather than silently mis-rendered, so the refusal is located and named rather
    /// than a `#VALUE!` guess. Vetting is only possible when the format is a literal; a NON-LITERAL
    /// (computed) format is ACCEPTED at parse and deferred to eval (accept-under-uncertainty — v1
    /// cannot statically vet a computed format, so it does not false-reject a call Excel would compute).
    UnsupportedFormat,

    // --- malformed reference syntax ---
    /// A `'`-quoted sheet name that is not well-formed: it was not closed before end-of-input, or its
    /// closing quote is not followed by the `!` a cross-sheet reference requires (`'My Sheet' + 1`).
    MalformedSheetName,

    // --- resource bound ---
    /// Nesting exceeded the parser's depth bound — a diagnostic, never a stack overflow
    /// (ast-standards PART 9, "bounded recursion").
    RecursionLimit,
}

impl DiagCode {
    /// Every code, once — the source of truth the self-consistency test walks.
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
        DiagCode::ReservedCrossSheet,
        DiagCode::ReservedRefFunction,
        DiagCode::UnsupportedFormat,
        DiagCode::MalformedSheetName,
        DiagCode::RecursionLimit,
    ];

    /// The stable kebab-case code string a consumer switches on and a `Diag` renders as
    /// `error[<code>]`.
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
            DiagCode::ReservedCrossSheet => "reserved-cross-sheet",
            DiagCode::ReservedRefFunction => "reserved-ref-function",
            DiagCode::UnsupportedFormat => "unsupported-format",
            DiagCode::MalformedSheetName => "malformed-sheet-name",
            DiagCode::RecursionLimit => "recursion-limit",
        }
    }

    /// A one-line summary of the rule this code enforces (docs/help; wording not frozen).
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
            DiagCode::ReservedCrossSheet => {
                "a 3D / multi-sheet range reference is reserved (not v1)"
            }
            DiagCode::ReservedRefFunction => {
                "a reference-returning function (INDIRECT/OFFSET) is reserved (not v1)"
            }
            DiagCode::UnsupportedFormat => "the TEXT format code is not in the supported v1 subset",
            DiagCode::MalformedSheetName => "a quoted sheet name is not well-formed",
            DiagCode::RecursionLimit => "the formula nests deeper than the parser's bound",
        }
    }

    /// Severity — every v1 refusal rejects.
    pub fn severity(self) -> Severity {
        Severity::Error
    }
}

/// A located refusal. Holds only well-formed data; it is never a panic and never a silent drop
/// (ast-standards PART 5). The `message` wording is free; the [`DiagCode`] is the stable API and
/// `span` locates it in the formula string.
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
        // Every variant appears in ALL, code strings are unique, kebab, non-empty summaries.
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
        // A drift guard: if a variant is added to the enum, this exhaustive match forces the author
        // to also add it to ALL (the match won't compile otherwise), and the count below updates.
        for c in DiagCode::ALL {
            // Exhaustive: adding a variant breaks compilation here until ALL & code_str cover it.
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
                | DiagCode::ReservedCrossSheet
                | DiagCode::ReservedRefFunction
                | DiagCode::UnsupportedFormat
                | DiagCode::MalformedSheetName
                | DiagCode::RecursionLimit => c.code_str(),
            };
        }
        assert_eq!(DiagCode::ALL.len(), 20);
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
