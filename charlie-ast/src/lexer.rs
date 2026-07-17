// Concern: the formula LEXER — turn a formula string into a flat `Vec<Token>` (each a `TokenKind` + byte `Span`), recognizing numbers (§4.3 finite grammar), `"`-strings (Excel `""` escaping), UPPERCASE-only `TRUE`/`FALSE` and the `#…!` error literals (deliberate, no case-folding), A1 cell references via `charlie_ast::a1`, the operator/paren/comma vocabulary, function-name-before-`(`, the `!` sheet-separator of a cross-sheet reference as a `TokenKind::SheetBang(name)` (bare `Sheet1!` and quoted `'My Sheet'!` names via `lex_quoted_sheet_name`, with `''` escapes and a `MalformedSheetName` refusal on an unterminated/`!`-less name), and the reserved `@`/`#` markers; a hostile byte is a located refusal, never a panic | Non-concern: how tokens NEST into an `Expr` (parser.rs owns precedence/arity/reserved-construct verdicts) and evaluating anything | IO: (a formula `&str`) -> `Result<Vec<Token>, Diag>`
//! The formula lexer: [`tokenize`] a formula string into [`Token`]s.
//!
//! ASCII-oriented and single-pass. It never panics on hostile input — every byte either extends a
//! token or produces a located [`Diag`] (ast-standards PART 5, "the parser is the one defended
//! boundary; never unwind"). Classification of an identifier-shaped lexeme (is it a cell reference,
//! a function name, `TRUE`/`FALSE`, or a bare/reserved name?) is resolved *here* so the parser reads
//! a clean token stream; the parser owns only how those tokens combine.

use crate::a1::parse_a1;
use crate::diag::{Diag, DiagCode, Span};
use crate::value::ErrKind;

/// One lexed token: what it is, and where it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// The lexical vocabulary. Identifier-shaped lexemes are pre-classified (see the module note): a
/// well-formed A1 address becomes [`TokenKind::CellRef`], a word followed immediately by `(` becomes
/// [`TokenKind::Func`], `TRUE`/`FALSE` become [`TokenKind::Bool`], a word followed by `!` becomes
/// [`TokenKind::SheetBang`] (reserved cross-sheet), and anything else identifier-shaped becomes
/// [`TokenKind::Name`] (a bare/reserved defined-name the parser refuses).
#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    /// A finite numeric literal (unsigned — a leading `-` is a separate unary operator).
    Num(f64),
    /// A `"`-quoted string literal, already unescaped (Excel `""` → `"`).
    Str(String),
    /// `TRUE` / `FALSE` (UPPERCASE-only, deliberate — no case-folding).
    Bool(bool),
    /// One of the seven live error literals, or the two reserved (`#SPILL!`/`#CALC!`) — round-tripped
    /// as first-class [`crate::Value::Error`] values (uppercase-only).
    Err(ErrKind),
    /// A resolved single-cell A1 reference lexeme (`A1`, `$A$1`) — column/row zero-based.
    CellRef {
        col: u32,
        row: u32,
        col_abs: bool,
        row_abs: bool,
    },
    /// An identifier-shaped lexeme that is *not* a cell ref / bool / func-with-paren — a bare
    /// defined-name (reserved in v1).
    Name(String),
    /// A word immediately followed by `(` — a function name (the `(` is a separate token).
    Func(String),
    /// A word immediately followed by `!` — a sheet name of a cross-sheet reference (reserved).
    SheetBang(String),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    /// `&` string concatenation.
    Amp,
    /// `%` postfix percent.
    Percent,
    /// `=` equal.
    Eq,
    /// `<>` not-equal.
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// `:` range operator.
    Colon,
    /// `,` union operator / argument separator.
    Comma,
    LParen,
    RParen,
    /// `@` prefix implicit-intersection (reserved node).
    At,
    /// `#` postfix spill operator (reserved node) — only when it is *not* the start of an error
    /// literal (those lex to [`TokenKind::Err`]).
    Hash,
}

/// Tokenize a formula body (with its leading `=` already stripped by the caller). Never panics.
pub fn tokenize(src: &str) -> Result<Vec<Token>, Diag> {
    let b = src.as_bytes();
    let mut i = 0usize;
    let mut out = Vec::new();

    while i < b.len() {
        let c = b[i];

        // Whitespace: insignificant to token boundaries. (The reserved *intersection* operator —
        // significant whitespace between two references — is detected by the PARSER from adjacent
        // primary tokens, not encoded here; scope.md defers intersection.)
        if c == b' ' || c == b'\t' || c == b'\r' || c == b'\n' {
            i += 1;
            continue;
        }

        let start = i;

        // A number: a digit, or a `.` immediately followed by a digit.
        if c.is_ascii_digit() || (c == b'.' && i + 1 < b.len() && b[i + 1].is_ascii_digit()) {
            let (tok, next) = lex_number(src, b, i)?;
            out.push(Token {
                kind: tok,
                span: Span::new(start, next),
            });
            i = next;
            continue;
        }

        // A string literal.
        if c == b'"' {
            let (s, next) = lex_string(src, b, i)?;
            out.push(Token {
                kind: TokenKind::Str(s),
                span: Span::new(start, next),
            });
            i = next;
            continue;
        }

        // An error literal (`#REF!` …) or the spill operator `#`.
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

        // A `'`-quoted sheet name of a cross-sheet reference (`'My Sheet'!A1`). The quotes let a
        // sheet name hold spaces/punctuation an unquoted word cannot; the interior `''` escapes a
        // literal `'` (Excel convention). A malformed quote is a located refusal, never a panic.
        if c == b'\'' {
            let (name, next) = lex_quoted_sheet_name(src, b, i)?;
            out.push(Token {
                kind: TokenKind::SheetBang(name),
                span: Span::new(start, next),
            });
            i = next;
            continue;
        }

        // An identifier-shaped lexeme: `$`, letters, digits (a cell ref, function name, bool, sheet
        // prefix, or bare name). Must start with `$` or an ASCII letter.
        if c == b'$' || c.is_ascii_alphabetic() {
            let (tok, next) = lex_word(src, b, i);
            out.push(Token {
                kind: tok,
                span: Span::new(start, next),
            });
            i = next;
            continue;
        }

        // Operators and punctuation (longest-match for the two-byte comparisons).
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
                // Any other byte cannot begin a token. Locate the *whole* offending char (so the
                // span lands on a UTF-8 boundary even for a multi-byte char), never a panic.
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

/// Lex a numeric literal starting at `i`. Grammar: `DIGIT* [ '.' DIGIT* ] [ ('e'|'E') ['+'|'-']
/// DIGIT+ ]`, requiring at least one digit overall. Returns the token and the byte index past it.
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
        // Only consume the exponent if it is well-formed; otherwise leave `e…` unconsumed (it would
        // become a separate word and the parser will reject the juxtaposition).
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
        // A magnitude that overflows to ±inf, or an otherwise-unparseable numeric run, is a located
        // refusal — never a non-finite `Number` (mirrors the literal lexer: a non-finite spelling is text, never a Number).
        _ => Err(Diag::new(
            DiagCode::InvalidNumber,
            Span::new(start, i),
            format!("`{lexeme}` is not a finite number"),
        )),
    }
}

/// Lex a `"`-quoted string starting at the opening quote. Interior `""` is an escaped `"` (Excel's
/// convention — NOT backslash). Returns the unescaped contents and the index past the closing quote.
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
        // Copy one whole UTF-8 char so interior multi-byte text survives intact.
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

/// Lex a `'`-quoted sheet name of a cross-sheet reference, starting at the opening quote. Interior
/// `''` is an escaped `'`. The closing quote MUST be followed immediately by `!` (a quoted name only
/// ever qualifies a cross-sheet reference); returns the unescaped name and the index past that `!`.
/// A missing closing quote or a missing trailing `!` is a located [`DiagCode::MalformedSheetName`].
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
            // Closing quote found — a cross-sheet reference requires the `!` sheet separator next.
            if j + 1 < b.len() && b[j + 1] == b'!' {
                return Ok((name, j + 2));
            }
            return Err(Diag::new(
                DiagCode::MalformedSheetName,
                Span::new(start, j + 1),
                "a quoted sheet name must be followed by `!` (a cross-sheet reference)",
            ));
        }
        // Copy one whole UTF-8 char so a multi-byte sheet name survives intact.
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

/// Lex and classify an identifier-shaped lexeme (`$`/letters/digits) starting at `i`. Never fails —
/// an un-parseable-as-reference word is a [`TokenKind::Name`] the parser refuses.
fn lex_word(src: &str, b: &[u8], i: usize) -> (TokenKind, usize) {
    let start = i;
    let mut j = i;
    while j < b.len() && (b[j] == b'$' || b[j].is_ascii_alphanumeric()) {
        j += 1;
    }
    let w = &src[start..j];

    // A word immediately followed by `(` is a function name (no space allowed — Excel-like).
    if j < b.len() && b[j] == b'(' {
        return (TokenKind::Func(w.to_string()), j);
    }
    // A word immediately followed by `!` is a cross-sheet sheet name (reserved). Consume the `!`.
    if j < b.len() && b[j] == b'!' {
        return (TokenKind::SheetBang(w.to_string()), j + 1);
    }
    // UPPERCASE-only boolean constants (deliberate — no case-folding).
    match w {
        "TRUE" => return (TokenKind::Bool(true), j),
        "FALSE" => return (TokenKind::Bool(false), j),
        _ => {}
    }
    // A well-formed A1 single-cell address becomes a reference. Refs are case-insensitive on the
    // column (Excel), but a leading-zero row (`A01`) is not a reference — it falls through to a name.
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

/// If `s` begins with one of the nine error literals, return its [`ErrKind`] and byte length. The
/// seven live errors and the two reserved (`#SPILL!`/`#CALC!`) are all recognized so a formula can
/// round-trip them; uppercase-only. Longest spellings are unambiguous — no
/// literal is a prefix of another (each ends in `!`, `?`, or the `A` of `#N/A`).
fn match_error_literal(s: &str) -> Option<(ErrKind, usize)> {
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

/// The UTF-8 byte length implied by a leading byte (1 for ASCII / continuation). Used only to size a
/// refusal span so it never splits a multi-byte char.
fn utf8_char_len(lead: u8) -> usize {
    match lead {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    }
}

/// The first `char` at byte offset `at` (which is a char boundary by construction). Falls back to
/// the replacement char rather than panicking if the offset is somehow mid-char.
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
        // A `-` is its own operator, not part of the number.
        assert_eq!(kinds("-4"), vec![TokenKind::Minus, TokenKind::Num(4.0)]);
        // Overflow to infinity is a located refusal, never a non-finite Number.
        assert_eq!(tokenize("1e999").unwrap_err().code, DiagCode::InvalidNumber);
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
        // lowercase is not a boolean — it is a bare name.
        assert_eq!(kinds("true"), vec![TokenKind::Name("true".to_string())]);
        assert_eq!(kinds("#DIV/0!"), vec![TokenKind::Err(ErrKind::Div0)]);
        assert_eq!(kinds("#N/A"), vec![TokenKind::Err(ErrKind::Na)]);
        assert_eq!(kinds("#NAME?"), vec![TokenKind::Err(ErrKind::Name)]);
        // reserved error spellings still round-trip as Err tokens.
        assert_eq!(kinds("#SPILL!"), vec![TokenKind::Err(ErrKind::Spill)]);
    }

    #[test]
    fn hash_is_spill_when_not_an_error_literal() {
        // `A1#` — the `#` is the spill operator (a lone `#`, no error spelling follows).
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
        // case-insensitive column.
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
        // A leading-zero row is not a ref — it is a bare name.
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
        // A quoted name may hold spaces an unquoted word cannot.
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
        // Interior `''` is an escaped `'`.
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
        // Malformed: unterminated, and a closing quote not followed by `!`.
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
        // A multi-byte char: the refusal span must not split it.
        let d = tokenize("λ").unwrap_err();
        assert_eq!(d.code, DiagCode::UnexpectedChar);
        assert_eq!(d.span, Span::new(0, 2));
        // A lone `.` is not a number and not an operator.
        assert_eq!(tokenize(".").unwrap_err().code, DiagCode::UnexpectedChar);
    }

    #[test]
    fn spans_are_sliceable_on_every_token() {
        // ast-standards PART 3: `&src[span]` must never panic and must recover the lexeme.
        let src = "SUM(A1:B2, 3.5) & \"x\"";
        for t in tokenize(src).unwrap() {
            let _ = &src[t.span.start..t.span.end]; // must not panic
        }
    }
}
