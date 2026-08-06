// Concern: rewrites a source formula into Excel-A1, resolving Table[...] refs | Non-concern: whether the result parses, defined names | IO: (raw) -> (=formula, Option<reason>)

use std::iter::Peekable;
use std::str::CharIndices;

use fsa1_ast::a1::parse_a1;

use crate::resolve::Resolution;

/// The returned string always carries the leading `=`. An UNTRANSLATABLE construct is preserved as
/// `=<source body>` with its reason, so one unsupported formula never aborts an import; a token that
/// merely fails to RESOLVE is kept verbatim inside a successful rewrite, with no reason.
pub fn translate_formula_ctx(
    raw: &str,
    res: &Resolution,
    sheet: &str,
    row: u32,
) -> (String, Option<String>) {
    let body = strip_lead(raw.trim());
    match rewrite_body(body, res, sheet, row) {
        Ok(translated) => (format!("={translated}"), None),
        Err(reason) => (format!("={body}"), Some(reason)),
    }
}

/// `pub(crate)` so `serialize` reports a kept-verbatim formula's source as the body it wrote to disk.
pub(crate) fn strip_lead(s: &str) -> &str {
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

fn rewrite_body(body: &str, res: &Resolution, sheet: &str, row: u32) -> Result<String, String> {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        match c {
            '"' => {
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
            // Copied ATOMICALLY: a quoted sheet name's interior words are never walked as identifiers.
            '\'' => {
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
                // The WHOLE token, so `Days` never matches inside `Calendar1Year`.
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
                let after_bang = out.trim_end().ends_with('!');
                let next = chars.peek().map(|&(_, c)| c);
                if next == Some('!') {
                    // A sheet qualifier, never a name: substituting here would corrupt a cross-sheet ref.
                    out.push_str(&ident);
                } else if next == Some('[') && !after_bang && res.is_table(&ident) {
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
                    // OpenFormula's niladic `TRUE()`/`FALSE()`; FSA1 models them as literals.
                    out.push_str(&ident);
                    if ident == "TRUE" || ident == "FALSE" {
                        chars.next();
                        if matches!(chars.peek(), Some((_, ')'))) {
                            chars.next();
                        } else {
                            out.push('(');
                        }
                    }
                } else {
                    out.push_str(&ident);
                }
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

/// The iterator must be positioned AT the opening `[`; nested brackets are preserved, the outer pair
/// stripped.
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

/// Rewrites the interior of one OpenFormula `[...]` token (`.A1`, `Sheet2.A1:.A3`, `'My Sheet'.A1`).
/// `$`-anchors and column case survive verbatim.
fn rewrite_reference(inner: &str) -> Result<String, String> {
    match inner.split_once(':') {
        None => {
            let (sheet, addr) = split_sheet_addr(inner)?;
            Ok(format!("{}{}", sheet_prefix(&sheet), addr))
        }
        Some((left, right)) => {
            let (lsheet, laddr) = split_sheet_addr(left)?;
            let (rsheet, raddr) = split_sheet_addr(right)?;
            // FSA1 qualifies a whole range with ONE sheet: refuse, never drop the qualifier.
            if !rsheet.is_empty() && rsheet != lsheet {
                return Err(format!(
                    "a 3-D range across sheets ({lsheet:?}..{rsheet:?}) is not supported"
                ));
            }
            Ok(format!("{}{}:{}", sheet_prefix(&lsheet), laddr, raddr))
        }
    }
}

/// Splits one endpoint into `(sheet, address)` — the sheet is empty for a same-sheet `.A1`. The
/// address is validated but returned as written, for FSA1 to parse.
fn split_sheet_addr(part: &str) -> Result<(String, &str), String> {
    let part = part.trim();
    if let Some(rest) = part.strip_prefix('\'') {
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
        let after = &rest[i + 1..];
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

fn sheet_prefix(sheet: &str) -> String {
    if sheet.is_empty() {
        String::new()
    } else {
        format!("{sheet}!")
    }
}

/// Grades a single A1 ADDRESS, so a bare `A` or `1` is malformed here — the open-axis form reaches
/// FSA1 through the RANGE path (`A:A`).
fn validate_addr(addr: &str) -> Result<(), String> {
    parse_a1(addr)
        .map(|_| ())
        .map_err(|e| format!("malformed cell address {addr:?}: {e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(raw: &str) -> String {
        translate_formula_ctx(raw, &Resolution::empty(), "", 0).0
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
        assert_eq!(ok("of:=FALSE()"), "=FALSE");
        assert_eq!(ok("of:=IF([.A1];TRUE();FALSE())"), "=IF(A1,TRUE,FALSE)");
        assert_eq!(
            ok("of:=IF([.A1];TRUE;FALSE)"),
            "=IF(A1,TRUE,FALSE)",
            "a bare TRUE/FALSE is already the literal"
        );
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
        assert_eq!(
            ok(r#"of:=CONCAT("a;b";"c")"#),
            r#"=CONCAT("a;b","c")"#,
            "a `;` inside a string literal is not an argument separator"
        );
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
    fn untranslatable_constructs_are_preserved_verbatim_not_refused() {
        assert_eq!(
            ok("of:=[Sheet1.A1:Sheet2.B2]"),
            "=[Sheet1.A1:Sheet2.B2]",
            "3-D range"
        );
        assert_eq!(ok("of:={1;2;3}"), "={1;2;3}", "inline array");
        assert_eq!(ok("of:=[.A1"), "=[.A1", "unterminated bracket");
        assert_eq!(
            ok(r#"of:=CONCAT("x"#),
            r#"=CONCAT("x"#,
            "unterminated string"
        );
        assert_eq!(ok("of:=[.ZZ]"), "=[.ZZ]", "no row");
        assert_eq!(ok("of:=[.99]"), "=[.99]", "no column");
    }

    #[test]
    fn the_untranslatability_reason_is_surfaced_not_discarded() {
        let ctx = |raw: &str| translate_formula_ctx(raw, &Resolution::empty(), "S", 0);
        assert_eq!(ctx("of:=1+2"), ("=1+2".to_string(), None));
        assert_eq!(
            ctx("of:=Unknown+1"),
            ("=Unknown+1".to_string(), None),
            "an unresolved NAME is a translated Ok, not an untranslatable construct"
        );
        let (body, reason) = ctx("of:={1;2;3}");
        assert_eq!(body, "={1;2;3}");
        assert!(
            reason.as_deref().unwrap().contains("inline array"),
            "reason: {reason:?}"
        );
        let (_, reason) = ctx("of:=[Sheet1.A1:Sheet2.B2]");
        assert!(
            reason.as_deref().unwrap().contains("3-D range"),
            "{reason:?}"
        );
    }

    /// A `Sales` table (A1:C4, cols Region/Q1/Q2) on `Data`, and no names — those resolve at load.
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
        translate_formula_ctx(raw, &ctx(), "Data", row).0
    }

    #[test]
    fn resolves_structured_refs_to_a1_but_leaves_names_verbatim() {
        assert_eq!(tr("TaxRate*100", 0), "=TaxRate*100");
        assert_eq!(tr("SUM(Sales[Q1])", 0), "=SUM(B2:B4)");
        assert_eq!(
            tr("SUM(Sales[[#Headers],[Amount]])", 0),
            "=SUM(Sales[[#Headers],[Amount]])",
            "an unknown column stays verbatim"
        );
        assert_eq!(tr("Sales[[#Headers],[Q1]]", 0), "=B1");
        assert_eq!(tr("Sales[@Q1]", 2), "=B3");
        assert_eq!(
            tr("Sales[@Q1]", 0),
            "=Sales[@Q1]",
            "row 0 is the header row, outside the data band"
        );
        assert_eq!(
            tr("Sales[@Q1]", 49),
            "=Sales[@Q1]",
            "row 49 is far below the table"
        );
        assert_eq!(
            tr(r#"SUMIFS(Sales[Q2],Sales[Q1],">="&E4)"#, 0),
            r#"=SUMIFS(C2:C4,B2:B4,">="&E4)"#
        );
    }

    #[test]
    fn token_boundaries_are_respected() {
        assert_eq!(
            tr("TaxRateExtra+1", 0),
            "=TaxRateExtra+1",
            "a name never matches inside a longer identifier"
        );
        assert_eq!(
            tr(r#"IF(A1,"TaxRate","Sales[Q1]")"#, 0),
            r#"=IF(A1,"TaxRate","Sales[Q1]")"#
        );
        assert_eq!(tr("SUM(A1:A2)", 0), "=SUM(A1:A2)");
        assert_eq!(tr("Unknown+1", 0), "=Unknown+1");
        assert_eq!(tr("Other[Col]", 0), "=Other[Col]");
        assert_eq!(tr("E4+1", 0), "=E4+1", "a plain cell ref is not a name");
    }

    #[test]
    fn a_sheet_qualified_reference_passes_through_unchanged() {
        let r = Resolution::empty();
        let tr = |raw: &str| translate_formula_ctx(raw, &r, "Sheet1", 0).0;
        assert_eq!(tr("Data!A1"), "=Data!A1");
        assert_eq!(tr("SUM(Data!A1:A3)"), "=SUM(Data!A1:A3)");
        assert_eq!(tr("Data!Data"), "=Data!Data");
    }

    #[test]
    fn a_quoted_sheet_name_is_copied_atomically() {
        let r = Resolution::empty();
        let tr = |raw: &str| translate_formula_ctx(raw, &r, "Sheet1", 0).0;
        assert_eq!(tr("'Annual Report'!A1"), "='Annual Report'!A1");
        assert_eq!(
            tr("SUM('Annual Report'!A1:A3)"),
            "=SUM('Annual Report'!A1:A3)"
        );
        assert_eq!(tr("'It''s Data'!A1"), "='It''s Data'!A1");
        assert_eq!(
            tr("'Annual Report!A1"),
            "='Annual Report!A1",
            "an unterminated quoted sheet name is kept verbatim"
        );
    }
}
