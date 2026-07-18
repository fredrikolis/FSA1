// Concern: UNIT-TEST pins for the text family built-ins (CONCAT TEXTJOIN LEFT RIGHT MID LEN FIND SEARCH SUBSTITUTE REPLACE TRIM UPPER LOWER TEXT) exercised through `FUNCS` dispatch — stringification/flattening, clamped substring extraction, case-sensitive FIND vs wildcard SEARCH (incl. the multi-star ReDoS regression), and TEXT's format subset with its serial-band gate + non-literal deferral | Non-concern: the text impls (`func/text.rs`) and the shared test fixtures (the parent `tests` module owns `num`/`call`/`col_range`/`txt`/`text`) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
use super::*;

#[test]
fn concat_and_textjoin() {
    let g = Grid::new(1, vec![Value::Blank]);
    // CONCAT stringifies each arg (number → general text, bool → TRUE/FALSE).
    assert_eq!(
        eval(
            &call(
                "CONCAT",
                vec![txt("a"), num(1.0), Expr::Lit(Value::Bool(true))]
            ),
            &g
        ),
        Value::Text("a1TRUE".into())
    );
    // CONCAT flattens a range (in-range blank → "").
    let r = Grid::new(
        1,
        vec![Value::Text("x".into()), Value::Blank, Value::Number(2.0)],
    );
    assert_eq!(
        eval(&call("CONCAT", vec![col_range(3)]), &r),
        Value::Text("x2".into())
    );
    // TEXTJOIN with ignore_empty=TRUE drops the blank; delimiter between kept pieces.
    assert_eq!(
        eval(
            &call(
                "TEXTJOIN",
                vec![txt("-"), Expr::Lit(Value::Bool(true)), col_range(3)]
            ),
            &r
        ),
        Value::Text("x-2".into())
    );
    // ignore_empty=FALSE keeps the empty slot (doubled delimiter).
    assert_eq!(
        eval(
            &call(
                "TEXTJOIN",
                vec![txt("-"), Expr::Lit(Value::Bool(false)), col_range(3)]
            ),
            &r
        ),
        Value::Text("x--2".into())
    );
    // An error anywhere propagates.
    let e = Grid::new(1, vec![Value::Number(1.0), Value::Error(ErrKind::Div0)]);
    assert_eq!(
        eval(&call("CONCAT", vec![col_range(2)]), &e),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn left_right_mid_len_clamp_and_coerce() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        text(eval(&call("LEFT", vec![txt("hello"), num(2.0)]), &g)),
        "he"
    );
    assert_eq!(text(eval(&call("LEFT", vec![txt("hi")]), &g)), "h"); // default 1
    assert_eq!(
        text(eval(&call("RIGHT", vec![txt("hello"), num(3.0)]), &g)),
        "llo"
    );
    // Out-of-range count clamps to the whole string.
    assert_eq!(
        text(eval(&call("LEFT", vec![txt("hi"), num(99.0)]), &g)),
        "hi"
    );
    // Negative count is #VALUE!.
    assert_eq!(
        eval(&call("LEFT", vec![txt("hi"), num(-1.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    // MID from 1-based start, clamped take; start past end → "".
    assert_eq!(
        text(eval(
            &call("MID", vec![txt("hello"), num(2.0), num(3.0)]),
            &g
        )),
        "ell"
    );
    assert_eq!(
        text(eval(
            &call("MID", vec![txt("hello"), num(10.0), num(3.0)]),
            &g
        )),
        ""
    );
    assert_eq!(
        eval(&call("MID", vec![txt("hi"), num(0.0), num(1.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    // LEN over the general text of non-text values.
    assert_eq!(
        eval(&call("LEN", vec![txt("hello")]), &g),
        Value::Number(5.0)
    );
    assert_eq!(
        eval(&call("LEN", vec![Expr::Lit(Value::Bool(true))]), &g),
        Value::Number(4.0)
    );
}

#[test]
fn find_is_case_sensitive_search_is_wildcard() {
    let g = Grid::new(1, vec![Value::Blank]);
    // FIND: 1-based, case-SENSITIVE.
    assert_eq!(
        eval(&call("FIND", vec![txt("l"), txt("hello")]), &g),
        Value::Number(3.0)
    );
    assert_eq!(
        eval(&call("FIND", vec![txt("l"), txt("hello"), num(4.0)]), &g),
        Value::Number(4.0)
    );
    // Case mismatch → not found → #VALUE!.
    assert_eq!(
        eval(&call("FIND", vec![txt("H"), txt("hello")]), &g),
        Value::Error(ErrKind::Value)
    );
    // Empty needle returns start_num.
    assert_eq!(
        eval(&call("FIND", vec![txt(""), txt("abc")]), &g),
        Value::Number(1.0)
    );
    // SEARCH: case-INSENSITIVE and wildcards.
    assert_eq!(
        eval(&call("SEARCH", vec![txt("H"), txt("hello")]), &g),
        Value::Number(1.0)
    );
    assert_eq!(
        eval(&call("SEARCH", vec![txt("l?o"), txt("hello")]), &g),
        Value::Number(3.0)
    );
    assert_eq!(
        eval(&call("SEARCH", vec![txt("e*o"), txt("hello")]), &g),
        Value::Number(2.0)
    );
    // A literal `?` via `~?`.
    assert_eq!(
        eval(&call("SEARCH", vec![txt("~?"), txt("a?b")]), &g),
        Value::Number(2.0)
    );
    // start_num past len+1 is #VALUE!.
    assert_eq!(
        eval(&call("FIND", vec![txt("a"), txt("abc"), num(5.0)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn search_multi_star_matches_and_terminates_fast() {
    // Regression: the wildcard matcher is iterative (single-star backtrack), so a MULTI-star
    // pattern is O(text·pattern), not the old exponential recursive backtracking (a ReDoS). Both
    // arms complete instantly; the no-match arm is the one that used to blow up.
    let g = Grid::new(1, vec![Value::Blank]);
    // Multi-star, leftmost anchored match at position 1.
    assert_eq!(
        eval(&call("SEARCH", vec![txt("h*o*d"), txt("hello world")]), &g),
        Value::Number(1.0)
    );
    // The pathological shape: many stars over a long run with NO final match → #VALUE!, fast.
    let hay = "a".repeat(64);
    assert_eq!(
        eval(&call("SEARCH", vec![txt("*a*a*a*a*a*z"), txt(&hay)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn substitute_and_replace() {
    let g = Grid::new(1, vec![Value::Blank]);
    // SUBSTITUTE all occurrences.
    assert_eq!(
        text(eval(
            &call("SUBSTITUTE", vec![txt("a-b-c"), txt("-"), txt("+")]),
            &g
        )),
        "a+b+c"
    );
    // SUBSTITUTE the Nth only.
    assert_eq!(
        text(eval(
            &call(
                "SUBSTITUTE",
                vec![txt("a-b-c"), txt("-"), txt("+"), num(2.0)]
            ),
            &g
        )),
        "a-b+c"
    );
    // Empty old_text returns text unchanged.
    assert_eq!(
        text(eval(
            &call("SUBSTITUTE", vec![txt("abc"), txt(""), txt("X")]),
            &g
        )),
        "abc"
    );
    // instance_num < 1 is #VALUE!.
    assert_eq!(
        eval(
            &call("SUBSTITUTE", vec![txt("a-b"), txt("-"), txt("+"), num(0.0)]),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
    // REPLACE positional splice.
    assert_eq!(
        text(eval(
            &call("REPLACE", vec![txt("abcdef"), num(2.0), num(3.0), txt("X")]),
            &g
        )),
        "aXef"
    );
    // start past end appends.
    assert_eq!(
        text(eval(
            &call("REPLACE", vec![txt("ab"), num(9.0), num(0.0), txt("!")]),
            &g
        )),
        "ab!"
    );
}

#[test]
fn trim_upper_lower() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(text(eval(&call("TRIM", vec![txt("  a   b  ")]), &g)), "a b");
    assert_eq!(text(eval(&call("UPPER", vec![txt("aBc")]), &g)), "ABC");
    assert_eq!(text(eval(&call("LOWER", vec![txt("aBc")]), &g)), "abc");
}

#[test]
fn text_format_subset_and_error_paths() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        text(eval(&call("TEXT", vec![num(12.5), txt("0.00")]), &g)),
        "12.50"
    );
    assert_eq!(
        text(eval(&call("TEXT", vec![num(-7.0), txt("0.00")]), &g)),
        "-7.00"
    );
    assert_eq!(
        text(eval(&call("TEXT", vec![num(1234567.0), txt("#,##0")]), &g)),
        "1,234,567"
    );
    assert_eq!(
        text(eval(&call("TEXT", vec![num(0.5), txt("0%")]), &g)),
        "50%"
    );
    assert_eq!(
        text(eval(&call("TEXT", vec![num(0.1234), txt("0.00%")]), &g)),
        "12.34%"
    );
    assert_eq!(
        text(eval(&call("TEXT", vec![num(5.0), txt("General")]), &g)),
        "5"
    );
    // The 1900 date system with the leap-year bug: serial 60 is the phantom 1900-02-29,
    // serial 61 is 1900-03-01, serial 44927 is 2023-01-01.
    assert_eq!(
        text(eval(
            &call("TEXT", vec![num(44927.0), txt("yyyy-mm-dd")]),
            &g
        )),
        "2023-01-01"
    );
    assert_eq!(
        text(eval(&call("TEXT", vec![num(60.0), txt("yyyy-mm-dd")]), &g)),
        "1900-02-29"
    );
    assert_eq!(
        text(eval(&call("TEXT", vec![num(61.0), txt("yyyy-mm-dd")]), &g)),
        "1900-03-01"
    );
    // Serial-band gate (regression: a large serial used to overflow `civil_from_days` — a panic
    // under overflow-checks, a wrapped nonsense date in release — instead of a located refusal).
    // The band `[1, MAX_SERIAL]` is refused as `#VALUE!` on BOTH edges plus `NaN`, never rendered.
    assert_eq!(
        eval(&call("TEXT", vec![num(1e300), txt("yyyy-mm-dd")]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("TEXT", vec![num(2_958_466.0), txt("yyyy-mm-dd")]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("TEXT", vec![num(0.0), txt("yyyy-mm-dd")]), &g),
        Value::Error(ErrKind::Value)
    );
    // The exact upper edge (9999-12-31 = serial 2958465) still renders.
    assert_eq!(
        text(eval(
            &call("TEXT", vec![num(2_958_465.0), txt("yyyy-mm-dd")]),
            &g
        )),
        "9999-12-31"
    );
    // Error propagation and a non-numeric value into a numeric format.
    assert_eq!(
        eval(
            &call(
                "TEXT",
                vec![Expr::Lit(Value::Error(ErrKind::Div0)), txt("0.00")]
            ),
            &g
        ),
        Value::Error(ErrKind::Div0)
    );
    assert_eq!(
        eval(&call("TEXT", vec![txt("abc"), txt("0.00")]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn text_unsupported_literal_format_is_a_parse_refusal() {
    use crate::parse;
    // A supported literal format parses.
    assert!(parse("=TEXT(1,\"0.00\")").is_ok());
    // An unsupported literal format is a located `unsupported-format` refusal, not a wrong guess.
    let d = parse("=TEXT(1,\"$#,##0.00\")").unwrap_err();
    assert_eq!(d.code, crate::DiagCode::UnsupportedFormat);
}

#[test]
fn text_nonliteral_format_is_accepted_and_deferred_to_eval() {
    use crate::parse;
    // Accept-under-uncertainty (ast-standards PART 6): a computed format v1 cannot vet statically
    // is NOT refused at parse — a false-reject is the cardinal sin, since it RESOLVES-to-supported
    // at runtime (real Excel accepts and computes `=TEXT(A1, B1)`).
    let expr = parse("=TEXT(1,A1)").expect("a non-literal format parses (deferred to eval)");
    // A1 resolves to a SUPPORTED format → computes (the false-reject the old blanket refusal made).
    let supported = Grid::new(1, vec![Value::Text("0.00".to_string())]);
    assert_eq!(eval(&expr, &supported), Value::Text("1.00".to_string()));
    // A1 resolves to an UNSUPPORTED format → the deferred `#VALUE!` (a false-NEGATIVE, allowed).
    let unsupported = Grid::new(1, vec![Value::Text("$#,##0.00".to_string())]);
    assert_eq!(eval(&expr, &unsupported), Value::Error(ErrKind::Value));
}
