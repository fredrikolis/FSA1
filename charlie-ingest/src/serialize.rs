// Concern: spell a format-neutral `SheetSource` as charlie grid FILES — ONE FILE PER NON-BLANK CELL (CORE3: a cell is its own file, so an agent edits a tiny per-cell file directly with no write command), named by that cell's A1 coordinate (`A1`, `H3`), whose content is that single cell spelled as one grid-only field: number→lossless literal that re-lexes bit-exact; date-serial→that same number; text→verbatim, apostrophe-forced (`'`) only when a bare spelling would re-lex as the wrong type or start a formula; bool→TRUE/FALSE; error→its `#…!` literal; formula→`=<translated>` via `translate` (preserved VERBATIM even when untranslatable, so a single unsupported formula becomes a per-cell GRID6 error at load rather than aborting the import). EVERY field is then run through charlie-model's `encode_field` (backslash/tab/newline → the escapes `\\`/`\t`/`\n`), so a cell containing a tab, newline, or backslash writes losslessly and `import -> deserialize` round-trips its exact text — an embedded newline no longer makes a cell unrepresentable. A BLANK source cell produces NO file (a gap reads blank). Each file's content is pure grid with NO annotation/header line and is a 1×1 grid filling its bare-`A1` range exactly (GRID1/GRID4) | Non-concern: reading the source (reader.rs), the OpenFormula→A1 rewrite itself (translate.rs), the FIELD-ESCAPE alphabet (charlie-model owns `encode_field` and the deserializer's decode), and WRITING files to disk (lib.rs owns the IO) | IO: (a `&SheetSource`) -> `Result<Vec<(filename, content)>, IngestError>`
//! Serialize a neutral sheet to charlie grid files: [`sheet_files`] emits one file per non-blank cell,
//! named by its A1 coordinate (CORE3). The per-cell spelling ([`cell_field`]) is the logical inverse of
//! charlie-model's TSV literal grammar; charlie-model's [`encode_field`](charlie_model::encode_field)
//! then applies the field escaping uniformly, so the on-disk field is the exact inverse of the
//! split-then-decode deserializer (verified by re-deserializing).

use charlie_ast::a1::format_cell;
use charlie_ast::{ErrKind, Value};
use charlie_model::{display_value, encode_field, lex_literal};

use crate::error::IngestError;
use crate::resolve::Resolution;
use crate::source::{SheetSource, SourceCell};
use crate::translate::translate_formula_ctx;

/// Turn one neutral sheet into its charlie grid files: ONE FILE PER NON-BLANK CELL, named by that
/// cell's A1 coordinate (`A1`, `H3`, `D2`), whose content is the single cell's literal or `=formula`
/// (CORE3: a cell is its own file, edited directly). A BLANK source cell produces NO file — a gap
/// reads blank. An empty sheet yields no files (an empty tab folder). Row-major order, so the returned
/// files are in reading order. Each file is a 1×1 grid filling its bare-`A1` range exactly (GRID4).
pub fn sheet_files(
    sheet: &SheetSource,
    res: &Resolution,
) -> Result<Vec<(String, String)>, IngestError> {
    let mut files = Vec::new();
    for row in 0..sheet.rows {
        for col in 0..sheet.cols {
            let cell = &sheet.cells[(row * sheet.cols + col) as usize];
            // A blank cell has no file (a gap reads blank) — this is what makes the on-disk layout
            // sparse and each remaining file a single editable cell.
            if matches!(cell, SourceCell::Blank) {
                continue;
            }
            // Spell the cell's logical field, then apply the UNIFORM field escaping (backslash/tab/
            // newline -> `\\`/`\t`/`\n`) so the on-disk field is the exact inverse of the deserializer's
            // split-then-decode — a tab/newline/backslash in any cell round-trips losslessly. A formula
            // is translated + reference-resolved against `res` for THIS cell's sheet + 0-based row (the
            // relative `Table[@Col]`/`[#This Row]` forms need the row).
            let content = encode_field(&cell_field(cell, res, &sheet.name, row));
            files.push((format_cell(col, row), content));
        }
    }
    Ok(files)
}

/// Spell one source cell as its LOGICAL TSV field (before the uniform field escaping the caller applies
/// via [`encode_field`]). Infallible: every cell — including any text — is representable, so an embedded
/// tab/newline/backslash is no longer a refusal (the field escaping carries it losslessly).
fn cell_field(cell: &SourceCell, res: &Resolution, sheet: &str, row: u32) -> String {
    match cell {
        SourceCell::Blank => String::new(),
        // A lossless decimal that re-lexes to the SAME f64 (Rust's shortest round-trip `Display`), not
        // the General display format (which rounds to 15 sig-figs) — the on-disk literal must round-trip
        // bit-exact, and the render layer applies General formatting on the way out.
        SourceCell::Number(n) | SourceCell::DateSerial(n) => num_field(*n),
        SourceCell::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        // The error's `#…!` literal is spelled by the model's value formatter (the inverse of its error
        // lexer), so the two never drift.
        SourceCell::Error(k) => error_literal(*k),
        SourceCell::Text(s) => text_field(s),
        // A formula is translated to charlie's Excel-A1 grammar AND reference-resolved (defined names +
        // `Table[…]` structured refs) against `res` for this cell's `sheet`+`row`, preserved VERBATIM
        // when untranslatable/unresolvable (translate never fails now — GRID6 flags such a cell at load,
        // not here), so a single unsupported formula never aborts the import.
        SourceCell::Formula(raw) => translate_formula_ctx(raw, res, sheet, row),
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

/// Spell a text value as its LOGICAL TSV field — one that re-lexes to exactly `Text(s)` once the
/// deserializer has resolved the field escaping ([`encode_field`], applied by the caller). Because the
/// field escaping carries any tab/newline/backslash losslessly, this only disambiguates a bare spelling
/// from another value type: a value that would else lex as a number/bool/error/blank, or start a
/// formula (`=`) or force-text (`'`), gets a leading apostrophe (`'123`, `'=A1`). Every text is
/// representable.
fn text_field(s: &str) -> String {
    // A bare field is safe only if it re-lexes to the same text AND is not caught by a FIELD-level rule
    // the per-token `lex_literal` does not see: a field beginning with `=` is a FORMULA to the TSV
    // deserializer, so `=A1`-shaped text must be force-texted even though `lex_literal` calls it text.
    // `lex_literal(s)` sees exactly the decoded text the deserializer will (encode then decode == s).
    let lexes_bare = lex_literal(s) == Value::Text(s.to_string()) && !s.starts_with('=');
    if lexes_bare {
        s.to_string()
    } else {
        format!("'{s}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use charlie_model::{Cell, deserialize_tsv};

    /// Every spelled field must re-deserialize (through the model's FIELD deserializer — the same
    /// split-then-decode a real load runs, AFTER the uniform `encode_field`) to exactly the text it came
    /// from. This catches both field-level rules (a leading `=` becoming a formula) and the escaping
    /// round trip (a tab/newline/backslash surviving encode -> decode).
    fn roundtrip_text(s: &str) {
        let field = encode_field(&text_field(s));
        let grid = deserialize_tsv("A1", &field).expect("a single field deserializes");
        assert_eq!(
            grid.cells[0],
            Cell::Value(Value::Text(s.to_string())),
            "{s:?} spelled as {field:?} did not re-deserialize to the same text",
        );
    }

    #[test]
    fn plain_text_is_bare() {
        assert_eq!(text_field("hello"), "hello".to_string());
        roundtrip_text("hello");
    }

    #[test]
    fn ambiguous_text_is_force_texted() {
        // A value that would else lex as a number/bool/error/formula is apostrophe-forced.
        for s in [
            "123", "-4.5", "TRUE", "FALSE", "#REF!", "=A1", "'already", "",
        ] {
            let f = text_field(s);
            assert!(f.starts_with('\''), "{s:?} -> {f:?} should force-text");
            roundtrip_text(s);
        }
    }

    #[test]
    fn tab_quote_and_backslash_text_round_trip_via_field_escaping() {
        // A tab, a leading/embedded quote, and a backslash are all now carried by the uniform field
        // escaping (no double-quote literal form): the LOGICAL field is bare (or apostrophe-forced), and
        // `encode_field` spells the specials. Each must round-trip to its exact text.
        for s in [
            "a\tb",
            "\"quoted\"",
            "trailing\t",
            "a\\b",
            "C:\\path",
            "end\\",
        ] {
            roundtrip_text(s);
        }
    }

    #[test]
    fn empty_text_round_trips() {
        roundtrip_text("");
    }

    #[test]
    fn newline_text_round_trips_as_a_multi_line_cell() {
        // The motivating case: an embedded newline is representable now — it writes `\n` and the cell
        // deserializes back to the exact multi-line text (no longer a refusal).
        roundtrip_text("line1\nline2");
        assert_eq!(text_field("line1\nline2"), "line1\nline2".to_string());
        assert_eq!(encode_field(&text_field("line1\nline2")), "line1\\nline2");
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
            cell_field(&SourceCell::Bool(true), &Resolution::empty(), "S", 0),
            "TRUE"
        );
    }

    #[test]
    fn a_single_cell_sheet_is_one_a1_file() {
        // CORE3: the one non-blank cell becomes its own file named `A1` (a bare cell, never `A1:A1`).
        let sheet = SheetSource {
            name: "S".to_string(),
            rows: 1,
            cols: 1,
            cells: vec![SourceCell::Number(7.0)],
        };
        let files = sheet_files(&sheet, &Resolution::empty()).unwrap();
        assert_eq!(files, vec![("A1".to_string(), "7".to_string())]);
    }

    #[test]
    fn each_non_blank_cell_is_its_own_a1_named_file_and_a_blank_makes_no_file() {
        // CORE3: 2x2 with A1=1, B1 blank, A2 formula, B2 text -> THREE per-cell files (B1 has none),
        // each named by its A1 coordinate in row-major reading order.
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
        let files = sheet_files(&sheet, &Resolution::empty()).unwrap();
        assert_eq!(
            files,
            vec![
                ("A1".to_string(), "1".to_string()),
                ("A2".to_string(), "=A1+1".to_string()),
                ("B2".to_string(), "x".to_string()),
            ]
        );
    }

    #[test]
    fn an_untranslatable_formula_is_preserved_verbatim_as_a_per_cell_file_grid6() {
        // GRID6/CORE3: an untranslatable formula (a 3-D range) still gets its own file, preserved
        // verbatim as `=<body>` — the import succeeds; charlie's loader flags the cell at load.
        let sheet = SheetSource {
            name: "S".to_string(),
            rows: 1,
            cols: 1,
            cells: vec![SourceCell::Formula("of:=[Sheet1.A1:Sheet2.B2]".to_string())],
        };
        let files = sheet_files(&sheet, &Resolution::empty()).unwrap();
        assert_eq!(
            files,
            vec![("A1".to_string(), "=[Sheet1.A1:Sheet2.B2]".to_string())]
        );
    }

    #[test]
    fn a_cell_with_a_newline_tab_or_backslash_is_written_escaped_and_round_trips() {
        // The end-to-end serialize side of the motivating case: a cell holding a newline (or tab, or
        // backslash) writes its ESCAPED field and re-deserializes to the exact same text.
        let sheet = SheetSource {
            name: "S".to_string(),
            rows: 1,
            cols: 3,
            cells: vec![
                SourceCell::Text("line1\nline2".to_string()),
                SourceCell::Text("a\tb".to_string()),
                SourceCell::Text("C:\\dir".to_string()),
            ],
        };
        let files = sheet_files(&sheet, &Resolution::empty()).unwrap();
        assert_eq!(
            files,
            vec![
                ("A1".to_string(), "line1\\nline2".to_string()),
                ("B1".to_string(), "a\\tb".to_string()),
                ("C1".to_string(), "C:\\\\dir".to_string()),
            ]
        );
        // Each written field deserializes back to its exact source text (import -> deserialize round trip).
        for (want, (_name, field)) in ["line1\nline2", "a\tb", "C:\\dir"].iter().zip(files.iter()) {
            let grid = deserialize_tsv("A1", field).expect("loads");
            assert_eq!(grid.cells[0], Cell::Value(Value::Text(want.to_string())));
        }
    }
}
