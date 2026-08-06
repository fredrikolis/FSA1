// Concern: the crate's public surface plus parse_file, its whole-file load entry | Non-concern: formula semantics, xlsx serde | IO: (name, contents) -> ParsedFile
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
    Name, NameRepr, NameScope, NameTable, NameTarget, RawNameEntry, is_cell_filename, quote_sheet,
};
pub use overlap::{Rect, detect_overlaps};
pub use presentation::{Presentation, Rule, Target, spell_block};
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

/// One loaded file: the closed range its name declares, and the grid and presentation its content
/// deserialized to. `array_formula` marks the second legal form — a lone `1x1` `=formula` grid under
/// a multi-coordinate range, evaluated once and spread across the region.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedFile {
    pub region: Rect,
    pub declared_shape: Shape,
    pub grid: Grid,
    pub array_formula: bool,
    pub presentation: Option<Presentation>,
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

/// The file content is its grid — no header, annotation, or metadata line, and the first line is the
/// first row — optionally followed by a presentation block, which is found from the END so a cell
/// spelling `@scope {` cannot truncate the grid.
pub fn parse_file(name: &str, contents: &str) -> Result<ParsedFile, Vec<Diagnostic>> {
    let declared = parse_filename(name).map_err(|d| vec![d])?;
    let declared_shape = declared.declared_shape;
    let (grid_src, block) = presentation::split(contents);

    let grid = deserialize_tsv(name, grid_src).map_err(|d| vec![d])?;
    let Some(array_formula) = conforms(&grid, declared_shape) else {
        return Err(vec![dimension_mismatch(
            name,
            &grid,
            declared_shape,
            &block,
        )]);
    };
    // Only the array-formula reading can tie: under an exact fill the block's own lines are left over, so reading the whole file as a grid always yields more rows than the range declares.
    if block.is_some() && array_formula && whole_file_also_conforms(name, contents, declared_shape)
    {
        return Err(vec![Diagnostic::new(
            Code::AmbiguousGridTail,
            Loc::file(name),
            format!(
                "{name:?} fills its range both as a {}x{} grid and as one =formula followed by a \
                 presentation block: which of the two is meant cannot be decided from the file",
                declared_shape.rows, declared_shape.cols,
            ),
        )]);
    }

    let presentation = match &block {
        Some(b) => Some(presentation::parse_block(name, b, declared_shape)?),
        None => None,
    };
    Ok(ParsedFile {
        region: declared.region,
        declared_shape,
        grid,
        array_formula,
        presentation,
    })
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

fn whole_file_also_conforms(name: &str, contents: &str, declared: Shape) -> bool {
    deserialize_tsv(name, contents)
        .ok()
        .is_some_and(|g| conforms(&g, declared).is_some())
}

fn dimension_mismatch(
    name: &str,
    grid: &Grid,
    declared: Shape,
    block: &Option<presentation::Block<'_>>,
) -> Diagnostic {
    // Without this an author who wrote a block reads a row count that is nowhere in the file.
    let split_off = match block {
        Some(b) => format!(
            " (a presentation block was split off at line {}, and holds no cell)",
            b.line()
        ),
        None => String::new(),
    };
    Diagnostic::new(
        Code::DimensionMismatch,
        Loc::file(name),
        format!(
            "the grid is {}x{} but the file's range {name:?} declares {}x{}: the grid must \
             fill the closed range exactly (GRID4), or be a single =formula whose array value \
             fills the range (GRID5){split_off}",
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

    #[test]
    fn a_presentation_block_loads_beside_the_grid_it_styles() {
        let f = parse_file(
            "A1:C2",
            "1\t2\t3\n4\t5\t6\n@scope {\n  td { text-align: right }\n}",
        )
        .unwrap();
        assert_eq!(f.grid.shape, Shape { rows: 2, cols: 3 });
        let rules = f.presentation.expect("a presentation").rules;
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].target, Target::All);
    }

    #[test]
    fn a_file_without_a_block_carries_no_presentation() {
        assert_eq!(parse_file("A1:C1", "1\t2\t3").unwrap().presentation, None);
    }

    #[test]
    fn a_text_cell_spelling_the_block_open_stays_a_cell() {
        let f = parse_file("A1:A3", "1\n@scope {\n3").unwrap();
        assert_eq!(f.grid.shape, Shape { rows: 3, cols: 1 });
        assert_eq!(f.presentation, None);
        assert_eq!(
            f.grid.cells[1],
            Cell::Value {
                value: Value::Text("@scope {".to_string()),
                format: None
            }
        );
    }

    #[test]
    fn a_block_that_is_also_a_legal_grid_tail_is_refused_rather_than_guessed() {
        let d = parse_file("A1:A3", "=SUM(B1:B9)\n@scope {\n}").unwrap_err();
        assert_eq!(d[0].code, Code::AmbiguousGridTail);
    }

    #[test]
    fn a_grid_that_stops_fitting_once_the_block_is_split_off_names_the_block() {
        let d = parse_file("A1:A4", "1\n2\n@scope {\n}").unwrap_err();
        assert_eq!(d[0].code, Code::DimensionMismatch);
        assert!(
            d[0].message.contains("split off at line 3"),
            "{}",
            d[0].message
        );
    }

    #[test]
    fn a_malformed_block_refuses_the_file_with_every_fault_at_once() {
        let d = parse_file("A1:C1", "1\t2\t3\n@scope {\n  th { color: red }\n}").unwrap_err();
        assert_eq!(d.len(), 2, "{d:?}");
        assert_eq!(d[0].code, Code::PresentationSelector);
        assert_eq!(d[1].code, Code::PresentationValue);
    }
}
