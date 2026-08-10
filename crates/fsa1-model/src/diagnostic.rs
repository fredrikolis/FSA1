// Concern: the located-refusal type and the stable registry of refusal codes | Non-concern: detecting a refusal, printing the lint table | IO: (Code, Loc, message) -> Diagnostic

use fsa1_ast::ErrKind;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// The stable API a consumer switches on: a message's WORDING is free to change, its code is not.
/// [`Code::summary`] states each variant's rule and [`Code::help`] its remediation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    MalformedFilename,
    LowercaseColumn,
    LeadingZeroRow,
    DollarInFilename,
    NonCanonicalRange,
    DegenerateRange,
    WholeColumnRowReserved,
    RaggedGrid,
    DimensionMismatch,
    Overlap,
    Cycle,
    FormulaSyntax,
    MalformedEscape,
    RangeTooLarge,
    CellOutOfRange,
    NameRefusal,
    ForgeRefusal,
    PresentationInGrid,
    DuplicateSidecarRoot,
    SidecarScopeCrossing,
    TabLayerIndex,
    PresentationSyntax,
    PresentationSelector,
    PresentationProperty,
    NonCanonicalPresentation,
    XlsxNotCarried,
    FigureInRoot,
    FigureSyntax,
    FigureBinding,
    FigurePlacement,
    FigureSidecarClash,
    UnclaimedSidecar,
}

impl Code {
    pub const ALL: &'static [Code] = &[
        Code::MalformedFilename,
        Code::LowercaseColumn,
        Code::LeadingZeroRow,
        Code::DollarInFilename,
        Code::NonCanonicalRange,
        Code::DegenerateRange,
        Code::WholeColumnRowReserved,
        Code::RaggedGrid,
        Code::DimensionMismatch,
        Code::Overlap,
        Code::Cycle,
        Code::FormulaSyntax,
        Code::MalformedEscape,
        Code::RangeTooLarge,
        Code::CellOutOfRange,
        Code::NameRefusal,
        Code::ForgeRefusal,
        Code::PresentationInGrid,
        Code::DuplicateSidecarRoot,
        Code::SidecarScopeCrossing,
        Code::TabLayerIndex,
        Code::PresentationSyntax,
        Code::PresentationSelector,
        Code::PresentationProperty,
        Code::NonCanonicalPresentation,
        Code::XlsxNotCarried,
        Code::FigureInRoot,
        Code::FigureSyntax,
        Code::FigureBinding,
        Code::FigurePlacement,
        Code::FigureSidecarClash,
        Code::UnclaimedSidecar,
    ];

    pub fn code_str(self) -> &'static str {
        match self {
            Code::MalformedFilename => "malformed-filename",
            Code::LowercaseColumn => "lowercase-column",
            Code::LeadingZeroRow => "leading-zero-row",
            Code::DollarInFilename => "dollar-in-filename",
            Code::NonCanonicalRange => "non-canonical-range",
            Code::DegenerateRange => "degenerate-range",
            Code::WholeColumnRowReserved => "whole-column-row-reserved",
            Code::RaggedGrid => "ragged-grid",
            Code::DimensionMismatch => "dimension-mismatch",
            Code::Overlap => "overlap",
            Code::Cycle => "cycle",
            Code::FormulaSyntax => "formula-syntax",
            Code::MalformedEscape => "malformed-escape",
            Code::RangeTooLarge => "range-too-large",
            Code::CellOutOfRange => "cell-out-of-range",
            Code::NameRefusal => "name-refusal",
            Code::ForgeRefusal => "forge-refusal",
            Code::PresentationInGrid => "presentation-in-grid",
            Code::DuplicateSidecarRoot => "duplicate-sidecar-root",
            Code::SidecarScopeCrossing => "sidecar-scope-crossing",
            Code::TabLayerIndex => "tab-layer-index",
            Code::PresentationSyntax => "presentation-syntax",
            Code::PresentationSelector => "presentation-selector",
            Code::PresentationProperty => "presentation-property",
            Code::NonCanonicalPresentation => "non-canonical-presentation",
            Code::XlsxNotCarried => "xlsx-not-carried",
            Code::FigureInRoot => "figure-in-root",
            Code::FigureSyntax => "figure-syntax",
            Code::FigureBinding => "figure-binding",
            Code::FigurePlacement => "figure-placement",
            Code::FigureSidecarClash => "figure-sidecar-clash",
            Code::UnclaimedSidecar => "unclaimed-sidecar",
        }
    }

    /// The rule this code enforces.
    pub fn summary(self) -> &'static str {
        match self {
            Code::DuplicateSidecarRoot => "one root's presentation is stated by one sidecar",
            Code::SidecarScopeCrossing => {
                "a tab's sidecar roots are disjoint, or nest with the inner root a single cell"
            }
            Code::TabLayerIndex => {
                "a tab layer's rules reach every block, so over one they select the whole region or declare only a size"
            }
            Code::MalformedFilename => "filename is not a well-formed A1 closed range",
            Code::LowercaseColumn => "column letters must be uppercase",
            Code::LeadingZeroRow => "row numbers must not have a leading zero",
            Code::DollarInFilename => "$ is not allowed in a filename (bodies only)",
            Code::NonCanonicalRange => "a range must be written top-left:bottom-right",
            Code::DegenerateRange => "a single cell is the range A1, never a 1x1 A1:A1",
            Code::WholeColumnRowReserved => "whole-column/row ranges are not a closed range",
            Code::RaggedGrid => "a TSV grid's rows must have equal field counts",
            Code::DimensionMismatch => "the grid must fill the declared range exactly",
            Code::Overlap => "two files in a tab claim intersecting cells",
            Code::Cycle => "a formula cell must not depend on itself (directly or via a chain)",
            Code::FormulaSyntax => "a formula body must parse into a fsa1-ast expression",
            Code::MalformedEscape => {
                "a backslash in a TSV field must begin an escape: \\t, \\n, or \\\\"
            }
            Code::RangeTooLarge => "a referenced range must not exceed the materialization bound",
            Code::CellOutOfRange => "a traced tab index must be within the workbook's sheets",
            Code::NameRefusal => {
                "a name must be identified by an identifier, not an A1 address, with matched range corners"
            }
            Code::ForgeRefusal => {
                "a reference-forging call (INDIRECT/OFFSET) must resolve to a static reference: its arguments must not themselves forge, must not cycle back to it, and must name an on-grid target"
            }
            Code::PresentationInGrid => {
                "presentation lives in a range-named sidecar, never in a range file's grid"
            }
            Code::PresentationSyntax => {
                "a presentation sidecar holds `<selector> { <property>: <value>; ... }` rules and nothing else"
            }
            Code::PresentationSelector => {
                "a presentation selector must be one of the eight region-relative forms, each index within the region"
            }
            Code::PresentationProperty => {
                "an axis size is declared only on a selector that names the axis being sized"
            }
            Code::NonCanonicalPresentation => {
                "a presentation sidecar has one spelling per appearance"
            }
            Code::XlsxNotCarried => {
                "an .xlsx carries only the properties FSA1 models, so a declaration outside them cannot be packed"
            }
            Code::FigureInRoot => "a figure states one tab's ranges, so it lives in a tab folder",
            Code::FigureSyntax => "a figure's body must parse as JSON",
            Code::FigureBinding => {
                "a figure's data name must be an A1 reference this workbook resolves to a table its first row can key"
            }
            Code::FigurePlacement => {
                "a figure's placement sidecar holds one `figure` rule stating where on the sheet it sits"
            }
            Code::FigureSidecarClash => {
                "a figure named for a range states its own placement, so it takes no sidecar"
            }
            Code::UnclaimedSidecar => {
                "a sidecar naming no range places the figure of the same stem, so its tab must hold one"
            }
        }
    }

    /// What to change to clear this refusal, code-level and general; a fault's own specifics ride in
    /// its `message`.
    pub fn help(self) -> &'static str {
        match self {
            Code::DuplicateSidecarRoot => {
                "two sidecars name one root, so neither can be said to come later -- delete one, or merge its rules into the other"
            }
            Code::SidecarScopeCrossing => {
                "a scope is applied over ONE subtree, so give the inner root a single cell, shrink it to fall wholly outside the outer one, or state both regions in the one sidecar that covers them"
            }
            Code::TabLayerIndex => {
                "the layer's indices count in the TAB's content, which no block need cover: move the rule into a rooted \
                 `<range>.css` over the cells it means -- writing that sidecar if none covers them -- or drop the index \
                 so the rule is a bare `fsa1-cell` over the whole tab; a rule declaring only `width`/`height` may keep its index"
            }
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
                "use a closed rectangle naming both corners (e.g. `A1:A100`); a FILE NAME must be a closed range (FS2) — `A:A`/`3:3` are legal inside a formula, never as a file name"
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
            Code::MalformedEscape => {
                "write a literal backslash as `\\\\`, a tab as `\\t`, a newline as `\\n`; a backslash before anything else (or at the field's end) is malformed"
            }
            Code::RangeTooLarge => {
                "reference a smaller rectangle; the range exceeds the model's materialization bound"
            }
            Code::CellOutOfRange => {
                "trace a cell in an existing tab: pick a tab index within the workbook's sheet list"
            }
            Code::NameRefusal => {
                "rename the name entry so its identifier is not an A1 address; give a range name both a `.begin` and a `.end` corner on the same sheet with begin above-left of end"
            }
            Code::ForgeRefusal => {
                "rewrite the INDIRECT/OFFSET call so its target is static: avoid nesting a forging call inside another's arguments, avoid an argument that depends on the call's own cell, and keep the computed reference on the grid"
            }
            Code::PresentationInGrid => {
                "move the trailing `@scope { ... }` block's rules into a sidecar beside the range file, named `<range>.css` for the absolute A1 range they styled, and drop the `@scope { ... }` frame"
            }
            Code::PresentationSyntax => {
                "write each rule as `<selector> { <property>: <value>; <property>: <value> }`, separate declarations with `;` and never end on one, give each selector one rule, and drop `!important` and any at-rule"
            }
            Code::PresentationSelector => {
                "use one of `fsa1-cell`, `fsa1-row:first-child fsa1-cell`, `fsa1-row:last-child fsa1-cell`, \
                 `fsa1-row:nth-child(k) fsa1-cell`, `fsa1-cell:first-child`, `fsa1-cell:last-child`, `fsa1-cell:nth-child(k)`; \
                 indices are 1-based and region-relative, and no selector names ONE cell -- give that cell its own <cell>.css"
            }
            Code::PresentationProperty => {
                "move the size onto a selector that names its own axis: `width` onto `fsa1-cell` or `fsa1-cell:nth-child(k)`, \
                 `height` onto `fsa1-cell` or `fsa1-row:nth-child(k) fsa1-cell` -- a periodic selector names no one axis and carries no size. \
                 Every other property is declared on any selector"
            }
            Code::NonCanonicalPresentation => {
                "apply the rewrite the message names: index 1 is `:first-child` and the last index `:last-child`, an axis of extent 1 carries no selector of its own, and rules run all, then rows, then columns, then cells ascending"
            }
            Code::XlsxNotCarried => {
                "restate the declaration in a property the .xlsx carries -- a colour as `#rrggbb`, a size as `<n>pt`, a width as `<n>ch`, a border as `<width> <style> <colour>`, a keyword from its property's set -- or delete it; `check` and the page take it as written, Excel alone cannot"
            }
            Code::FigureInRoot => {
                "move the `<name>.json` into the tab folder whose ranges it binds; a figure names no coordinate at the workbook root"
            }
            Code::FigureSyntax => {
                "correct the file so it is one JSON object holding a Vega-Lite spec (balance braces and brackets, quote every key, drop trailing commas)"
            }
            Code::FigureBinding => {
                "write each `data.name` as an A1 reference -- one corner (`A1`) or two joined by `:` (`A1:D4`), optionally prefixed `<tab>!` -- naming a rectangle the workbook fills, whose FIRST ROW is a full set of distinct field names"
            }
            Code::FigurePlacement => {
                "write the file as one rule, `  figure { <property>: <value>; ... }` on one line with a closing newline, declaring `anchor`, `height`, `left`, `top`, `width` alphabetically and once each; \
                 `anchor` is required and is a cell (`D2`) or a range (`D2:K17`), and only a cell anchor takes the four lengths, each a non-negative `px`/`in`/`cm`/`pt`"
            }
            Code::FigureSidecarClash => {
                "delete the sidecar, or rename the figure to a NAME (`Chart1.json`) if its placement needs the sub-cell offsets only a sidecar states"
            }
            Code::UnclaimedSidecar => {
                "rename the sidecar to the figure it places -- `<figure>.css` beside `<figure>.json` in the same tab folder -- or delete it; a sidecar whose stem names no range and no figure states nothing"
            }
        }
    }

    /// `Some` exactly where the refusal surfaces as a cell VALUE rather than a structural fault, and
    /// the one place the model cites — never redefines — the AST error taxonomy.
    pub fn err_class(self) -> Option<ErrKind> {
        match self {
            Code::RaggedGrid => Some(ErrKind::Value),
            // Read by `fill_array_region`, so a refusal and the value it produces agree.
            Code::DimensionMismatch => Some(ErrKind::Spill),
            Code::Cycle => Some(ErrKind::Ref),
            Code::RangeTooLarge => Some(ErrKind::Num),
            Code::FormulaSyntax => Some(ErrKind::Name),
            Code::MalformedEscape => Some(ErrKind::Value),
            Code::ForgeRefusal => Some(ErrKind::Ref),
            _ => None,
        }
    }

    pub fn severity(self) -> Severity {
        Severity::Error
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteSpan {
    pub offset: usize,
    pub len: usize,
}

/// Whether a [`Fix`] may be applied unattended: `MaybeIncorrect` is a suggestion wanting review.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applicability {
    MachineApplicable,
    MaybeIncorrect,
}

/// Overwrite `span` bytes of the located name or file with `replacement`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fix {
    pub applicability: Applicability,
    pub span: ByteSpan,
    pub replacement: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Loc {
    File {
        name: String,
        span: Option<ByteSpan>,
    },
    /// 1-based, line 1 being the first grid row; a point anchor spells `end == start`.
    Body {
        file: String,
        line: u32,
        col: u32,
        end_line: u32,
        end_col: u32,
    },
    Tab {
        tab: String,
    },
    /// Sheet-qualified, so an eval-time refusal on an address that exists on two tabs is traceable.
    TabFile {
        tab: String,
        name: String,
    },
}

impl Loc {
    pub fn file(name: &str) -> Loc {
        Loc::File {
            name: name.to_string(),
            span: None,
        }
    }

    pub fn file_at(name: &str, offset: usize, len: usize) -> Loc {
        Loc::File {
            name: name.to_string(),
            span: Some(ByteSpan { offset, len }),
        }
    }

    pub fn body(file: &str, line: u32, col: u32) -> Loc {
        Loc::Body {
            file: file.to_string(),
            line,
            col,
            end_line: line,
            end_col: col,
        }
    }

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

    pub fn tab_file(tab: &str, name: &str) -> Loc {
        Loc::TabFile {
            tab: tab.to_string(),
            name: name.to_string(),
        }
    }
}

impl fmt::Display for Loc {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: Code,
    pub loc: Loc,
    pub message: String,
    /// `Some` only where the remediation is a known deterministic rewrite. Boxed so the rare, large
    /// fix does not bloat the common no-fix `Err` payload.
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
        assert_eq!(
            Code::ALL.len(),
            32,
            "every Code variant must be listed in ALL"
        );
        let mut codes: Vec<&str> = Code::ALL.iter().map(|c| c.code_str()).collect();
        codes.sort_unstable();
        let before = codes.len();
        codes.dedup();
        assert_eq!(before, codes.len(), "code strings must be unique");
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
        assert_eq!(Code::RangeTooLarge.err_class(), Some(ErrKind::Num));
        assert_eq!(Code::DimensionMismatch.err_class(), Some(ErrKind::Spill));
        assert_eq!(Code::MalformedFilename.err_class(), None);
        assert_eq!(Code::FormulaSyntax.err_class(), Some(ErrKind::Name));
        assert_eq!(Code::MalformedEscape.err_class(), Some(ErrKind::Value));
        assert_eq!(Code::ForgeRefusal.err_class(), Some(ErrKind::Ref));
    }

    #[test]
    fn tab_file_loc_is_sheet_qualified() {
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
