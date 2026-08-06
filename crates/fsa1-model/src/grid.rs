// Concern: the cell grid and its TSV encoding in both directions | Non-concern: which range a grid must fill, evaluation | IO: (file, content) -> Grid; (field) -> escaped text

use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::format::{Format, lex_formatted_number};
use fsa1_ast::{ErrKind, Expr, Shape, Value, parse, parse_iso_serial};

/// A formula keeps BOTH its parsed [`Expr`] and its verbatim `src`, so the model never re-parses to
/// display. A `format` changes only how a cell is shown and never enters evaluation, for a formula
/// exactly as for a literal; a broken formula's format is moot, so [`Cell::LoadError`] carries none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    Value {
        value: Value,
        format: Option<Format>,
    },
    /// `src` EXCLUDES the trailing `~<code>` marker, which is re-emitted from `format` on serialize.
    Formula {
        src: String,
        expr: Expr,
        format: Option<Format>,
    },
    /// Content FSA1 could not deserialize. NOT a whole-file failure: every other cell in the file
    /// still loads and evaluates, and this one resolves to [`load_error_value`] at eval.
    LoadError { src: String, diag: Diagnostic },
}

/// Single home, so the resolver, the evaluate defensive arm, and the computation hash all spell a
/// load-error cell's value identically.
pub fn load_error_value(diag: &Diagnostic) -> Value {
    Value::Error(diag.code.err_class().unwrap_or(ErrKind::Name))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    pub shape: Shape,
    /// Row-major, `shape.rows * shape.cols` cells.
    pub cells: Vec<Cell>,
}

impl Grid {
    /// `(row, col)` is a LOCAL offset within the grid, not an absolute A1 coordinate.
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

/// Field boundaries are fixed FIRST — a raw tab or newline is always a delimiter — and escapes are
/// resolved AFTER, per field. The whole content is the grid, so grid row `n` is file line `n`
/// (1-based). Only a ragged grid is a whole-file `Err`; a bad field is a [`Cell::LoadError`] instead.
/// Whether the resulting shape FILLS the declared range is [`crate::parse_file`]'s question.
pub fn deserialize_tsv(file: &str, content: &str) -> Result<Grid, Diagnostic> {
    // A stray trailing CRLF must add no phantom row; an INTERIOR CR stays inside its token's text.
    let content = content.strip_suffix('\n').unwrap_or(content);
    let content = content.strip_suffix('\r').unwrap_or(content);

    // A `1x1` Blank fills an `A1` file exactly, and under a multi-cell range the fills-range check refuses it — consistent with an unclaimed gap already reading Blank.
    if content.is_empty() {
        return Ok(Grid {
            shape: Shape { rows: 1, cols: 1 },
            cells: vec![Cell::Value {
                value: Value::Blank,
                format: None,
            }],
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
        // The field's byte offset within its line, so a parse error can point at the exact column.
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

/// Infallible: a field always yields a cell. `byte` is the field's byte offset within its file line,
/// so a located diagnostic points at the true column. Escapes are resolved before the
/// formula-vs-literal test, which they cannot change, because the parser must see decoded text.
fn deserialize_field(file: &str, file_line: u32, byte: usize, raw: &str) -> Cell {
    let decoded = match decode_field(raw) {
        Ok(d) => d,
        Err(pos) => {
            // The span covers the backslash and the char after it, the malformed pair.
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
        // Split only when the tail is a catalog code AND the head parses: a format-less formula's own `~` sits inside a string or quoted sheet name, whose closing `"`/`'`/`!` no code carries.
        let (head, marker) = split_format_marker(&decoded);
        if let Some(code) = marker
            && let Ok(expr) = parse(head)
        {
            return Cell::Formula {
                src: head.to_string(),
                expr,
                format: Format::from_code(code),
            };
        }
        match parse(&decoded) {
            Ok(expr) => Cell::Formula {
                src: decoded,
                expr,
                format: None,
            },
            // An unknown format (`=SUM(A1)~bogus`) lands here too: its unsplit `~` is an `UnexpectedChar`, so a bad format surfaces as a refusal rather than being dropped.
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
        let (value, format) = lex_literal(&decoded);
        Cell::Value { value, format }
    }
}

/// Splits on the field's LAST `~` when the tail classifies as a catalog [`Format`]. The CALLER must
/// also check that `head` is well-formed for its context — a parseable formula, or a valid ISO value
/// — and take the split only when both hold.
pub fn split_format_marker(field: &str) -> (&str, Option<&str>) {
    match field.rsplit_once('~') {
        Some((head, tail)) if Format::from_code(tail).is_some() => (head, Some(tail)),
        _ => (field, None),
    }
}

/// The inverse of [`encode_field`]. `Err(offset)` carries the byte offset, within `field`, of the
/// backslash that begins no escape.
fn decode_field(field: &str) -> Result<String, usize> {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.char_indices();
    while let Some((i, c)) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some((_, 't')) => out.push('\t'),
                Some((_, 'n')) => out.push('\n'),
                Some((_, '\\')) => out.push('\\'),
                // Located at the backslash, so the fix (write `\\`) is unambiguous.
                _ => return Err(i),
            }
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

/// The SINGLE home every writer uses to spell a cell's field to disk, so `encode -> deserialize`
/// round-trips a cell's exact text — an embedded tab, newline, or backslash included. Backslash is
/// matched first so its own escape is not re-escaped.
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

/// `token` is already DECODED, so it may carry a raw tab, newline, or backslash and is never
/// re-decoded here. The returned `value` is always the bare value the engine sees; the `format` is
/// `Some` only for a displayed-form literal. The arm order below is the precedence.
pub fn lex_literal(token: &str) -> (Value, Option<Format>) {
    if token.is_empty() {
        return (Value::Blank, None);
    }
    if let Some(rest) = token.strip_prefix('\'') {
        return (Value::Text(rest.to_string()), None);
    }
    // Uppercase only: `true` and `#ref!` are Text, so each domain has one on-disk spelling.
    match token {
        "TRUE" => return (Value::Bool(true), None),
        "FALSE" => return (Value::Bool(false), None),
        _ => {}
    }
    if let Some(kind) = error_literal(token) {
        return (Value::Error(kind), None);
    }
    // A NUMBER code is not a date literal: a number literal is self-describing, needing no marker.
    let (head, marker) = split_format_marker(token);
    if let Some(code) = marker
        && let Some(fmt @ (Format::Date(_) | Format::Time(_) | Format::DateTime(_))) =
            Format::from_code(code)
        && let Some(serial) = parse_iso_serial(head)
    {
        return (Value::Number(serial), Some(fmt));
    }
    if let Some((value, fmt)) = lex_formatted_number(token) {
        return (Value::Number(value), Some(fmt));
    }
    if let Some(n) = parse_number(token) {
        return (Value::Number(n), None);
    }
    (Value::Text(token.to_string()), None)
}

/// Only finite values count: `inf`, `nan`, and overflows like `1e999` are text, not numbers.
fn parse_number(token: &str) -> Option<f64> {
    match token.parse::<f64>() {
        Ok(n) if n.is_finite() => Some(n),
        _ => None,
    }
}

/// The seven AUTHOR-WRITABLE error literals. `#SPILL!` and `#CALC!` are engine-produced only, so as
/// a literal token they fall through to text.
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
    use crate::format::{CurrencySymbol, DatePattern};

    fn grid(content: &str) -> Grid {
        deserialize_tsv("F2:F11", content).expect("should deserialize")
    }

    fn val(value: Value) -> Cell {
        Cell::Value {
            value,
            format: None,
        }
    }

    #[test]
    fn a_single_formula_field_is_a_formula_cell() {
        let g = grid("=B2*C2");
        assert_eq!(g.shape, Shape { rows: 1, cols: 1 });
        match &g.cells[0] {
            Cell::Formula { src, expr, format } => {
                assert_eq!(src, "=B2*C2");
                assert_eq!(*expr, parse("=B2*C2").unwrap());
                assert_eq!(*format, None);
            }
            other => panic!("expected a formula cell, got {other:?}"),
        }
    }

    #[test]
    fn an_explicit_grid_mixes_literals_and_formulas_cell_by_cell() {
        let g = grid("10\t20\n=A1\t=B1*2");
        assert_eq!(g.shape, Shape { rows: 2, cols: 2 });
        assert_eq!(g.cells[0], val(Value::Number(10.0)));
        assert_eq!(g.cells[1], val(Value::Number(20.0)));
        assert!(matches!(&g.cells[2], Cell::Formula { src, .. } if src == "=A1"));
        assert!(matches!(&g.cells[3], Cell::Formula { src, .. } if src == "=B1*2"));
    }

    #[test]
    fn empty_content_is_a_single_blank_cell() {
        let g = grid("");
        assert_eq!(g.shape, Shape { rows: 1, cols: 1 });
        assert_eq!(g.cells, vec![val(Value::Blank)]);
    }

    #[test]
    fn a_double_tab_makes_the_middle_field_blank() {
        let g = grid("a\t\tb");
        assert_eq!(g.shape, Shape { rows: 1, cols: 3 });
        assert_eq!(
            g.cells,
            vec![
                val(Value::Text("a".to_string())),
                val(Value::Blank),
                val(Value::Text("b".to_string())),
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
        let g = deserialize_tsv("A1", "=SUM(").expect("the file loads, the cell is the error");
        assert_eq!(g.shape, Shape { rows: 1, cols: 1 });
        match &g.cells[0] {
            Cell::LoadError { src, diag } => {
                assert_eq!(src, "=SUM(");
                assert_eq!(diag.code, Code::FormulaSyntax);
                assert!(
                    matches!(diag.loc, Loc::Body { line: 1, .. }),
                    "{:?}",
                    diag.loc
                );
                assert_eq!(load_error_value(diag), Value::Error(ErrKind::Name));
            }
            other => panic!("expected a load-error cell, got {other:?}"),
        }
    }

    #[test]
    fn an_unparseable_formula_does_not_abort_its_neighbours() {
        let g = deserialize_tsv("A1:C1", "1\t=SUM(\t=A1+1").expect("the file loads");
        assert_eq!(g.shape, Shape { rows: 1, cols: 3 });
        assert_eq!(g.cells[0], val(Value::Number(1.0)));
        assert!(matches!(&g.cells[1], Cell::LoadError { src, .. } if src == "=SUM("));
        assert!(matches!(&g.cells[2], Cell::Formula { src, .. } if src == "=A1+1"));
    }

    #[test]
    fn literal_lexing_covers_every_token_form() {
        assert_eq!(lex_literal(""), (Value::Blank, None));
        assert_eq!(lex_literal("123"), (Value::Number(123.0), None));
        assert_eq!(lex_literal("-4"), (Value::Number(-4.0), None));
        assert_eq!(lex_literal("2.5"), (Value::Number(2.5), None));
        assert_eq!(lex_literal("1.2e6"), (Value::Number(1_200_000.0), None));
        assert_eq!(lex_literal("TRUE"), (Value::Bool(true), None));
        assert_eq!(lex_literal("FALSE"), (Value::Bool(false), None));
        assert_eq!(lex_literal("#REF!"), (Value::Error(ErrKind::Ref), None));
        assert_eq!(lex_literal("#DIV/0!"), (Value::Error(ErrKind::Div0), None));
        assert_eq!(lex_literal("#N/A"), (Value::Error(ErrKind::Na), None));
        assert_eq!(
            lex_literal("hello"),
            (Value::Text("hello".to_string()), None)
        );
        assert_eq!(lex_literal("'123"), (Value::Text("123".to_string()), None));
        // A bare `"..."` is text WITH its quotes: there is no quoted-literal escaping layer.
        assert_eq!(lex_literal("a\tb"), (Value::Text("a\tb".to_string()), None));
        assert_eq!(lex_literal("a\\b"), (Value::Text("a\\b".to_string()), None));
        assert_eq!(
            lex_literal(r#""quoted""#),
            (Value::Text(r#""quoted""#.to_string()), None)
        );
        assert_eq!(lex_literal("inf"), (Value::Text("inf".to_string()), None));
        assert_eq!(lex_literal("NaN"), (Value::Text("NaN".to_string()), None));
        assert_eq!(
            lex_literal("#SPILL!"),
            (Value::Text("#SPILL!".to_string()), None)
        );
    }

    #[test]
    fn displayed_form_literals_recover_value_and_format() {
        assert_eq!(
            lex_literal("12.50"),
            (Value::Number(12.5), Some(Format::Fixed { decimals: 2 }))
        );
        assert_eq!(
            lex_literal("1,234.00"),
            (Value::Number(1234.0), Some(Format::Grouped { decimals: 2 }))
        );
        assert_eq!(
            lex_literal("12.50%"),
            (Value::Number(0.125), Some(Format::Percent { decimals: 2 }))
        );
        assert_eq!(
            lex_literal("$1,234.00"),
            (
                Value::Number(1234.0),
                Some(Format::Currency {
                    symbol: CurrencySymbol::Dollar,
                    grouping: true,
                    decimals: 2
                })
            )
        );
        assert_eq!(
            lex_literal("2021-05-15~m/d/yyyy"),
            (Value::Number(44331.0), Some(Format::Date(DatePattern::Mdy)))
        );
        assert_eq!(lex_literal("12"), (Value::Number(12.0), None));
        assert_eq!(lex_literal("2.5"), (Value::Number(2.5), None));
    }

    #[test]
    fn a_formula_carries_its_trailing_format_marker() {
        for (field, want_src, want_fmt) in [
            (
                "=SUM(B1:B5)~$#,##0.00",
                "=SUM(B1:B5)",
                Format::Currency {
                    symbol: CurrencySymbol::Dollar,
                    grouping: true,
                    decimals: 2,
                },
            ),
            ("=B1/B2~0.00%", "=B1/B2", Format::Percent { decimals: 2 }),
            (
                "=TODAY()~m/d/yyyy",
                "=TODAY()",
                Format::Date(DatePattern::Mdy),
            ),
        ] {
            let g = deserialize_tsv("A1", field).expect("loads");
            match &g.cells[0] {
                Cell::Formula { src, expr, format } => {
                    assert_eq!(src, want_src, "src is the marker-free formula");
                    assert_eq!(*expr, parse(want_src).unwrap());
                    assert_eq!(*format, Some(want_fmt));
                }
                other => panic!("expected a formatted formula, got {other:?}"),
            }
        }
        let g = deserialize_tsv("A1", "=SUM(B1:B5)").expect("loads");
        assert!(matches!(
            &g.cells[0],
            Cell::Formula { src, format: None, .. } if src == "=SUM(B1:B5)"
        ));
    }

    #[test]
    fn the_format_marker_split_is_adversarially_unambiguous() {
        // A `~` inside a string or sheet name leaves a `"`/`'`/`!` in the tail, which no code has.
        let g = deserialize_tsv("A1", r#"=A1&"~USD""#).expect("loads");
        assert!(matches!(
            &g.cells[0],
            Cell::Formula { src, format: None, .. } if src == r#"=A1&"~USD""#
        ));

        let g = deserialize_tsv("A1", r#"=A1&"end~0.00""#).expect("loads");
        assert!(matches!(
            &g.cells[0],
            Cell::Formula { src, format: None, .. } if src == r#"=A1&"end~0.00""#
        ));

        let g = deserialize_tsv("A1", "='Sheet~1'!A1").expect("loads");
        assert!(matches!(
            &g.cells[0],
            Cell::Formula { src, format: None, .. } if src == "='Sheet~1'!A1"
        ));

        // Here the `~` is OUTSIDE the string, so it splits.
        let g = deserialize_tsv("A1", r#"=A1&"x"~0.00%"#).expect("loads");
        assert!(matches!(
            &g.cells[0],
            Cell::Formula { src, format: Some(Format::Percent { decimals: 2 }), .. }
                if src == r#"=A1&"x""#
        ));

        let g = deserialize_tsv("A1", "=SUM(A1)~bogus").expect("the file loads");
        match &g.cells[0] {
            Cell::LoadError { src, diag } => {
                assert_eq!(src, "=SUM(A1)~bogus");
                assert_eq!(diag.code, Code::FormulaSyntax);
                assert_eq!(load_error_value(diag), Value::Error(ErrKind::Name));
            }
            other => panic!("expected a located load-error cell, got {other:?}"),
        }
    }

    #[test]
    fn an_array_formula_region_carries_its_format_uniformly() {
        // The marker split runs BEFORE any array handling.
        let g = deserialize_tsv("C1:C3", "=SORT(A1:A3)~0.00").expect("loads");
        assert_eq!(g.shape, Shape { rows: 1, cols: 1 });
        let plain = deserialize_tsv("C1:C3", "=SORT(A1:A3)").expect("loads");
        match (&g.cells[0], &plain.cells[0]) {
            (
                Cell::Formula { src, expr, format },
                Cell::Formula {
                    src: plain_src,
                    expr: plain_expr,
                    ..
                },
            ) => {
                assert_eq!(src, "=SORT(A1:A3)");
                assert_eq!(src, plain_src);
                assert_eq!(
                    expr, plain_expr,
                    "the parsed array expr is unchanged by the marker"
                );
                assert_eq!(*format, Some(Format::Fixed { decimals: 2 }));
            }
            other => panic!("expected two formula cells, got {other:?}"),
        }
    }

    #[test]
    fn encode_field_escapes_the_three_specials_uniformly() {
        assert_eq!(encode_field("plain"), "plain");
        assert_eq!(encode_field("a\tb"), "a\\tb");
        assert_eq!(encode_field("a\nb"), "a\\nb");
        assert_eq!(encode_field("a\\b"), "a\\\\b");
        // Escaped, not re-processed: a backslash+t as CONTENT becomes `\\t`.
        assert_eq!(encode_field("\\t"), "\\\\t");
        assert_eq!(encode_field("end\\"), "end\\\\");
    }

    #[test]
    fn encode_then_deserialize_round_trips_a_cells_exact_text() {
        for text in [
            "plain",
            "a\tb",
            "line1\nline2",
            "a\\b",
            "C:\\path\\file",
            "trailing\\",
            "\t\n\\",
            "mix\ta\nb\\c",
        ] {
            let field = encode_field(text);
            let g = deserialize_tsv("A1", &field).expect("encoded field deserializes");
            assert_eq!(g.shape, Shape { rows: 1, cols: 1 });
            assert_eq!(
                g.cells[0],
                val(Value::Text(text.to_string())),
                "text {text:?} encoded as {field:?} did not round-trip",
            );
        }
    }

    #[test]
    fn a_multi_line_cell_holds_its_newline_without_splitting_the_grid() {
        let g = deserialize_tsv("A1", "top\\nbottom").expect("loads");
        assert_eq!(g.shape, Shape { rows: 1, cols: 1 });
        assert_eq!(g.cells[0], val(Value::Text("top\nbottom".to_string())));
    }

    #[test]
    fn a_malformed_escape_is_a_located_grid6_error_cell() {
        let g = deserialize_tsv("A1", "a\\xb").expect("the file loads, the cell is the error");
        assert_eq!(g.shape, Shape { rows: 1, cols: 1 });
        match &g.cells[0] {
            Cell::LoadError { src, diag } => {
                assert_eq!(src, "a\\xb", "the raw source is kept for --functions");
                assert_eq!(diag.code, Code::MalformedEscape);
                // The backslash is field byte 1, so column 2 (1-based).
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
                assert_eq!(load_error_value(diag), Value::Error(ErrKind::Value));
            }
            other => panic!("expected a load-error cell, got {other:?}"),
        }
    }

    #[test]
    fn a_trailing_backslash_is_a_malformed_escape() {
        let g = deserialize_tsv("A1", "end\\").expect("the file loads");
        assert!(matches!(
            &g.cells[0],
            Cell::LoadError { diag, .. } if diag.code == Code::MalformedEscape
        ));
    }

    #[test]
    fn a_malformed_escape_does_not_abort_its_neighbours() {
        let g = deserialize_tsv("A1:C1", "1\thi\tbad\\z").expect("the file loads");
        assert_eq!(g.shape, Shape { rows: 1, cols: 3 });
        assert_eq!(g.cells[0], val(Value::Number(1.0)));
        assert_eq!(g.cells[1], val(Value::Text("hi".to_string())));
        match &g.cells[2] {
            Cell::LoadError { diag, .. } => {
                assert_eq!(diag.code, Code::MalformedEscape);
                // Third field starts at line byte 5, backslash at field byte 3: 5+3+1 = 9.
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
