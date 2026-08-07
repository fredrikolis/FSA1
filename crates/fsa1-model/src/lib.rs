// Concern: the crate's public entry points | Non-concern: formula semantics, xlsx serde | IO: (a name and contents, or an alias target) -> the parsed file, or the spelling this host stores
//! The filesystem spreadsheet model: a tab is a folder, a file's name is the closed A1 range its
//! content fills, and that content deserializes (TSV) to a [`Grid`] of literals and `=formula` cells.
//! [`Workbook`] resolves those grids for `fsa1_ast::eval`; there is no drag-fill anywhere, and
//! every fault is a located [`Diagnostic`] rather than a panic or a silent drop.

pub mod declaration;
pub mod diagnostic;
pub mod filename;
pub mod format;
pub mod geometry;
pub mod grid;
pub mod names;
pub mod overlap;
pub mod overlay;
pub mod presentation;
pub mod render;
pub mod sample;
pub mod scope;
pub mod style;
pub mod view;
pub mod workbook;

pub use declaration::{
    Border, BorderLine, Chars, Declaration, Edge, FontStyle, FontWeight, Points, Rgb, TextAlign,
    TextDecoration, VerticalAlign, WhiteSpace,
};
pub use diagnostic::{Applicability, ByteSpan, Code, Diagnostic, Fix, Loc, Severity};
pub use filename::{
    FileName, RANGE_SEP, RANGE_SEP_POSIX, RANGE_SEP_WINDOWS, parse_filename, reseparate_range_name,
};
pub use format::{
    CUSTOM_NUMFMT_ID, CurrencySymbol, DatePattern, DateTimePattern, Format, TimePattern,
};
pub use geometry::{AxisRun, declared_heights, declared_widths};
pub use grid::{
    Cell, Grid, deserialize_tsv, encode_field, lex_literal, load_error_value, split_format_marker,
};
pub use names::{
    Name, NameRepr, NameScope, NameTable, NameTarget, PRESENTATION_SUFFIX, RawNameEntry,
    is_cell_filename, is_presentation_entry, is_tab_layer, presentation_stem, quote_sheet,
};
pub use overlap::{Rect, detect_overlaps};
pub use overlay::Overlay;
pub use presentation::{Presentation, Rule, Target, parse_rules, spell_rules};
pub use render::{
    MAX_VIEWPORT_CELLS, RenderGrid, RenderMode, RenderRow, combined_cell, display_value,
    parse_viewport, render, viewport_cell_count,
};
pub use sample::sample_workbook;
pub use scope::Scope;
pub use style::{BlankPaint, CellStyle, DEFAULT_FONT_FAMILY, DEFAULT_FONT_SIZE, default_style};
pub use view::{NameView, SheetView, View, ViewScope, view};
pub use workbook::{
    CellSource, Direction, FileEntry, FormulaOutcome, TraceNode, TraceStatus, Workbook,
};

use fsa1_ast::Shape;

/// One loaded file: the closed range its name declares, and the grid its content deserialized to.
/// `array_formula` marks the second legal form — a lone `1x1` `=formula` grid under a multi-coordinate
/// range, evaluated once and spread across the region.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedFile {
    pub region: Rect,
    pub declared_shape: Shape,
    pub grid: Grid,
    pub array_formula: bool,
}

/// The `.gitattributes` a workbook carries so git cannot corrupt it. Under Windows' default
/// `core.autocrlf=true` a checkout rewrites cell files to CRLF, and `deserialize_tsv` strips only a
/// FILE-FINAL CRLF — every other row would keep a stray `\r` as cell text. Pinning the tree to LF
/// prevents that; `.gitattributes` is a reserved entry, so the loader ignores the file itself.
pub const WORKBOOK_GITATTRIBUTES: &str = "\
# FSA1 workbook - a filesystem spreadsheet, meant to live in git.
# A cell file is LF-delimited UTF-8 text; its NAME is the cell/range address. Never let git rewrite
# line endings: a CRLF checkout would append a stray carriage return to every grid row.
* text eol=lf
";

/// Write [`WORKBOOK_GITATTRIBUTES`] into `root/.gitattributes`, never clobbering a user's own.
pub fn write_workbook_gitattributes(root: &std::path::Path) -> std::io::Result<()> {
    let path = root.join(".gitattributes");
    if path.exists() {
        return Ok(());
    }
    std::fs::write(path, WORKBOOK_GITATTRIBUTES)
}

/// The name a range-addressed entry carries ON THIS HOST, given its canonical `:` spelling. One
/// call, so a caller that has to name a file never has to know which separator this host writes.
pub fn range_file_name(canonical: &str) -> String {
    respell_range(canonical, RANGE_SEP)
}

/// The canonical `:` spelling of a range-addressed entry's name, whatever this host wrote. The
/// inverse of [`range_file_name`], so a reader comparing names never has to know which separator
/// made them.
pub fn canonical_range_name(name: &str) -> String {
    respell_range(name, RANGE_SEP_POSIX)
}

/// The name a tab ENTRY takes where a range is spelled with `to`. A sidecar is addressed by its STEM,
/// so this respells that and re-suffixes — one home for the host's separator across both entry kinds
/// a range can name, so a convert cannot respell half a tab. `None` for a name addressing no range.
pub fn reseparate_entry_name(name: &str, to: char) -> Option<String> {
    let (addressed, suffix) = match presentation_stem(name) {
        Some(stem) => (stem, PRESENTATION_SUFFIX),
        None => (name, ""),
    };
    Some(format!("{}{suffix}", reseparate_range_name(addressed, to)?))
}

fn respell_range(name: &str, to: char) -> String {
    reseparate_entry_name(name, to).unwrap_or_else(|| name.to_string())
}

/// [`range_file_name`] applied to the last component of a path, which is where a name becomes a
/// file. A path with no final component, or one no host respells, is returned unchanged.
pub fn range_file_path(rel: &std::path::Path) -> std::path::PathBuf {
    match rel.file_name().and_then(|n| n.to_str()) {
        Some(name) => rel.with_file_name(range_file_name(name)),
        None => rel.to_path_buf(),
    }
}

/// Write the alias a defined name is stored as. This is the ONE place the host has a say: a symlink
/// where the platform has them, and where it does not, a file holding the same relative target as
/// its bare text -- which is what a symlink-flattening checkout leaves behind, and what the loader's
/// degraded-path reader was built for. The loader resolves both forms identically.
#[cfg(unix)]
pub fn write_name_alias(target: &str, link: &std::path::Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(link).is_ok() {
        let _ = std::fs::remove_file(link);
    }
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
pub fn write_name_alias(target: &str, link: &std::path::Path) -> std::io::Result<()> {
    // BARE, no `=`: a leading `=` says "formula" to the reader, and this is a path.
    std::fs::write(link, target)
}

/// The file content is its grid, whole — no header, annotation, or metadata line, the first line the
/// first row, and no trailing block: presentation lives in a sidecar named for the range it styles,
/// so a grid that still ends in one is a located refusal rather than a shorter grid.
pub fn parse_file(name: &str, contents: &str) -> Result<ParsedFile, Vec<Diagnostic>> {
    let declared = parse_filename(name).map_err(|d| vec![d])?;
    let declared_shape = declared.declared_shape;
    if let Some(line) = trailing_block_line(contents) {
        return Err(vec![presentation_in_grid(Loc::body(name, line, 1))]);
    }

    let grid = deserialize_tsv(name, contents).map_err(|d| vec![d])?;
    let Some(array_formula) = conforms(&grid, declared_shape) else {
        return Err(vec![dimension_mismatch(name, &grid, declared_shape)]);
    };
    Ok(ParsedFile {
        region: declared.region,
        declared_shape,
        grid,
        array_formula,
    })
}

const OPEN: &str = "@scope {";
const CLOSE: &str = "}";

/// The 1-based line a RETIRED in-grid presentation block opens on, or `None` where the file is all
/// grid. Found from the END — the last non-empty line must be `}`, brace-matched backwards to a line
/// that is exactly `@scope {` — so a CELL whose text is `@scope {` never reads as one.
fn trailing_block_line(content: &str) -> Option<u32> {
    let mut lines: Vec<&str> = Vec::new();
    for line in content.split('\n') {
        lines.push(line);
    }
    let close = lines.iter().rposition(|l| !l.is_empty())?;
    if lines[close] != CLOSE {
        return None;
    }
    match_open(&lines, close).map(|open| (open + 1) as u32)
}

/// Returns the line whose `{` closes the outermost brace, and ONLY when that line is exactly
/// `@scope {` — a grid holding stray braces therefore matches nothing and stays whole.
fn match_open(lines: &[&str], close: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (idx, line) in lines[..=close].iter().enumerate().rev() {
        for ch in line.chars().rev() {
            match ch {
                '}' => depth += 1,
                '{' => {
                    depth -= 1;
                    if depth == 0 {
                        return (*line == OPEN).then_some(idx);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// The wording a retired in-grid block earns, naming the sidecar an author should move it to.
pub fn presentation_in_grid(loc: Loc) -> Diagnostic {
    Diagnostic::new(
        Code::PresentationInGrid,
        loc,
        format!(
            "presentation is a `<range>{PRESENTATION_SUFFIX}` sidecar, never a block in a grid: \
             move the rules into a file named for the absolute range they styled, dropping the \
             `@scope {{ ... }}` frame"
        ),
    )
}

/// The two shapes a grid may take under a declared range: it fills the range exactly, or it is the
/// lone `=formula` whose array value does. `Some` carries whether it is the latter.
fn conforms(grid: &Grid, declared: Shape) -> Option<bool> {
    if grid.shape == declared {
        return Some(false);
    }
    let spans_multiple = (declared.rows as u64) * (declared.cols as u64) > 1;
    let is_single_formula = grid.shape == (Shape { rows: 1, cols: 1 })
        && matches!(grid.cells.first(), Some(Cell::Formula { .. }));
    (spans_multiple && is_single_formula).then_some(true)
}

fn dimension_mismatch(name: &str, grid: &Grid, declared: Shape) -> Diagnostic {
    Diagnostic::new(
        Code::DimensionMismatch,
        Loc::file(name),
        format!(
            "the grid is {}x{} but the file's range {name:?} declares {}x{}: the grid must \
             fill the closed range exactly (GRID4), or be a single =formula whose array value \
             fills the range (GRID5)",
            grid.shape.rows, grid.shape.cols, declared.rows, declared.cols,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use fsa1_ast::{ErrKind, Value};
    use grid::Cell;

    #[test]
    fn loads_a_header_row_exact_match() {
        let f = parse_file("A1:D1", "Product\tUnit Price\tQty\tLine Total").unwrap();
        assert_eq!(f.declared_shape, Shape { rows: 1, cols: 4 });
        assert_eq!(f.grid.shape, Shape { rows: 1, cols: 4 });
        assert_eq!(
            f.grid.cells[0],
            Cell::Value {
                value: Value::Text("Product".to_string()),
                format: None
            }
        );
    }

    #[test]
    fn loads_a_single_formula_cell() {
        let f = parse_file("A1", "=B2*C2").unwrap();
        assert!(matches!(&f.grid.cells[0], Cell::Formula { src, .. } if src == "=B2*C2"));
    }

    #[test]
    fn loads_an_explicit_mixed_grid() {
        let f = parse_file("B2:D4", "1\t2\t3\n=A1\t=B1\t=C1\n7\t8\t9").unwrap();
        assert_eq!(f.grid.shape, Shape { rows: 3, cols: 3 });
        assert_eq!(
            f.grid.cells[0],
            Cell::Value {
                value: Value::Number(1.0),
                format: None
            }
        );
        assert!(matches!(&f.grid.cells[3], Cell::Formula { src, .. } if src == "=A1"));
    }

    #[test]
    fn loads_a_blank_cell() {
        let f = parse_file("A1", "").unwrap();
        assert_eq!(f.grid.shape, Shape { rows: 1, cols: 1 });
        assert_eq!(
            f.grid.cells,
            vec![Cell::Value {
                value: Value::Blank,
                format: None
            }]
        );
    }

    #[test]
    fn a_single_cell_ref_error_reads_as_the_error_literal() {
        let f = parse_file("A1", "#REF!").unwrap();
        assert_eq!(f.grid.shape, Shape { rows: 1, cols: 1 });
        assert_eq!(
            f.grid.cells,
            vec![Cell::Value {
                value: Value::Error(ErrKind::Ref),
                format: None
            }]
        );
    }

    #[test]
    fn a_grid_that_does_not_fill_the_range_is_a_dimension_error() {
        let d = parse_file("B2:D4", "1\t2\n3\t4").unwrap_err();
        assert_eq!(d[0].code, Code::DimensionMismatch);
    }

    #[test]
    fn a_bad_filename_is_rejected_before_the_body() {
        let d = parse_file("g8:a3", "1\t2").unwrap_err();
        assert_eq!(d[0].code, Code::LowercaseColumn);
    }

    /// The retired form, pinned here rather than as a fixture: a range file is its grid whole, and a
    /// trailing block is ONE refusal naming where presentation now lives — never a shorter grid, and
    /// never a second fault about the rows that block's lines are no longer counted as.
    #[test]
    fn a_range_file_ending_in_a_block_is_one_refusal_naming_the_sidecar() {
        for content in [
            "1\t2\t3\n@scope {\n  td { text-align: right }\n}",
            "1\t2\t3\n@scope {\n}",
            "1\t2\t3\n@scope {\n  th { color: red }\n}",
        ] {
            let d = parse_file("A1:C1", content).unwrap_err();
            assert_eq!(d.len(), 1, "{content:?} -> {d:?}");
            assert_eq!(d[0].code, Code::PresentationInGrid, "{content:?}");
            assert!(
                matches!(d[0].loc, Loc::Body { line: 2, .. }),
                "{content:?}: {:?}",
                d[0].loc
            );
            assert!(
                d[0].message.contains(PRESENTATION_SUFFIX),
                "{content:?} -> {}",
                d[0].message
            );
        }
    }

    /// The detector is anchored to the file's LAST line, so an interior `@scope {` is cell text and a
    /// grid holding one still loads. A false positive here would turn a legal grid into a refusal.
    #[test]
    fn a_text_cell_spelling_the_block_open_stays_a_cell() {
        let f = parse_file("A1:A3", "1\n@scope {\n3").unwrap();
        assert_eq!(f.grid.shape, Shape { rows: 3, cols: 1 });
        assert_eq!(
            f.grid.cells[1],
            Cell::Value {
                value: Value::Text("@scope {".to_string()),
                format: None
            }
        );
    }

    /// The whole file is the grid, with nothing split off it: the row count a mismatch reports is the
    /// one an author can count in the file.
    #[test]
    fn a_dimension_mismatch_counts_every_line_of_the_file() {
        let d = parse_file("A1:A4", "1\n2\n3").unwrap_err();
        assert_eq!(d[0].code, Code::DimensionMismatch);
        assert!(d[0].message.contains("3x1"), "{}", d[0].message);
    }
    /// The detector's whole job now: say WHICH line a retired in-grid block opens on, so the refusal
    /// that replaces it points at one. Every case a grid could be mistaken for a block is still here,
    /// because a false positive would turn a legal grid into a refusal.
    #[test]
    fn a_grid_that_merely_looks_like_a_block_opens_none() {
        for content in [
            "Rent\t1500\n@scope {\tx\ty\nSalaries\t1600",
            // Anchored to the file's LAST line, so an interior `@scope {` cell is inert.
            "@scope {\n1\n2",
            "a\nx { y\n}",
            "a\n}",
            "{\n}",
            "a\n}\n}",
            // An unbalanced tail matches nothing, so the file is judged as the grid it is.
            "@scope {\n  td color: #3f0421 }\n}",
            "@scope {\n  td { color: #3f0421\n}",
        ] {
            assert_eq!(trailing_block_line(content), None, "{content:?}");
        }
    }
    #[test]
    fn a_retired_in_grid_block_is_found_at_the_line_it_opens_on() {
        assert_eq!(
            trailing_block_line("1\t2\n3\t4\n@scope {\n  td { color: #3f0421 }\n}"),
            Some(3),
        );
        // Blank lines after the close do not hide it.
        assert_eq!(
            trailing_block_line("1\n@scope {\n  td { color: #3f0421 }\n}\n\n"),
            Some(2),
        );
        // A multi-line rule body is still brace-matched back to the open.
        assert_eq!(
            trailing_block_line("1\n@scope {\n  td {\n    color: #3f0421\n  }\n}"),
            Some(2),
        );
    }
}
