// Concern: the GRID (GRID1) — the resolved cells of a file's closed range: for every coordinate one `Cell`, either an explicit literal `Value`, a parsed formula (`Expr`, plus its verbatim source for the "show formulas" render), or a GRID6 LOAD-ERROR cell (an `=formula` charlie cannot parse — its verbatim source plus the located `Diagnostic`, which resolves to a located error VALUE rather than failing the whole file) — and the TSV DESERIALIZER (GRID2, the current on-disk format): tab-separated columns, newline-separated rows; a field beginning with `=` is a parsed formula (or, if unparseable, a located error cell, GRID6), any other field a lexed literal, an empty field a Blank; a ragged grid is a located `#VALUE!`-class file-level refusal. Includes the per-token literal lexer (number/bool/error/text with force-text and quoted-string escapes) | Non-concern: whether the grid fills its file's declared range exactly (GRID4 — the dimension check lives in `parse_file`), EVALUATING a formula (charlie-ast owns eval; workbook.rs drives it), SURFACING a load-error cell's diagnostic to `check` (workbook.rs `lint` scans grids for it), and the filename that declares the range (filename.rs) | IO: (a file's content `&str`) -> `Result<Grid, Diagnostic>` (a structural fault is `Err`; an unparseable formula is a `Cell::LoadError` inside the grid, GRID6)
//! The grid and its TSV deserializer: [`Grid`], [`Cell`], [`deserialize_tsv`], [`lex_literal`].

use crate::diagnostic::{Code, Diagnostic, Loc};
use charlie_ast::{ErrKind, Expr, Shape, Value, parse};

/// One resolved cell of a [`Grid`]: an explicit literal value, a parsed `=formula` (GRID1/VAL2), or a
/// GRID6 load-error cell. A formula keeps both its parsed [`Expr`] (what the engine evaluates) and its
/// verbatim `src` text (what the `--functions` "show formulas" render echoes), so the model never
/// re-parses to display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    /// A literal value (number/text/bool/error/blank).
    Value(Value),
    /// A parsed formula and its source text (the leading `=` included).
    Formula { src: String, expr: Expr },
    /// A GRID6 load-error cell: an `=formula` whose content charlie-ast could not parse. It keeps its
    /// verbatim source text (`src`, echoed by `--functions` so an agent sees what to fix) and the
    /// located [`Diagnostic`] (surfaced by `check`, and cited by [`load_error_value`] to spell the
    /// cell's error value at eval, VAL3). It is NOT a whole-file failure (GRID6): every other cell in
    /// the file still loads and evaluates.
    LoadError { src: String, diag: Diagnostic },
}

/// The located error VALUE a GRID6 [`Cell::LoadError`] cell resolves to (VAL3): the spreadsheet-error
/// class its diagnostic cites (an unparseable `=formula` is a [`Code::FormulaSyntax`] refusal, which
/// cites `#NAME?`). Single home so the resolver, the evaluate defensive arm, and the computation hash
/// all spell the same error value for a load-error cell.
pub fn load_error_value(diag: &Diagnostic) -> Value {
    Value::Error(diag.code.err_class().unwrap_or(ErrKind::Name))
}

/// The resolved cells of a file's closed range (GRID1): a [`Shape`] and one [`Cell`] per coordinate,
/// stored row-major. The engine (workbook.rs) evaluates over this grid; the deserializer builds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    pub shape: Shape,
    /// Row-major, `shape.rows * shape.cols` cells.
    pub cells: Vec<Cell>,
}

impl Grid {
    /// The cell at zero-based `(row, col)` within the grid (a local offset, not an absolute A1
    /// coordinate). Panics only on an out-of-range index — a `debug_assert` guards the DbC invariant
    /// that callers offset within `shape`.
    pub fn cell_at(&self, row: u32, col: u32) -> &Cell {
        let idx = (row as usize) * (self.shape.cols as usize) + (col as usize);
        debug_assert!(
            idx < self.cells.len(),
            "grid index ({row},{col}) out of range for a {}x{} grid",
            self.shape.rows,
            self.shape.cols,
        );
        &self.cells[idx]
    }
}

/// Deserialize a file's content into a [`Grid`] (GRID2, the TSV format): each physical line is a row
/// split on tabs; a field beginning with `=` parses to a formula cell, any other field lexes to a
/// literal, and an empty field is a `Blank`. Never panics. A ragged grid is a located [`Diagnostic`]
/// (`Err`, a structural file-level refusal). An UNPARSEABLE `=formula` is NOT a whole-file failure
/// (GRID6): it deserializes to a [`Cell::LoadError`] carrying its verbatim source and located
/// diagnostic, so every other cell still loads. `file` names the file for diagnostics; the entire
/// content is the grid (GRID1) — there is no header/annotation line, so grid row `n` is file line `n`
/// (1-based).
///
/// The grid's own dimensions come from the content; whether they FILL the file's declared range
/// (GRID4) is checked separately in [`crate::parse_file`].
pub fn deserialize_tsv(file: &str, content: &str) -> Result<Grid, Diagnostic> {
    // A single trailing newline is stripped and ignored (a lone trailing CR after it tolerated too),
    // so a stray CRLF at end-of-file adds no phantom row. An interior CR is NOT stripped — it rides
    // inside its token's text.
    let content = content.strip_suffix('\n').unwrap_or(content);
    let content = content.strip_suffix('\r').unwrap_or(content);

    // An empty body is a single Blank cell (the 0-D range `A1` written with no body). This resolves
    // toward ACCEPT for the single-cell case (ast-standards PART 6: a false-reject is the cardinal
    // sin) — a `1x1` Blank fills an `A1` file exactly; for a multi-cell range it is short and the
    // GRID4 fills-range check refuses it. Consistent with an unclaimed gap already reading Blank.
    if content.is_empty() {
        return Ok(Grid {
            shape: Shape { rows: 1, cols: 1 },
            cells: vec![Cell::Value(Value::Blank)],
        });
    }

    let lines: Vec<&str> = content.split('\n').collect();
    let cols = lines[0].split('\t').count();
    let mut cells = Vec::with_capacity(lines.len() * cols);
    for (row_idx, line) in lines.iter().enumerate() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != cols {
            return Err(Diagnostic::new(
                Code::RaggedGrid,
                Loc::body(file, (row_idx + 1) as u32, 1),
                format!(
                    "ragged TSV grid: row {} has {} field(s), expected {} (#VALUE!-class)",
                    row_idx + 1,
                    fields.len(),
                    cols,
                ),
            ));
        }
        // Track the byte offset of each field within its line, so a formula parse error can point at
        // the exact column (line byte offset + the refusal's span into the token, 1-based).
        let mut byte = 0usize;
        for field in fields {
            cells.push(deserialize_field(file, (row_idx + 1) as u32, byte, field));
            byte += field.len() + 1; // + the tab separator
        }
    }
    Ok(Grid {
        shape: Shape {
            rows: lines.len() as u32,
            cols: cols as u32,
        },
        cells,
    })
}

/// Deserialize one TSV field: a `=`-prefixed field is a parsed formula, anything else a lexed literal.
/// An `=formula` charlie-ast cannot parse becomes a GRID6 [`Cell::LoadError`] carrying its verbatim
/// source and the located refusal — a per-cell error, never a whole-file failure. Infallible: a field
/// always yields a cell (the ragged-grid structural fault is caught in [`deserialize_tsv`]).
fn deserialize_field(file: &str, file_line: u32, byte: usize, token: &str) -> Cell {
    if token.starts_with('=') {
        match parse(token) {
            Ok(expr) => Cell::Formula {
                src: token.to_string(),
                expr,
            },
            // GRID6: an unparseable/unsupported formula is a located error VALUE in the grid, not a
            // whole-file refusal. Keep the verbatim source (so `--functions` shows what to fix) and the
            // located diagnostic (so `check` reports it); the cell resolves to `#NAME?` at eval.
            Err(diag) => Cell::LoadError {
                src: token.to_string(),
                diag: Diagnostic::new(
                    Code::FormulaSyntax,
                    Loc::body_span(
                        file,
                        file_line,
                        (byte + diag.span.start + 1) as u32,
                        file_line,
                        (byte + diag.span.end + 1) as u32,
                    ),
                    format!("cannot parse formula {token:?}: {}", diag.message),
                ),
            },
        }
    } else {
        Cell::Value(lex_literal(token))
    }
}

/// Lex one literal token into a [`Value`]. Precedence: apostrophe force-text, then double-quoted
/// text, then `TRUE`/`FALSE`, then the seven error literals, then a finite number, else text. An empty
/// token is `Blank`.
pub fn lex_literal(token: &str) -> Value {
    if token.is_empty() {
        return Value::Blank;
    }
    if let Some(rest) = token.strip_prefix('\'') {
        return Value::Text(rest.to_string());
    }
    if token.len() >= 2 && token.starts_with('"') && token.ends_with('"') {
        return Value::Text(unescape_quoted(&token[1..token.len() - 1]));
    }
    // TRUE/FALSE and the seven error literals below match UPPERCASE only — deliberate, no
    // case-folding (`true`, `#ref!` fall through to Text), so the boolean/error domain has exactly
    // one canonical spelling on disk.
    match token {
        "TRUE" => return Value::Bool(true),
        "FALSE" => return Value::Bool(false),
        _ => {}
    }
    if let Some(kind) = error_literal(token) {
        return Value::Error(kind);
    }
    if let Some(n) = parse_number(token) {
        return Value::Number(n);
    }
    Value::Text(token.to_string())
}

/// Parse a numeric literal. Only finite values count: `inf`/`nan` and overflows (e.g. `1e999`) are
/// text, not numbers — this also keeps the volatile float-parse spellings out of the number domain.
fn parse_number(token: &str) -> Option<f64> {
    match token.parse::<f64>() {
        Ok(n) if n.is_finite() => Some(n),
        _ => None,
    }
}

/// The seven author-writable error literals. `#SPILL!`/`#CALC!` are ENGINE-PRODUCED ONLY (a formula's
/// value, never a cell's literal content), so as a literal token they fall through to text.
fn error_literal(token: &str) -> Option<ErrKind> {
    match token {
        "#REF!" => Some(ErrKind::Ref),
        "#DIV/0!" => Some(ErrKind::Div0),
        "#VALUE!" => Some(ErrKind::Value),
        "#NAME?" => Some(ErrKind::Name),
        "#N/A" => Some(ErrKind::Na),
        "#NULL!" => Some(ErrKind::Null),
        "#NUM!" => Some(ErrKind::Num),
        _ => None,
    }
}

/// Unescape the interior of a double-quoted literal: `\t` -> tab, `\"` -> quote, `\\` -> backslash.
fn unescape_quoted(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(content: &str) -> Grid {
        deserialize_tsv("F2:F11", content).expect("should deserialize")
    }

    #[test]
    fn a_single_formula_field_is_a_formula_cell() {
        let g = grid("=B2*C2");
        assert_eq!(g.shape, Shape { rows: 1, cols: 1 });
        match &g.cells[0] {
            Cell::Formula { src, expr } => {
                assert_eq!(src, "=B2*C2");
                assert_eq!(*expr, parse("=B2*C2").unwrap());
            }
            other => panic!("expected a formula cell, got {other:?}"),
        }
    }

    #[test]
    fn an_explicit_grid_mixes_literals_and_formulas_cell_by_cell() {
        // VAL1: a range file's content is the EXPLICIT grid — each cell independently a literal or a
        // formula, no single-formula drag-fill. Here row 1 is two literals, row 2 two formulas.
        let g = grid("10\t20\n=A1\t=B1*2");
        assert_eq!(g.shape, Shape { rows: 2, cols: 2 });
        assert_eq!(g.cells[0], Cell::Value(Value::Number(10.0)));
        assert_eq!(g.cells[1], Cell::Value(Value::Number(20.0)));
        assert!(matches!(&g.cells[2], Cell::Formula { src, .. } if src == "=A1"));
        assert!(matches!(&g.cells[3], Cell::Formula { src, .. } if src == "=B1*2"));
    }

    #[test]
    fn empty_content_is_a_single_blank_cell() {
        let g = grid("");
        assert_eq!(g.shape, Shape { rows: 1, cols: 1 });
        assert_eq!(g.cells, vec![Cell::Value(Value::Blank)]);
    }

    #[test]
    fn a_double_tab_makes_the_middle_field_blank() {
        // GRID2: an empty field is a Blank cell — a double tab blanks the middle cell.
        let g = grid("a\t\tb");
        assert_eq!(g.shape, Shape { rows: 1, cols: 3 });
        assert_eq!(
            g.cells,
            vec![
                Cell::Value(Value::Text("a".to_string())),
                Cell::Value(Value::Blank),
                Cell::Value(Value::Text("b".to_string())),
            ]
        );
    }

    #[test]
    fn a_trailing_newline_is_ignored() {
        let g = grid("1\t2\t3\n");
        assert_eq!(g.shape, Shape { rows: 1, cols: 3 });
    }

    #[test]
    fn a_ragged_grid_is_a_value_class_refusal() {
        let d = deserialize_tsv("B2:D3", "10\t20\t30\n40\t50").unwrap_err();
        assert_eq!(d.code, Code::RaggedGrid);
        assert_eq!(d.code.err_class(), Some(ErrKind::Value));
    }

    #[test]
    fn an_unparseable_formula_is_a_located_error_cell_not_a_file_refusal() {
        // GRID6: an unparseable `=formula` deserializes to a located error cell (never a whole-file
        // `Err`), so the file still loads and every other cell resolves. Here the whole content is the
        // one bad formula.
        let g =
            deserialize_tsv("A1", "=SUM(").expect("GRID6: the file loads, the cell is the error");
        assert_eq!(g.shape, Shape { rows: 1, cols: 1 });
        match &g.cells[0] {
            Cell::LoadError { src, diag } => {
                assert_eq!(src, "=SUM(");
                assert_eq!(diag.code, Code::FormulaSyntax);
                // Located on the first grid row (file line 1 — the whole content is the grid).
                assert!(
                    matches!(diag.loc, Loc::Body { line: 1, .. }),
                    "{:?}",
                    diag.loc
                );
                // The cell resolves to a located `#NAME?` error value (VAL3).
                assert_eq!(load_error_value(diag), Value::Error(ErrKind::Name));
            }
            other => panic!("expected a load-error cell, got {other:?}"),
        }
    }

    #[test]
    fn an_unparseable_formula_does_not_abort_its_neighbours() {
        // GRID6: one bad formula cell in a row leaves the literal and good formula beside it intact.
        let g = deserialize_tsv("A1:C1", "1\t=SUM(\t=A1+1").expect("the file loads (GRID6)");
        assert_eq!(g.shape, Shape { rows: 1, cols: 3 });
        assert_eq!(g.cells[0], Cell::Value(Value::Number(1.0)));
        assert!(matches!(&g.cells[1], Cell::LoadError { src, .. } if src == "=SUM("));
        assert!(matches!(&g.cells[2], Cell::Formula { src, .. } if src == "=A1+1"));
    }

    #[test]
    fn literal_lexing_covers_every_token_form() {
        assert_eq!(lex_literal(""), Value::Blank);
        assert_eq!(lex_literal("123"), Value::Number(123.0));
        assert_eq!(lex_literal("-4"), Value::Number(-4.0));
        assert_eq!(lex_literal("2.5"), Value::Number(2.5));
        assert_eq!(lex_literal("1.2e6"), Value::Number(1_200_000.0));
        assert_eq!(lex_literal("TRUE"), Value::Bool(true));
        assert_eq!(lex_literal("FALSE"), Value::Bool(false));
        assert_eq!(lex_literal("#REF!"), Value::Error(ErrKind::Ref));
        assert_eq!(lex_literal("#DIV/0!"), Value::Error(ErrKind::Div0));
        assert_eq!(lex_literal("#N/A"), Value::Error(ErrKind::Na));
        assert_eq!(lex_literal("hello"), Value::Text("hello".to_string()));
        // Force-text a numeric-looking value with a leading apostrophe.
        assert_eq!(lex_literal("'123"), Value::Text("123".to_string()));
        // Double-quoted text with escapes.
        assert_eq!(lex_literal(r#""a\tb""#), Value::Text("a\tb".to_string()));
        // inf/nan are text, never a non-finite Number.
        assert_eq!(lex_literal("inf"), Value::Text("inf".to_string()));
        assert_eq!(lex_literal("NaN"), Value::Text("NaN".to_string()));
        // An engine-produced-only error spelling is not an author-writable literal token -> text.
        assert_eq!(lex_literal("#SPILL!"), Value::Text("#SPILL!".to_string()));
    }
}
