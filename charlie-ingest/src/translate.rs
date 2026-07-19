// Concern: translate an ODS/OpenFormula formula string into charlie's Excel-A1 grammar — strip the `of:=`/`of:`/`=` lead; rewrite bracketed references `[.A1]`→`A1`, `[.A1:.A3]`→`A1:A3`, `[Sheet2.A1]`→`Sheet2!A1`, `['My Sheet'.A1]`→`'My Sheet'!A1` (preserving `$`-anchors); map the ODS `;` argument separator to `,` (only outside string literals); keep Excel-compatible function names as written EXCEPT the niladic booleans `TRUE()`/`FALSE()`, normalized to charlie's `TRUE`/`FALSE` literals; and preserve anything UNTRANSLATABLE (a 3D range, an inline array `{…}`, a malformed reference, an unterminated string) VERBATIM as `=<source body>` rather than aborting — so the import always succeeds and charlie's loader flags such a cell as a located GRID6 error (visible in `--functions`, reported by `check`), never a silently-wrong formula and never a whole-import failure | Non-concern: whether the translated formula PARSES/EVALUATES in charlie (charlie-ast owns that; an untranslatable/unsupported formula surfaces as a load-time GRID6 error cell downstream) and reading the cell (reader.rs) | IO: (a raw OpenFormula `&str`) -> `String` (the `=…` charlie formula, best-effort verbatim on an untranslatable construct)
//! OpenFormula → charlie Excel-A1 translation: [`translate_formula`]. The returned string always
//! includes the leading `=`; an untranslatable construct is preserved verbatim (GRID6 flags it at load).

use charlie_ast::a1::parse_a1;

/// Translate a raw source-dialect formula into a charlie `=formula` (always with the leading `=`).
/// A successful rewrite yields the Excel-A1 form; an UNTRANSLATABLE construct (a 3-D range, an inline
/// array, a malformed reference, an unterminated string) is preserved VERBATIM as `=<source body>`
/// (the lead stripped) rather than refused — the import still succeeds, and charlie's deserializer
/// flags the cell as a located GRID6 error (`--functions` shows this raw text; `check` reports it).
/// This preserves the source formula so an agent can see and fix exactly what charlie could not parse.
pub fn translate_formula(raw: &str) -> String {
    let body = strip_lead(raw.trim());
    match rewrite_body(body) {
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

/// Scan the formula body, rewriting bracketed references and `;`→`,` while leaving string literals
/// verbatim. An inline array `{…}` is refused (ODS array syntax differs from charlie's and a naive
/// `;`→`,` would silently transpose it).
fn rewrite_body(body: &str) -> Result<String, String> {
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
            c if c.is_ascii_alphabetic() => {
                // An identifier (a function name or a bare/niladic name). Accumulate it, then normalize
                // OpenFormula's niladic booleans `TRUE()`/`FALSE()` — which charlie models as LITERALS,
                // not functions — into the bare `TRUE`/`FALSE`. Every other name (function or bare) is
                // kept verbatim (charlie's function names are Excel-compatible).
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
                out.push_str(&ident);
                if (ident == "TRUE" || ident == "FALSE") && matches!(chars.peek(), Some((_, '('))) {
                    chars.next(); // consume '('
                    if matches!(chars.peek(), Some((_, ')'))) {
                        chars.next(); // consume ')' — a niladic call becomes the bare literal
                    } else {
                        // `TRUE(<args>)` is not the niladic form; leave the `(` for the normal loop.
                        out.push('(');
                    }
                }
            }
            other => out.push(other),
        }
    }
    Ok(out)
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

    fn ok(raw: &str) -> String {
        translate_formula(raw)
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
        assert_eq!(
            translate_formula("of:=[Sheet1.A1:Sheet2.B2]"),
            "=[Sheet1.A1:Sheet2.B2]"
        ); // 3-D range
        assert_eq!(translate_formula("of:={1;2;3}"), "={1;2;3}"); // inline array
        assert_eq!(translate_formula("of:=[.A1"), "=[.A1"); // unterminated bracket
        assert_eq!(translate_formula(r#"of:=CONCAT("x"#), r#"=CONCAT("x"#); // unterminated string
        assert_eq!(translate_formula("of:=[.ZZ]"), "=[.ZZ]"); // no row -> malformed address
        assert_eq!(translate_formula("of:=[.99]"), "=[.99]"); // no column -> malformed address
    }
}
