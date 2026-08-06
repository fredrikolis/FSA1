// Concern: the fixture literal grammar | Non-concern: the record grammar, evaluating a formula | IO: (&str) -> Value; (&Value) -> String

use fsa1_ast::{ErrKind, Shape, Value};

pub fn parse(raw: &str) -> Result<Value, String> {
    let s = raw.trim();
    if s.is_empty() || s == "<blank>" {
        return Ok(Value::Blank);
    }
    if let Some(rest) = s.strip_prefix('"') {
        return parse_text(rest);
    }
    if s.starts_with('{') {
        return parse_array(s);
    }
    if s.starts_with('#') {
        return parse_error(s);
    }
    if s.eq_ignore_ascii_case("TRUE") {
        return Ok(Value::Bool(true));
    }
    if s.eq_ignore_ascii_case("FALSE") {
        return Ok(Value::Bool(false));
    }
    match s.parse::<f64>() {
        Ok(n) if n.is_finite() => Ok(Value::Number(n)),
        _ => Err(format!("not a literal: {s:?}")),
    }
}

fn parse_text(rest: &str) -> Result<Value, String> {
    let bytes = rest.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                out.push('"');
                i += 2;
                continue;
            }
            if i + 1 == bytes.len() {
                return Ok(Value::Text(out));
            }
            return Err(format!("trailing bytes after a closing quote in {rest:?}"));
        }
        let ch = rest[i..]
            .chars()
            .next()
            .ok_or_else(|| format!("bad utf-8 in {rest:?}"))?;
        out.push(ch);
        i += ch.len_utf8();
    }
    Err(format!("unterminated string literal: \"{rest}"))
}

fn parse_error(s: &str) -> Result<Value, String> {
    let kind = match s {
        "#REF!" => ErrKind::Ref,
        "#DIV/0!" => ErrKind::Div0,
        "#VALUE!" => ErrKind::Value,
        "#NAME?" => ErrKind::Name,
        "#N/A" => ErrKind::Na,
        "#NULL!" => ErrKind::Null,
        "#NUM!" => ErrKind::Num,
        "#SPILL!" => ErrKind::Spill,
        "#CALC!" => ErrKind::Calc,
        other => return Err(format!("unknown error literal: {other:?}")),
    };
    Ok(Value::Error(kind))
}

fn parse_array(s: &str) -> Result<Value, String> {
    let inner = s
        .strip_prefix('{')
        .and_then(|r| r.strip_suffix('}'))
        .ok_or_else(|| format!("malformed array literal: {s:?}"))?;
    let mut cells = Vec::new();
    let mut cols: Option<u32> = None;
    let mut rows: u32 = 0;
    for row in inner.split(';') {
        let items: Vec<&str> = row.split(',').collect();
        let width = items.len() as u32;
        match cols {
            None => cols = Some(width),
            Some(c) if c != width => {
                return Err(format!("ragged array literal (row widths differ): {s:?}"));
            }
            _ => {}
        }
        for item in items {
            let v = parse(item)?;
            if matches!(v, Value::Array(..)) {
                return Err(format!("nested array literal is not allowed: {s:?}"));
            }
            cells.push(v);
        }
        rows += 1;
    }
    Ok(Value::Array(
        Shape {
            rows,
            cols: cols.unwrap_or(0),
        },
        cells,
    ))
}

/// A diagnostic render, not a canonicalizer: a number takes Rust's shortest round-tripping form.
pub fn show(v: &Value) -> String {
    match v {
        Value::Blank => "<blank>".to_string(),
        Value::Number(n) => format!("{n}"),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Value::Text(t) => format!("{t:?}"),
        Value::Error(k) => error_text(*k).to_string(),
        Value::Array(shape, cells) => {
            let mut s = String::from("{");
            for r in 0..shape.rows {
                if r > 0 {
                    s.push(';');
                }
                for c in 0..shape.cols {
                    if c > 0 {
                        s.push(',');
                    }
                    let idx = (r * shape.cols + c) as usize;
                    s.push_str(&cells.get(idx).map(show).unwrap_or_default());
                }
            }
            s.push('}');
            s
        }
    }
}

fn error_text(k: ErrKind) -> &'static str {
    match k {
        ErrKind::Ref => "#REF!",
        ErrKind::Div0 => "#DIV/0!",
        ErrKind::Value => "#VALUE!",
        ErrKind::Name => "#NAME?",
        ErrKind::Na => "#N/A",
        ErrKind::Null => "#NULL!",
        ErrKind::Num => "#NUM!",
        ErrKind::Spill => "#SPILL!",
        ErrKind::Calc => "#CALC!",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars_round_trip_through_parse() {
        assert_eq!(parse("6").unwrap(), Value::Number(6.0));
        assert_eq!(parse(" 2.5 ").unwrap(), Value::Number(2.5));
        assert_eq!(parse("-3").unwrap(), Value::Number(-3.0));
        assert_eq!(parse("TRUE").unwrap(), Value::Bool(true));
        assert_eq!(parse("false").unwrap(), Value::Bool(false));
        assert_eq!(parse("").unwrap(), Value::Blank);
        assert_eq!(parse("<blank>").unwrap(), Value::Blank);
        assert_eq!(parse("#DIV/0!").unwrap(), Value::Error(ErrKind::Div0));
    }

    #[test]
    fn text_handles_escaping_and_spaces() {
        assert_eq!(
            parse("\"hello world\"").unwrap(),
            Value::Text("hello world".into())
        );
        assert_eq!(parse("\"a\"\"b\"").unwrap(), Value::Text("a\"b".into()));
        assert_eq!(parse("\"5\"").unwrap(), Value::Text("5".into()));
        assert!(parse("\"unterminated").is_err());
    }

    #[test]
    fn arrays_parse_row_major_and_reject_ragged() {
        let v = parse("{1,2;3,4}").unwrap();
        assert_eq!(
            v,
            Value::Array(
                Shape { rows: 2, cols: 2 },
                vec![
                    Value::Number(1.0),
                    Value::Number(2.0),
                    Value::Number(3.0),
                    Value::Number(4.0)
                ]
            )
        );
        assert!(parse("{1,2;3}").is_err());
    }

    #[test]
    fn show_is_a_readable_inverse() {
        assert_eq!(show(&Value::Number(6.0)), "6");
        assert_eq!(show(&Value::Blank), "<blank>");
        assert_eq!(show(&Value::Error(ErrKind::Value)), "#VALUE!");
        assert_eq!(show(&Value::Text("hi".into())), "\"hi\"");
    }

    #[test]
    fn a_typo_fails_fast() {
        assert!(parse("6q").is_err());
        assert!(parse("#BOGUS!").is_err());
    }
}
