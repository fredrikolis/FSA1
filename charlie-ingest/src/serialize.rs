// Concern: spell a format-neutral `SheetSource` as charlie grid FILE(S) — map each `SourceCell` to one grid-only TSV field (number→lossless literal that re-lexes bit-exact; date-serial→that same number; text→verbatim, force-text (`'`) or double-quote-escaped only when a bare spelling would re-lex as the wrong type or break the TSV grid; bool→TRUE/FALSE; error→its `#…!` literal; blank→empty field; formula→`=<translated>` via `translate`), and assemble the sheet's whole A1-anchored used rectangle as the DEFAULT single range file (`A1:<lastcol><lastrow>`, or the bare `A1` for a 1×1 sheet) whose grid fills the range exactly (GRID4), the content being pure grid with NO annotation/header line (GRID1) | Non-concern: reading the source (reader.rs), the OpenFormula→A1 rewrite itself (translate.rs), and WRITING files to disk (lib.rs owns the IO) | IO: (a `&SheetSource`) -> `Result<Vec<(filename, content)>, IngestError>`
//! Serialize a neutral sheet to charlie grid files: [`sheet_files`]. The per-cell spelling
//! ([`cell_field`]) is the inverse of charlie-model's TSV deserializer, verified by re-lexing.

use charlie_ast::a1::format_cell;
use charlie_ast::{ErrKind, Value};
use charlie_model::{display_value, lex_literal};

use crate::error::IngestError;
use crate::source::{SheetSource, SourceCell};
use crate::translate::translate_formula;

/// Turn one neutral sheet into its charlie grid file(s): the default mapping is the whole used
/// rectangle as ONE range file. An empty sheet yields no files (an empty tab folder). The filename is
/// the closed range `A1:<lastcol><lastrow>`, or the bare cell `A1` for a 1×1 sheet (a `1×1` range name
/// like `A1:A1` is illegal — filename.rs `DegenerateRange`).
pub fn sheet_files(sheet: &SheetSource) -> Result<Vec<(String, String)>, IngestError> {
    if sheet.is_empty() {
        return Ok(Vec::new());
    }
    let filename = if sheet.rows == 1 && sheet.cols == 1 {
        "A1".to_string()
    } else {
        format!("A1:{}", format_cell(sheet.cols - 1, sheet.rows - 1))
    };

    let mut lines = Vec::with_capacity(sheet.rows as usize);
    for row in 0..sheet.rows {
        let mut fields = Vec::with_capacity(sheet.cols as usize);
        for col in 0..sheet.cols {
            let cell = &sheet.cells[(row * sheet.cols + col) as usize];
            fields.push(cell_field(cell, &sheet.name, col, row)?);
        }
        lines.push(fields.join("\t"));
    }
    // Pure grid, no annotation/header line (GRID1); rows are newline-separated (the TSV deserializer).
    Ok(vec![(filename, lines.join("\n"))])
}

/// Spell one source cell as a single TSV field. `col`/`row` are zero-based (for the located refusal an
/// untranslatable formula or unrepresentable text raises).
pub fn cell_field(
    cell: &SourceCell,
    sheet: &str,
    col: u32,
    row: u32,
) -> Result<String, IngestError> {
    match cell {
        SourceCell::Blank => Ok(String::new()),
        // A lossless decimal that re-lexes to the SAME f64 (Rust's shortest round-trip `Display`), not
        // the General display format (which rounds to 15 sig-figs) — the on-disk literal must round-trip
        // bit-exact, and the render layer applies General formatting on the way out.
        SourceCell::Number(n) | SourceCell::DateSerial(n) => Ok(num_field(*n)),
        SourceCell::Bool(b) => Ok(if *b { "TRUE" } else { "FALSE" }.to_string()),
        // The error's `#…!` literal is spelled by the model's value formatter (the inverse of its error
        // lexer), so the two never drift.
        SourceCell::Error(k) => Ok(error_literal(*k)),
        SourceCell::Text(s) => text_field(s)
            .ok_or_else(|| IngestError::at_cell(sheet, format_cell(col, row), text_reason(s))),
        SourceCell::Formula(raw) => translate_formula(raw)
            .map_err(|reason| IngestError::at_cell(sheet, format_cell(col, row), reason)),
    }
}

/// A number as a lossless TSV literal — Rust's shortest `Display` round-trips through the model's
/// numeric lexer bit-for-bit. A calamine numeric/date cell is always finite, so no `inf`/`NaN` guard is
/// needed (and those would lex to text, not a number, anyway).
fn num_field(n: f64) -> String {
    n.to_string()
}

/// The `#…!` literal for an error value, via the model's single value-spelling home.
fn error_literal(k: ErrKind) -> String {
    display_value(&Value::Error(k))
}

/// Spell a text value as a TSV field that re-lexes to exactly `Text(s)`, or `None` when it cannot be
/// represented (an embedded newline — the TSV grid is newline-delimited, so a cell cannot contain one).
///
/// - A bare spelling is used when `s` re-lexes to `Text(s)` and holds no tab.
/// - A tab, or a leading `"`, forces the double-quote form (`"…"` with `\t`/`\"`/`\\` escapes).
/// - Otherwise a leading apostrophe force-texts a value that would else lex as a number/bool/error/
///   formula (`'123`, `'=A1`).
fn text_field(s: &str) -> Option<String> {
    if s.contains('\n') || s.contains('\r') {
        return None;
    }
    // A bare field is safe only if it re-lexes to the same text AND is not caught by a FIELD-level rule
    // the per-token `lex_literal` does not see: a field beginning with `=` is a FORMULA to the TSV
    // deserializer, so `=A1`-shaped text must be force-texted even though `lex_literal` calls it text.
    let lexes_bare = lex_literal(s) == Value::Text(s.to_string()) && !s.starts_with('=');
    if lexes_bare && !s.contains('\t') {
        return Some(s.to_string());
    }
    if s.contains('\t') || s.starts_with('"') {
        return Some(quote(s));
    }
    Some(format!("'{s}"))
}

/// The reason an embedded-newline text is refused (the only unrepresentable text case).
fn text_reason(_s: &str) -> String {
    "a text value contains a newline, which charlie's newline-delimited TSV grid cannot represent"
        .to_string()
}

/// Double-quote a text value, escaping `\`, `"`, and tab so the model's quoted-literal lexer
/// (`\t`/`\"`/`\\`) restores it exactly. (Newlines are rejected by the caller, so none reach here.)
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use charlie_model::{Cell, deserialize_tsv};

    /// Every spelled field must re-deserialize (through the model's FIELD deserializer, not just the
    /// per-token lexer) to exactly the text it came from — this catches field-level rules like a
    /// leading `=` becoming a formula.
    fn roundtrip_text(s: &str) {
        let field = text_field(s).unwrap_or_else(|| panic!("{s:?} should be representable"));
        let grid = deserialize_tsv("A1", &field).expect("a single field deserializes");
        assert_eq!(
            grid.cells[0],
            Cell::Value(Value::Text(s.to_string())),
            "{s:?} spelled as {field:?} did not re-deserialize to the same text",
        );
    }

    #[test]
    fn plain_text_is_bare() {
        assert_eq!(text_field("hello"), Some("hello".to_string()));
        roundtrip_text("hello");
    }

    #[test]
    fn ambiguous_text_is_force_texted() {
        // A value that would else lex as a number/bool/error/formula is apostrophe-forced.
        for s in ["123", "-4.5", "TRUE", "FALSE", "#REF!", "=A1", "'already"] {
            let f = text_field(s).unwrap();
            assert!(f.starts_with('\''), "{s:?} -> {f:?} should force-text");
            roundtrip_text(s);
        }
    }

    #[test]
    fn tabbed_or_quote_leading_text_is_quoted() {
        for s in ["a\tb", "\"quoted\"", "trailing\t"] {
            let f = text_field(s).unwrap();
            assert!(f.starts_with('"'), "{s:?} -> {f:?} should be double-quoted");
            roundtrip_text(s);
        }
    }

    #[test]
    fn empty_text_round_trips() {
        roundtrip_text("");
    }

    #[test]
    fn newline_text_is_unrepresentable() {
        assert_eq!(text_field("a\nb"), None);
    }

    #[test]
    fn numbers_are_lossless() {
        for n in [0.0, 30.0, -3.5, 45306.0, 1e20, 1e-9, 0.1 + 0.2] {
            let f = num_field(n);
            assert_eq!(lex_literal(&f), Value::Number(n), "{n} spelled {f:?}");
        }
    }

    #[test]
    fn errors_and_bools_spell_their_literals() {
        assert_eq!(error_literal(ErrKind::Div0), "#DIV/0!");
        assert_eq!(error_literal(ErrKind::Na), "#N/A");
        assert_eq!(
            cell_field(&SourceCell::Bool(true), "S", 0, 0).unwrap(),
            "TRUE"
        );
    }

    #[test]
    fn a_single_cell_sheet_is_named_a1_not_a1_a1() {
        let sheet = SheetSource {
            name: "S".to_string(),
            rows: 1,
            cols: 1,
            cells: vec![SourceCell::Number(7.0)],
        };
        let files = sheet_files(&sheet).unwrap();
        assert_eq!(files, vec![("A1".to_string(), "7".to_string())]);
    }

    #[test]
    fn a_rectangle_fills_its_range_exactly() {
        // 2x2 with a blank: A1=1, B1 blank, A2 formula, B2 text.
        let sheet = SheetSource {
            name: "S".to_string(),
            rows: 2,
            cols: 2,
            cells: vec![
                SourceCell::Number(1.0),
                SourceCell::Blank,
                SourceCell::Formula("of:=[.A1]+1".to_string()),
                SourceCell::Text("x".to_string()),
            ],
        };
        let files = sheet_files(&sheet).unwrap();
        assert_eq!(
            files,
            vec![("A1:B2".to_string(), "1\t\n=A1+1\tx".to_string())]
        );
    }
}
