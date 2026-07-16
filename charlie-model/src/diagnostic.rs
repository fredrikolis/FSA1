// Concern: the single-sourced DIAGNOSTIC registry — the stable `Code` enum (one code string + severity + summary + spreadsheet-error class per refusal), the `Loc` a refusal points at (filename byte / body line-col / tab / sheet-qualified file), and the located `Diagnostic` value with an ASCII `Display`; every model refusal is one of these, never a panic or a silent drop | Non-concern: DETECTING any violation (the filename/body/conformance/overlap modules AND the demand-driven eval engine in `workbook.rs` — which raises the cycle/formula-syntax/depth-limit/range-too-large eval-time codes — raise these) and the formula-eval error taxonomy (charlie-ast's `ErrKind` owns that; a `Code` only cites the class it belongs to) | IO: (`Code`, `Loc`, message) -> a rendered ASCII refusal
//! Diagnostics: [`Code`] (the single-sourced registry), [`Loc`], [`Severity`], [`Diagnostic`].

use charlie_ast::ErrKind;
use std::fmt;

/// Severity of a diagnostic. Orthogonal to the verdict (ast-standards PART 5); every W2 refusal is
/// an [`Severity::Error`] (rejects). `Warning` is reserved for advisory diagnostics that ride along
/// on accepted input in a later phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    /// RESERVED — no W2 diagnostic uses this yet.
    Warning,
}

/// A stable diagnostic code — the API a consumer switches on. The *wording* of a message is not
/// frozen; the code is (ast-standards PART 5, "single-sourced code registry").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    /// A filename that is not a well-formed `<addr>.cell` / `<addr>:<addr>.range` (FORMAT §2).
    MalformedFilename,
    /// A lowercase column letter in a filename (`a1.cell`) — non-canonical (FORMAT §2/§11).
    LowercaseColumn,
    /// A leading zero in a filename row (`A01.cell`) — non-canonical (FORMAT §2/§11).
    LeadingZeroRow,
    /// A `$` in a filename (`$A$1.cell`) — `$` lives in formula bodies only (FORMAT §2/§11).
    DollarInFilename,
    /// A range not written top-left`:`bottom-right (`G8:A3.range`) — non-canonical (FORMAT §2/§11).
    NonCanonicalRange,
    /// A 1x1 range (`A1:A1.range`) — a single cell must be `.cell` (FORMAT §2/§11).
    DegenerateRange,
    /// A whole-column / whole-row range (`A:A`, `3:3`) — reserved, not v1 (FORMAT §2).
    WholeColumnRowReserved,
    /// Line 1 is not a `# `-prefixed annotation (FORMAT §8/§11).
    MissingAnnotation,
    /// A body that is both a literal line and a formula, or more than one formula (FORMAT §4/§11).
    DualBody,
    /// A literal block with unequal field counts per line (FORMAT §5) — a `#VALUE!`-class refusal.
    RaggedBlock,
    /// A body shape that neither matches nor broadcasts to the declared shape (FORMAT §6) — a
    /// `#SPILL!`-class refusal.
    NonConforming,
    /// Two files in one tab claim intersecting cells (FORMAT §7).
    Overlap,
    /// A `=formula` cell depends on itself, directly or through a chain (demand-driven eval, B3) —
    /// a `#REF!`-class refusal. The evaluator refuses the cycle instead of hanging / overflowing.
    Cycle,
    /// A `=formula` body that charlie-ast cannot parse into an expression (demand-driven eval, B3).
    /// A located refusal, never a silent drop: the cell resolves to `#NAME?`.
    FormulaSyntax,
    /// A `=formula` cross-cell dependency CHAIN that is finite but deeper than the model's pull-depth
    /// bound (demand-driven eval, B3) — a `#NUM!`-class refusal. Distinct from [`Code::Cycle`]: the
    /// chain terminates, it is merely too long to evaluate by native recursion, so the deepest link
    /// is refused (never a stack overflow) rather than looping forever.
    DepthLimit,
    /// A `=formula` references a rectangular range whose cell-count exceeds the model's
    /// materialization bound (demand-driven eval, B3) — a `#NUM!`-class refusal. Bounds a reference
    /// to a syntactically-valid but pathologically-large range (`A2:ZZ100000`) so it refuses with a
    /// located diagnostic rather than materializing every cell into an OOM abort.
    RangeTooLarge,
}

impl Code {
    /// Every code, once — the source of truth the self-consistency test walks.
    pub const ALL: &'static [Code] = &[
        Code::MalformedFilename,
        Code::LowercaseColumn,
        Code::LeadingZeroRow,
        Code::DollarInFilename,
        Code::NonCanonicalRange,
        Code::DegenerateRange,
        Code::WholeColumnRowReserved,
        Code::MissingAnnotation,
        Code::DualBody,
        Code::RaggedBlock,
        Code::NonConforming,
        Code::Overlap,
        Code::Cycle,
        Code::FormulaSyntax,
        Code::DepthLimit,
        Code::RangeTooLarge,
    ];

    /// The stable kebab-case code string a consumer switches on and a diagnostic renders as
    /// `error[<code>]`.
    pub fn code_str(self) -> &'static str {
        match self {
            Code::MalformedFilename => "malformed-filename",
            Code::LowercaseColumn => "lowercase-column",
            Code::LeadingZeroRow => "leading-zero-row",
            Code::DollarInFilename => "dollar-in-filename",
            Code::NonCanonicalRange => "non-canonical-range",
            Code::DegenerateRange => "degenerate-range",
            Code::WholeColumnRowReserved => "whole-column-row-reserved",
            Code::MissingAnnotation => "missing-annotation",
            Code::DualBody => "dual-body",
            Code::RaggedBlock => "ragged-block",
            Code::NonConforming => "non-conforming",
            Code::Overlap => "overlap",
            Code::Cycle => "cycle",
            Code::FormulaSyntax => "formula-syntax",
            Code::DepthLimit => "depth-limit",
            Code::RangeTooLarge => "range-too-large",
        }
    }

    /// A one-line summary of the rule this code enforces (docs/help; wording not frozen).
    pub fn summary(self) -> &'static str {
        match self {
            Code::MalformedFilename => "filename is not a well-formed .cell/.range address",
            Code::LowercaseColumn => "column letters must be uppercase",
            Code::LeadingZeroRow => "row numbers must not have a leading zero",
            Code::DollarInFilename => "$ is not allowed in a filename (bodies only)",
            Code::NonCanonicalRange => "a range must be written top-left:bottom-right",
            Code::DegenerateRange => "a single cell must be .cell, never a 1x1 .range",
            Code::WholeColumnRowReserved => "whole-column/row ranges are reserved, not v1",
            Code::MissingAnnotation => "line 1 must be a '# ' annotation",
            Code::DualBody => "a body is exactly one of a literal block or one =formula",
            Code::RaggedBlock => "a literal block's rows must have equal field counts",
            Code::NonConforming => "body shape must match or broadcast to the declared shape",
            Code::Overlap => "two files in a tab claim intersecting cells",
            Code::Cycle => "a formula cell must not depend on itself (directly or via a chain)",
            Code::FormulaSyntax => "a formula body must parse into a charlie-ast expression",
            Code::DepthLimit => "a formula dependency chain must not exceed the pull-depth bound",
            Code::RangeTooLarge => "a referenced range must not exceed the materialization bound",
        }
    }

    /// The `ErrKind` class this refusal belongs to, where FORMAT.md names one — the ragged-block
    /// (`#VALUE!`) and non-conforming (`#SPILL!`) refusals. Purely structural refusals map to
    /// `None`. This is the one place the model *cites* (never redefines) the AST error taxonomy.
    pub fn err_class(self) -> Option<ErrKind> {
        match self {
            Code::RaggedBlock => Some(ErrKind::Value),
            Code::NonConforming => Some(ErrKind::Spill),
            Code::Cycle => Some(ErrKind::Ref),
            Code::DepthLimit => Some(ErrKind::Num),
            Code::RangeTooLarge => Some(ErrKind::Num),
            _ => None,
        }
    }

    /// Severity — every W2 refusal rejects.
    pub fn severity(self) -> Severity {
        Severity::Error
    }
}

/// Where a diagnostic points. Heterogeneous by construction (ast-standards PART 3): a filename
/// issue anchors on the name (and an optional byte offset), a body issue on a file line/col, an
/// overlap on the tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Loc {
    /// Anchored on a filename, optionally at a byte offset into it.
    File { name: String, byte: Option<usize> },
    /// Anchored on a file's body at a 1-based line/column (line 1 is the annotation).
    Body { file: String, line: u32, col: u32 },
    /// Anchored on a tab (folder) as a whole.
    Tab { tab: String },
    /// Anchored on a specific file *within a named tab* — a sheet-qualified cell/range file. Used by
    /// the eval-time refusals (cycle / depth-limit / range-too-large / non-conforming), where the
    /// same A1 address (`A1.cell`) can exist on more than one tab, so a bare filename cannot be
    /// traced back to the offending file.
    TabFile { tab: String, name: String },
}

impl Loc {
    pub fn file(name: &str) -> Loc {
        Loc::File {
            name: name.to_string(),
            byte: None,
        }
    }

    pub fn file_at(name: &str, byte: usize) -> Loc {
        Loc::File {
            name: name.to_string(),
            byte: Some(byte),
        }
    }

    pub fn body(file: &str, line: u32, col: u32) -> Loc {
        Loc::Body {
            file: file.to_string(),
            line,
            col,
        }
    }

    pub fn tab(tab: &str) -> Loc {
        Loc::Tab {
            tab: tab.to_string(),
        }
    }

    /// A sheet-qualified file anchor (`Beta/A1.cell`) — the eval-time refusals' location, so a
    /// diagnostic can be traced to the offending file even when the same address exists on two tabs.
    pub fn tab_file(tab: &str, name: &str) -> Loc {
        Loc::TabFile {
            tab: tab.to_string(),
            name: name.to_string(),
        }
    }
}

impl fmt::Display for Loc {
    /// The located pointer, single-sourced here so both the [`Diagnostic`] renderer and the CLI's
    /// lint table spell a location the same way.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Loc::File {
                name,
                byte: Some(b),
            } => write!(f, "{name} (byte {b})"),
            Loc::File { name, byte: None } => write!(f, "{name}"),
            Loc::Body { file, line, col } => write!(f, "{file}:{line}:{col}"),
            Loc::Tab { tab } => write!(f, "tab {tab:?}"),
            Loc::TabFile { tab, name } => write!(f, "{tab}/{name}"),
        }
    }
}

/// A located refusal. Holds only well-formed data; it is never a panic and never a silent drop
/// (ast-standards PART 5). The `message` wording is free; the [`Code`] is the stable API.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: Code,
    pub loc: Loc,
    pub message: String,
}

impl Diagnostic {
    pub fn new(code: Code, loc: Loc, message: String) -> Diagnostic {
        Diagnostic { code, loc, message }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error[{}]: {}", self.code.code_str(), self.message)?;
        write!(f, "\n  --> {}", self.loc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_self_consistent() {
        // Every variant appears in ALL exactly once, and code strings are unique.
        assert_eq!(Code::ALL.len(), 16);
        let mut codes: Vec<&str> = Code::ALL.iter().map(|c| c.code_str()).collect();
        codes.sort_unstable();
        let before = codes.len();
        codes.dedup();
        assert_eq!(before, codes.len(), "code strings must be unique");
        // Every code has a non-empty summary and a code string with no spaces.
        for c in Code::ALL {
            assert!(!c.summary().is_empty());
            assert!(!c.code_str().contains(' '));
            assert_eq!(c.severity(), Severity::Error);
        }
    }

    #[test]
    fn err_classes_cite_the_ast_taxonomy() {
        assert_eq!(Code::RaggedBlock.err_class(), Some(ErrKind::Value));
        assert_eq!(Code::NonConforming.err_class(), Some(ErrKind::Spill));
        assert_eq!(Code::Cycle.err_class(), Some(ErrKind::Ref));
        assert_eq!(Code::DepthLimit.err_class(), Some(ErrKind::Num));
        assert_eq!(Code::RangeTooLarge.err_class(), Some(ErrKind::Num));
        assert_eq!(Code::MalformedFilename.err_class(), None);
        assert_eq!(Code::FormulaSyntax.err_class(), None);
    }

    #[test]
    fn tab_file_loc_is_sheet_qualified() {
        // The eval-time anchor spells the tab AND the file, so the same address on two tabs is
        // unambiguous (`Beta/A1.cell`, not a bare `A1.cell`).
        let d = Diagnostic::new(
            Code::Cycle,
            Loc::tab_file("Beta", "A1.cell"),
            "circular reference".to_string(),
        );
        let s = d.to_string();
        assert!(s.is_ascii());
        assert!(s.contains("--> Beta/A1.cell"), "{s}");
    }

    #[test]
    fn display_is_ascii_and_located() {
        let d = Diagnostic::new(
            Code::DollarInFilename,
            Loc::file_at("$A$1.cell", 0),
            "no $ in a filename".to_string(),
        );
        let s = d.to_string();
        assert!(s.is_ascii());
        assert!(s.contains("error[dollar-in-filename]"));
        assert!(s.contains("--> $A$1.cell (byte 0)"));
    }
}
