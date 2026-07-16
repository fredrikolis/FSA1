// Concern: BODY classification (FORMAT §4/§5) — split a file's post-annotation body into exactly one of a literal TSV block (parsed to a `(Shape, Vec<Value>)` grid, rejecting a ragged block) or a single opaque `=formula` (stored verbatim, NOT evaluated), rejecting a dual/multi body; plus per-token literal lexing (§4.3) into charlie-ast `Value`s | Non-concern: EVALUATING a formula or checking its shape against the declared range (that is charlie-ast/W3 and conformance.rs), and the annotation line itself (lib.rs strips it) | IO: (a body `&str`) -> `Result<Body, Diagnostic>`
//! Body grammar: [`classify_body`], [`lex_literal`], [`Body`], [`LiteralBlock`].

use crate::diagnostic::{Code, Diagnostic, Loc};
use charlie_ast::{ErrKind, Shape, Value};

/// A file body — exactly one of the two forms FORMAT §4 allows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    /// A single `=formula`, stored **opaque** (verbatim, leading `=` included). W2 does not
    /// evaluate it or know its result shape — that is W3.
    Formula(String),
    /// A literal block, parsed to a grid.
    Literal(LiteralBlock),
}

/// A parsed literal block: its on-disk shape and the row-major cell values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiteralBlock {
    pub shape: Shape,
    pub cells: Vec<Value>,
}

/// Classify a file's body (everything after the line-1 annotation). Never panics; a malformed body
/// yields a located [`Diagnostic`]. `file` names the file for diagnostics; body line `n` reports as
/// file line `n + 1` (the annotation is line 1).
pub fn classify_body(file: &str, body: &str) -> Result<Body, Diagnostic> {
    // A trailing newline is allowed and ignored (FORMAT §3). Tolerate a lone trailing CR too.
    let body = body.strip_suffix('\n').unwrap_or(body);
    let body = body.strip_suffix('\r').unwrap_or(body);

    if body.is_empty() {
        // An empty body is a single Blank scalar: a blank `.cell` (FORMAT §3, "empty line 2 ⇒
        // Blank"), and for a `.range` it scalar-fills Blank under §6. FORMAT §3 blesses a blank body
        // explicitly only for `.cell`, leaving a blank `.range` UNDER-SPECIFIED. We resolve that
        // toward ACCEPT (ast-standards PART 6 — a false-reject is the cardinal sin): a blank `.range`
        // reads as a `1x1` Blank that §6 fills across the region, which is harmless and consistent
        // with §7 (unclaimed cells already read as Blank). This is a deliberate accept-under-
        // uncertainty, not a silent one: the alternative (reject) would refuse an input the spec
        // never forbids. If a corpus fixture ever pins a blank `.range` to a *different* verdict,
        // that is a B1 signal to record — none does today.
        return Ok(Body::Literal(LiteralBlock {
            shape: Shape { rows: 1, cols: 1 },
            cells: vec![Value::Blank],
        }));
    }

    let lines: Vec<&str> = body.split('\n').collect();

    // FORMAT §4.1 anchors the body FORM to its *first non-empty line*: the body is a `=formula` iff
    // that line begins with `=`; otherwise it is a literal block (§4.2). We deliberately do NOT scan
    // every line for a leading `=` — a later literal field that happens to begin with `=` (e.g. a
    // second row `=A1\t30` under a literal first row) is a literal token (§4.3 has no `=`-prefixed
    // literal, so it lexes to Text), not a formula. Anchoring here avoids a false-reject of such a
    // block (ast-standards PART 6) and matches the § this classifier cites; leading blank lines are
    // ignored (a formula may be preceded by blank lines, e.g. an empty line 2).
    let first = lines.iter().position(|l| !l.is_empty());

    if let Some(fi) = first
        && lines[fi].starts_with('=')
    {
        // Formula form (§4.1). Exactly one `=formula`: any *other* non-empty line — a second formula
        // OR a literal line — is a dual/multi body and is rejected (§4.1 "multiple formula lines are
        // illegal", §11 "both a literal line and an =formula line → reject"). Name the conflicting
        // lines by FILE line number (annotation is line 1, so body line `n` is file line `n + 1`) so
        // the refusal is located, per the oracle ledger (invalid-forms/.../dual-body/EXPECTED.md).
        if let Some((idx, extra)) = lines
            .iter()
            .enumerate()
            .skip(fi + 1)
            .find(|(_, l)| !l.is_empty())
        {
            let extra_kind = if extra.starts_with('=') {
                "a second =formula"
            } else {
                "a literal line"
            };
            return Err(Diagnostic::new(
                Code::DualBody,
                Loc::body(file, (idx + 2) as u32, 1),
                format!(
                    "a body is either one =formula or a literal block, not both: \
                     line {} is the =formula, line {} is {extra_kind} \
                     (exactly one body form, FORMAT §4/§11)",
                    fi + 2,
                    idx + 2,
                ),
            ));
        }
        return Ok(Body::Formula(lines[fi].to_string()));
    }

    // A literal block: every physical line is a row, split on tabs (FORMAT §5).
    let cols = lines[0].split('\t').count();
    let mut cells = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != cols {
            return Err(Diagnostic::new(
                Code::RaggedBlock,
                Loc::body(file, (idx + 2) as u32, 1),
                format!(
                    "ragged literal block: row {} has {} field(s), expected {} (#VALUE!-class)",
                    idx + 1,
                    fields.len(),
                    cols,
                ),
            ));
        }
        for f in fields {
            cells.push(lex_literal(f));
        }
    }
    Ok(Body::Literal(LiteralBlock {
        shape: Shape {
            rows: lines.len() as u32,
            cols: cols as u32,
        },
        cells,
    }))
}

/// Lex one literal token into a [`Value`] (FORMAT §4.3). Precedence: apostrophe force-text, then
/// double-quoted text, then `TRUE`/`FALSE`, then the seven error literals, then a finite number,
/// else text. An empty token is `Blank`.
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

/// The seven v1 error literals (FORMAT §4.3). `#SPILL!`/`#CALC!` are reserved and NOT literal
/// tokens, so they fall through to text.
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

    fn literal(body: &str) -> LiteralBlock {
        match classify_body("f.range", body).expect("should classify") {
            Body::Literal(lb) => lb,
            Body::Formula(_) => panic!("expected literal"),
        }
    }

    #[test]
    fn single_formula_is_opaque() {
        let b = classify_body("D2:D6.range", "=B2*C2").unwrap();
        assert_eq!(b, Body::Formula("=B2*C2".to_string()));
    }

    #[test]
    fn blank_lines_around_a_formula_are_ignored() {
        assert_eq!(
            classify_body("f.cell", "\n=SUM(A1:A3)\n").unwrap(),
            Body::Formula("=SUM(A1:A3)".to_string())
        );
    }

    #[test]
    fn empty_body_is_blank_scalar() {
        let lb = literal("");
        assert_eq!(lb.shape, Shape { rows: 1, cols: 1 });
        assert_eq!(lb.cells, vec![Value::Blank]);
    }

    #[test]
    fn one_line_block_is_a_row_vector() {
        let lb = literal("10\t20\t30");
        assert_eq!(lb.shape, Shape { rows: 1, cols: 3 });
        assert_eq!(
            lb.cells,
            vec![
                Value::Number(10.0),
                Value::Number(20.0),
                Value::Number(30.0)
            ]
        );
    }

    #[test]
    fn one_field_per_line_block_is_a_col_vector() {
        let lb = literal("Widget\nGadget\nCog");
        assert_eq!(lb.shape, Shape { rows: 3, cols: 1 });
    }

    #[test]
    fn exact_grid() {
        let lb = literal("10\t20\t30\n40\t50\t60");
        assert_eq!(lb.shape, Shape { rows: 2, cols: 3 });
        assert_eq!(lb.cells.len(), 6);
    }

    #[test]
    fn ragged_block_is_value_class_refusal() {
        let d = classify_body("f.range", "10\t20\t30\n40\t50").unwrap_err();
        assert_eq!(d.code, Code::RaggedBlock);
        assert_eq!(d.code.err_class(), Some(ErrKind::Value));
    }

    #[test]
    fn formula_first_then_literal_is_dual_body_naming_both_lines() {
        // FORMAT §4.1/§11: the first non-empty line is the =formula, a later non-empty literal line
        // makes it a dual body -> reject, naming both FILE line numbers (annotation is file line 1).
        let d = classify_body("f.range", "=B2*C2\nHello").unwrap_err();
        assert_eq!(d.code, Code::DualBody);
        assert!(
            d.message.contains("line 2 is the =formula"),
            "must name the formula's file line: {}",
            d.message
        );
        assert!(
            d.message.contains("line 3 is a literal line"),
            "must name the conflicting literal's file line: {}",
            d.message
        );
        assert_eq!(
            d.loc,
            Loc::body("f.range", 3, 1),
            "located at the conflicting literal line"
        );
    }

    #[test]
    fn multiple_formulas_is_dual_body() {
        let d = classify_body("f.range", "=A1\n=B1").unwrap_err();
        assert_eq!(d.code, Code::DualBody);
        assert!(d.message.contains("a second =formula"), "{}", d.message);
    }

    #[test]
    fn later_equals_prefixed_field_is_a_literal_block_not_a_formula() {
        // FORMAT §4.1 anchors formula detection to the FIRST non-empty line. A literal block whose
        // first line is data but a LATER row's first field begins with `=` is a literal block (§4.3
        // has no `=`-prefixed literal token, so `=A1` lexes to Text) -- NOT a false-rejected dual
        // body. This is the corner the old every-line `=` scan wrongly rejected.
        let lb = literal("10\t20\n=A1\t30");
        assert_eq!(lb.shape, Shape { rows: 2, cols: 2 });
        assert_eq!(
            lb.cells,
            vec![
                Value::Number(10.0),
                Value::Number(20.0),
                Value::Text("=A1".to_string()),
                Value::Number(30.0),
            ]
        );
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
        // A reserved error spelling is not a literal error token -> text.
        assert_eq!(lex_literal("#SPILL!"), Value::Text("#SPILL!".to_string()));
    }
}
