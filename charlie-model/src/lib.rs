// Concern: charlie-model — the filesystem SPREADSHEET model, exposed as: the filename->closed-range parser (`filename`, FT-3), the TSV DESERIALIZER and the GRID it produces (`grid`, FT-4/FT-5), the overlap detector (`overlap`), the single-sourced diagnostic registry (`diagnostic`), the RENDER MODEL (`render`) that turns a viewport into a plain-data ASCII grid of value/formula/annotation strings for the CLI to draw, and (`workbook`) the DEMAND-DRIVEN evaluation engine that loads a sheet-directory and implements `charlie_ast::Resolver` over it — pulling each formula cell through `charlie_ast::eval`, memoized and cycle-safe; `parse_file` ties the filename+annotation+deserializer into one loaded `ParsedFile` (its declared range and its grid), enforcing FT-8 (the grid fills the range exactly); `Workbook` resolves cells to `Value`s on demand and additionally exposes `Workbook::eval_formula`, the AD-HOC `=formula` evaluator (the `charlie eval` entry) returning a `FormulaOutcome` so the CLI branches its exit code without depending on `charlie-ast` | Non-concern: the formula LANGUAGE (charlie-ast owns lex/parse/eval; the model only DRIVES it via the Resolver), xlsx serde, and the CLI surface (charlie-cli) | IO: (a filename + file contents) -> `Result<ParsedFile, Diagnostic>`; (a sheet-directory) -> a `Workbook` that resolves cells to `Value`s on demand
//! # charlie-model — the filesystem spreadsheet model
//!
//! **CHARTER.** `charlie-model` owns the on-disk encoding: a tab is a folder and a file's *name* is a
//! closed A1 range (FT-3), whose *content* deserializes into a [`Grid`] — for every coordinate one
//! cell, an explicit value or a parsed `=formula` (FT-4). The current deserializer is TSV (FT-5): tab
//! columns, newline rows, each field a literal or an `=formula`. It is the middle crate of the
//! firewall `charlie-cli -> charlie-model -> charlie-ast`: it depends on `charlie-ast` for the
//! ref/value/shape types and the shared A1 grammar (the one allowed firewall edge), and the AST never
//! learns of the filesystem model.
//!
//! [`Workbook`] loads a sheet-directory and implements [`charlie_ast::Resolver`] over the grids, so
//! `charlie_ast::eval` PULLS each formula cell through the model — memoized, cycle-safe (a reference
//! cycle is a located `#REF!`-class refusal, never a hang), and lazy (only transitively-requested
//! cells compute). A cell's value derives only from its own content (FT-9): a range file is an
//! EXPLICIT grid, with no single-formula-offset (drag-fill) mechanism anywhere. Everything is a
//! *located refusal* ([`Diagnostic`]) — never a panic, never a silent drop (ast-standards PART 5).

pub mod diagnostic;
pub mod filename;
pub mod grid;
pub mod overlap;
pub mod render;
pub mod workbook;

pub use diagnostic::{Code, Diagnostic, Loc, Severity};
pub use filename::{FileName, parse_filename};
pub use grid::{Cell, Grid, deserialize_tsv, lex_literal};
pub use overlap::{Rect, detect_overlaps};
pub use render::{
    MAX_VIEWPORT_CELLS, RenderGrid, RenderMode, RenderRow, display_value, parse_viewport, render,
    viewport_cell_count,
};
pub use workbook::{CellSource, FormulaOutcome, Workbook};

use charlie_ast::Shape;

/// One fully-loaded file: the declared closed range (region + shape) and the grid its content
/// deserialized to. This is the end-to-end artifact for a single file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedFile {
    pub region: Rect,
    pub declared_shape: Shape,
    pub grid: Grid,
}

/// Load one file from its name and contents: parse the filename to a closed range, verify the line-1
/// `# ` annotation, deserialize the body to a [`Grid`], and enforce FT-8 (the grid fills the declared
/// range exactly). Never panics; the first violation is returned as a located [`Diagnostic`].
pub fn parse_file(name: &str, contents: &str) -> Result<ParsedFile, Diagnostic> {
    let declared = parse_filename(name)?;

    // Line 1 is the mandatory `# ` annotation; the body is everything after it.
    let (line1, rest) = match contents.split_once('\n') {
        Some((first, rest)) => (first, rest),
        None => (contents, ""),
    };
    if !line1.starts_with("# ") {
        return Err(Diagnostic::new(
            Code::MissingAnnotation,
            Loc::body(name, 1, 1),
            "line 1 must be a '# ' annotation".to_string(),
        ));
    }

    let grid = deserialize_tsv(name, rest)?;
    // FT-8: the deserialized grid must fill the file's closed range exactly.
    if grid.shape != declared.declared_shape {
        return Err(Diagnostic::new(
            Code::DimensionMismatch,
            Loc::file(name),
            format!(
                "the grid is {}x{} but the file's range {name:?} declares {}x{}: the grid must \
                 fill the closed range exactly (FT-8)",
                grid.shape.rows,
                grid.shape.cols,
                declared.declared_shape.rows,
                declared.declared_shape.cols,
            ),
        ));
    }

    Ok(ParsedFile {
        region: declared.region,
        declared_shape: declared.declared_shape,
        grid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use charlie_ast::Value;
    use grid::Cell;

    const ANN: &str = "# Concern: x | Non-concern: y | IO: input\n";

    #[test]
    fn loads_a_header_row_exact_match() {
        // A1:D1, declared 1x4, a 1x4 literal row -> fills the range.
        let contents = format!("{ANN}Product\tUnit Price\tQty\tLine Total");
        let f = parse_file("A1:D1", &contents).unwrap();
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
        let contents = format!("{ANN}=B2*C2");
        let f = parse_file("A1", &contents).unwrap();
        assert!(matches!(&f.grid.cells[0], Cell::Formula { src, .. } if src == "=B2*C2"));
    }

    #[test]
    fn loads_an_explicit_mixed_grid() {
        // FT-9: a range file's content is the EXPLICIT grid; each cell is independently a literal or
        // a formula. B2:D4 declares 3x3, and the body is a full 3x3 grid.
        let contents = format!("{ANN}1\t2\t3\n=A1\t=B1\t=C1\n7\t8\t9");
        let f = parse_file("B2:D4", &contents).unwrap();
        assert_eq!(f.grid.shape, Shape { rows: 3, cols: 3 });
        assert_eq!(f.grid.cells[0], Cell::Value(Value::Number(1.0)));
        assert!(matches!(&f.grid.cells[3], Cell::Formula { src, .. } if src == "=A1"));
    }

    #[test]
    fn loads_a_blank_cell() {
        let f = parse_file("A1", "# ann only, no body").unwrap();
        assert_eq!(f.grid.shape, Shape { rows: 1, cols: 1 });
        assert_eq!(f.grid.cells, vec![Cell::Value(Value::Blank)]);
    }

    #[test]
    fn missing_annotation_is_rejected() {
        // First line is data, not a `# ` annotation.
        let d = parse_file("A1:D1", "Product\tPrice\tQty\tTotal").unwrap_err();
        assert_eq!(d.code, Code::MissingAnnotation);
    }

    #[test]
    fn a_grid_that_does_not_fill_the_range_is_a_dimension_error() {
        // FT-8: B2:D4 declares 3x3, but the body is only 2x2 -> a located dimension error.
        let contents = format!("{ANN}1\t2\n3\t4");
        let d = parse_file("B2:D4", &contents).unwrap_err();
        assert_eq!(d.code, Code::DimensionMismatch);
    }

    #[test]
    fn a_bad_filename_is_rejected_before_the_body() {
        let d = parse_file("g8:a3", &format!("{ANN}1\t2")).unwrap_err();
        // Lowercase is caught per-address before the ordering check.
        assert_eq!(d.code, Code::LowercaseColumn);
    }
}
