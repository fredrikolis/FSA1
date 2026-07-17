// Concern: the single-sourced DIAGNOSTIC registry — the stable `Code` enum (one code string + severity + summary + remediation help + spreadsheet-error class per refusal), the `Loc` a refusal points at (filename byte / body line-col / tab / sheet-qualified file), and the located `Diagnostic` value with an ASCII `Display`; every model refusal is one of these, never a panic or a silent drop | Non-concern: DETECTING any violation (the filename/grid/overlap modules AND the demand-driven eval engine in `workbook.rs` — which raises the cycle/depth-limit/range-too-large eval-time codes — raise these) and the formula-eval error taxonomy (charlie-ast's `ErrKind` owns that; a `Code` only cites the class it belongs to) | IO: (`Code`, `Loc`, message) -> a rendered ASCII refusal
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
    /// A filename that is not a well-formed A1 closed range (`A1`, `F2:F11`, `B2:D9`) (FS2).
    MalformedFilename,
    /// A lowercase column letter in a filename (`a1`) — non-canonical (FS2).
    LowercaseColumn,
    /// A leading zero in a filename row (`A01`) — non-canonical (FS2).
    LeadingZeroRow,
    /// A `$` in a filename (`$A$1`) — `$` lives in formula bodies only (FS2).
    DollarInFilename,
    /// A range not written top-left`:`bottom-right (`G8:A3`) — non-canonical (FS2).
    NonCanonicalRange,
    /// A `1x1` range spelled `A1:A1` — a single cell is the 0-D range `A1` (FS2).
    DegenerateRange,
    /// A whole-column / whole-row range (`A:A`, `3:3`) — not a closed range, reserved (FS2).
    WholeColumnRowReserved,
    /// Line 1 is not a `# `-prefixed annotation.
    MissingAnnotation,
    /// A TSV grid with unequal field counts per row — a `#VALUE!`-class refusal.
    RaggedGrid,
    /// A deserialized grid whose dimensions do not fill the file's declared closed range exactly
    /// (GRID4) — a located dimension error.
    DimensionMismatch,
    /// Two files in one tab claim intersecting cells (a hard reject; the overlap policy lives in
    /// [`crate::overlap`]).
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
        Code::RaggedGrid,
        Code::DimensionMismatch,
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
            Code::RaggedGrid => "ragged-grid",
            Code::DimensionMismatch => "dimension-mismatch",
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
            Code::MalformedFilename => "filename is not a well-formed A1 closed range",
            Code::LowercaseColumn => "column letters must be uppercase",
            Code::LeadingZeroRow => "row numbers must not have a leading zero",
            Code::DollarInFilename => "$ is not allowed in a filename (bodies only)",
            Code::NonCanonicalRange => "a range must be written top-left:bottom-right",
            Code::DegenerateRange => "a single cell is the range A1, never a 1x1 A1:A1",
            Code::WholeColumnRowReserved => "whole-column/row ranges are not a closed range",
            Code::MissingAnnotation => "line 1 must be a '# ' annotation",
            Code::RaggedGrid => "a TSV grid's rows must have equal field counts",
            Code::DimensionMismatch => "the grid must fill the declared range exactly",
            Code::Overlap => "two files in a tab claim intersecting cells",
            Code::Cycle => "a formula cell must not depend on itself (directly or via a chain)",
            Code::FormulaSyntax => "a formula body must parse into a charlie-ast expression",
            Code::DepthLimit => "a formula dependency chain must not exceed the pull-depth bound",
            Code::RangeTooLarge => "a referenced range must not exceed the materialization bound",
        }
    }

    /// Remediation prose — what to change to clear this refusal, as a structured field distinct from
    /// the located `message` (cli-interface-standards Part 2 "Diagnostics": `help`, so an agent reads
    /// the fix from a dedicated field instead of parsing it out of free-text). Code-level and general;
    /// the per-instance specifics (which address, which canonical spelling) ride in the `message`.
    pub fn help(self) -> &'static str {
        match self {
            Code::MalformedFilename => {
                "rename the file to a well-formed A1 closed range: a single cell `A1`, or a rectangle written top-left:bottom-right like `B2:D9`"
            }
            Code::LowercaseColumn => {
                "uppercase the column letter(s) in the filename (e.g. `a1` becomes `A1`)"
            }
            Code::LeadingZeroRow => {
                "drop the leading zero from the row number in the filename (e.g. `A01` becomes `A1`)"
            }
            Code::DollarInFilename => {
                "remove the `$` from the filename; `$` anchors live only inside formula bodies (e.g. `$A$1` becomes `A1`)"
            }
            Code::NonCanonicalRange => {
                "rewrite the range filename top-left:bottom-right, with the min column and row first (e.g. `G8:A3` becomes `A3:G8`)"
            }
            Code::DegenerateRange => {
                "a single cell is written as its bare address, never a 1x1 range: rename `A1:A1` to `A1`"
            }
            Code::WholeColumnRowReserved => {
                "use a closed rectangle naming both corners (e.g. `A1:A100`); whole-column/row spans (`A:A`, `3:3`) are reserved"
            }
            Code::MissingAnnotation => {
                "add a `# Concern: ... | Non-concern: ... | IO: ...` annotation as line 1 of the file body"
            }
            Code::RaggedGrid => {
                "give every TSV row the same number of tab-separated fields (pad short rows with empty fields)"
            }
            Code::DimensionMismatch => {
                "make the grid's rows x cols match the filename's declared range exactly (e.g. `B2:D9` needs 8 rows by 3 columns)"
            }
            Code::Overlap => {
                "move or resize one of the two files so their declared ranges no longer intersect within the tab"
            }
            Code::Cycle => {
                "break the dependency cycle: a formula cell must not, directly or through a chain, depend on itself"
            }
            Code::FormulaSyntax => {
                "correct the `=formula` so it parses (balance parentheses, fix operators and function names)"
            }
            Code::DepthLimit => {
                "shorten the formula dependency chain below the pull-depth bound (flatten intermediate cells)"
            }
            Code::RangeTooLarge => {
                "reference a smaller rectangle; the range exceeds the model's materialization bound"
            }
        }
    }

    /// The `ErrKind` class this refusal belongs to, where FORMAT.md names one — the ragged-block
    /// (`#VALUE!`) and non-conforming (`#SPILL!`) refusals. Purely structural refusals map to
    /// `None`. This is the one place the model *cites* (never redefines) the AST error taxonomy.
    pub fn err_class(self) -> Option<ErrKind> {
        match self {
            Code::RaggedGrid => Some(ErrKind::Value),
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

/// A byte span within a located file or filename: `len` bytes starting at byte `offset`. The
/// machine-exact half of a diagnostic location (cli-interface-standards Part 2 "Diagnostics": a
/// located finding carries a byte `span` `{offset,length}`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteSpan {
    pub offset: usize,
    pub len: usize,
}

/// Whether a [`Fix`] may be applied unattended (cli-interface-standards Part 2 "Diagnostics"): a
/// deterministic rewrite is [`Applicability::MachineApplicable`] (an agent may apply it); a heuristic
/// suggestion is [`Applicability::MaybeIncorrect`] (an agent should review first).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applicability {
    MachineApplicable,
    MaybeIncorrect,
}

/// A structured remediation edit: overwrite `span` bytes of the located name/file with `replacement`.
/// Present only when the fix is a KNOWN deterministic edit (a non-canonical filename's canonical
/// spelling); omitted for refusals with no single machine edit (cli-interface-standards Part 2:
/// `fix` "when known").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fix {
    pub applicability: Applicability,
    pub span: ByteSpan,
    pub replacement: String,
}

/// Where a diagnostic points. Heterogeneous by construction (ast-standards PART 3): a filename
/// issue anchors on the name (and an optional byte span), a body issue on a file start..end line/col,
/// an overlap on the tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Loc {
    /// Anchored on a filename, optionally at a byte span (offset + length) into it.
    File {
        name: String,
        span: Option<ByteSpan>,
    },
    /// Anchored on a file's body over a 1-based `line`:`col` start to an `end_line`:`end_col` end
    /// (line 1 is the annotation). A point anchor spells `end == start`.
    Body {
        file: String,
        line: u32,
        col: u32,
        end_line: u32,
        end_col: u32,
    },
    /// Anchored on a tab (folder) as a whole.
    Tab { tab: String },
    /// Anchored on a specific file *within a named tab* — a sheet-qualified cell/range file. Used by
    /// the eval-time refusals (cycle / depth-limit / range-too-large / non-conforming), where the
    /// same A1 address (a file named `A1`) can exist on more than one tab, so a bare filename cannot be
    /// traced back to the offending file.
    TabFile { tab: String, name: String },
}

impl Loc {
    pub fn file(name: &str) -> Loc {
        Loc::File {
            name: name.to_string(),
            span: None,
        }
    }

    /// A filename anchor spanning `len` bytes at byte `offset` — the offending token's extent in the
    /// name (the machine-exact `span` a fix's edit overwrites).
    pub fn file_at(name: &str, offset: usize, len: usize) -> Loc {
        Loc::File {
            name: name.to_string(),
            span: Some(ByteSpan { offset, len }),
        }
    }

    /// A point body anchor: the located extent is the single start position (`end == start`).
    pub fn body(file: &str, line: u32, col: u32) -> Loc {
        Loc::Body {
            file: file.to_string(),
            line,
            col,
            end_line: line,
            end_col: col,
        }
    }

    /// A spanned body anchor over a 1-based `start`..`end` line/column — the located token's extent
    /// (e.g. a formula sub-expression's `span.start`..`span.end`).
    pub fn body_span(file: &str, line: u32, col: u32, end_line: u32, end_col: u32) -> Loc {
        Loc::Body {
            file: file.to_string(),
            line,
            col,
            end_line,
            end_col,
        }
    }

    pub fn tab(tab: &str) -> Loc {
        Loc::Tab {
            tab: tab.to_string(),
        }
    }

    /// A sheet-qualified file anchor (`Beta/A1`) — the eval-time refusals' location, so a
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
                span: Some(s),
            } => write!(f, "{name} (byte {})", s.offset),
            Loc::File { name, span: None } => write!(f, "{name}"),
            Loc::Body {
                file, line, col, ..
            } => write!(f, "{file}:{line}:{col}"),
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
    /// A structured remediation edit, present only when the fix is a KNOWN deterministic rewrite
    /// (cli-interface-standards Part 2 "Diagnostics": `fix` "when known"); `None` otherwise. Boxed so
    /// the rare-and-large fix does not bloat the common no-fix [`Diagnostic`] on the `Err` path.
    pub fix: Option<Box<Fix>>,
}

impl Diagnostic {
    pub fn new(code: Code, loc: Loc, message: String) -> Diagnostic {
        Diagnostic {
            code,
            loc,
            message,
            fix: None,
        }
    }

    /// Attach a structured remediation edit (a machine-applicable canonical rewrite). Builder form so
    /// the common no-fix construction stays [`Diagnostic::new`].
    pub fn with_fix(mut self, fix: Fix) -> Diagnostic {
        self.fix = Some(Box::new(fix));
        self
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
        assert_eq!(Code::ALL.len(), 15);
        let mut codes: Vec<&str> = Code::ALL.iter().map(|c| c.code_str()).collect();
        codes.sort_unstable();
        let before = codes.len();
        codes.dedup();
        assert_eq!(before, codes.len(), "code strings must be unique");
        // Every code has a non-empty summary, non-empty remediation help, and a code string with no
        // spaces.
        for c in Code::ALL {
            assert!(!c.summary().is_empty());
            assert!(
                !c.help().is_empty(),
                "{} needs remediation help",
                c.code_str()
            );
            assert!(!c.code_str().contains(' '));
            assert_eq!(c.severity(), Severity::Error);
        }
    }

    #[test]
    fn err_classes_cite_the_ast_taxonomy() {
        assert_eq!(Code::RaggedGrid.err_class(), Some(ErrKind::Value));
        assert_eq!(Code::Cycle.err_class(), Some(ErrKind::Ref));
        assert_eq!(Code::DepthLimit.err_class(), Some(ErrKind::Num));
        assert_eq!(Code::RangeTooLarge.err_class(), Some(ErrKind::Num));
        assert_eq!(Code::MalformedFilename.err_class(), None);
        assert_eq!(Code::DimensionMismatch.err_class(), None);
        assert_eq!(Code::FormulaSyntax.err_class(), None);
    }

    #[test]
    fn tab_file_loc_is_sheet_qualified() {
        // The eval-time anchor spells the tab AND the file, so the same address on two tabs is
        // unambiguous (`Beta/A1`, not a bare `A1`).
        let d = Diagnostic::new(
            Code::Cycle,
            Loc::tab_file("Beta", "A1"),
            "circular reference".to_string(),
        );
        let s = d.to_string();
        assert!(s.is_ascii());
        assert!(s.contains("--> Beta/A1"), "{s}");
    }

    #[test]
    fn display_is_ascii_and_located() {
        let d = Diagnostic::new(
            Code::DollarInFilename,
            Loc::file_at("$A$1", 0, 4),
            "no $ in a filename".to_string(),
        );
        let s = d.to_string();
        assert!(s.is_ascii());
        assert!(s.contains("error[dollar-in-filename]"));
        assert!(s.contains("--> $A$1 (byte 0)"));
    }
}
