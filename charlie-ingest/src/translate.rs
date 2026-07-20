// Concern: translate a source formula string (ODS/OpenFormula OR xlsx Excel-A1) into charlie's Excel-A1 grammar AND resolve its references at import (CORE3) so the engine only ever sees A1 — strip the `of:=`/`of:`/`=` lead; rewrite ODS bracketed references `[.A1]`→`A1`, `[.A1:.A3]`→`A1:A3`, `[Sheet2.A1]`→`Sheet2!A1`, `['My Sheet'.A1]`→`'My Sheet'!A1` (preserving `$`-anchors); map the ODS `;` argument separator to `,` (only outside string literals); keep Excel-compatible function names as written EXCEPT the niladic booleans `TRUE()`/`FALSE()`, normalized to charlie's `TRUE`/`FALSE` literals; and — via the workbook `Resolution` — replace a DEFINED-NAME token with its A1 target and a `Table[…]` STRUCTURED reference with its A1 range, guarding TOKEN BOUNDARIES (never inside a string literal, never a function name it precedes with `(`, never the tail after a `!` sheet qualifier, only a whole identifier — so `Days` never matches inside `Calendar1Year`); a name/table/column/region that does not resolve is left VERBATIM (HARD RULE 5 — it loads as a located GRID6 `#NAME?`, never a silently-wrong range); anything else UNTRANSLATABLE (a 3-D range, an inline array `{…}`, a malformed reference, an unterminated string) is preserved VERBATIM as `=<source body>` so the import always succeeds | Non-concern: whether the translated formula PARSES/EVALUATES in charlie (charlie-ast owns that; an untranslatable/unsupported formula surfaces as a load-time GRID6 error cell downstream), READING the name/table metadata (reader.rs + xlsx_meta.rs), and the resolution LOGIC/geometry itself (resolve.rs owns the name map + structured-ref A1 math) | IO: (a raw formula `&str`, the workbook `Resolution`, the formula cell's sheet + 0-based row) -> `String` (the `=…` charlie formula, best-effort verbatim on an untranslatable/unresolvable construct)
//! Source formula → charlie Excel-A1 translation + import-time reference resolution:
//! [`translate_formula_ctx`] (context-free translation passes an empty [`Resolution`]). The returned
//! string always includes the leading `=`; an untranslatable/unresolvable construct is preserved
//! verbatim (GRID6).

use std::iter::Peekable;
use std::str::CharIndices;

use charlie_ast::a1::parse_a1;

use crate::resolve::Resolution;

/// Translate a raw source-dialect formula into a charlie `=formula` (always with the leading `=`),
/// resolving defined names and `Table[…]` structured references against `res` (a name/table target for
/// the formula's own `sheet` + 0-based `row`, which the relative `@`/`#This Row` forms need). A
/// successful rewrite yields the resolved Excel-A1 form; an UNTRANSLATABLE construct (a 3-D range, an
/// inline array, a malformed reference, an unterminated string) is preserved VERBATIM as `=<source body>`
/// rather than refused — the import still succeeds and charlie's deserializer flags such a cell as a
/// located GRID6 error (`--functions` shows the raw text; `check` reports it), so an agent sees exactly
/// what charlie could not resolve. A name/table token that simply does not resolve is likewise left as
/// written (a located `#NAME?` at load, never a silently-wrong range — HARD RULE 5).
pub fn translate_formula_ctx(raw: &str, res: &Resolution, sheet: &str, row: u32) -> String {
    let body = strip_lead(raw.trim());
    match rewrite_body(body, res, sheet, row) {
        Ok(translated) => format!("={translated}"),
        // GRID6: keep the source body verbatim so a single untranslatable/unsupported formula no longer
        // aborts the whole import — it becomes a per-cell located error at load instead.
        Err(_) => format!("={body}"),
    }
}

/// Strip the OpenFormula lead: `of:=` (LibreOffice's namespaced form), or a bare `of:`, or a bare `=`.
fn strip_lead(s: &str) -> &str {
    if let Some(rest) = s.strip_prefix("of:=") {
        rest
    } else if let Some(rest) = s.strip_prefix("of:") {
        rest
    } else if let Some(rest) = s.strip_prefix('=') {
        rest
    } else {
        s
    }
}

/// Scan the formula body, rewriting bracketed ODS references and `;`→`,`, resolving defined-name and
/// `Table[…]` structured-reference tokens (via `res`, for the formula's `sheet`+`row`), while leaving
/// string literals verbatim. An inline array `{…}` is refused (ODS array syntax differs from charlie's
/// and a naive `;`→`,` would silently transpose it).
fn rewrite_body(body: &str, res: &Resolution, sheet: &str, row: u32) -> Result<String, String> {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        match c {
            '"' => {
                // A string literal: copy verbatim through the closing quote, honouring the `""`
                // escape (a doubled quote is a literal quote, not the terminator — Excel and
                // OpenFormula share this convention, so it passes straight through).
                out.push('"');
                loop {
                    match chars.next() {
                        Some((_, '"')) => {
                            out.push('"');
                            if matches!(chars.peek(), Some((_, '"'))) {
                                let (_, q) = chars.next().expect("peeked");
                                out.push(q);
                            } else {
                                break;
                            }
                        }
                        Some((_, other)) => out.push(other),
                        None => return Err("unterminated string literal in formula".to_string()),
                    }
                }
            }
            '\'' => {
                // A single-quoted sheet name in the Excel-A1 dialect (`'Annual Report'!A1`). Copy it
                // ATOMICALLY through the closing quote — honouring the `''` escape (a doubled quote is a
                // literal quote inside the name, Excel's convention) — so its interior words are never
                // walked as bare identifiers and mistaken for defined names. Without this arm a name that
                // collides with a word inside a quoted sheet name would be substituted INTO a cross-sheet
                // reference, silently corrupting it (HARD RULE 5). (ODS quotes its sheet names INSIDE the
                // `[…]` bracket, handled by the `[` arm; a top-level `'…'` is only the xlsx sheet form.)
                out.push('\'');
                loop {
                    match chars.next() {
                        Some((_, '\'')) => {
                            out.push('\'');
                            if matches!(chars.peek(), Some((_, '\''))) {
                                let (_, q) = chars.next().expect("peeked");
                                out.push(q);
                            } else {
                                break;
                            }
                        }
                        Some((_, other)) => out.push(other),
                        None => {
                            return Err("unterminated quoted sheet name in formula".to_string());
                        }
                    }
                }
            }
            '[' => {
                let mut inner = String::new();
                loop {
                    match chars.next() {
                        Some((_, ']')) => break,
                        Some((_, other)) => inner.push(other),
                        None => return Err("unterminated `[reference]` in formula".to_string()),
                    }
                }
                out.push_str(&rewrite_reference(&inner)?);
            }
            ';' => out.push(','),
            '{' | '}' => {
                return Err("an inline array `{…}` is not translatable from ODS".to_string());
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                // An identifier: a function name, a defined name, or a table name preceding a `[…]`
                // structured reference. Accumulate the WHOLE token (so `Days` never matches inside
                // `Calendar1Year`), then decide by what follows and by the resolution context.
                let mut ident = String::new();
                ident.push(c);
                while let Some((_, nc)) = chars.peek() {
                    if nc.is_ascii_alphanumeric() || *nc == '_' || *nc == '.' {
                        ident.push(*nc);
                        chars.next();
                    } else {
                        break;
                    }
                }
                // A token right after a `!` is the tail of a sheet-qualified reference, never a name/table.
                let after_bang = out.trim_end().ends_with('!');
                let next = chars.peek().map(|&(_, c)| c);
                if next == Some('!') {
                    // This identifier is the LHS SHEET QUALIFIER of a reference (`Data!A1`): the `!` and
                    // its address follow. It is a sheet name, never a defined name — push it verbatim so a
                    // defined name that collides with a sheet name cannot be substituted INTO the qualifier
                    // and silently corrupt a previously-correct cross-sheet formula (HARD RULE 5). The
                    // `!` (and the RHS, guarded by `after_bang`) flow on through the loop unchanged.
                    out.push_str(&ident);
                } else if next == Some('[') && !after_bang && res.is_table(&ident) {
                    // A `Table[…]` structured reference: consume the balanced `[…]` group and resolve it
                    // to A1; if it does not resolve, keep the token verbatim (a located #NAME? at load).
                    let inner = consume_bracket_group(&mut chars)?;
                    match res.resolve_structured(&ident, &inner, sheet, row) {
                        Some(a1) => out.push_str(&a1),
                        None => {
                            out.push_str(&ident);
                            out.push('[');
                            out.push_str(&inner);
                            out.push(']');
                        }
                    }
                } else if next == Some('(') {
                    // A function call. Normalize OpenFormula's niladic booleans `TRUE()`/`FALSE()` —
                    // which charlie models as LITERALS — into the bare `TRUE`/`FALSE`; every other
                    // function name is Excel-compatible and kept verbatim (the `(` flows to the loop).
                    out.push_str(&ident);
                    if ident == "TRUE" || ident == "FALSE" {
                        chars.next(); // consume '('
                        if matches!(chars.peek(), Some((_, ')'))) {
                            chars.next(); // a niladic call becomes the bare literal
                        } else {
                            out.push('('); // `TRUE(<args>)` is not niladic; keep the `(`
                        }
                    }
                } else {
                    // A bare identifier: a defined NAME or a plain word — kept VERBATIM. Defined names
                    // are no longer inlined at import (HARD RULE 2); they are emitted as on-disk FS4
                    // entries and resolved at LOAD (an unknown name token loads as a located `#NAME?`).
                    // `after_bang` (the tail after a `!`) is likewise verbatim, so a name colliding with
                    // a sheet qualifier is never mangled (the `[`-table branch above still uses it).
                    out.push_str(&ident);
                }
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

/// Consume a balanced `[…]` structured-reference group (the iterator is positioned AT the opening `[`),
/// returning the inner text (nested brackets preserved, outer stripped). An unbalanced group is an
/// error, so the whole formula is kept verbatim (a located GRID6 error at load).
fn consume_bracket_group(chars: &mut Peekable<CharIndices<'_>>) -> Result<String, String> {
    match chars.next() {
        Some((_, '[')) => {}
        _ => return Err("expected `[` opening a structured reference".to_string()),
    }
    let mut depth = 1;
    let mut inner = String::new();
    for (_, c) in chars.by_ref() {
        match c {
            '[' => {
                depth += 1;
                inner.push('[');
            }
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(inner);
                }
                inner.push(']');
            }
            other => inner.push(other),
        }
    }
    Err("unterminated structured reference".to_string())
}

/// Rewrite the interior of one `[...]` reference token into charlie A1: a single ref (`.A1`,
/// `Sheet2.A1`, `'My Sheet'.A1`) or a range (`.A1:.A3`, `Sheet2.A1:.A3`). `$`-anchors and column case
/// are preserved verbatim; the sheet qualifier is emitted as `Sheet!`/`'My Sheet'!`.
fn rewrite_reference(inner: &str) -> Result<String, String> {
    match inner.split_once(':') {
        None => {
            let (sheet, addr) = split_sheet_addr(inner)?;
            Ok(format!("{}{}", sheet_prefix(&sheet), addr))
        }
        Some((left, right)) => {
            let (lsheet, laddr) = split_sheet_addr(left)?;
            let (rsheet, raddr) = split_sheet_addr(right)?;
            // charlie's range grammar qualifies the WHOLE range with one sheet (`Sheet1!A1:A3`); a
            // per-endpoint 3-D range (`Sheet1.A1:Sheet2.A3`) is reserved, so a differing right-hand
            // sheet is a located refusal rather than a dropped qualifier.
            if !rsheet.is_empty() && rsheet != lsheet {
                return Err(format!(
                    "a 3-D range across sheets ({lsheet:?}..{rsheet:?}) is not supported"
                ));
            }
            Ok(format!("{}{}:{}", sheet_prefix(&lsheet), laddr, raddr))
        }
    }
}

/// Split one OpenFormula reference endpoint into `(sheet, address)`. The sheet is everything before the
/// `.` that separates it from the cell; it is empty for a same-sheet `.A1`, a bare name for `Sheet2.A1`,
/// or a quoted `'My Sheet'` (with `''` escapes) for a name with spaces. The address is validated as an
/// A1 address (via the shared `parse_a1`) so garbage is refused, but its raw text (`$`-anchors, case) is
/// returned unchanged for charlie to parse.
fn split_sheet_addr(part: &str) -> Result<(String, &str), String> {
    let part = part.trim();
    if let Some(rest) = part.strip_prefix('\'') {
        // A quoted sheet name: find the closing quote, treating `''` as an escaped quote inside it.
        let bytes = rest.as_bytes();
        let mut i = 0;
        loop {
            match bytes.get(i) {
                Some(b'\'') => {
                    if bytes.get(i + 1) == Some(&b'\'') {
                        i += 2;
                    } else {
                        break;
                    }
                }
                Some(_) => i += 1,
                None => return Err(format!("unterminated quoted sheet name in {part:?}")),
            }
        }
        let sheet = &rest[..i];
        let after = &rest[i + 1..]; // past the closing quote
        let addr = after
            .strip_prefix('.')
            .ok_or_else(|| format!("expected `.` after sheet name in {part:?}"))?;
        validate_addr(addr)?;
        Ok((format!("'{sheet}'"), addr))
    } else {
        let (sheet, addr) = match part.split_once('.') {
            Some((s, a)) => (s.to_string(), a),
            None => return Err(format!("reference {part:?} has no `.cell` component")),
        };
        validate_addr(addr)?;
        Ok((sheet, addr))
    }
}

/// The `Sheet!` / `'My Sheet'!` prefix for a (possibly empty) sheet qualifier. A quoted name keeps its
/// quotes (charlie's lexer reads `'…'!` for a name with spaces); an empty qualifier is same-sheet.
fn sheet_prefix(sheet: &str) -> String {
    if sheet.is_empty() {
        String::new()
    } else {
        format!("{sheet}!")
    }
}

/// Validate that `addr` is a well-formed A1 address (optionally `$`-anchored). A whole-column (`A`),
/// whole-row (`1`), or otherwise malformed token is refused — never passed through to become a silent
/// charlie parse failure with a worse location.
fn validate_addr(addr: &str) -> Result<(), String> {
    parse_a1(addr)
        .map(|_| ())
        .map_err(|e| format!("malformed cell address {addr:?}: {e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Context-free translation (empty resolution) — the ODS/xlsx grammar paths that carry no names.
    fn ok(raw: &str) -> String {
        translate_formula_ctx(raw, &Resolution::empty(), "", 0)
    }

    #[test]
    fn strips_the_of_lead_and_keeps_the_equals() {
        assert_eq!(ok("of:=1+2"), "=1+2");
        assert_eq!(ok("=1+2"), "=1+2");
        assert_eq!(ok("of:1+2"), "=1+2");
    }

    #[test]
    fn rewrites_same_sheet_refs_and_ranges() {
        assert_eq!(ok("of:=[.A3]*2"), "=A3*2");
        assert_eq!(ok("of:=SUM([.A1:.A2])"), "=SUM(A1:A2)");
    }

    #[test]
    fn rewrites_cross_sheet_refs() {
        assert_eq!(ok("of:=[Sheet1.A3]"), "=Sheet1!A3");
        assert_eq!(ok("of:=SUM([Sheet1.A1:.A2])"), "=SUM(Sheet1!A1:A2)");
        assert_eq!(ok("of:=['My Sheet'.A3]"), "='My Sheet'!A3");
    }

    #[test]
    fn normalizes_niladic_boolean_calls_to_literals() {
        // OpenFormula writes TRUE()/FALSE() as niladic calls; charlie models them as literals.
        assert_eq!(ok("of:=FALSE()"), "=FALSE");
        assert_eq!(ok("of:=IF([.A1];TRUE();FALSE())"), "=IF(A1,TRUE,FALSE)");
        // A bare TRUE/FALSE (no parens) is left as the literal it already is.
        assert_eq!(ok("of:=IF([.A1];TRUE;FALSE)"), "=IF(A1,TRUE,FALSE)");
        // A `;` inside a string is preserved; a niladic call elsewhere still normalizes.
        assert_eq!(
            ok(r#"of:=CONCAT("a;b";FALSE())"#),
            r#"=CONCAT("a;b",FALSE)"#
        );
    }

    #[test]
    fn maps_semicolons_to_commas_outside_strings() {
        assert_eq!(
            ok(r#"of:=VLOOKUP("banana";[.A1:.B3];2;FALSE())"#),
            r#"=VLOOKUP("banana",A1:B3,2,FALSE)"#
        );
        assert_eq!(
            ok(r#"of:=IF([.B1]>0;"pos";"neg")"#),
            r#"=IF(B1>0,"pos","neg")"#
        );
        // A `;` INSIDE a string literal is not an argument separator and must be preserved.
        assert_eq!(ok(r#"of:=CONCAT("a;b";"c")"#), r#"=CONCAT("a;b","c")"#);
    }

    #[test]
    fn preserves_dollar_anchors_and_case() {
        assert_eq!(ok("of:=[.$A$1]"), "=$A$1");
        assert_eq!(ok("of:=[.A$1:.$B2]"), "=A$1:$B2");
    }

    #[test]
    fn quoted_string_with_escaped_quote_passes_through() {
        assert_eq!(
            ok(r#"of:=CONCAT("say ""hi""";[.A1])"#),
            r#"=CONCAT("say ""hi""",A1)"#
        );
    }

    #[test]
    fn untranslatable_constructs_are_preserved_verbatim_for_grid6_not_refused() {
        // GRID6: an untranslatable construct is kept as `=<source body>` (lead stripped), never a
        // refusal — the import succeeds and charlie's loader flags the cell as a located error. The
        // preserved text is exactly the source body so an agent sees what to fix in `--functions`.
        assert_eq!(ok("of:=[Sheet1.A1:Sheet2.B2]"), "=[Sheet1.A1:Sheet2.B2]"); // 3-D range
        assert_eq!(ok("of:={1;2;3}"), "={1;2;3}"); // inline array
        assert_eq!(ok("of:=[.A1"), "=[.A1"); // unterminated bracket
        assert_eq!(ok(r#"of:=CONCAT("x"#), r#"=CONCAT("x"#); // unterminated string
        assert_eq!(ok("of:=[.ZZ]"), "=[.ZZ]"); // no row -> malformed address
        assert_eq!(ok("of:=[.99]"), "=[.99]"); // no column -> malformed address
    }

    /// A small workbook resolution: a `Sales` table (A1:C4, cols Region/Q1/Q2) on `Data`. Defined NAMES
    /// are no longer resolved by `translate` (they are emitted as FS4 entries and resolved at load), so
    /// the resolution carries only the TABLE geometry.
    fn ctx() -> Resolution {
        let mut r = Resolution::empty();
        r.add_table(
            "Sales",
            "Data",
            vec!["Region".into(), "Q1".into(), "Q2".into()],
            "A1:C4",
            1,
            0,
        );
        r
    }

    fn tr(raw: &str, row: u32) -> String {
        translate_formula_ctx(raw, &ctx(), "Data", row)
    }

    #[test]
    fn resolves_structured_refs_to_a1_but_leaves_names_verbatim() {
        // A defined name is LEFT VERBATIM (resolved at load, FS4); a structured column -> the data body;
        // @Col -> this row.
        assert_eq!(tr("TaxRate*100", 0), "=TaxRate*100");
        assert_eq!(tr("SUM(Sales[Q1])", 0), "=SUM(B2:B4)");
        assert_eq!(
            tr("SUM(Sales[[#Headers],[Amount]])", 0),
            "=SUM(Sales[[#Headers],[Amount]])"
        ); // bad col -> verbatim
        assert_eq!(tr("Sales[[#Headers],[Q1]]", 0), "=B1");
        // @Col resolves against the FORMULA's own 0-based row (row 2 -> B3).
        assert_eq!(tr("Sales[@Q1]", 2), "=B3");
        // @Col authored OUTSIDE the data band (row 0 is the header row; row 49 is far below) does NOT
        // resolve to a stray cell — it stays verbatim -> a located #NAME? (HARD RULE 5).
        assert_eq!(tr("Sales[@Q1]", 0), "=Sales[@Q1]");
        assert_eq!(tr("Sales[@Q1]", 49), "=Sales[@Q1]");
        // The 58296 shape: SUMIFS over structured refs becomes a pure-A1 SUMIFS.
        assert_eq!(
            tr(r#"SUMIFS(Sales[Q2],Sales[Q1],">="&E4)"#, 0),
            r#"=SUMIFS(C2:C4,B2:B4,">="&E4)"#
        );
    }

    #[test]
    fn token_boundaries_are_respected() {
        // A name is matched only as a WHOLE token — never as a substring of a longer identifier, and
        // never inside a string literal.
        assert_eq!(tr("TaxRateExtra+1", 0), "=TaxRateExtra+1"); // longer ident, not the name
        assert_eq!(
            tr(r#"IF(A1,"TaxRate","Sales[Q1]")"#, 0),
            r#"=IF(A1,"TaxRate","Sales[Q1]")"#
        );
        // A function name is not treated as a defined name (it is followed by `(`).
        assert_eq!(tr("SUM(A1:A2)", 0), "=SUM(A1:A2)");
        // An unknown name/table is left verbatim (loads as a located #NAME?, never silently wrong).
        assert_eq!(tr("Unknown+1", 0), "=Unknown+1");
        assert_eq!(tr("Other[Col]", 0), "=Other[Col]");
        // A plain cell ref is untouched (E4 is not a name).
        assert_eq!(tr("E4+1", 0), "=E4+1");
    }

    #[test]
    fn a_sheet_qualified_reference_passes_through_unchanged() {
        // A cross-sheet reference is copied verbatim (translate no longer substitutes names, so neither
        // the `Sheet!` qualifier nor the address after the `!` is ever rewritten).
        let r = Resolution::empty();
        let tr = |raw: &str| translate_formula_ctx(raw, &r, "Sheet1", 0);
        assert_eq!(tr("Data!A1"), "=Data!A1");
        assert_eq!(tr("SUM(Data!A1:A3)"), "=SUM(Data!A1:A3)");
        assert_eq!(tr("Data!Data"), "=Data!Data");
    }

    #[test]
    fn a_quoted_sheet_name_is_copied_atomically() {
        // A single-quoted sheet name is copied through the closing quote (its interior words are never
        // walked as bare identifiers), honouring the `''` escape.
        let r = Resolution::empty();
        let tr = |raw: &str| translate_formula_ctx(raw, &r, "Sheet1", 0);
        assert_eq!(tr("'Annual Report'!A1"), "='Annual Report'!A1");
        assert_eq!(
            tr("SUM('Annual Report'!A1:A3)"),
            "=SUM('Annual Report'!A1:A3)"
        );
        assert_eq!(tr("'It''s Data'!A1"), "='It''s Data'!A1");
        // An unterminated quoted sheet name is untranslatable -> kept verbatim as `=<body>` (GRID6).
        assert_eq!(tr("'Annual Report!A1"), "='Annual Report!A1");
    }
}
