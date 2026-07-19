// Concern: UNIT-TEST pins for the STRING-MANIPULATION text family built-ins (CONCAT CONCATENATE TEXTJOIN LEFT RIGHT MID LEN FIND SEARCH SUBSTITUTE REPLACE REPT TRIM UPPER LOWER PROPER EXACT T CLEAN TEXTBEFORE TEXTAFTER TEXTSPLIT VALUE NUMBERVALUE CHAR CODE UNICHAR UNICODE) exercised through `FUNCS` dispatch — stringification/flattening, clamped substring extraction, case-sensitive FIND vs wildcard SEARCH (incl. the multi-star ReDoS regression), REPT's cap, PROPER word casing, EXACT case-sensitivity, T/CLEAN, delimiter split (TEXTBEFORE/AFTER/SPLIT), VALUE/NUMBERVALUE numeric-text parsing, and CHAR/CODE + UNICHAR/UNICODE round-trips | Non-concern: the value->text FORMATTING functions (TEXT/FIXED/DOLLAR — func/tests/text_format.rs pins those), the text impls (`func/text.rs`), and the shared test fixtures (the parent `tests` module owns `num`/`call`/`col_range`/`txt`/`text`) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
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
    // CONCATENATE is the legacy alias of CONCAT (same body).
    assert_eq!(
        eval(&call("CONCATENATE", vec![txt("a"), txt("b"), txt("c")]), &g),
        Value::Text("abc".into())
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
fn rept_repeats_and_caps() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        text(eval(&call("REPT", vec![txt("ab"), num(3.0)]), &g)),
        "ababab"
    );
    // Count 0 → empty; the count truncates toward zero.
    assert_eq!(text(eval(&call("REPT", vec![txt("x"), num(0.0)]), &g)), "");
    assert_eq!(
        text(eval(&call("REPT", vec![txt("x"), num(2.9)]), &g)),
        "xx"
    );
    // A negative count is #VALUE!.
    assert_eq!(
        eval(&call("REPT", vec![txt("x"), num(-1.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    // Over Excel's 32767-char cap is a located #VALUE!, never an unbounded allocation.
    assert_eq!(
        eval(&call("REPT", vec![txt("ab"), num(20000.0)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn proper_exact_value_char_code() {
    let g = Grid::new(1, vec![Value::Blank]);
    // PROPER capitalizes each word, lower-cases the rest.
    assert_eq!(
        text(eval(&call("PROPER", vec![txt("hello world")]), &g)),
        "Hello World"
    );
    assert_eq!(
        text(eval(&call("PROPER", vec![txt("a-b c'd")]), &g)),
        "A-B C'D"
    );
    // EXACT is case-SENSITIVE (unlike `=`).
    assert_eq!(
        eval(&call("EXACT", vec![txt("abc"), txt("abc")]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("EXACT", vec![txt("abc"), txt("aBc")]), &g),
        Value::Bool(false)
    );
    // VALUE parses numeric text (incl. a trailing %), passes numbers through, blank → 0.
    assert_eq!(
        eval(&call("VALUE", vec![txt("123")]), &g),
        Value::Number(123.0)
    );
    assert_eq!(
        eval(&call("VALUE", vec![txt(" 12.5 ")]), &g),
        Value::Number(12.5)
    );
    assert_eq!(
        eval(&call("VALUE", vec![txt("50%")]), &g),
        Value::Number(0.5)
    );
    assert_eq!(eval(&call("VALUE", vec![num(7.0)]), &g), Value::Number(7.0));
    // A boolean and non-numeric text are #VALUE!.
    assert_eq!(
        eval(&call("VALUE", vec![Expr::Lit(Value::Bool(true))]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("VALUE", vec![txt("abc")]), &g),
        Value::Error(ErrKind::Value)
    );
    // CHAR/CODE round-trip; CHAR out of 1..=255 is #VALUE!, CODE of "" is #VALUE!.
    assert_eq!(text(eval(&call("CHAR", vec![num(65.0)]), &g)), "A");
    assert_eq!(eval(&call("CODE", vec![txt("A")]), &g), Value::Number(65.0));
    assert_eq!(
        eval(&call("CODE", vec![txt("ABC")]), &g),
        Value::Number(65.0)
    );
    assert_eq!(
        eval(&call("CHAR", vec![num(0.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("CHAR", vec![num(256.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("CODE", vec![txt("")]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn char_code_use_the_windows_1252_code_page() {
    let g = Grid::new(1, vec![Value::Blank]);
    // The 128..=159 band is the Windows-1252 (ANSI) typographic set Excel's CHAR emits — NOT the raw
    // C1 control characters Latin-1 would give.
    for (code, ch) in [
        (128.0, "\u{20AC}"), // euro
        (133.0, "\u{2026}"), // horizontal ellipsis
        (146.0, "\u{2019}"), // right single quote
        (150.0, "\u{2013}"), // en dash
        (151.0, "\u{2014}"), // em dash
    ] {
        assert_eq!(text(eval(&call("CHAR", vec![num(code)]), &g)), ch);
        // CODE is the exact inverse across the band.
        assert_eq!(
            eval(&call("CODE", vec![txt(ch)]), &g),
            Value::Number(code),
            "CODE round-trip for {code}"
        );
    }
    // The Latin-1 identity ranges still map 1:1 (both edges of the 160..=255 band).
    assert_eq!(text(eval(&call("CHAR", vec![num(233.0)]), &g)), "é");
    assert_eq!(
        eval(&call("CODE", vec![txt("é")]), &g),
        Value::Number(233.0)
    );
    // A character outside the code page is CODE 63 ('?'), matching Excel.
    assert_eq!(eval(&call("CODE", vec![txt("☃")]), &g), Value::Number(63.0));
}

#[test]
fn value_parses_money_thousands_percent_and_parens() {
    let g = Grid::new(1, vec![Value::Blank]);
    // Thousands separators and a leading currency symbol are stripped.
    assert_eq!(
        eval(&call("VALUE", vec![txt("1,000")]), &g),
        Value::Number(1000.0)
    );
    assert_eq!(
        eval(&call("VALUE", vec![txt("$5")]), &g),
        Value::Number(5.0)
    );
    assert_eq!(
        eval(&call("VALUE", vec![txt("$1,234.50")]), &g),
        Value::Number(1234.5)
    );
    // Accounting-style parentheses are a negative (currency inside the parens is fine too).
    assert_eq!(
        eval(&call("VALUE", vec![txt("(123)")]), &g),
        Value::Number(-123.0)
    );
    assert_eq!(
        eval(&call("VALUE", vec![txt("($1,000)")]), &g),
        Value::Number(-1000.0)
    );
    // A malformed grouping is refused (a false-negative #VALUE!, never a silent misread).
    assert_eq!(
        eval(&call("VALUE", vec![txt("1,00,0")]), &g),
        Value::Error(ErrKind::Value)
    );
    // en-US currency is a LEADING `$` only: a trailing `$` is outside the subset, so `VALUE("5$")`
    // is a false-negative #VALUE! rather than a wrong `5`.
    assert_eq!(
        eval(&call("VALUE", vec![txt("5$")]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn value_parses_iso_dates_and_clock_times() {
    let g = Grid::new(1, vec![Value::Blank]);
    // A yyyy-mm-dd date maps to its 1900-system serial (2020-01-01 = 43831, cross-checked in the date
    // fixtures); 2023-01-01 = 44927.
    assert_eq!(
        eval(&call("VALUE", vec![txt("2020-01-01")]), &g),
        Value::Number(43831.0)
    );
    // A clock time is a day fraction (noon = 0.5); seconds are optional.
    assert_eq!(
        eval(&call("VALUE", vec![txt("12:00")]), &g),
        Value::Number(0.5)
    );
    assert_eq!(
        eval(&call("VALUE", vec![txt("06:00:00")]), &g),
        Value::Number(0.25)
    );
    // Date and time together sum to a fractional serial.
    assert_eq!(
        eval(&call("VALUE", vec![txt("2023-01-01 12:00")]), &g),
        Value::Number(44927.5)
    );
    // Out-of-range date/time fields are #VALUE!.
    assert_eq!(
        eval(&call("VALUE", vec![txt("2023-13-01")]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("VALUE", vec![txt("25:00")]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn t_and_clean() {
    let g = Grid::new(1, vec![Value::Blank]);
    // T returns text unchanged, else "".
    assert_eq!(text(eval(&call("T", vec![txt("hello")]), &g)), "hello");
    assert_eq!(text(eval(&call("T", vec![num(123.0)]), &g)), "");
    assert_eq!(
        text(eval(&call("T", vec![Expr::Lit(Value::Bool(true))]), &g)),
        ""
    );
    // T of an error propagates.
    assert_eq!(
        eval(&call("T", vec![Expr::Lit(Value::Error(ErrKind::Div0))]), &g),
        Value::Error(ErrKind::Div0)
    );
    // CLEAN strips control characters (< 32) — here an embedded bell (0x07) and a newline.
    assert_eq!(
        text(eval(&call("CLEAN", vec![txt("a\u{7}b\nc")]), &g)),
        "abc"
    );
}

#[test]
fn textbefore_and_textafter() {
    let g = Grid::new(1, vec![Value::Blank]);
    // Default instance 1.
    assert_eq!(
        text(eval(&call("TEXTBEFORE", vec![txt("a-b-c"), txt("-")]), &g)),
        "a"
    );
    assert_eq!(
        text(eval(&call("TEXTAFTER", vec![txt("a-b-c"), txt("-")]), &g)),
        "b-c"
    );
    // Explicit instance 2.
    assert_eq!(
        text(eval(
            &call("TEXTBEFORE", vec![txt("a-b-c"), txt("-"), num(2.0)]),
            &g
        )),
        "a-b"
    );
    assert_eq!(
        text(eval(
            &call("TEXTAFTER", vec![txt("a-b-c"), txt("-"), num(2.0)]),
            &g
        )),
        "c"
    );
    // Negative instance counts from the end.
    assert_eq!(
        text(eval(
            &call("TEXTBEFORE", vec![txt("a-b-c"), txt("-"), num(-1.0)]),
            &g
        )),
        "a-b"
    );
    // Case-insensitive match_mode = 1.
    assert_eq!(
        text(eval(
            &call(
                "TEXTAFTER",
                vec![txt("aXbxc"), txt("x"), num(1.0), num(1.0)]
            ),
            &g
        )),
        "bxc"
    );
    // Not found → #N/A by default, or the if_not_found fallback (arg 6).
    assert_eq!(
        eval(&call("TEXTBEFORE", vec![txt("abc"), txt("-")]), &g),
        Value::Error(ErrKind::Na)
    );
    assert_eq!(
        text(eval(
            &call(
                "TEXTAFTER",
                vec![
                    txt("abc"),
                    txt("-"),
                    num(1.0),
                    num(0.0),
                    num(0.0),
                    txt("none")
                ]
            ),
            &g
        )),
        "none"
    );
    // instance 0 is #VALUE!.
    assert_eq!(
        eval(
            &call("TEXTBEFORE", vec![txt("a-b"), txt("-"), num(0.0)]),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
    // if_not_found is a PLAIN argument (eager): an error-valued fallback surfaces even when the
    // delimiter IS found — TEXTBEFORE is not a lazy IFERROR-family function.
    assert_eq!(
        eval(
            &call(
                "TEXTBEFORE",
                vec![
                    txt("a-b"),
                    txt("-"),
                    num(1.0),
                    Expr::Lit(Value::Blank),
                    Expr::Lit(Value::Blank),
                    Expr::Lit(Value::Error(ErrKind::Div0))
                ]
            ),
            &g
        ),
        Value::Error(ErrKind::Div0)
    );
    // As an argument-evaluation error it also precedes the body's instance==0 refusal.
    assert_eq!(
        eval(
            &call(
                "TEXTAFTER",
                vec![
                    txt("a-b"),
                    txt("-"),
                    num(0.0),
                    Expr::Lit(Value::Blank),
                    Expr::Lit(Value::Blank),
                    Expr::Lit(Value::Error(ErrKind::Div0))
                ]
            ),
            &g
        ),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn textsplit_builds_an_array() {
    let g = Grid::new(1, vec![Value::Blank]);
    // A single column delimiter → a 1×3 row array; a single-cell eval collapses to the top-left.
    assert_eq!(
        eval(&call("TEXTSPLIT", vec![txt("a,b,c"), txt(",")]), &g),
        Value::Array(
            crate::value::Shape { rows: 1, cols: 3 },
            vec![t("a"), t("b"), t("c")]
        )
    );
    // A row delimiter (arg 3) makes it 2D; a ragged row pads with the default #N/A.
    assert_eq!(
        eval(
            &call("TEXTSPLIT", vec![txt("a,b;c"), txt(","), txt(";")]),
            &g
        ),
        Value::Array(
            crate::value::Shape { rows: 2, cols: 2 },
            vec![t("a"), t("b"), t("c"), Value::Error(ErrKind::Na)]
        )
    );
    // ignore_empty (arg 4) drops empty fields.
    assert_eq!(
        eval(
            &call(
                "TEXTSPLIT",
                vec![
                    txt("a,,b"),
                    txt(","),
                    Expr::Lit(Value::Blank),
                    Expr::Lit(Value::Bool(true))
                ]
            ),
            &g
        ),
        Value::Array(
            crate::value::Shape { rows: 1, cols: 2 },
            vec![t("a"), t("b")]
        )
    );
}

#[test]
fn numbervalue_parses_with_explicit_separators() {
    let g = Grid::new(1, vec![Value::Blank]);
    // Default separators (. decimal, , group).
    assert_eq!(
        eval(&call("NUMBERVALUE", vec![txt("1,234.5")]), &g),
        Value::Number(1234.5)
    );
    // A trailing % divides by 100; two trailing %% divide by 100 twice.
    assert_eq!(
        eval(&call("NUMBERVALUE", vec![txt("2.5%")]), &g),
        Value::Number(0.025)
    );
    assert_eq!(
        eval(&call("NUMBERVALUE", vec![txt("25%%")]), &g),
        Value::Number(0.0025)
    );
    // Only TRAILING percent signs count; an embedded or leading '%' is #VALUE! (oracle-pinned).
    assert_eq!(
        eval(&call("NUMBERVALUE", vec![txt("2%5")]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("NUMBERVALUE", vec![txt("%25")]), &g),
        Value::Error(ErrKind::Value)
    );
    // Custom European separators (, decimal, . group).
    assert_eq!(
        eval(
            &call("NUMBERVALUE", vec![txt("1.234,56"), txt(","), txt(".")]),
            &g
        ),
        Value::Number(1234.56)
    );
    // Embedded whitespace is ignored; an empty string is 0.
    assert_eq!(
        eval(&call("NUMBERVALUE", vec![txt(" 3 . 5 ")]), &g),
        Value::Number(3.5)
    );
    assert_eq!(
        eval(&call("NUMBERVALUE", vec![txt("")]), &g),
        Value::Number(0.0)
    );
    // A group separator right of the decimal separator is #VALUE!.
    assert_eq!(
        eval(&call("NUMBERVALUE", vec![txt("1.2,3")]), &g),
        Value::Error(ErrKind::Value)
    );
    // Equal decimal/group separators are #VALUE!.
    assert_eq!(
        eval(
            &call("NUMBERVALUE", vec![txt("1.2"), txt("."), txt(".")]),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn unichar_and_unicode_round_trip() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(text(eval(&call("UNICHAR", vec![num(65.0)]), &g)), "A");
    assert_eq!(
        text(eval(&call("UNICHAR", vec![num(8364.0)]), &g)),
        "\u{20AC}" // euro — beyond the CHAR/Win-1252 byte range
    );
    assert_eq!(
        eval(&call("UNICODE", vec![txt("A")]), &g),
        Value::Number(65.0)
    );
    assert_eq!(
        eval(&call("UNICODE", vec![txt("\u{20AC}")]), &g),
        Value::Number(8364.0)
    );
    // UNICHAR(0) and a code point in the surrogate range are #VALUE!; UNICODE("") is #VALUE!.
    assert_eq!(
        eval(&call("UNICHAR", vec![num(0.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("UNICHAR", vec![num(55296.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("UNICODE", vec![txt("")]), &g),
        Value::Error(ErrKind::Value)
    );
}
