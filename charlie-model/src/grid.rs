// Concern: the GRID (GRID1) — the resolved cells of a file's closed range: for every coordinate one `Cell`, either an explicit literal `Value`, a parsed formula (`Expr`, plus its source for the "show formulas" render), or a GRID6 LOAD-ERROR cell (content charlie cannot deserialize — an unparseable `=formula` or a malformed TSV escape — its verbatim source plus the located `Diagnostic`, which resolves to a located error VALUE rather than failing the whole file) — and the TSV DESERIALIZER/ENCODER (GRID2, the current on-disk format): columns split on UNESCAPED tab, rows on UNESCAPED newline; then each field is DECODED (`\t`->tab, `\n`->newline, `\\`->backslash — a backslash before anything else, or a trailing one, is a malformed cell -> located GRID6 error), so a cell can hold a tab/newline/backslash. A decoded field beginning with `=` is a parsed formula (or, if unparseable, a GRID6 error cell), any other field a lexed literal, an empty field a Blank; a ragged grid is a located `#VALUE!`-class file-level refusal. Owns the inverse `encode_field` (backslash/tab/newline -> the three escapes, uniform) every writer uses, plus the per-token literal lexer (number/bool/error/text with apostrophe force-text) | Non-concern: whether the grid fills its file's declared range exactly (GRID4 — the dimension check lives in `parse_file`), EVALUATING a formula (charlie-ast owns eval; workbook.rs drives it), SURFACING a load-error cell's diagnostic to `check` (workbook.rs `lint` scans grids for it), and the filename that declares the range (filename.rs) | IO: (a file's content `&str`) -> `Result<Grid, Diagnostic>` (a structural fault is `Err`; an unparseable formula or malformed escape is a `Cell::LoadError` inside the grid, GRID6)
//! The grid and its TSV deserializer/encoder: [`Grid`], [`Cell`], [`deserialize_tsv`], [`encode_field`],
//! [`lex_literal`]. Field escaping is single-homed here: [`encode_field`] is the inverse of the
//! split-then-decode the deserializer performs, so `encode -> deserialize` round-trips a cell's exact
//! text (tabs, newlines, and backslashes included).

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
    /// A GRID6 load-error cell: content charlie could not deserialize — an `=formula` charlie-ast
    /// could not parse, or a field with a malformed escape (a backslash not beginning `\t`/`\n`/`\\`,
    /// or a trailing backslash). It keeps its source text (`src`, echoed by `--functions` so an agent
    /// sees what to fix) and the located [`Diagnostic`] (surfaced by `check`, and cited by
    /// [`load_error_value`] to spell the cell's error value at eval, VAL3). It is NOT a whole-file
    /// failure (GRID6): every other cell in the file still loads and evaluates.
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

/// Deserialize a file's content into a [`Grid`] (GRID2, the TSV format). Field boundaries are fixed
/// FIRST — rows split on an UNESCAPED newline, columns on an UNESCAPED tab (a raw tab/newline is always
/// a delimiter; a tab/newline that belongs to a cell is written `\t`/`\n`, so it is never a raw
/// delimiter) — and escapes are resolved AFTER, per field, by [`decode_field`]. A decoded field
/// beginning with `=` parses to a formula cell, any other field lexes to a literal, and an empty field
/// is a `Blank`. Never panics. A ragged grid is a located [`Diagnostic`] (`Err`, a structural
/// file-level refusal). An UNPARSEABLE `=formula` OR a MALFORMED escape is NOT a whole-file failure
/// (GRID6): it deserializes to a [`Cell::LoadError`] carrying its source and located diagnostic, so
/// every other cell still loads. `file` names the file for diagnostics; the entire content is the grid
/// (GRID1) — there is no header/annotation line, so grid row `n` is file line `n` (1-based).
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

/// Deserialize one TSV field. The field's escapes are RESOLVED FIRST ([`decode_field`]): a malformed
/// escape (a backslash not beginning `\t`/`\n`/`\\`, or a trailing backslash) makes this a GRID6
/// [`Cell::LoadError`] located at the offending backslash — a per-cell error, never a whole-file
/// failure. Otherwise the DECODED text is interpreted: a `=`-prefixed field is a parsed formula
/// (unparseable -> a GRID6 [`Cell::LoadError`]), anything else a lexed literal. Infallible: a field
/// always yields a cell (the ragged-grid structural fault is caught in [`deserialize_tsv`]). `byte` is
/// the field's byte offset within its file line, so a located diagnostic points at the true column.
fn deserialize_field(file: &str, file_line: u32, byte: usize, raw: &str) -> Cell {
    // Resolve the field escapes before deciding formula-vs-literal (escapes never produce a leading
    // `=`, so the formula test is equivalent on raw or decoded text, but the parser must see the
    // decoded formula). A malformed escape is a GRID6 located error VALUE, never a silent literal.
    let decoded = match decode_field(raw) {
        Ok(d) => d,
        Err(pos) => {
            // Point the diagnostic at the offending backslash (field byte offset -> file column,
            // 1-based); the span covers the backslash and the char after it (the malformed pair).
            let col = (byte + pos + 1) as u32;
            return Cell::LoadError {
                src: raw.to_string(),
                diag: Diagnostic::new(
                    Code::MalformedEscape,
                    Loc::body_span(file, file_line, col, file_line, col + 1),
                    format!(
                        "malformed escape in field {raw:?} at byte {pos}: a backslash must begin \\t, \\n, or \\\\ (write a literal backslash as \\\\)"
                    ),
                ),
            };
        }
    };
    if decoded.starts_with('=') {
        match parse(&decoded) {
            Ok(expr) => Cell::Formula { src: decoded, expr },
            // GRID6: an unparseable/unsupported formula is a located error VALUE in the grid, not a
            // whole-file refusal. Keep the (decoded) source (so `--functions` shows what to fix) and the
            // located diagnostic (so `check` reports it); the cell resolves to `#NAME?` at eval.
            Err(diag) => Cell::LoadError {
                diag: Diagnostic::new(
                    Code::FormulaSyntax,
                    Loc::body_span(
                        file,
                        file_line,
                        (byte + diag.span.start + 1) as u32,
                        file_line,
                        (byte + diag.span.end + 1) as u32,
                    ),
                    format!("cannot parse formula {decoded:?}: {}", diag.message),
                ),
                src: decoded,
            },
        }
    } else {
        Cell::Value(lex_literal(&decoded))
    }
}

/// Decode one TSV field's escapes: `\t`->TAB, `\n`->NEWLINE, `\\`->backslash (the inverse of
/// [`encode_field`]). A backslash that does not begin one of those three escapes — or a trailing
/// backslash — is a malformed escape: `Err(offset)` carries the byte offset of the offending backslash
/// within `field` (so the caller can locate the GRID6 error, GRID6/CORE2). This is the STRICT, UNIFORM
/// decode the deserializer applies to every field (SPEC "current deserializer").
fn decode_field(field: &str) -> Result<String, usize> {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.char_indices();
    while let Some((i, c)) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some((_, 't')) => out.push('\t'),
                Some((_, 'n')) => out.push('\n'),
                Some((_, '\\')) => out.push('\\'),
                // A backslash before anything else, or a trailing backslash (`None`), is malformed —
                // located at the backslash so the fix (write `\\`) is unambiguous.
                _ => return Err(i),
            }
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

/// Encode one field's content for TSV: escape a backslash to `\\`, a TAB to `\t`, a NEWLINE to `\n`,
/// UNIFORMLY (the inverse of [`decode_field`]). This is the SINGLE home every writer uses to spell a
/// cell's field to disk — `charlie-ingest` import and any grid->TSV serializer — so `encode -> the
/// deserializer` round-trips a cell's exact text (a tab, newline, or backslash included) and an
/// embedded newline no longer forces a cell to be unrepresentable. Backslash is matched first so its
/// escape is not re-escaped; the three cases are disjoint, so order is otherwise immaterial.
pub fn encode_field(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    for c in field.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    out
}

/// Lex one DECODED literal field into a [`Value`] (field escapes are already resolved by
/// [`decode_field`], so `token` may carry a raw tab/newline/backslash). Precedence: apostrophe
/// force-text, then `TRUE`/`FALSE`, then the seven error literals, then a finite number, else text. An
/// empty token is `Blank`.
pub fn lex_literal(token: &str) -> Value {
    if token.is_empty() {
        return Value::Blank;
    }
    if let Some(rest) = token.strip_prefix('\'') {
        return Value::Text(rest.to_string());
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
        // A DECODED field carrying a raw tab/backslash is plain text (field escapes are resolved
        // upstream by `decode_field`, so `lex_literal` never re-decodes; a bare `"..."` is literal
        // text WITH its quotes — there is no quoted-literal escaping layer).
        assert_eq!(lex_literal("a\tb"), Value::Text("a\tb".to_string()));
        assert_eq!(lex_literal("a\\b"), Value::Text("a\\b".to_string()));
        assert_eq!(
            lex_literal(r#""quoted""#),
            Value::Text(r#""quoted""#.to_string())
        );
        // inf/nan are text, never a non-finite Number.
        assert_eq!(lex_literal("inf"), Value::Text("inf".to_string()));
        assert_eq!(lex_literal("NaN"), Value::Text("NaN".to_string()));
        // An engine-produced-only error spelling is not an author-writable literal token -> text.
        assert_eq!(lex_literal("#SPILL!"), Value::Text("#SPILL!".to_string()));
    }

    #[test]
    fn encode_field_escapes_the_three_specials_uniformly() {
        assert_eq!(encode_field("plain"), "plain");
        assert_eq!(encode_field("a\tb"), "a\\tb");
        assert_eq!(encode_field("a\nb"), "a\\nb");
        assert_eq!(encode_field("a\\b"), "a\\\\b");
        // A backslash is escaped, not re-processed: `\t` (backslash+t) as CONTENT becomes `\\t`.
        assert_eq!(encode_field("\\t"), "\\\\t");
        // A trailing backslash and an embedded newline both survive the round trip below.
        assert_eq!(encode_field("end\\"), "end\\\\");
    }

    #[test]
    fn encode_then_deserialize_round_trips_a_cells_exact_text() {
        // The core contract: encode_field is the inverse of the split-then-decode deserializer, so a
        // cell's exact text — tab, newline, backslash included — survives `encode -> deserialize`.
        for text in [
            "plain",
            "a\tb",           // an embedded tab
            "line1\nline2",   // an embedded newline (multi-line cell)
            "a\\b",           // a literal backslash
            "C:\\path\\file", // several backslashes
            "trailing\\",     // a trailing backslash
            "\t\n\\",         // all three at once
            "mix\ta\nb\\c",
        ] {
            let field = encode_field(text);
            let g = deserialize_tsv("A1", &field).expect("encoded field deserializes");
            assert_eq!(g.shape, Shape { rows: 1, cols: 1 });
            assert_eq!(
                g.cells[0],
                Cell::Value(Value::Text(text.to_string())),
                "text {text:?} encoded as {field:?} did not round-trip",
            );
        }
    }

    #[test]
    fn a_multi_line_cell_holds_its_newline_without_splitting_the_grid() {
        // A raw newline is a ROW delimiter; the ESCAPED newline `\n` is content (so a cell can hold
        // multi-line text) — the grid stays 1x1, one cell carrying the newline.
        let g = deserialize_tsv("A1", "top\\nbottom").expect("loads");
        assert_eq!(g.shape, Shape { rows: 1, cols: 1 });
        assert_eq!(
            g.cells[0],
            Cell::Value(Value::Text("top\nbottom".to_string()))
        );
    }

    #[test]
    fn a_malformed_escape_is_a_located_grid6_error_cell() {
        // GRID6: a backslash not beginning \t/\n/\\ (here `\x`) makes the CELL a located error value,
        // NOT a silent literal and NOT a whole-file refusal.
        let g =
            deserialize_tsv("A1", "a\\xb").expect("GRID6: the file loads, the cell is the error");
        assert_eq!(g.shape, Shape { rows: 1, cols: 1 });
        match &g.cells[0] {
            Cell::LoadError { src, diag } => {
                assert_eq!(src, "a\\xb", "the raw source is kept for --functions");
                assert_eq!(diag.code, Code::MalformedEscape);
                // Located at the offending backslash: byte 1 of the field -> column 2 (1-based).
                assert!(
                    matches!(
                        diag.loc,
                        Loc::Body {
                            line: 1,
                            col: 2,
                            ..
                        }
                    ),
                    "{:?}",
                    diag.loc
                );
                // The cell resolves to a located `#VALUE!` error value (VAL3).
                assert_eq!(load_error_value(diag), Value::Error(ErrKind::Value));
            }
            other => panic!("expected a load-error cell, got {other:?}"),
        }
    }

    #[test]
    fn a_trailing_backslash_is_a_malformed_escape() {
        let g = deserialize_tsv("A1", "end\\").expect("GRID6: the file loads");
        assert!(matches!(
            &g.cells[0],
            Cell::LoadError { diag, .. } if diag.code == Code::MalformedEscape
        ));
    }

    #[test]
    fn a_malformed_escape_does_not_abort_its_neighbours() {
        // GRID6 locality: one malformed-escape cell leaves the literal and good cell beside it intact,
        // and the error is located at the true column of the third field.
        let g = deserialize_tsv("A1:C1", "1\thi\tbad\\z").expect("the file loads (GRID6)");
        assert_eq!(g.shape, Shape { rows: 1, cols: 3 });
        assert_eq!(g.cells[0], Cell::Value(Value::Number(1.0)));
        assert_eq!(g.cells[1], Cell::Value(Value::Text("hi".to_string())));
        match &g.cells[2] {
            Cell::LoadError { diag, .. } => {
                assert_eq!(diag.code, Code::MalformedEscape);
                // Third field starts at line byte 5 (`1<tab>hi<tab>` = 1+1+2+1), backslash at field
                // byte 3 -> column 5+3+1 = 9 (1-based).
                assert!(
                    matches!(diag.loc, Loc::Body { col: 9, .. }),
                    "{:?}",
                    diag.loc
                );
            }
            other => panic!("expected a load-error cell, got {other:?}"),
        }
    }

    #[test]
    fn a_formula_field_with_an_escaped_backslash_round_trips_and_parses() {
        // A formula is a field too: an escaped backslash in a string literal decodes before parsing,
        // so the ENGINE sees the real formula and --functions echoes the decoded source.
        let field = encode_field(r#"="C:\dir""#);
        assert_eq!(field, r#"="C:\\dir""#);
        let g = deserialize_tsv("A1", &field).expect("loads");
        assert!(
            matches!(&g.cells[0], Cell::Formula { src, .. } if src == r#"="C:\dir""#),
            "{:?}",
            g.cells[0]
        );
    }
}
