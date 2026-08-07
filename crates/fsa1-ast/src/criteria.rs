// Concern: parses a criteria value and decides whether a cell satisfies it | Non-concern: which cells a function scans, aggregating the matches | IO: (&Value) -> Criterion; (&Value) -> bool

use crate::value::{ErrKind, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

/// `Empty` is the blank/non-blank selector — an empty comparand, as in `""` or `"<>"`.
#[derive(Clone, Debug, PartialEq)]
enum Comparand {
    Num(f64),
    Text(String),
    Empty,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Criterion {
    op: Op,
    comparand: Comparand,
}

/// Takes an ALREADY-EVALUATED criteria value; an error value propagates to the caller.
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

/// The DATABASE grammar differs from [`parse_criterion`] in one way: BARE text matches
/// begins-with, while a leading `=` still forces exact equality. Every other case defers.
pub(crate) fn parse_db_criterion(v: &Value) -> Result<Criterion, ErrKind> {
    if let Value::Text(s) = v {
        let bare_text = !s.is_empty()
            && !s.starts_with(['<', '>', '='])
            && !s.parse::<f64>().is_ok_and(f64::is_finite);
        if bare_text {
            return Ok(Criterion {
                op: Op::Eq,
                comparand: Comparand::Text(format!("{s}*")),
            });
        }
    }
    parse_criterion(v)
}

fn parse_text_criterion(s: &str) -> Criterion {
    let (op, rest) = split_op(s);
    let comparand = if rest.is_empty() {
        Comparand::Empty
    } else if let Ok(n) = rest.parse::<f64>() {
        // A `Number` is always finite, so `inf`/`1e999` stays a text pattern.
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

/// Two-char operators come first in the table, so `"<>"` never mis-parses as `<` then `>`.
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
    pub(crate) fn matches(&self, cell: &Value) -> bool {
        match &self.comparand {
            Comparand::Empty => match self.op {
                // The non-blank selector `<>` counts an error cell too; any other operator with an empty comparand is degenerate.
                Op::Eq => is_blank_or_empty(cell),
                Op::Ne => !is_blank_or_empty(cell),
                _ => false,
            },
            _ if matches!(cell, Value::Blank) => false,
            Comparand::Num(c) => self.match_num(cell, *c),
            Comparand::Text(t) => self.match_text(cell, t),
        }
    }

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

    /// A text pattern selects TEXT cells only — a number/bool is never coerced to its text form.
    /// The exception is a pattern that IS an error literal: `"#REF!"` is how a criterion names an
    /// error cell, and without this the only way to count broken cells answers zero.
    fn match_text(&self, cell: &Value, pattern: &str) -> bool {
        // Upper-cased: the lexer reads uppercase-only source, but every criterion here folds case.
        let upper = pattern.to_ascii_uppercase();
        if let Value::Error(k) = cell
            && let Some((lit, len)) = crate::lexer::match_error_literal(&upper)
            && len == upper.len()
        {
            return match self.op {
                Op::Eq => *k == lit,
                Op::Ne => *k != lit,
                // An error has no ordering, so a relational criterion selects nothing.
                _ => false,
            };
        }
        let Value::Text(s) = cell else {
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

fn is_blank_or_empty(v: &Value) -> bool {
    matches!(v, Value::Blank) || matches!(v, Value::Text(s) if s.is_empty())
}

fn bool_text(b: bool) -> String {
    if b { "TRUE" } else { "FALSE" }.to_string()
}

#[derive(Clone, Copy, PartialEq)]
enum Tok {
    Star,
    Any,
    Lit(char),
}

/// Excel wildcards, case-insensitive: `*` any run, `?` one char, `~` escapes the next `*`/`?`/`~`.
/// The lookup family reuses this exact grammar, so there is one wildcard engine, not two.
pub(crate) fn wildcard_match(pattern: &str, text: &str) -> bool {
    let toks = compile(pattern);
    let chars: Vec<char> = text.to_ascii_lowercase().chars().collect();

    // Two-pointer match with `*` backtracking.
    let mut text_i = 0usize;
    let mut tok_i = 0usize;
    let mut star_tok_i: Option<usize> = None;
    let mut star_text_i = 0usize;
    while text_i < chars.len() {
        match toks.get(tok_i) {
            Some(Tok::Lit(c)) if *c == chars[text_i] => {
                text_i += 1;
                tok_i += 1;
            }
            Some(Tok::Any) => {
                text_i += 1;
                tok_i += 1;
            }
            Some(Tok::Star) => {
                star_tok_i = Some(tok_i);
                star_text_i = text_i;
                tok_i += 1;
            }
            _ => {
                // Backtrack to the last `*`, letting it consume one more char; with none, no match.
                if let Some(stk) = star_tok_i {
                    tok_i = stk + 1;
                    star_text_i += 1;
                    text_i = star_text_i;
                } else {
                    return false;
                }
            }
        }
    }
    // A full match needs every trailing token to be `*`.
    while matches!(toks.get(tok_i), Some(Tok::Star)) {
        tok_i += 1;
    }
    tok_i == toks.len()
}

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

    /// An error literal names an error CELL. Without it the only criterion an author would reach for
    /// to census broken cells selects none of them and answers a confident zero.
    #[test]
    fn an_error_literal_criterion_selects_that_error_kind() {
        let c = crit(text("#REF!"));
        assert!(c.matches(&Value::Error(ErrKind::Ref)));
        assert!(!c.matches(&Value::Error(ErrKind::Div0)));
        assert!(!c.matches(&Value::Number(1.0)));
        assert!(!c.matches(&text("other")));
    }

    #[test]
    fn a_negated_error_literal_selects_everything_else() {
        let c = crit(text("<>#REF!"));
        assert!(!c.matches(&Value::Error(ErrKind::Ref)));
        assert!(c.matches(&Value::Error(ErrKind::Div0)));
        assert!(c.matches(&Value::Number(1.0)));
    }

    /// The change is ADDITIVE: a text cell holding those characters still matches, and a pattern
    /// that merely CONTAINS an error literal is not one.
    #[test]
    fn an_error_literal_still_matches_a_text_cell_spelling_it() {
        assert!(crit(text("#REF!")).matches(&text("#REF!")));
        assert!(!crit(text("#REF!x")).matches(&Value::Error(ErrKind::Ref)));
        assert!(crit(text("#REF!x")).matches(&text("#REF!x")));
    }

    #[test]
    fn numeric_comparison_operators() {
        let c = crit(text(">10"));
        assert!(c.matches(&Value::Number(15.0)));
        assert!(!c.matches(&Value::Number(10.0)));
        assert!(!c.matches(&Value::Number(5.0)));
        assert!(
            !c.matches(&text("99")),
            "text never satisfies a numeric ordering"
        );

        let ge = crit(text(">=10"));
        assert!(ge.matches(&Value::Number(10.0)));
        let le = crit(text("<=10"));
        assert!(le.matches(&Value::Number(10.0)) && le.matches(&Value::Number(9.0)));
        let ne = crit(text("<>10"));
        assert!(ne.matches(&Value::Number(11.0)) && !ne.matches(&Value::Number(10.0)));
        assert!(
            ne.matches(&text("x")),
            "a non-numeric cell is not equal to 10"
        );
    }

    #[test]
    fn bare_number_is_equality() {
        let c = crit(Value::Number(5.0));
        assert!(c.matches(&Value::Number(5.0)));
        assert!(!c.matches(&Value::Number(6.0)));
        assert!(
            crit(text("5")).matches(&Value::Number(5.0)),
            "a numeric-looking string is the same numeric equality"
        );
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
        assert!(
            crit(text("ca?")).matches(&text("cat")),
            "? is exactly one char"
        );
        assert!(!crit(text("ca?")).matches(&text("ca")));
        assert!(!crit(text("ca?")).matches(&text("cart")));
        assert!(crit(text("a*o")).matches(&text("avocado")));
        assert!(crit(text("*")).matches(&text("anything")));
    }

    #[test]
    fn tilde_escapes_a_literal_wildcard() {
        let c = crit(text("a~*b"));
        assert!(c.matches(&text("a*b")));
        assert!(!c.matches(&text("axb")), "~* is a literal asterisk");
        assert!(crit(text("a~?")).matches(&text("a?")));
        assert!(!crit(text("a~?")).matches(&text("ax")));
    }

    #[test]
    fn ne_wildcard_and_text_ordering() {
        let ne = crit(text("<>a*"));
        assert!(ne.matches(&text("banana")) && !ne.matches(&text("apple")));
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
        assert!(
            crit(Value::Blank).matches(&Value::Blank),
            "a bare blank criteria value is the blank selector too"
        );
    }

    #[test]
    fn text_pattern_matches_text_cells_only_not_numbers_or_bools() {
        let star = crit(text("*"));
        assert!(star.matches(&text("apple")) && star.matches(&text("")));
        assert!(!star.matches(&Value::Number(5.0)) && !star.matches(&Value::Bool(true)));
        let one_star = crit(text("1*"));
        assert!(one_star.matches(&text("1x")));
        assert!(
            !one_star.matches(&Value::Number(15.0)),
            "the number 15 is never coerced to the text 15"
        );
        let q = crit(text("?"));
        assert!(q.matches(&text("a")) && !q.matches(&Value::Number(5.0)));
        let ne = crit(text("<>a*"));
        assert!(
            ne.matches(&Value::Number(5.0)) && ne.matches(&Value::Bool(false)),
            "a number/bool is not equal to a text pattern, so <> selects it"
        );
        assert!(!crit(text("apple")).matches(&Value::Number(5.0)));
    }

    #[test]
    fn db_grammar_bare_text_is_begins_with_but_leading_eq_is_exact() {
        let bare = parse_db_criterion(&text("App")).unwrap();
        assert!(bare.matches(&text("Apple")) && bare.matches(&text("applesauce")));
        assert!(
            bare.matches(&text("App")) && !bare.matches(&text("Pineapple")),
            "begins-with, not contains"
        );
        let exact = parse_db_criterion(&text("=App")).unwrap();
        assert!(exact.matches(&text("app")) && !exact.matches(&text("Apple")));
        assert!(
            parse_db_criterion(&text(">10"))
                .unwrap()
                .matches(&Value::Number(15.0))
        );
        let five = parse_db_criterion(&text("5")).unwrap();
        assert!(five.matches(&Value::Number(5.0)) && !five.matches(&Value::Number(50.0)));
        assert!(
            parse_db_criterion(&text("A*e"))
                .unwrap()
                .matches(&text("Apple"))
        );
        assert!(!parse_db_criterion(&text("1")).unwrap().matches(&text("15")));
    }

    #[test]
    fn error_cells_never_match_and_error_criteria_propagates() {
        assert!(!crit(text("<>10")).matches(&Value::Error(ErrKind::Na)));
        assert!(!crit(text("*")).matches(&Value::Error(ErrKind::Na)));
        assert_eq!(
            parse_criterion(&Value::Error(ErrKind::Div0)),
            Err(ErrKind::Div0)
        );
    }
}
