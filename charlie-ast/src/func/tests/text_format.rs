// Concern: UNIT-TEST pins for the value->text FORMATTING functions (TEXT FIXED DOLLAR) and the Excel number-format-code engine they share (func/text_format.rs) — the number path (fixed/grouped/percent/scientific/fraction masks, multi-section sign selection, currency + literals), the date/time path (yyyy-mm-dd, custom m/d/yyyy, h:mm[:ss], AM/PM, day/month names, elapsed [h]) with the 1900 leap-bug serial band gate, and the parse-time supported-subset gate (`validate_text_format`) with its accept-under-uncertainty deferral | Non-concern: the string-manipulation built-ins (func/tests/text.rs pins those) and the shared test fixtures (the parent `tests` module owns `num`/`call`/`txt`/`text`) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
use super::*;

/// The TEXT of a supported format applied to a number literal, over a blank 1-cell grid.
fn tf(value: f64, fmt: &str) -> String {
    let g = Grid::new(1, vec![Value::Blank]);
    text(eval(&call("TEXT", vec![num(value), txt(fmt)]), &g))
}

#[test]
fn text_number_masks() {
    // Fixed decimals, leading-zero pad, thousands grouping, percent.
    assert_eq!(tf(12.5, "0.00"), "12.50");
    assert_eq!(tf(-7.0, "0.00"), "-7.00"); // one section → auto minus
    assert_eq!(tf(1234567.0, "#,##0"), "1,234,567");
    assert_eq!(tf(1234.5, "#,##0.0"), "1,234.5");
    assert_eq!(tf(5.0, "00000"), "00005");
    assert_eq!(tf(0.5, "0%"), "50%");
    assert_eq!(tf(0.1234, "0.00%"), "12.34%");
    assert_eq!(tf(0.05, "0.00%"), "5.00%");
    assert_eq!(tf(0.123, "0%"), "12%");
    // Currency literal ($) kept in place; a single-section negative gets an auto minus.
    assert_eq!(tf(1234.5, "$#,##0.00"), "$1,234.50");
    assert_eq!(tf(-1234.5, "#,##0.00"), "-1,234.50");
    // General (case-insensitive) is the value's general text.
    assert_eq!(tf(5.0, "General"), "5");
    assert_eq!(tf(2.5, "geNeRal"), "2.5");
}

#[test]
fn text_multi_section_sign_selection() {
    // Two sections: [non-negative; negative]. The negative section supplies its own parentheses
    // (magnitude rendered, no auto minus) — the lib's `-(…)` output is a known reference bug.
    assert_eq!(tf(-1234.5, "#,##0.00;(#,##0.00)"), "(1,234.50)");
    assert_eq!(tf(1234.5, "#,##0.00;(#,##0.00)"), "1,234.50");
    // Four sections [pos;neg;zero;text]; an EMPTY section renders "". `0;;;` hides negatives & zero.
    assert_eq!(tf(0.0, "0;;;"), "");
    assert_eq!(tf(-5.0, "0;;;"), "");
    assert_eq!(tf(5.0, "0;;;"), "5");
    // A text value with a 4th (@) section renders through it; without a text section, non-coercible
    // text passes through UNCHANGED (a numeric format applies only to numbers).
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
    // Numeric-looking text still coerces into a numeric format.
    assert_eq!(
        text(eval(&call("TEXT", vec![txt("123"), txt("0.00")]), &g)),
        "123.00"
    );
}

#[test]
fn text_scientific_and_fraction() {
    // Scientific: one integer digit, two fraction, a signed two-digit exponent.
    assert_eq!(tf(12345.678, "0.00E+00"), "1.23E+04");
    assert_eq!(tf(0.0001234, "0.00E+00"), "1.23E-04");
    // Fractions (the reference lib does not support these — hand-verified against Excel).
    assert_eq!(tf(0.5, "?/?"), "1/2");
    assert_eq!(tf(0.25, "?/?"), "1/4");
    assert_eq!(tf(2.5, "# ?/?"), "2 1/2");
    // A two-digit denominator budget finds the closest fitting fraction (0.3 → 3/10, not 1/3).
    assert_eq!(tf(0.3, "?/??"), "3/10");
    assert_eq!(tf(0.333, "?/?"), "1/3"); // one-digit budget → the closest single-digit denominator
    // CORE2: a pathological denominator run (20+ `?` overflows `10^n`; a moderate run would hang a
    // 1..=max_den scan) must still resolve to the closest fraction — never panic, never hang.
    assert_eq!(tf(0.5, "?/????????????????????"), "1/2");
    assert_eq!(tf(0.25, "?/??????????????"), "1/4");
}

#[test]
fn text_date_and_time_masks() {
    // yyyy-mm-dd (the original subset) and custom masks. 44927 = 2023-01-01 (a Sunday).
    assert_eq!(tf(44927.0, "yyyy-mm-dd"), "2023-01-01");
    assert_eq!(tf(44927.0, "m/d/yyyy"), "1/1/2023");
    assert_eq!(tf(44927.0, "mmm d, yyyy"), "Jan 1, 2023");
    assert_eq!(tf(44927.0, "mmmm"), "January");
    assert_eq!(tf(44927.0, "dddd"), "Sunday");
    assert_eq!(tf(44927.0, "ddd"), "Sun");
    // Time of day from the fraction; month-vs-minute resolved by neighbours.
    assert_eq!(tf(0.5, "h:mm"), "12:00");
    assert_eq!(tf(0.5, "h:mm:ss"), "12:00:00");
    assert_eq!(tf(44927.75, "yyyy-mm-dd hh:mm"), "2023-01-01 18:00");
    // 12-hour clock with AM/PM (the lib mis-renders this — hand-verified).
    assert_eq!(tf(0.75, "h:mm AM/PM"), "6:00 PM");
    assert_eq!(tf(0.25, "h:mm AM/PM"), "6:00 AM");
    // Elapsed hours (the whole serial as a duration).
    assert_eq!(tf(1.5, "[h]:mm"), "36:00");
    // The leap-bug boundary is preserved: serial 60 is the phantom 1900-02-29, 61 is 1900-03-01.
    assert_eq!(tf(60.0, "yyyy-mm-dd"), "1900-02-29");
    assert_eq!(tf(61.0, "yyyy-mm-dd"), "1900-03-01");
}

#[test]
fn text_date_serial_band_gate() {
    let g = Grid::new(1, vec![Value::Blank]);
    // A date mask requires a serial in [1, MAX_SERIAL]; out-of-band is a located #VALUE!, never a
    // panic (regression: a huge serial once overflowed the civil-date conversion).
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
    // The exact upper edge (9999-12-31 = serial 2958465) still renders.
    assert_eq!(tf(2_958_465.0, "yyyy-mm-dd"), "9999-12-31");
    // A pure-time mask has no date field, so no band gate (a 0.5 fraction is noon).
    assert_eq!(tf(0.5, "h:mm"), "12:00");
}

#[test]
fn text_quoted_and_escaped_literal_sections() {
    // A section may be (or contain) a QUOTED literal run or a BACKSLASH-escaped char, rendered
    // verbatim around the number placeholders — these once errored as "unsupported" (RESIDUAL).
    // The ZERO section is a quoted-literal-only run: 0 selects it and shows just the literal.
    assert_eq!(tf(0.0, "0;-0;\"zero\""), "zero");
    // The positive section leads with a quoted literal, then a digit placeholder.
    assert_eq!(tf(5.0, "\"n=\"0"), "n=5");
    // A backslash escape renders the next char literally (here a leading `$` sign wart, then digits).
    assert_eq!(tf(5.0, "\\$0.00"), "$5.00");
    assert_eq!(tf(12.0, "0\\x"), "12x");
    // The negative section's own literals supply the sign: `-0` shows a literal `-` + the magnitude,
    // and `(0)` wraps the magnitude in parentheses (no auto minus from the engine).
    assert_eq!(tf(-3.0, "0;-0;"), "-3");
    assert_eq!(tf(-5.0, "0;(0)"), "(5)");
    // An empty negative section (2nd of `0;-0;`) is irrelevant here; the zero section (3rd, empty)
    // hides zero. A quoted `;` inside a run is a literal, not a section break.
    assert_eq!(tf(0.0, "0;0;\"a;b\""), "a;b");
    // Regressions: the plain numeric masks still render unchanged.
    assert_eq!(tf(0.25, "0%"), "25%");
    assert_eq!(tf(1234.5, "#,##0.00"), "1,234.50");
}

#[test]
fn text_text_section_selection() {
    let g = Grid::new(1, vec![Value::Blank]);
    let tt = |val: &str, fmt: &str| text(eval(&call("TEXT", vec![txt(val), txt(fmt)]), &g));
    // A lone `@` is a text section wherever it sits — `@` is the input text (was a #VALUE! gap).
    assert_eq!(tt("hi", "@"), "hi");
    // An `@`-bearing section anywhere is the text section; literals around `@` render verbatim.
    assert_eq!(tt("hi", "\"pre \"@\" post\""), "pre hi post");
    // The 4th section of a four-section code is the text section even for numeric-looking text.
    assert_eq!(tt("hi", "0;;;@"), "hi");
    assert_eq!(tt("123", "0.00;-0.00;0;@"), "123");
    // A `\@` (escaped) is a literal `@`, not the placeholder — the 4th section still governs text.
    assert_eq!(tt("hi", "0;;;\\@"), "@");
    // No text section (fewer than four sections, no `@`): numeric-looking text coerces and renders
    // through the numeric path; non-coercible text passes through UNCHANGED (a numeric format applies
    // only to numbers, so already-text is returned verbatim — matches Excel & the formulas reference).
    assert_eq!(tt("123", "0.00"), "123.00");
    assert_eq!(tt("5", "000"), "005");
    assert_eq!(tt("abc", "0.00"), "abc");
    // A four-section code with an empty text section hides text (the `;;;` idiom).
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
    // no_commas = TRUE.
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
    // decimals defaults to 2.
    assert_eq!(
        text(eval(&call("FIXED", vec![num(1234.567)]), &g)),
        "1,234.57"
    );
    // A negative decimals count rounds left of the point.
    assert_eq!(
        text(eval(&call("FIXED", vec![num(1234.5), num(-2.0)]), &g)),
        "1,200"
    );
    // A negative value keeps its leading minus.
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
    // A negative is wrapped in parentheses, no leading minus.
    assert_eq!(
        text(eval(&call("DOLLAR", vec![num(-1234.567), num(2.0)]), &g)),
        "($1,234.57)"
    );
    // decimals defaults to 2.
    assert_eq!(
        text(eval(&call("DOLLAR", vec![num(1234.567)]), &g)),
        "$1,234.57"
    );
    // A negative decimals count rounds left of the point.
    assert_eq!(
        text(eval(&call("DOLLAR", vec![num(1234.5), num(-2.0)]), &g)),
        "$1,200"
    );
}

#[test]
fn fixed_dollar_decimals_bounded_and_excel_exact() {
    // REGRESSION (CORE2): an unbounded user `decimals` must never hang or emit garbage. Excel caps
    // FIXED/DOLLAR at 127 fractional places; beyond that both refuse with #VALUE! (oracle-pinned:
    // FIXED(5,128)=#VALUE!, DOLLAR(5,200)=#VALUE!). A former O(n^2)/overflow blow-up on FIXED(5,1e7).
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
    // 127 is the last valid count — an exact value renders as all trailing zeros (not f64 noise).
    assert_eq!(
        text(eval(&call("FIXED", vec![num(5.0), num(127.0)]), &g)),
        format!("5.{}", "0".repeat(127))
    );
    // Excel-exact fractional expansion past f64's ~15 sig-digit precision (oracle-pinned).
    assert_eq!(
        text(eval(&call("FIXED", vec![num(5.0), num(30.0)]), &g)),
        format!("5.{}", "0".repeat(30))
    );
    assert_eq!(
        text(eval(&call("FIXED", vec![num(1.0 / 3.0), num(30.0)]), &g)),
        "0.333333333333333314829616256247"
    );
    // A large NEGATIVE decimals no longer overflows 10^|decimals| to inf → NaN; it rounds to 0.
    assert_eq!(
        text(eval(&call("FIXED", vec![num(5.0), num(-400.0)]), &g)),
        "0"
    );
}

#[test]
fn text_unsupported_literal_format_is_a_parse_refusal() {
    use crate::parse;
    // A supported literal format parses — including the ones the widened subset now covers.
    assert!(parse("=TEXT(1,\"0.00\")").is_ok());
    assert!(parse("=TEXT(1,\"$#,##0.00\")").is_ok());
    assert!(parse("=TEXT(1,\"m/d/yyyy\")").is_ok());
    assert!(parse("=TEXT(1,\"[h]:mm\")").is_ok());
    // A colour/condition bracket (outside the subset) is a located `unsupported-format` refusal.
    let d = parse("=TEXT(1,\"[Red]0.00\")").unwrap_err();
    assert_eq!(d.code, crate::DiagCode::UnsupportedFormat);
    // 5+ sections is likewise unsupported (Excel caps custom formats at four sections).
    assert_eq!(
        parse("=TEXT(1,\"0;0;0;0;0\")").unwrap_err().code,
        crate::DiagCode::UnsupportedFormat
    );
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
    let unsupported = Grid::new(1, vec![Value::Text("[Red]0.00".to_string())]);
    assert_eq!(eval(&expr, &unsupported), Value::Error(ErrKind::Value));
}
