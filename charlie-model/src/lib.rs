// Concern: charlie-model — the filesystem SPREADSHEET model, exposed as: the filename->closed-range parser (`filename`, FS2), the TSV DESERIALIZER and the GRID it produces (`grid`, GRID1/GRID2), the overlap detector (`overlap`), the single-sourced diagnostic registry (`diagnostic`), the RENDER MODEL (`render`) that turns a viewport into a plain-data ASCII grid of value/formula strings for the CLI to draw, the canonical live TUTORIAL workbook as data (`sample`, what `charlie-cli sample` writes out), and (`workbook`) the DEMAND-DRIVEN evaluation engine that loads a sheet-directory and implements `charlie_ast::Resolver` over it — pulling each formula cell through `charlie_ast::eval`, memoized and cycle-safe; `parse_file` ties the filename+deserializer into one loaded `ParsedFile` (its declared range and its grid), enforcing GRID4 (the grid fills the range exactly); `Workbook` resolves cells to `Value`s on demand and additionally exposes `Workbook::eval_formula`, the AD-HOC `=formula` evaluator (the `charlie-cli eval` entry) returning a `FormulaOutcome` so the CLI branches its exit code without depending on `charlie-ast` | Non-concern: the formula LANGUAGE (charlie-ast owns lex/parse/eval; the model only DRIVES it via the Resolver), xlsx serde, and the CLI surface (charlie-cli) | IO: (a filename + file contents) -> `Result<ParsedFile, Diagnostic>`; (a sheet-directory) -> a `Workbook` that resolves cells to `Value`s on demand
//! # charlie-model — the filesystem spreadsheet model
//!
//! **CHARTER.** `charlie-model` owns the on-disk encoding: a tab is a folder and a file's *name* is a
//! closed A1 range (FS2), whose *content* deserializes into a [`Grid`] — for every coordinate one
//! cell, an explicit value or a parsed `=formula` (GRID1). The current deserializer is TSV (GRID2): tab
//! columns, newline rows, each field a literal or an `=formula`. It is the middle crate of the
//! firewall `charlie-cli -> charlie-model -> charlie-ast`: it depends on `charlie-ast` for the
//! ref/value/shape types and the shared A1 grammar (the one allowed firewall edge), and the AST never
//! learns of the filesystem model.
//!
//! [`Workbook`] loads a sheet-directory and implements [`charlie_ast::Resolver`] over the grids, so
//! `charlie_ast::eval` PULLS each formula cell through the model — memoized, cycle-safe (a reference
//! cycle is a located `#REF!`-class refusal, never a hang), and lazy (only transitively-requested
//! cells compute). A cell's value derives only from its own content (VAL1): a range file is an
//! EXPLICIT grid, with no single-formula-offset (drag-fill) mechanism anywhere. Everything is a
//! *located refusal* ([`Diagnostic`]) — never a panic, never a silent drop (ast-standards PART 5).

pub mod diagnostic;
pub mod filename;
pub mod grid;
pub mod overlap;
pub mod render;
pub mod sample;
pub mod workbook;

pub use diagnostic::{Applicability, ByteSpan, Code, Diagnostic, Fix, Loc, Severity};
pub use filename::{FileName, parse_filename};
pub use grid::{Cell, Grid, deserialize_tsv, lex_literal, load_error_value};
pub use overlap::{Rect, detect_overlaps};
pub use render::{
    MAX_VIEWPORT_CELLS, RenderGrid, RenderMode, RenderRow, display_value, parse_viewport, render,
    viewport_cell_count,
};
pub use sample::sample_workbook;
pub use workbook::{CellSource, Direction, FormulaOutcome, TraceNode, TraceStatus, Workbook};

use charlie_ast::Shape;

/// One fully-loaded file: the declared closed range (region + shape) and the grid its content
/// deserialized to. This is the end-to-end artifact for a single file.
///
/// `array_formula` is the GRID5 disambiguation, decided once here (see [`parse_file`]): `true` iff the
/// whole file content is a single `=formula` whose declared range spans MORE than one coordinate — an
/// ARRAY-FORMULA REGION (VAL1: ONE array-formula cell spanning its range, not many cells). Its `grid`
/// is then the lone `1x1` formula cell; the region's shape is `declared_shape`, and the engine
/// evaluates the formula ONCE and fills each coordinate with the matching array element (GRID5). For a
/// normal per-cell file (`false`) the grid fills the range exactly (GRID4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedFile {
    pub region: Rect,
    pub declared_shape: Shape,
    pub grid: Grid,
    pub array_formula: bool,
}

/// Load one file from its name and contents: parse the filename to a closed range, deserialize the
/// content to a [`Grid`], and enforce GRID4 (the grid fills the declared range exactly). The entire
/// file content is the grid — there is no header, annotation, or metadata line, and the first line is
/// the first row (GRID1). Never panics; the first violation is returned as a located [`Diagnostic`].
pub fn parse_file(name: &str, contents: &str) -> Result<ParsedFile, Diagnostic> {
    let declared = parse_filename(name)?;

    let grid = deserialize_tsv(name, contents)?;
    let declared_shape = declared.declared_shape;
    // GRID4/GRID5 disambiguation — three cases, decided once:
    //   1. the grid fills the range exactly              -> the normal per-cell form (GRID4 satisfied);
    //   2. the grid is a SINGLE `=formula` cell and the  -> a GRID5 ARRAY-FORMULA REGION (the formula
    //      declared range spans MORE than one coordinate    computes once at eval and fills the range);
    //   3. anything else (a wrong-sized literal grid, a  -> a located dimension error (GRID4).
    //      wrong-sized multi-cell grid, a single literal)
    // A single LITERAL in a multi-cell range is case 3, NOT a region — only a lone `=formula` triggers
    // GRID5. A 1x1 file that holds an array formula is case 1 (grid fills its 1x1 range); it keeps the
    // array's top-left element at eval (implicit intersection, GRID5), handled by the engine.
    let fills_range = grid.shape == declared_shape;
    let spans_multiple = (declared_shape.rows as u64) * (declared_shape.cols as u64) > 1;
    let is_single_formula = grid.shape == (Shape { rows: 1, cols: 1 })
        && matches!(grid.cells.first(), Some(Cell::Formula { .. }));

    let array_formula = if fills_range {
        false
    } else if spans_multiple && is_single_formula {
        true
    } else {
        return Err(Diagnostic::new(
            Code::DimensionMismatch,
            Loc::file(name),
            format!(
                "the grid is {}x{} but the file's range {name:?} declares {}x{}: the grid must \
                 fill the closed range exactly (GRID4), or be a single =formula whose array value \
                 fills the range (GRID5)",
                grid.shape.rows, grid.shape.cols, declared_shape.rows, declared_shape.cols,
            ),
        ));
    };

    Ok(ParsedFile {
        region: declared.region,
        declared_shape,
        grid,
        array_formula,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use charlie_ast::{ErrKind, Value};
    use grid::Cell;

    #[test]
    fn loads_a_header_row_exact_match() {
        // A1:D1, declared 1x4, a 1x4 literal row -> fills the range. The entire content is the grid
        // (GRID1): the first line is the first row, with no header/annotation line.
        let f = parse_file("A1:D1", "Product\tUnit Price\tQty\tLine Total").unwrap();
        assert_eq!(f.declared_shape, Shape { rows: 1, cols: 4 });
        assert_eq!(f.grid.shape, Shape { rows: 1, cols: 4 });
        assert_eq!(
            f.grid.cells[0],
            Cell::Value(Value::Text("Product".to_string()))
        );
    }

    #[test]
    fn loads_a_single_formula_cell() {
        // A1 with a formula body -> a 1x1 grid whose one cell is a parsed formula.
        let f = parse_file("A1", "=B2*C2").unwrap();
        assert!(matches!(&f.grid.cells[0], Cell::Formula { src, .. } if src == "=B2*C2"));
    }

    #[test]
    fn loads_an_explicit_mixed_grid() {
        // VAL1: a range file's content is the EXPLICIT grid; each cell is independently a literal or
        // a formula. B2:D4 declares 3x3, and the body is a full 3x3 grid.
        let f = parse_file("B2:D4", "1\t2\t3\n=A1\t=B1\t=C1\n7\t8\t9").unwrap();
        assert_eq!(f.grid.shape, Shape { rows: 3, cols: 3 });
        assert_eq!(f.grid.cells[0], Cell::Value(Value::Number(1.0)));
        assert!(matches!(&f.grid.cells[3], Cell::Formula { src, .. } if src == "=A1"));
    }

    #[test]
    fn loads_a_blank_cell() {
        // Empty content is a single Blank cell (the 0-D range `A1` written with no body).
        let f = parse_file("A1", "").unwrap();
        assert_eq!(f.grid.shape, Shape { rows: 1, cols: 1 });
        assert_eq!(f.grid.cells, vec![Cell::Value(Value::Blank)]);
    }

    #[test]
    fn a_single_cell_ref_error_reads_as_the_error_literal() {
        // The content is exactly the grid (GRID1): a lone `#REF!` is the first (and only) row, so it
        // lexes to the error literal directly — no annotation line to disambiguate against.
        let f = parse_file("A1", "#REF!").unwrap();
        assert_eq!(f.grid.shape, Shape { rows: 1, cols: 1 });
        assert_eq!(f.grid.cells, vec![Cell::Value(Value::Error(ErrKind::Ref))]);
    }

    #[test]
    fn a_grid_that_does_not_fill_the_range_is_a_dimension_error() {
        // GRID4: B2:D4 declares 3x3, but the body is only 2x2 -> a located dimension error.
        let d = parse_file("B2:D4", "1\t2\n3\t4").unwrap_err();
        assert_eq!(d.code, Code::DimensionMismatch);
    }

    #[test]
    fn a_bad_filename_is_rejected_before_the_body() {
        let d = parse_file("g8:a3", "1\t2").unwrap_err();
        // Lowercase is caught per-address before the ordering check.
        assert_eq!(d.code, Code::LowercaseColumn);
    }
}
