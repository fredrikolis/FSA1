// Concern: pins TEXT FIXED DOLLAR and the format-code engine | Non-concern: the string built-ins, the impls | IO: (Grid, Expr) -> asserted Value
use super::*;

/// The TEXT of a supported format applied to a number literal, over a blank 1-cell grid.
fn tf(value: f64, fmt: &str) -> String {
    let g = Grid::new(1, vec![Value::Blank]);
    text(eval(&call("TEXT", vec![num(value), txt(fmt)]), &g))
}

#[test]
fn text_number_masks() {
    assert_eq!(tf(12.5, "0.00"), "12.50");
    assert_eq!(tf(-7.0, "0.00"), "-7.00"); // one section → auto minus
    assert_eq!(tf(1234567.0, "#,##0"), "1,234,567");
    assert_eq!(tf(1234.5, "#,##0.0"), "1,234.5");
    assert_eq!(tf(5.0, "00000"), "00005");
    assert_eq!(tf(0.5, "0%"), "50%");
    assert_eq!(tf(0.1234, "0.00%"), "12.34%");
    assert_eq!(tf(0.05, "0.00%"), "5.00%");
    assert_eq!(tf(0.123, "0%"), "12%");
    assert_eq!(tf(1234.5, "$#,##0.00"), "$1,234.50");
    assert_eq!(tf(-1234.5, "#,##0.00"), "-1,234.50");
    assert_eq!(tf(5.0, "General"), "5");
    assert_eq!(tf(2.5, "geNeRal"), "2.5");
}

#[test]
fn text_multi_section_sign_selection() {
    assert_eq!(tf(-1234.5, "#,##0.00;(#,##0.00)"), "(1,234.50)");
    assert_eq!(tf(1234.5, "#,##0.00;(#,##0.00)"), "1,234.50");
    assert_eq!(tf(0.0, "0;;;"), "");
    assert_eq!(tf(-5.0, "0;;;"), "");
    assert_eq!(tf(5.0, "0;;;"), "5");
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        text(eval(
            &call("TEXT", vec![txt("hi"), txt("0;0;0;\"<\"@\">\"")]),
            &g
        )),
        "<hi>"
    );
    assert_eq!(
        text(eval(&call("TEXT", vec![txt("abc"), txt("0.00")]), &g)),
        "abc"
    );
    assert_eq!(
        text(eval(&call("TEXT", vec![txt("123"), txt("0.00")]), &g)),
        "123.00"
    );
}

#[test]
fn text_scientific_and_fraction() {
    assert_eq!(tf(12345.678, "0.00E+00"), "1.23E+04");
    assert_eq!(tf(0.0001234, "0.00E+00"), "1.23E-04");
    assert_eq!(tf(0.5, "?/?"), "1/2");
    assert_eq!(tf(0.25, "?/?"), "1/4");
    assert_eq!(tf(2.5, "# ?/?"), "2 1/2");
    assert_eq!(tf(0.3, "?/??"), "3/10");
    assert_eq!(tf(0.333, "?/?"), "1/3"); // one-digit budget → the closest single-digit denominator
    assert_eq!(tf(0.5, "?/????????????????????"), "1/2");
    assert_eq!(tf(0.25, "?/??????????????"), "1/4");
}

#[test]
fn text_date_and_time_masks() {
    assert_eq!(tf(44927.0, "yyyy-mm-dd"), "2023-01-01");
    assert_eq!(tf(44927.0, "m/d/yyyy"), "1/1/2023");
    assert_eq!(tf(44927.0, "mmm d, yyyy"), "Jan 1, 2023");
    assert_eq!(tf(44927.0, "mmmm"), "January");
    assert_eq!(tf(44927.0, "dddd"), "Sunday");
    assert_eq!(tf(44927.0, "ddd"), "Sun");
    assert_eq!(tf(0.5, "h:mm"), "12:00");
    assert_eq!(tf(0.5, "h:mm:ss"), "12:00:00");
    assert_eq!(tf(44927.75, "yyyy-mm-dd hh:mm"), "2023-01-01 18:00");
    assert_eq!(tf(0.75, "h:mm AM/PM"), "6:00 PM");
    assert_eq!(tf(0.25, "h:mm AM/PM"), "6:00 AM");
    assert_eq!(tf(1.5, "[h]:mm"), "36:00");
    assert_eq!(tf(60.0, "yyyy-mm-dd"), "1900-02-29");
    assert_eq!(tf(61.0, "yyyy-mm-dd"), "1900-03-01");
}

#[test]
fn text_date_serial_band_gate() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("TEXT", vec![num(1e300), txt("yyyy-mm-dd")]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("TEXT", vec![num(2_958_466.0), txt("m/d/yyyy")]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("TEXT", vec![num(0.0), txt("yyyy-mm-dd")]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(tf(2_958_465.0, "yyyy-mm-dd"), "9999-12-31");
    assert_eq!(tf(0.5, "h:mm"), "12:00");
}

#[test]
fn text_quoted_and_escaped_literal_sections() {
    assert_eq!(tf(0.0, "0;-0;\"zero\""), "zero");
    assert_eq!(tf(5.0, "\"n=\"0"), "n=5");
    assert_eq!(tf(5.0, "\\$0.00"), "$5.00");
    assert_eq!(tf(12.0, "0\\x"), "12x");
    assert_eq!(tf(-3.0, "0;-0;"), "-3");
    assert_eq!(tf(-5.0, "0;(0)"), "(5)");
    assert_eq!(tf(0.0, "0;0;\"a;b\""), "a;b");
    assert_eq!(tf(0.25, "0%"), "25%");
    assert_eq!(tf(1234.5, "#,##0.00"), "1,234.50");
}

#[test]
fn text_text_section_selection() {
    let g = Grid::new(1, vec![Value::Blank]);
    let tt = |val: &str, fmt: &str| text(eval(&call("TEXT", vec![txt(val), txt(fmt)]), &g));
    assert_eq!(tt("hi", "@"), "hi");
    assert_eq!(tt("hi", "\"pre \"@\" post\""), "pre hi post");
    assert_eq!(tt("hi", "0;;;@"), "hi");
    assert_eq!(tt("123", "0.00;-0.00;0;@"), "123");
    assert_eq!(tt("hi", "0;;;\\@"), "@");
    assert_eq!(tt("123", "0.00"), "123.00");
    assert_eq!(tt("5", "000"), "005");
    assert_eq!(tt("abc", "0.00"), "abc");
    assert_eq!(tt("hi", "General;;;"), "");
}

#[test]
fn text_error_propagation() {
    let g = Grid::new(1, vec![Value::Blank]);
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
}

#[test]
fn fixed_rounds_and_groups() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        text(eval(&call("FIXED", vec![num(1234.567), num(2.0)]), &g)),
        "1,234.57"
    );
    assert_eq!(
        text(eval(
            &call(
                "FIXED",
                vec![num(1234.567), num(2.0), Expr::Lit(Value::Bool(true))]
            ),
            &g
        )),
        "1234.57"
    );
    assert_eq!(
        text(eval(&call("FIXED", vec![num(1234.567)]), &g)),
        "1,234.57"
    );
    assert_eq!(
        text(eval(&call("FIXED", vec![num(1234.5), num(-2.0)]), &g)),
        "1,200"
    );
    assert_eq!(
        text(eval(&call("FIXED", vec![num(-1234.5), num(1.0)]), &g)),
        "-1,234.5"
    );
}

#[test]
fn dollar_formats_currency_with_parens_for_negatives() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        text(eval(&call("DOLLAR", vec![num(1234.567), num(2.0)]), &g)),
        "$1,234.57"
    );
    assert_eq!(
        text(eval(&call("DOLLAR", vec![num(-1234.567), num(2.0)]), &g)),
        "($1,234.57)"
    );
    assert_eq!(
        text(eval(&call("DOLLAR", vec![num(1234.567)]), &g)),
        "$1,234.57"
    );
    assert_eq!(
        text(eval(&call("DOLLAR", vec![num(1234.5), num(-2.0)]), &g)),
        "$1,200"
    );
}

#[test]
fn fixed_dollar_decimals_bounded_and_excel_exact() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("FIXED", vec![num(5.0), num(10_000_000.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("FIXED", vec![num(5.0), num(128.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("FIXED", vec![num(5.0), num(400.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("DOLLAR", vec![num(5.0), num(200.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        text(eval(&call("FIXED", vec![num(5.0), num(127.0)]), &g)),
        format!("5.{}", "0".repeat(127))
    );
    assert_eq!(
        text(eval(&call("FIXED", vec![num(5.0), num(30.0)]), &g)),
        format!("5.{}", "0".repeat(30))
    );
    assert_eq!(
        text(eval(&call("FIXED", vec![num(1.0 / 3.0), num(30.0)]), &g)),
        "0.333333333333333314829616256247"
    );
    assert_eq!(
        text(eval(&call("FIXED", vec![num(5.0), num(-400.0)]), &g)),
        "0"
    );
}

#[test]
fn text_unsupported_literal_format_is_a_parse_refusal() {
    use crate::parse;
    assert!(parse("=TEXT(1,\"0.00\")").is_ok());
    assert!(parse("=TEXT(1,\"$#,##0.00\")").is_ok());
    assert!(parse("=TEXT(1,\"m/d/yyyy\")").is_ok());
    assert!(parse("=TEXT(1,\"[h]:mm\")").is_ok());
    let d = parse("=TEXT(1,\"[Red]0.00\")").unwrap_err();
    assert_eq!(d.code, crate::DiagCode::UnsupportedFormat);
    assert_eq!(
        parse("=TEXT(1,\"0;0;0;0;0\")").unwrap_err().code,
        crate::DiagCode::UnsupportedFormat
    );
}

#[test]
fn text_nonliteral_format_is_accepted_and_deferred_to_eval() {
    use crate::parse;
    let expr = parse("=TEXT(1,A1)").expect("a non-literal format parses (deferred to eval)");
    let supported = Grid::new(1, vec![Value::Text("0.00".to_string())]);
    assert_eq!(eval(&expr, &supported), Value::Text("1.00".to_string()));
    let unsupported = Grid::new(1, vec![Value::Text("[Red]0.00".to_string())]);
    assert_eq!(eval(&expr, &unsupported), Value::Error(ErrKind::Value));
}
