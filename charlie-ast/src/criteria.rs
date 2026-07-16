// Concern: the CRITERIA MINI-LANGUAGE shared by the `*IF(S)` family — parse one already-evaluated criteria `Value` into a `Criterion` (a comparison `Op` + a typed `Comparand`: number / text / empty), honouring the leading-operator spelling (`>`, `>=`, `<`, `<=`, `<>`, `=`), Excel wildcard matching (`*` any run, `?` one char, `~` escape) for text equality/inequality, and the blank/empty-comparand rule; then test a candidate cell `Value` for a match. A criteria value carrying an error propagates (`Err(kind)`). Concatenated criteria (`">"&ref`) need no special case — the evaluator already folds the `&` before the string reaches here | Non-concern: the aggregation loops and the criteria-vs-sum-range length-conformance check (func.rs owns SUMIF/COUNTIFS/… and reuses this) and how a range materializes into cells (eval.rs) | IO: (an evaluated criteria `Value`) -> `Result<Criterion, ErrKind>`; (a `Criterion`, a cell `Value`) -> `bool`
//! The criteria mini-language for the `*IF(S)` reporting family ([`Criterion`], [`parse_criterion`]).
//!
//! Excel's criteria are a tiny DSL: a scalar that is either a bare value (equality) or a string that
//! may lead with a comparison operator and whose remainder is a number or a wildcard text pattern.
//! This module is the ONE place that grammar lives, so every `*IF`/`*IFS` function shares an
//! identical, independently-tested notion of "does this cell match this criterion".

use crate::value::{ErrKind, Value};

/// A comparison operator extracted from a criteria string (or the implicit `Eq` of a bare value).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

/// The right-hand side of a criterion, typed at parse time: a number compares numerically, a text
/// pattern compares with wildcards (for `Eq`/`Ne`) or case-insensitively (for ordering), and `Empty`
/// (an empty comparand, e.g. `""` or `"<>"`) is the blank/non-blank selector.
#[derive(Clone, Debug, PartialEq)]
enum Comparand {
    Num(f64),
    Text(String),
    Empty,
}

/// A parsed criterion: an operator paired with its typed comparand. Built by [`parse_criterion`] and
/// applied with [`Criterion::matches`].
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Criterion {
    op: Op,
    comparand: Comparand,
}

/// Parse one already-evaluated criteria value into a [`Criterion`]. An error value propagates
/// (`Err(kind)`) so the calling function surfaces it; a multi-cell array in criteria position is
/// `#VALUE!` (the caller normally scalarizes first, but this stays total). A bare number/bool is an
/// equality criterion; blank is the empty-comparand (blank-matching) equality; a string is parsed
/// for a leading operator and a number-or-text remainder.
pub(crate) fn parse_criterion(v: &Value) -> Result<Criterion, ErrKind> {
    match v {
        Value::Error(k) => Err(*k),
        Value::Number(n) => Ok(Criterion {
            op: Op::Eq,
            comparand: Comparand::Num(*n),
        }),
        Value::Bool(b) => Ok(Criterion {
            op: Op::Eq,
            comparand: Comparand::Text(bool_text(*b)),
        }),
        Value::Blank => Ok(Criterion {
            op: Op::Eq,
            comparand: Comparand::Empty,
        }),
        Value::Text(s) => Ok(parse_text_criterion(s)),
        Value::Array(..) => Err(ErrKind::Value),
    }
}

/// Parse a criteria STRING: strip a leading comparison operator, then classify the remainder as a
/// finite number (numeric compare), an empty string (blank selector), or a wildcard text pattern.
fn parse_text_criterion(s: &str) -> Criterion {
    let (op, rest) = split_op(s);
    let comparand = if rest.is_empty() {
        Comparand::Empty
    } else if let Ok(n) = rest.parse::<f64>() {
        // A non-finite spelling (`inf`, `1e999`) is NOT a number here — it stays a text pattern,
        // mirroring the lexer/coercion finiteness invariant (Number is always finite).
        if n.is_finite() {
            Comparand::Num(n)
        } else {
            Comparand::Text(rest.to_string())
        }
    } else {
        Comparand::Text(rest.to_string())
    };
    Criterion { op, comparand }
}

/// Split a leading comparison operator off a criteria string. Two-char operators are tried before
/// their one-char prefixes (`<>`/`<=` before `<`, `>=` before `>`) so `"<>"` never mis-parses as
/// `<` then `>`. No leading operator ⇒ an implicit `Eq` over the whole string.
fn split_op(s: &str) -> (Op, &str) {
    const OPS: &[(&str, Op)] = &[
        ("<>", Op::Ne),
        (">=", Op::Ge),
        ("<=", Op::Le),
        (">", Op::Gt),
        ("<", Op::Lt),
        ("=", Op::Eq),
    ];
    for (prefix, op) in OPS {
        if let Some(rest) = s.strip_prefix(prefix) {
            return (*op, rest);
        }
    }
    (Op::Eq, s)
}

impl Criterion {
    /// Test a candidate cell value against this criterion. Against a TYPED (number/text) comparand an
    /// error or array cell never matches. The one exception is the empty-comparand `<>` NON-BLANK
    /// selector, which counts every non-blank cell INCLUDING an error (Excel: `COUNTIF(range,"<>")`
    /// counts an error cell). A blank cell matches ONLY the empty-comparand equality (`""`); with a
    /// non-empty comparand a blank is a non-match (and `<>x` non-blank cells that differ from `x`
    /// still match).
    pub(crate) fn matches(&self, cell: &Value) -> bool {
        match &self.comparand {
            Comparand::Empty => match self.op {
                // `""`/`"="` selects blanks (and empty text); `"<>"` selects non-blanks. Any other
                // operator with an empty comparand is degenerate ⇒ no match.
                Op::Eq => is_blank_or_empty(cell),
                Op::Ne => !is_blank_or_empty(cell),
                _ => false,
            },
            // A non-empty comparand never matches a blank cell.
            _ if matches!(cell, Value::Blank) => false,
            Comparand::Num(c) => self.match_num(cell, *c),
            Comparand::Text(t) => self.match_text(cell, t),
        }
    }

    /// Numeric-comparand matching. A numeric cell compares by the operator; a text/bool cell only
    /// satisfies `<>` (it is "not equal" to any number) and never an ordering/equality; an
    /// error/array cell never matches.
    fn match_num(&self, cell: &Value, c: f64) -> bool {
        match cell {
            Value::Number(n) => match self.op {
                Op::Eq => *n == c,
                Op::Ne => *n != c,
                Op::Gt => *n > c,
                Op::Ge => *n >= c,
                Op::Lt => *n < c,
                Op::Le => *n <= c,
            },
            Value::Error(_) | Value::Array(..) | Value::Blank => false,
            // A non-numeric (text/bool) cell is "not equal" to a number — matches only `<>`.
            _ => self.op == Op::Ne,
        }
    }

    /// Text-comparand matching over a GENUINE text cell only. Excel's `*IF(S)` wildcard/text criteria
    /// select TEXT cells exclusively: a number/bool cell is NEVER coerced to its text form and
    /// matched, so `COUNTIF([apple,5],"*")` counts only `apple`, and `COUNTIF([15,"1x"],"1*")` counts
    /// only `1x`. A non-text cell is "not equal" to any text pattern, so it satisfies ONLY `<>`
    /// (number/bool); a blank/error/array cell never matches a text comparand at all. For a genuine
    /// text cell, `Eq`/`Ne` use wildcards (`*`, `?`, `~`) case-insensitively and ordering (`>`, `<`,
    /// …) compares case-insensitively.
    fn match_text(&self, cell: &Value, pattern: &str) -> bool {
        let Value::Text(s) = cell else {
            // A non-text cell is never matched by a text pattern. A number/bool is "not equal" to it
            // (so `<>` matches, nothing else); blank/error/array never match a text comparand.
            return self.op == Op::Ne && matches!(cell, Value::Number(_) | Value::Bool(_));
        };
        match self.op {
            Op::Eq => wildcard_match(pattern, s),
            Op::Ne => !wildcard_match(pattern, s),
            _ => {
                let ord = s.to_ascii_lowercase().cmp(&pattern.to_ascii_lowercase());
                use std::cmp::Ordering;
                match self.op {
                    Op::Gt => ord == Ordering::Greater,
                    Op::Ge => ord != Ordering::Less,
                    Op::Lt => ord == Ordering::Less,
                    Op::Le => ord != Ordering::Greater,
                    _ => false,
                }
            }
        }
    }
}

/// A cell is blank-or-empty iff it is `Blank` or the empty text string (the `""` selector's target).
fn is_blank_or_empty(v: &Value) -> bool {
    matches!(v, Value::Blank) || matches!(v, Value::Text(s) if s.is_empty())
}

/// Excel's canonical text for a boolean (matches the `&`-concat / general form).
fn bool_text(b: bool) -> String {
    if b { "TRUE" } else { "FALSE" }.to_string()
}

/// One compiled wildcard-pattern token.
#[derive(Clone, Copy, PartialEq)]
enum Tok {
    /// `*` — matches any run of characters (including empty).
    Star,
    /// `?` — matches exactly one character.
    Any,
    /// A literal character (already lowercased; a `~`-escaped `*`/`?`/`~` compiles to its literal).
    Lit(char),
}

/// Case-insensitive Excel wildcard match: `*` any run, `?` one char, `~` escapes the next
/// `*`/`?`/`~` to a literal (a lone `~` is itself literal). Both sides are ASCII-lowercased so the
/// match folds case like Excel's criteria. Exposed `pub(crate)` because the lookup family reuses the
/// EXACT same wildcard grammar for a text needle (`MATCH` mode 0 and `XLOOKUP` match_mode 2), so
/// there is one wildcard engine, not two.
pub(crate) fn wildcard_match(pattern: &str, text: &str) -> bool {
    let toks = compile(pattern);
    let t: Vec<char> = text.to_ascii_lowercase().chars().collect();

    // Classic two-pointer wildcard matcher with `*` backtracking.
    let mut ti = 0usize; // index into text
    let mut tki = 0usize; // index into tokens
    let mut star_tk: Option<usize> = None; // last `*` token index
    let mut star_ti = 0usize; // text index when that `*` was taken
    while ti < t.len() {
        match toks.get(tki) {
            Some(Tok::Lit(c)) if *c == t[ti] => {
                ti += 1;
                tki += 1;
            }
            Some(Tok::Any) => {
                ti += 1;
                tki += 1;
            }
            Some(Tok::Star) => {
                star_tk = Some(tki);
                star_ti = ti;
                tki += 1;
            }
            _ => {
                // Mismatch (or ran out of tokens): backtrack to the last `*`, consuming one more
                // text char under it; with no `*` to fall back on, the match fails.
                if let Some(stk) = star_tk {
                    tki = stk + 1;
                    star_ti += 1;
                    ti = star_ti;
                } else {
                    return false;
                }
            }
        }
    }
    // Any trailing tokens must all be `*` for a full match.
    while matches!(toks.get(tki), Some(Tok::Star)) {
        tki += 1;
    }
    tki == toks.len()
}

/// Compile a (lowercased) pattern into wildcard tokens, resolving `~` escapes.
fn compile(pattern: &str) -> Vec<Tok> {
    let chars: Vec<char> = pattern.to_ascii_lowercase().chars().collect();
    let mut toks = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '~' && matches!(chars.get(i + 1), Some('*' | '?' | '~')) {
            toks.push(Tok::Lit(chars[i + 1]));
            i += 2;
        } else if c == '*' {
            toks.push(Tok::Star);
            i += 1;
        } else if c == '?' {
            toks.push(Tok::Any);
            i += 1;
        } else {
            toks.push(Tok::Lit(c));
            i += 1;
        }
    }
    toks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crit(v: Value) -> Criterion {
        parse_criterion(&v).expect("criterion parses")
    }
    fn text(s: &str) -> Value {
        Value::Text(s.into())
    }

    #[test]
    fn numeric_comparison_operators() {
        let c = crit(text(">10"));
        assert!(c.matches(&Value::Number(15.0)));
        assert!(!c.matches(&Value::Number(10.0)));
        assert!(!c.matches(&Value::Number(5.0)));
        // A text cell never satisfies a numeric ordering.
        assert!(!c.matches(&text("99")));

        let ge = crit(text(">=10"));
        assert!(ge.matches(&Value::Number(10.0)));
        let le = crit(text("<=10"));
        assert!(le.matches(&Value::Number(10.0)) && le.matches(&Value::Number(9.0)));
        let ne = crit(text("<>10"));
        assert!(ne.matches(&Value::Number(11.0)) && !ne.matches(&Value::Number(10.0)));
        // `<>10` matches a non-numeric cell (it is "not equal" to 10).
        assert!(ne.matches(&text("x")));
    }

    #[test]
    fn bare_number_is_equality() {
        let c = crit(Value::Number(5.0));
        assert!(c.matches(&Value::Number(5.0)));
        assert!(!c.matches(&Value::Number(6.0)));
        // A numeric-looking string criterion parses to the same numeric equality.
        assert!(crit(text("5")).matches(&Value::Number(5.0)));
    }

    #[test]
    fn text_equality_is_case_insensitive() {
        let c = crit(text("Apple"));
        assert!(c.matches(&text("apple")) && c.matches(&text("APPLE")));
        assert!(!c.matches(&text("apples")));
    }

    #[test]
    fn wildcards_star_and_question() {
        assert!(crit(text("a*")).matches(&text("avocado")));
        assert!(!crit(text("a*")).matches(&text("banana")));
        // `?` is exactly one char.
        assert!(crit(text("ca?")).matches(&text("cat")));
        assert!(!crit(text("ca?")).matches(&text("ca")));
        assert!(!crit(text("ca?")).matches(&text("cart")));
        // interior star
        assert!(crit(text("a*o")).matches(&text("avocado")));
        assert!(crit(text("*")).matches(&text("anything")));
    }

    #[test]
    fn tilde_escapes_a_literal_wildcard() {
        // `~*` is a literal asterisk, not "any run".
        let c = crit(text("a~*b"));
        assert!(c.matches(&text("a*b")));
        assert!(!c.matches(&text("axb")));
        // `~?` is a literal question mark.
        assert!(crit(text("a~?")).matches(&text("a?")));
        assert!(!crit(text("a~?")).matches(&text("ax")));
    }

    #[test]
    fn ne_wildcard_and_text_ordering() {
        // `<>a*` — not starting with a.
        let ne = crit(text("<>a*"));
        assert!(ne.matches(&text("banana")) && !ne.matches(&text("apple")));
        // text ordering compares case-insensitively, text cells only.
        let gt = crit(text(">m"));
        assert!(gt.matches(&text("nurse")) && !gt.matches(&text("apple")));
        assert!(!gt.matches(&Value::Number(999.0)));
    }

    #[test]
    fn empty_comparand_selects_blanks() {
        let eq = crit(text("="));
        assert!(eq.matches(&Value::Blank) && eq.matches(&text("")));
        assert!(!eq.matches(&text("x")) && !eq.matches(&Value::Number(0.0)));
        let ne = crit(text("<>"));
        assert!(ne.matches(&text("x")) && ne.matches(&Value::Number(0.0)));
        assert!(!ne.matches(&Value::Blank) && !ne.matches(&text("")));
        // A bare blank criteria value is the blank selector too.
        assert!(crit(Value::Blank).matches(&Value::Blank));
    }

    #[test]
    fn text_pattern_matches_text_cells_only_not_numbers_or_bools() {
        // `*` (match-anything text pattern) selects TEXT cells only — a number/bool cell is not text.
        let star = crit(text("*"));
        assert!(star.matches(&text("apple")) && star.matches(&text("")));
        assert!(!star.matches(&Value::Number(5.0)) && !star.matches(&Value::Bool(true)));
        // A prefix wildcard over a mixed range: "1*" matches the text "1x", NOT the number 15.
        let one_star = crit(text("1*"));
        assert!(one_star.matches(&text("1x")));
        assert!(!one_star.matches(&Value::Number(15.0)));
        // `?` (one char) matches a 1-char TEXT cell, never a number.
        let q = crit(text("?"));
        assert!(q.matches(&text("a")) && !q.matches(&Value::Number(5.0)));
        // A number/bool cell IS "not equal" to a text pattern, so `<>` selects it.
        let ne = crit(text("<>a*"));
        assert!(ne.matches(&Value::Number(5.0)) && ne.matches(&Value::Bool(false)));
        // A bare (non-numeric) text equality likewise skips a number cell.
        assert!(!crit(text("apple")).matches(&Value::Number(5.0)));
    }

    #[test]
    fn error_cells_never_match_and_error_criteria_propagates() {
        // An error CELL never matches, even `<>`.
        assert!(!crit(text("<>10")).matches(&Value::Error(ErrKind::Na)));
        assert!(!crit(text("*")).matches(&Value::Error(ErrKind::Na)));
        // An error CRITERIA value propagates.
        assert_eq!(
            parse_criterion(&Value::Error(ErrKind::Div0)),
            Err(ErrKind::Div0)
        );
    }
}
