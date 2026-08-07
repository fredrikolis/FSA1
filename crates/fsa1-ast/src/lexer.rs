// Concern: tokenizes a formula string, classifying each identifier-shaped lexeme | Non-concern: how tokens combine, the intersection operator, evaluation | IO: (&str) -> Vec<Token> or a Diag

use crate::a1::parse_a1;
use crate::diag::{Diag, DiagCode, Span};
use crate::value::ErrKind;

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Num(f64),
    Str(String),
    Bool(bool),
    Err(ErrKind),
    CellRef {
        col: u32,
        row: u32,
        col_abs: bool,
        row_abs: bool,
    },
    Name(String),
    Func(String),
    SheetBang(String),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Amp,
    Percent,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Colon,
    Comma,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Semicolon,
    At,
    Hash,
}

/// Excel STORES a post-OOXML function as `_xlfn.NAME` (worksheet-only: `_xlfn._xlws.NAME`) while
/// DISPLAYING the bare `NAME`. Matched exactly, lowercase; `_xlnm.` names a defined name, not this.
fn future_fn_prefix_len(s: &str) -> Option<usize> {
    const XLFN_XLWS: &str = "_xlfn._xlws.";
    const XLFN: &str = "_xlfn.";
    if s.starts_with(XLFN_XLWS) {
        Some(XLFN_XLWS.len())
    } else if s.starts_with(XLFN) {
        Some(XLFN.len())
    } else {
        None
    }
}

/// Takes a formula BODY: the caller has already stripped the leading `=`.
pub fn tokenize(src: &str) -> Result<Vec<Token>, Diag> {
    let b = src.as_bytes();
    let mut i = 0usize;
    let mut out = Vec::new();

    while i < b.len() {
        let c = b[i];

        if c == b' ' || c == b'\t' || c == b'\r' || c == b'\n' {
            i += 1;
            continue;
        }

        let start = i;

        if c.is_ascii_digit() || (c == b'.' && i + 1 < b.len() && b[i + 1].is_ascii_digit()) {
            let (tok, next) = lex_number(src, b, i)?;
            out.push(Token {
                kind: tok,
                span: Span::new(start, next),
            });
            i = next;
            continue;
        }

        if c == b'"' {
            let (s, next) = lex_string(src, b, i)?;
            out.push(Token {
                kind: TokenKind::Str(s),
                span: Span::new(start, next),
            });
            i = next;
            continue;
        }

        if c == b'#' {
            if let Some((kind, len)) = match_error_literal(&src[i..]) {
                out.push(Token {
                    kind: TokenKind::Err(kind),
                    span: Span::new(start, i + len),
                });
                i += len;
            } else {
                out.push(Token {
                    kind: TokenKind::Hash,
                    span: Span::new(start, i + 1),
                });
                i += 1;
            }
            continue;
        }

        if c == b'\'' {
            let (name, next) = lex_quoted_sheet_name(src, b, i)?;
            out.push(Token {
                kind: TokenKind::SheetBang(name),
                span: Span::new(start, next),
            });
            i = next;
            continue;
        }

        // Only before a real call: a looser guard would absorb the prefix into a Bool or a CellRef, silently changing which cell the formula reads.
        if c == b'_'
            && let Some(plen) = future_fn_prefix_len(&src[i..])
            && b.get(i + plen).is_some_and(u8::is_ascii_alphabetic)
            && let (tok @ TokenKind::Func(_), next) = lex_word(src, b, i + plen)
        {
            out.push(Token {
                kind: tok,
                span: Span::new(start, next),
            });
            i = next;
            continue;
        }

        if c == b'$' || c.is_ascii_alphabetic() {
            let (tok, next) = lex_word(src, b, i);
            out.push(Token {
                kind: tok,
                span: Span::new(start, next),
            });
            i = next;
            continue;
        }

        let (kind, len) = match c {
            b'+' => (TokenKind::Plus, 1),
            b'-' => (TokenKind::Minus, 1),
            b'*' => (TokenKind::Star, 1),
            b'/' => (TokenKind::Slash, 1),
            b'^' => (TokenKind::Caret, 1),
            b'&' => (TokenKind::Amp, 1),
            b'%' => (TokenKind::Percent, 1),
            b':' => (TokenKind::Colon, 1),
            b',' => (TokenKind::Comma, 1),
            b'(' => (TokenKind::LParen, 1),
            b')' => (TokenKind::RParen, 1),
            b'{' => (TokenKind::LBrace, 1),
            b'}' => (TokenKind::RBrace, 1),
            b';' => (TokenKind::Semicolon, 1),
            b'@' => (TokenKind::At, 1),
            b'=' => (TokenKind::Eq, 1),
            b'<' => {
                if i + 1 < b.len() && b[i + 1] == b'>' {
                    (TokenKind::Ne, 2)
                } else if i + 1 < b.len() && b[i + 1] == b'=' {
                    (TokenKind::Le, 2)
                } else {
                    (TokenKind::Lt, 1)
                }
            }
            b'>' => {
                if i + 1 < b.len() && b[i + 1] == b'=' {
                    (TokenKind::Ge, 2)
                } else {
                    (TokenKind::Gt, 1)
                }
            }
            _ => {
                let ch_len = utf8_char_len(c);
                return Err(Diag::new(
                    DiagCode::UnexpectedChar,
                    Span::new(start, (start + ch_len).min(b.len())),
                    format!("unexpected character {:?}", first_char_at(src, start)),
                ));
            }
        };
        out.push(Token {
            kind,
            span: Span::new(start, start + len),
        });
        i += len;
    }

    Ok(out)
}

/// Grammar `DIGIT* [ '.' DIGIT* ] [ ('e'|'E') ['+'|'-'] DIGIT+ ]`, at least one digit overall.
fn lex_number(src: &str, b: &[u8], mut i: usize) -> Result<(TokenKind, usize), Diag> {
    let start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        // A malformed exponent stays unconsumed, becoming a separate word the parser rejects.
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    let lexeme = &src[start..i];
    match lexeme.parse::<f64>() {
        Ok(n) if n.is_finite() => Ok((TokenKind::Num(n), i)),
        _ => Err(Diag::new(
            DiagCode::InvalidNumber,
            Span::new(start, i),
            format!("`{lexeme}` is not a finite number"),
        )),
    }
}

/// Interior `""` is an escaped `"` — Excel's convention, NOT a backslash.
fn lex_string(src: &str, b: &[u8], i: usize) -> Result<(String, usize), Diag> {
    let start = i;
    let mut j = i + 1;
    let mut out = String::new();
    while j < b.len() {
        if b[j] == b'"' {
            if j + 1 < b.len() && b[j + 1] == b'"' {
                out.push('"');
                j += 2;
                continue;
            }
            return Ok((out, j + 1));
        }
        let ch = first_char_at(src, j);
        out.push(ch);
        j += ch.len_utf8();
    }
    Err(Diag::new(
        DiagCode::UnterminatedString,
        Span::new(start, b.len()),
        "a \"-string was not closed before end of formula",
    ))
}

/// Interior `''` is an escaped `'`; the closing quote must be followed immediately by `!`.
fn lex_quoted_sheet_name(src: &str, b: &[u8], i: usize) -> Result<(String, usize), Diag> {
    let start = i;
    let mut j = i + 1;
    let mut name = String::new();
    while j < b.len() {
        if b[j] == b'\'' {
            if j + 1 < b.len() && b[j + 1] == b'\'' {
                name.push('\'');
                j += 2;
                continue;
            }
            if j + 1 < b.len() && b[j + 1] == b'!' {
                return Ok((name, j + 2));
            }
            return Err(Diag::new(
                DiagCode::MalformedSheetName,
                Span::new(start, j + 1),
                "a quoted sheet name must be followed by `!` (a cross-sheet reference)",
            ));
        }
        let ch = first_char_at(src, j);
        name.push(ch);
        j += ch.len_utf8();
    }
    Err(Diag::new(
        DiagCode::MalformedSheetName,
        Span::new(start, b.len()),
        "a '-quoted sheet name was not closed before end of formula",
    ))
}

/// Never fails: a word that is no reference, bool or call becomes a [`TokenKind::Name`].
fn lex_word(src: &str, b: &[u8], i: usize) -> (TokenKind, usize) {
    let start = i;
    let mut j = i;
    while j < b.len() {
        if b[j] == b'$' || b[j].is_ascii_alphanumeric() {
            j += 1;
        } else if b[j] == b'.' && j + 1 < b.len() && b[j + 1].is_ascii_alphabetic() {
            // Function names hold dots (`STDEV.S`); demanding a LETTER leaves `A1.5` a ref then `.5`.
            j += 1;
        } else {
            break;
        }
    }
    let w = &src[start..j];

    if j < b.len() && b[j] == b'(' {
        return (TokenKind::Func(w.to_string()), j);
    }
    if j < b.len() && b[j] == b'!' {
        return (TokenKind::SheetBang(w.to_string()), j + 1);
    }
    match w {
        "TRUE" => return (TokenKind::Bool(true), j),
        "FALSE" => return (TokenKind::Bool(false), j),
        _ => {}
    }
    // A leading-zero row (`A01`) is a bare name, never a reference.
    if let Ok(a) = parse_a1(w)
        && !a.row_had_leading_zero
    {
        return (
            TokenKind::CellRef {
                col: a.col,
                row: a.row,
                col_abs: a.col_abs,
                row_abs: a.row_abs,
            },
            j,
        );
    }
    (TokenKind::Name(w.to_string()), j)
}

/// First match wins safely: no error literal is a prefix of another. Uppercase-only.
pub(crate) fn match_error_literal(s: &str) -> Option<(ErrKind, usize)> {
    const LITS: &[(&str, ErrKind)] = &[
        ("#DIV/0!", ErrKind::Div0),
        ("#VALUE!", ErrKind::Value),
        ("#NAME?", ErrKind::Name),
        ("#NULL!", ErrKind::Null),
        ("#SPILL!", ErrKind::Spill),
        ("#CALC!", ErrKind::Calc),
        ("#REF!", ErrKind::Ref),
        ("#NUM!", ErrKind::Num),
        ("#N/A", ErrKind::Na),
    ];
    for (lit, kind) in LITS {
        if s.starts_with(lit) {
            return Some((*kind, lit.len()));
        }
    }
    None
}

/// Sizes a refusal span so it never splits a multi-byte char.
fn utf8_char_len(lead: u8) -> usize {
    match lead {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    }
}

fn first_char_at(src: &str, at: usize) -> char {
    src[at..].chars().next().unwrap_or('\u{FFFD}')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        tokenize(src).unwrap().into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn numbers_are_finite_only() {
        assert_eq!(kinds("123"), vec![TokenKind::Num(123.0)]);
        assert_eq!(kinds("3.25"), vec![TokenKind::Num(3.25)]);
        assert_eq!(kinds(".5"), vec![TokenKind::Num(0.5)]);
        assert_eq!(kinds("1.2e6"), vec![TokenKind::Num(1_200_000.0)]);
        assert_eq!(kinds("1E-3"), vec![TokenKind::Num(0.001)]);
        assert_eq!(kinds("-4"), vec![TokenKind::Minus, TokenKind::Num(4.0)]);
        assert_eq!(
            tokenize("1e999").unwrap_err().code,
            DiagCode::InvalidNumber,
            "overflow refuses; a Number is always finite"
        );
    }

    #[test]
    fn strings_use_excel_doubled_quote_escaping() {
        assert_eq!(kinds(r#""hi""#), vec![TokenKind::Str("hi".to_string())]);
        assert_eq!(
            kinds(r#""he said ""hi""""#),
            vec![TokenKind::Str("he said \"hi\"".to_string())]
        );
        assert_eq!(kinds(r#""""#), vec![TokenKind::Str(String::new())]);
        assert_eq!(
            tokenize("\"oops").unwrap_err().code,
            DiagCode::UnterminatedString
        );
    }

    #[test]
    fn booleans_and_errors_are_uppercase_only() {
        assert_eq!(kinds("TRUE"), vec![TokenKind::Bool(true)]);
        assert_eq!(kinds("FALSE"), vec![TokenKind::Bool(false)]);
        assert_eq!(kinds("true"), vec![TokenKind::Name("true".to_string())]);
        assert_eq!(kinds("#DIV/0!"), vec![TokenKind::Err(ErrKind::Div0)]);
        assert_eq!(kinds("#N/A"), vec![TokenKind::Err(ErrKind::Na)]);
        assert_eq!(kinds("#NAME?"), vec![TokenKind::Err(ErrKind::Name)]);
        assert_eq!(kinds("#SPILL!"), vec![TokenKind::Err(ErrKind::Spill)]);
    }

    #[test]
    fn hash_is_spill_when_not_an_error_literal() {
        assert_eq!(
            kinds("A1#"),
            vec![
                TokenKind::CellRef {
                    col: 0,
                    row: 0,
                    col_abs: false,
                    row_abs: false
                },
                TokenKind::Hash
            ]
        );
    }

    #[test]
    fn cell_refs_functions_names_and_sheet_prefix() {
        assert_eq!(
            kinds("$A$1"),
            vec![TokenKind::CellRef {
                col: 0,
                row: 0,
                col_abs: true,
                row_abs: true
            }]
        );
        assert_eq!(
            kinds("aa10"),
            vec![TokenKind::CellRef {
                col: 26,
                row: 9,
                col_abs: false,
                row_abs: false
            }]
        );
        assert_eq!(
            kinds("SUM("),
            vec![TokenKind::Func("SUM".to_string()), TokenKind::LParen]
        );
        assert_eq!(
            kinds("STDEV.S("),
            vec![TokenKind::Func("STDEV.S".to_string()), TokenKind::LParen]
        );
        assert_eq!(
            kinds("PERCENTILE.INC("),
            vec![
                TokenKind::Func("PERCENTILE.INC".to_string()),
                TokenKind::LParen
            ]
        );
        assert_eq!(
            kinds("A1.5"),
            vec![
                TokenKind::CellRef {
                    col: 0,
                    row: 0,
                    col_abs: false,
                    row_abs: false
                },
                TokenKind::Num(0.5)
            ]
        );
        assert_eq!(kinds("A01"), vec![TokenKind::Name("A01".to_string())]);
        assert_eq!(
            kinds("Sheet1!A1"),
            vec![
                TokenKind::SheetBang("Sheet1".to_string()),
                TokenKind::CellRef {
                    col: 0,
                    row: 0,
                    col_abs: false,
                    row_abs: false
                }
            ]
        );
    }

    #[test]
    fn quoted_sheet_names_lex_with_spaces_and_escapes() {
        assert_eq!(
            kinds("'My Sheet'!A1"),
            vec![
                TokenKind::SheetBang("My Sheet".to_string()),
                TokenKind::CellRef {
                    col: 0,
                    row: 0,
                    col_abs: false,
                    row_abs: false
                }
            ]
        );
        assert_eq!(
            kinds("'O''Brien'!B2"),
            vec![
                TokenKind::SheetBang("O'Brien".to_string()),
                TokenKind::CellRef {
                    col: 1,
                    row: 1,
                    col_abs: false,
                    row_abs: false
                }
            ]
        );
        assert_eq!(
            tokenize("'oops").unwrap_err().code,
            DiagCode::MalformedSheetName
        );
        assert_eq!(
            tokenize("'Sheet'+1").unwrap_err().code,
            DiagCode::MalformedSheetName
        );
    }

    #[test]
    fn array_literal_delimiters_lex() {
        assert_eq!(
            kinds("{1;2}"),
            vec![
                TokenKind::LBrace,
                TokenKind::Num(1.0),
                TokenKind::Semicolon,
                TokenKind::Num(2.0),
                TokenKind::RBrace,
            ]
        );
    }

    #[test]
    fn operators_longest_match() {
        assert_eq!(
            kinds("<= >= <> < > = &"),
            vec![
                TokenKind::Le,
                TokenKind::Ge,
                TokenKind::Ne,
                TokenKind::Lt,
                TokenKind::Gt,
                TokenKind::Eq,
                TokenKind::Amp,
            ]
        );
    }

    #[test]
    fn hostile_bytes_are_located_never_panic() {
        assert_eq!(tokenize("\\").unwrap_err().code, DiagCode::UnexpectedChar);
        let d = tokenize("λ").unwrap_err();
        assert_eq!(d.code, DiagCode::UnexpectedChar);
        assert_eq!(d.span, Span::new(0, 2), "the span must not split a char");
        assert_eq!(tokenize(".").unwrap_err().code, DiagCode::UnexpectedChar);
    }

    #[test]
    fn excels_stored_future_function_prefix_lexes_as_the_bare_function() {
        assert_eq!(
            kinds("_xlfn.MINIFS("),
            vec![TokenKind::Func("MINIFS".to_string()), TokenKind::LParen]
        );
        assert_eq!(
            kinds("_xlfn._xlws.FILTER("),
            vec![TokenKind::Func("FILTER".to_string()), TokenKind::LParen]
        );
        assert_eq!(
            kinds("_xlfn.FORECAST.LINEAR("),
            vec![
                TokenKind::Func("FORECAST.LINEAR".to_string()),
                TokenKind::LParen
            ]
        );
        assert_eq!(
            kinds("SUM(_xlfn.MAXIFS("),
            vec![
                TokenKind::Func("SUM".to_string()),
                TokenKind::LParen,
                TokenKind::Func("MAXIFS".to_string()),
                TokenKind::LParen
            ]
        );
    }

    #[test]
    fn only_the_future_function_prefix_is_accepted_no_other_underscore_lexeme() {
        assert_eq!(
            tokenize("_xlnm.Print_Area").unwrap_err().code,
            DiagCode::UnexpectedChar,
            "_xlnm. is the defined-name namespace, not a function namespace"
        );
        assert_eq!(
            tokenize("_xlws.FILTER(").unwrap_err().code,
            DiagCode::UnexpectedChar
        );
        assert_eq!(
            tokenize("_XLFN.MINIFS(").unwrap_err().code,
            DiagCode::UnexpectedChar,
            "the prefix is matched exactly, lowercase"
        );
        assert_eq!(
            tokenize("_xlfn.$A$1").unwrap_err().code,
            DiagCode::UnexpectedChar
        );
        assert_eq!(tokenize("_").unwrap_err().code, DiagCode::UnexpectedChar);
        assert_eq!(
            tokenize("_xlfn.").unwrap_err().code,
            DiagCode::UnexpectedChar
        );
        assert_eq!(
            tokenize("_xlfn._xlws.").unwrap_err().code,
            DiagCode::UnexpectedChar
        );
        assert_eq!(
            tokenize("_xlfn._xlfn.X(").unwrap_err().code,
            DiagCode::UnexpectedChar
        );
        assert_eq!(
            tokenize("_xlfn.日本(").unwrap_err().code,
            DiagCode::UnexpectedChar,
            "the computed offset must not split a char"
        );
        assert_eq!(
            kinds(r#""_xlfn.MINIFS""#),
            vec![TokenKind::Str("_xlfn.MINIFS".to_string())],
            "a prefix-shaped string literal is data"
        );
    }

    #[test]
    fn a_non_call_tail_is_refused_never_silently_reinterpreted() {
        for src in [
            "_xlfn.TRUE",
            "_xlfn.Sheet1!A1",
            "_xlfn.A1",
            "_xlfn.FOO",
            "_xlfn.$",
        ] {
            assert_eq!(
                tokenize(src).unwrap_err().code,
                DiagCode::UnexpectedChar,
                "{src} must refuse, not reinterpret"
            );
        }
    }

    #[test]
    fn a_future_function_tokens_span_covers_the_whole_written_lexeme() {
        let src = "_xlfn.MINIFS(A1)";
        let t = &tokenize(src).unwrap()[0];
        assert_eq!(t.kind, TokenKind::Func("MINIFS".to_string()));
        assert_eq!(&src[t.span.start..t.span.end], "_xlfn.MINIFS");
    }

    #[test]
    fn spans_are_sliceable_on_every_token() {
        let src = "SUM(A1:B2, 3.5) & \"x\"";
        for t in tokenize(src).unwrap() {
            let _ = &src[t.span.start..t.span.end];
        }
    }
}
