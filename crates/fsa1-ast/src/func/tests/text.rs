// Concern: pins the string built-ins | Non-concern: the formatting built-ins, the impls | IO: (Grid, Expr) -> asserted Value
use super::*;

#[test]
fn concat_and_textjoin() {
    let g = Grid::new(1, vec![Value::Blank]);
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
    assert_eq!(
        eval(&call("CONCATENATE", vec![txt("a"), txt("b"), txt("c")]), &g),
        Value::Text("abc".into())
    );
    let r = Grid::new(
        1,
        vec![Value::Text("x".into()), Value::Blank, Value::Number(2.0)],
    );
    assert_eq!(
        eval(&call("CONCAT", vec![col_range(3)]), &r),
        Value::Text("x2".into())
    );
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
    assert_eq!(
        text(eval(&call("LEFT", vec![txt("hi"), num(99.0)]), &g)),
        "hi"
    );
    assert_eq!(
        eval(&call("LEFT", vec![txt("hi"), num(-1.0)]), &g),
        Value::Error(ErrKind::Value)
    );
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
    assert_eq!(
        eval(&call("FIND", vec![txt("l"), txt("hello")]), &g),
        Value::Number(3.0)
    );
    assert_eq!(
        eval(&call("FIND", vec![txt("l"), txt("hello"), num(4.0)]), &g),
        Value::Number(4.0)
    );
    assert_eq!(
        eval(&call("FIND", vec![txt("H"), txt("hello")]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("FIND", vec![txt(""), txt("abc")]), &g),
        Value::Number(1.0)
    );
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
    assert_eq!(
        eval(&call("SEARCH", vec![txt("~?"), txt("a?b")]), &g),
        Value::Number(2.0)
    );
    assert_eq!(
        eval(&call("FIND", vec![txt("a"), txt("abc"), num(5.0)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn search_multi_star_matches_and_terminates_fast() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("SEARCH", vec![txt("h*o*d"), txt("hello world")]), &g),
        Value::Number(1.0)
    );
    let hay = "a".repeat(64);
    assert_eq!(
        eval(&call("SEARCH", vec![txt("*a*a*a*a*a*z"), txt(&hay)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn substitute_and_replace() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        text(eval(
            &call("SUBSTITUTE", vec![txt("a-b-c"), txt("-"), txt("+")]),
            &g
        )),
        "a+b+c"
    );
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
    assert_eq!(
        text(eval(
            &call("SUBSTITUTE", vec![txt("abc"), txt(""), txt("X")]),
            &g
        )),
        "abc"
    );
    assert_eq!(
        eval(
            &call("SUBSTITUTE", vec![txt("a-b"), txt("-"), txt("+"), num(0.0)]),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        text(eval(
            &call("REPLACE", vec![txt("abcdef"), num(2.0), num(3.0), txt("X")]),
            &g
        )),
        "aXef"
    );
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
    assert_eq!(text(eval(&call("REPT", vec![txt("x"), num(0.0)]), &g)), "");
    assert_eq!(
        text(eval(&call("REPT", vec![txt("x"), num(2.9)]), &g)),
        "xx"
    );
    assert_eq!(
        eval(&call("REPT", vec![txt("x"), num(-1.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("REPT", vec![txt("ab"), num(20000.0)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn proper_exact_value_char_code() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        text(eval(&call("PROPER", vec![txt("hello world")]), &g)),
        "Hello World"
    );
    assert_eq!(
        text(eval(&call("PROPER", vec![txt("a-b c'd")]), &g)),
        "A-B C'D"
    );
    assert_eq!(
        eval(&call("EXACT", vec![txt("abc"), txt("abc")]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("EXACT", vec![txt("abc"), txt("aBc")]), &g),
        Value::Bool(false)
    );
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
    assert_eq!(
        eval(&call("VALUE", vec![Expr::Lit(Value::Bool(true))]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("VALUE", vec![txt("abc")]), &g),
        Value::Error(ErrKind::Value)
    );
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
    for (code, ch) in [
        (128.0, "\u{20AC}"), // euro
        (133.0, "\u{2026}"), // horizontal ellipsis
        (146.0, "\u{2019}"), // right single quote
        (150.0, "\u{2013}"), // en dash
        (151.0, "\u{2014}"), // em dash
    ] {
        assert_eq!(text(eval(&call("CHAR", vec![num(code)]), &g)), ch);
        assert_eq!(
            eval(&call("CODE", vec![txt(ch)]), &g),
            Value::Number(code),
            "CODE round-trip for {code}"
        );
    }
    assert_eq!(text(eval(&call("CHAR", vec![num(233.0)]), &g)), "é");
    assert_eq!(
        eval(&call("CODE", vec![txt("é")]), &g),
        Value::Number(233.0)
    );
    assert_eq!(eval(&call("CODE", vec![txt("☃")]), &g), Value::Number(63.0));
}

#[test]
fn value_parses_money_thousands_percent_and_parens() {
    let g = Grid::new(1, vec![Value::Blank]);
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
    assert_eq!(
        eval(&call("VALUE", vec![txt("(123)")]), &g),
        Value::Number(-123.0)
    );
    assert_eq!(
        eval(&call("VALUE", vec![txt("($1,000)")]), &g),
        Value::Number(-1000.0)
    );
    assert_eq!(
        eval(&call("VALUE", vec![txt("1,00,0")]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("VALUE", vec![txt("5$")]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn value_parses_iso_dates_and_clock_times() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("VALUE", vec![txt("2020-01-01")]), &g),
        Value::Number(43831.0)
    );
    assert_eq!(
        eval(&call("VALUE", vec![txt("12:00")]), &g),
        Value::Number(0.5)
    );
    assert_eq!(
        eval(&call("VALUE", vec![txt("06:00:00")]), &g),
        Value::Number(0.25)
    );
    assert_eq!(
        eval(&call("VALUE", vec![txt("2023-01-01 12:00")]), &g),
        Value::Number(44927.5)
    );
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
    assert_eq!(text(eval(&call("T", vec![txt("hello")]), &g)), "hello");
    assert_eq!(text(eval(&call("T", vec![num(123.0)]), &g)), "");
    assert_eq!(
        text(eval(&call("T", vec![Expr::Lit(Value::Bool(true))]), &g)),
        ""
    );
    assert_eq!(
        eval(&call("T", vec![Expr::Lit(Value::Error(ErrKind::Div0))]), &g),
        Value::Error(ErrKind::Div0)
    );
    assert_eq!(
        text(eval(&call("CLEAN", vec![txt("a\u{7}b\nc")]), &g)),
        "abc"
    );
}

#[test]
fn textbefore_and_textafter() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        text(eval(&call("TEXTBEFORE", vec![txt("a-b-c"), txt("-")]), &g)),
        "a"
    );
    assert_eq!(
        text(eval(&call("TEXTAFTER", vec![txt("a-b-c"), txt("-")]), &g)),
        "b-c"
    );
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
    assert_eq!(
        text(eval(
            &call("TEXTBEFORE", vec![txt("a-b-c"), txt("-"), num(-1.0)]),
            &g
        )),
        "a-b"
    );
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
    assert_eq!(
        eval(
            &call("TEXTBEFORE", vec![txt("a-b"), txt("-"), num(0.0)]),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
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
    assert_eq!(
        eval(&call("TEXTSPLIT", vec![txt("a,b,c"), txt(",")]), &g),
        Value::Array(
            crate::value::Shape { rows: 1, cols: 3 },
            vec![t("a"), t("b"), t("c")]
        )
    );
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
    assert_eq!(
        eval(&call("NUMBERVALUE", vec![txt("1,234.5")]), &g),
        Value::Number(1234.5)
    );
    assert_eq!(
        eval(&call("NUMBERVALUE", vec![txt("2.5%")]), &g),
        Value::Number(0.025)
    );
    assert_eq!(
        eval(&call("NUMBERVALUE", vec![txt("25%%")]), &g),
        Value::Number(0.0025)
    );
    assert_eq!(
        eval(&call("NUMBERVALUE", vec![txt("2%5")]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("NUMBERVALUE", vec![txt("%25")]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(
            &call("NUMBERVALUE", vec![txt("1.234,56"), txt(","), txt(".")]),
            &g
        ),
        Value::Number(1234.56)
    );
    assert_eq!(
        eval(&call("NUMBERVALUE", vec![txt(" 3 . 5 ")]), &g),
        Value::Number(3.5)
    );
    assert_eq!(
        eval(&call("NUMBERVALUE", vec![txt("")]), &g),
        Value::Number(0.0)
    );
    assert_eq!(
        eval(&call("NUMBERVALUE", vec![txt("1.2,3")]), &g),
        Value::Error(ErrKind::Value)
    );
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
