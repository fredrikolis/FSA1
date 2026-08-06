// Concern: builds an Expr tree from a formula's tokens | Non-concern: lexing, evaluating, the function registry's contents | IO: (&str) -> Expr or a located Diag

use crate::diag::{Diag, DiagCode, Span};
use crate::expr::{BinOp, Expr, UnOp};
use crate::func;
use crate::lexer::{Token, TokenKind, tokenize};
use crate::refs::{RangeNode, RefNode, SheetName};
use crate::value::{Shape, Value};

pub const MAX_DEPTH: u32 = 128;

/// Bounds a hostile FLAT chain (`1+1+1+…`), which no depth guard catches: the recursive
/// `Box<Expr>` destructor would overflow the stack on drop even if the tree were built.
pub const MAX_TOKENS: usize = 4096;

/// A leading `=` is skipped, not stripped, so every refusal span still indexes `formula` itself.
pub fn parse(formula: &str) -> Result<Expr, Diag> {
    let tokens = tokenize(formula)?;
    let start = usize::from(matches!(
        tokens.first().map(|t| &t.kind),
        Some(TokenKind::Eq)
    ));
    let rest = &tokens[start..];

    if rest.is_empty() {
        return Err(Diag::new(
            DiagCode::EmptyFormula,
            Span::at(formula.len()),
            "the formula is empty",
        ));
    }
    if rest.len() > MAX_TOKENS {
        return Err(Diag::new(
            DiagCode::RecursionLimit,
            rest[MAX_TOKENS].span,
            format!("formula exceeds the {MAX_TOKENS}-token size bound"),
        ));
    }

    let mut p = Parser {
        tokens: rest,
        pos: 0,
        depth: 0,
        end: formula.len(),
    };
    let expr = p.parse_expr(0)?;
    if let Some(t) = p.peek() {
        let code = match t.kind {
            TokenKind::Comma => DiagCode::ReservedUnion,
            TokenKind::RParen => DiagCode::UnbalancedParen,
            _ => DiagCode::UnexpectedToken,
        };
        return Err(Diag::new(code, t.span, leftover_message(code)));
    }
    Ok(expr)
}

fn leftover_message(code: DiagCode) -> String {
    match code {
        DiagCode::ReservedUnion => "the , union operator is reserved in v1".to_string(),
        DiagCode::UnbalancedParen => "a ) has no matching (".to_string(),
        _ => "unexpected trailing input after a complete formula".to_string(),
    }
}

struct Parser<'t> {
    tokens: &'t [Token],
    pos: usize,
    depth: u32,
    end: usize,
}

/// `Rate` is letters too, so the `A`..`XFD` bound is what separates a column word from a name;
/// adjacency to `:` (the caller's check) is what separates `A:A` from a bare `A`.
fn is_column_word(w: &str) -> bool {
    !w.is_empty()
        && w.len() <= 3
        && w.bytes().all(|b| b.is_ascii_alphabetic())
        && column_index(w) <= MAX_COL
}

fn column_index(w: &str) -> u32 {
    w.bytes().fold(0u32, |acc, b| {
        acc.saturating_mul(26)
            .saturating_add((b.to_ascii_uppercase() - b'A') as u32 + 1)
    }) - 1
}

/// `None` unless `n` is a whole 1-indexed row, so `1.5:2` and `0:0` stay ordinary numbers.
fn row_index(n: f64) -> Option<u32> {
    if n.fract() != 0.0 || n < 1.0 || n > (MAX_ROW as f64) + 1.0 {
        return None;
    }
    Some(n as u32 - 1)
}

/// The largest row an A1 lexeme may spell — an addressing limit, not a bound on the sheet.
const MAX_ROW: u32 = 1_048_575;

/// The largest column an A1 lexeme may spell (`XFD`).
const MAX_COL: u32 = 16_383;

const BINDING_FORMS: &[&str] = &["LET", "LAMBDA"];

impl<'t> Parser<'t> {
    fn peek(&self) -> Option<&'t Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&'t Token> {
        let t = self.tokens.get(self.pos);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn eof_span(&self) -> Span {
        Span::at(self.tokens.last().map_or(self.end, |t| t.span.end))
    }

    /// Every nesting path routes through here, so the depth guard is single-sourced.
    fn parse_expr(&mut self, min_bp: u8) -> Result<Expr, Diag> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            let span = self.peek().map_or_else(|| self.eof_span(), |t| t.span);
            return Err(Diag::new(
                DiagCode::RecursionLimit,
                span,
                format!("formula nests deeper than the {MAX_DEPTH}-level bound"),
            ));
        }
        let r = self.parse_expr_inner(min_bp);
        self.depth -= 1;
        r
    }

    fn parse_expr_inner(&mut self, min_bp: u8) -> Result<Expr, Diag> {
        let mut lhs = self.parse_prefix()?;

        while let Some(tok) = self.peek() {
            let kind = tok.kind.clone();
            let span = tok.span;

            if let Some(bp) = postfix_bp(&kind) {
                if bp < min_bp {
                    break;
                }
                self.advance();
                lhs = match kind {
                    TokenKind::Percent => Expr::Unary(UnOp::Percent, Box::new(lhs)),
                    TokenKind::Hash => Expr::SpillRef(Box::new(lhs)),
                    _ => unreachable!(),
                };
                continue;
            }

            if let Some((l_bp, r_bp)) = infix_bp(&kind) {
                if l_bp < min_bp {
                    break;
                }
                self.advance();
                let rhs = self.parse_expr(r_bp)?;
                lhs = self.build_infix(&kind, lhs, rhs, span)?;
                continue;
            }

            match kind {
                TokenKind::Comma | TokenKind::RParen => break,
                _ if starts_primary(&kind) => {
                    let (code, msg) = if ref_like(&kind) {
                        (
                            DiagCode::ReservedIntersection,
                            "the space intersection operator is reserved in v1",
                        )
                    } else {
                        (DiagCode::UnexpectedToken, "expected an operator")
                    };
                    return Err(Diag::new(code, span, msg));
                }
                _ => break,
            }
        }

        Ok(lhs)
    }

    fn parse_prefix(&mut self) -> Result<Expr, Diag> {
        let Some(tok) = self.advance() else {
            return Err(Diag::new(
                DiagCode::UnexpectedEof,
                self.eof_span(),
                "input ended where a value was expected",
            ));
        };
        let span = tok.span;
        match &tok.kind {
            // Adjacency to `:` is what makes a bare row number a reference; elsewhere it is a number.
            TokenKind::Num(n)
                if row_index(*n).is_some() && (self.peek_is_colon() || self.prev_was_colon()) =>
            {
                Ok(Expr::Ref(RefNode {
                    col: RangeNode::OPEN,
                    row: row_index(*n).expect("guarded"),
                    col_abs: false,
                    row_abs: false,
                    sheet: None,
                }))
            }
            TokenKind::Num(n) => Ok(Expr::Lit(Value::Number(*n))),
            TokenKind::Str(s) => Ok(Expr::Lit(Value::Text(s.clone()))),
            TokenKind::Bool(b) => Ok(Expr::Lit(Value::Bool(*b))),
            TokenKind::Err(k) => Ok(Expr::Lit(Value::Error(*k))),
            TokenKind::CellRef {
                col,
                row,
                col_abs,
                row_abs,
            } => Ok(Expr::Ref(RefNode {
                col: *col,
                row: *row,
                col_abs: *col_abs,
                row_abs: *row_abs,
                sheet: None,
            })),
            TokenKind::Minus => {
                let rhs = self.parse_expr(PREFIX_BP)?;
                Ok(Expr::Unary(UnOp::Neg, Box::new(rhs)))
            }
            TokenKind::Plus => {
                let rhs = self.parse_expr(PREFIX_BP)?;
                Ok(Expr::Unary(UnOp::Plus, Box::new(rhs)))
            }
            TokenKind::At => {
                let rhs = self.parse_expr(AT_BP)?;
                Ok(Expr::ImplicitIntersect(Box::new(rhs)))
            }
            TokenKind::LParen => {
                let inner = self.parse_expr(0)?;
                match self.peek().map(|t| &t.kind) {
                    Some(TokenKind::RParen) => {
                        self.advance();
                        Ok(inner)
                    }
                    _ => Err(Diag::new(
                        DiagCode::UnclosedParen,
                        self.eof_span(),
                        "a ( was never closed",
                    )),
                }
            }
            TokenKind::LBrace => self.parse_array_literal(span),
            TokenKind::Func(name) => self.parse_call(name.clone(), span),
            // Likewise for a column word: only adjacency to `:` makes `A` a column, not a name.
            TokenKind::Name(n)
                if is_column_word(n) && (self.peek_is_colon() || self.prev_was_colon()) =>
            {
                let col = column_index(n);
                Ok(Expr::Ref(RefNode {
                    col,
                    row: RangeNode::OPEN,
                    col_abs: false,
                    row_abs: false,
                    sheet: None,
                }))
            }
            TokenKind::Name(n) => Err(Diag::new(
                DiagCode::ReservedName,
                span,
                format!("`{n}` is a defined name — reserved in v1"),
            )),
            TokenKind::SheetBang(name) => self.parse_sheet_qualified(name.clone()),
            TokenKind::RParen => Err(Diag::new(
                DiagCode::UnbalancedParen,
                span,
                "a ) has no matching (",
            )),
            _ => Err(Diag::new(
                DiagCode::UnexpectedToken,
                span,
                "expected a value, reference, or ( here",
            )),
        }
    }

    /// The `Name!` prefix was just consumed; a cell reference or an open axis MUST follow it.
    fn parse_sheet_qualified(&mut self, name: String) -> Result<Expr, Diag> {
        match self.advance() {
            Some(Token {
                kind:
                    TokenKind::CellRef {
                        col,
                        row,
                        col_abs,
                        row_abs,
                    },
                ..
            }) => Ok(Expr::Ref(RefNode {
                col: *col,
                row: *row,
                col_abs: *col_abs,
                row_abs: *row_abs,
                sheet: Some(SheetName::new(name)),
            })),
            Some(Token {
                kind: TokenKind::Name(w),
                ..
            }) if is_column_word(w) && self.peek_is_colon() => {
                let col = column_index(w);
                Ok(Expr::Ref(RefNode {
                    col,
                    row: RangeNode::OPEN,
                    col_abs: false,
                    row_abs: false,
                    sheet: Some(SheetName::new(name)),
                }))
            }
            Some(Token {
                kind: TokenKind::Num(n),
                ..
            }) if row_index(*n).is_some() && self.peek_is_colon() => {
                let row = row_index(*n).expect("guarded");
                Ok(Expr::Ref(RefNode {
                    col: RangeNode::OPEN,
                    row,
                    col_abs: false,
                    row_abs: false,
                    sheet: Some(SheetName::new(name)),
                }))
            }
            other => {
                let span = other.map_or_else(|| self.eof_span(), |t| t.span);
                Err(Diag::new(
                    DiagCode::UnexpectedToken,
                    span,
                    format!(
                        "the sheet prefix `{name}!` must be followed by a cell reference or an open \
                         axis (`{name}!A:A`, `{name}!1:1`)"
                    ),
                ))
            }
        }
    }

    /// The opening `{` was just consumed. `,` separates columns and `;` separates rows; the whole
    /// literal folds to one `Expr::Lit` array value.
    fn parse_array_literal(&mut self, open: Span) -> Result<Expr, Diag> {
        let mut rows: Vec<Vec<Value>> = vec![Vec::new()];
        // One span per row (its first element) so a ragged refusal locates the offending row.
        let mut row_spans: Vec<Span> = Vec::new();
        loop {
            let elem_span = self.peek().map(|t| t.span);
            let v = self.parse_array_element()?;
            let row = rows.last_mut().expect("row vec is never empty");
            row.push(v);
            if row.len() == 1 {
                row_spans.push(elem_span.unwrap_or(open));
            }
            match self.peek().map(|t| &t.kind) {
                Some(TokenKind::Comma) => {
                    self.advance();
                }
                Some(TokenKind::Semicolon) => {
                    self.advance();
                    rows.push(Vec::new());
                }
                Some(TokenKind::RBrace) => {
                    self.advance();
                    break;
                }
                Some(_) => {
                    let span = self.peek().expect("peek matched Some").span;
                    return Err(Diag::new(
                        DiagCode::MalformedArray,
                        span,
                        "expected `,`, `;`, or `}` in an array literal",
                    ));
                }
                None => {
                    return Err(Diag::new(
                        DiagCode::UnexpectedEof,
                        self.eof_span(),
                        "a { array literal was never closed",
                    ));
                }
            }
        }
        let cols = rows[0].len();
        let nrows = rows.len();
        if let Some(bad) = rows.iter().position(|r| r.len() != cols) {
            return Err(Diag::new(
                DiagCode::MalformedArray,
                row_spans.get(bad).copied().unwrap_or(open),
                "an array literal must be rectangular (every row the same width)",
            ));
        }
        let flat: Vec<Value> = rows.into_iter().flatten().collect();
        Ok(Expr::Lit(Value::Array(
            Shape {
                rows: nrows as u32,
                cols: cols as u32,
            },
            flat,
        )))
    }

    /// An element is a numeric/text/logical/error CONSTANT, a number optionally signed.
    fn parse_array_element(&mut self) -> Result<Value, Diag> {
        let neg = match self.peek().map(|t| &t.kind) {
            Some(TokenKind::Minus) => {
                self.advance();
                true
            }
            Some(TokenKind::Plus) => {
                self.advance();
                false
            }
            _ => false,
        };
        let Some(tok) = self.advance() else {
            return Err(Diag::new(
                DiagCode::UnexpectedEof,
                self.eof_span(),
                "a { array literal was never closed",
            ));
        };
        let span = tok.span;
        // A sign is legal only before a number; `-"x"` / `-TRUE` are not Excel array constants.
        let v = match &tok.kind {
            TokenKind::Num(n) => Value::Number(if neg { -n } else { *n }),
            TokenKind::Str(s) if !neg => Value::Text(s.clone()),
            TokenKind::Bool(b) if !neg => Value::Bool(*b),
            TokenKind::Err(k) if !neg => Value::Error(*k),
            _ => {
                return Err(Diag::new(
                    DiagCode::MalformedArray,
                    span,
                    "an array literal element must be a numeric, text, logical, or error constant",
                ));
            }
        };
        Ok(v)
    }

    fn peek_is_colon(&self) -> bool {
        matches!(self.peek().map(|t| &t.kind), Some(TokenKind::Colon))
    }

    /// `pos` already points past the current token, so `- 2` is the one before it.
    fn prev_was_colon(&self) -> bool {
        self.pos
            .checked_sub(2)
            .and_then(|i| self.tokens.get(i))
            .is_some_and(|t| matches!(t.kind, TokenKind::Colon))
    }

    /// Resolves the NAME before the arguments, so an unsupported binding form names ITSELF rather
    /// than blaming the first identifier it binds.
    fn parse_call(&mut self, name: String, name_span: Span) -> Result<Expr, Diag> {
        debug_assert!(matches!(
            self.peek().map(|t| &t.kind),
            Some(TokenKind::LParen)
        ));
        self.advance();

        let fid = match func::lookup(&name) {
            Some(fid) => fid,
            None => {
                return Err(Diag::new(
                    if BINDING_FORMS.iter().any(|f| f.eq_ignore_ascii_case(&name)) {
                        DiagCode::UnsupportedFunction
                    } else {
                        DiagCode::UnknownFunction
                    },
                    name_span,
                    if BINDING_FORMS.iter().any(|f| f.eq_ignore_ascii_case(&name)) {
                        format!(
                            "`{name}` is not supported — it binds its own identifiers, which v1 has \
                             no node for (the arguments are not the problem)"
                        )
                    } else {
                        format!("`{name}` is not a recognized function")
                    },
                ));
            }
        };

        let mut args = Vec::new();
        if matches!(self.peek().map(|t| &t.kind), Some(TokenKind::RParen)) {
            self.advance();
        } else {
            loop {
                // An empty slot is an OMITTED argument, read as blank; it still counts toward arity.
                if matches!(
                    self.peek().map(|t| &t.kind),
                    Some(TokenKind::Comma | TokenKind::RParen)
                ) {
                    args.push(Expr::Lit(Value::Blank));
                } else {
                    args.push(self.parse_expr(0)?);
                }
                match self.peek().map(|t| &t.kind) {
                    Some(TokenKind::Comma) => {
                        self.advance();
                    }
                    Some(TokenKind::RParen) => {
                        self.advance();
                        break;
                    }
                    Some(_) => {
                        let t = self.peek().unwrap();
                        return Err(Diag::new(
                            DiagCode::UnexpectedToken,
                            t.span,
                            "expected , or ) in an argument list",
                        ));
                    }
                    None => {
                        return Err(Diag::new(
                            DiagCode::UnexpectedEof,
                            self.eof_span(),
                            "an argument list was not closed",
                        ));
                    }
                }
            }
        }

        let def = func::def(fid).expect("lookup returned a valid id");
        if !def.arity_ok(args.len()) {
            return Err(Diag::new(
                DiagCode::BadArity,
                name_span,
                format!(
                    "`{}` takes {} argument(s), got {}",
                    name,
                    arity_phrase(def),
                    args.len()
                ),
            ));
        }
        // A per-function static check: an unsupported LITERAL argument refuses here, while a computed one is accepted and deferred to eval.
        def.validate_args(&args, name_span)?;
        Ok(Expr::Call(fid, args))
    }

    fn build_infix(
        &self,
        kind: &TokenKind,
        lhs: Expr,
        rhs: Expr,
        span: Span,
    ) -> Result<Expr, Diag> {
        let op = match kind {
            TokenKind::Plus => BinOp::Add,
            TokenKind::Minus => BinOp::Sub,
            TokenKind::Star => BinOp::Mul,
            TokenKind::Slash => BinOp::Div,
            TokenKind::Caret => BinOp::Pow,
            TokenKind::Amp => BinOp::Concat,
            TokenKind::Eq => BinOp::Eq,
            TokenKind::Ne => BinOp::Ne,
            TokenKind::Lt => BinOp::Lt,
            TokenKind::Le => BinOp::Le,
            TokenKind::Gt => BinOp::Gt,
            TokenKind::Ge => BinOp::Ge,
            TokenKind::Colon => return fold_range(lhs, rhs, span),
            _ => unreachable!("build_infix called with a non-infix token"),
        };
        Ok(Expr::Binary(op, Box::new(lhs), Box::new(rhs)))
    }
}

/// Both endpoints must be static cell references. A qualifier on the LEFT endpoint carries the
/// whole range; endpoints naming different sheets are a reserved 3D reference.
fn fold_range(lhs: Expr, rhs: Expr, span: Span) -> Result<Expr, Diag> {
    let (Expr::Ref(a), Expr::Ref(b)) = (&lhs, &rhs) else {
        return Err(Diag::new(
            DiagCode::ReservedDynamicRange,
            span,
            "a `:` range needs two static cell references (dynamic ranges are reserved in v1)",
        ));
    };
    let sheet = match (&a.sheet, &b.sheet) {
        (left, None) => left.clone(),
        (Some(x), Some(y)) if x == y => Some(x.clone()),
        _ => {
            return Err(Diag::new(
                DiagCode::ReservedCrossSheet,
                span,
                "a 3D / multi-sheet range (endpoints on different sheets) is reserved in v1",
            ));
        }
    };
    // A MIXED range (one endpoint open, one bounded) would invent a corner, so it refuses.
    let open_rows = a.row == RangeNode::OPEN && b.row == RangeNode::OPEN;
    let open_cols = a.col == RangeNode::OPEN && b.col == RangeNode::OPEN;
    if (a.row == RangeNode::OPEN) != (b.row == RangeNode::OPEN)
        || (a.col == RangeNode::OPEN) != (b.col == RangeNode::OPEN)
    {
        return Err(Diag::new(
            DiagCode::ReservedDynamicRange,
            span,
            "a `:` range mixes an open axis with a bounded one (write `A:A`, `1:1`, or `A1:B2`)",
        ));
    }

    // Each `$`-anchor flag travels with the corner it lands on, so `$E$2:E2` round-trips.
    let (start_col, start_col_abs, end_col, end_col_abs) = if a.col <= b.col {
        (a.col, a.col_abs, b.col, b.col_abs)
    } else {
        (b.col, b.col_abs, a.col, a.col_abs)
    };
    let (start_row, start_row_abs, end_row, end_row_abs) = if a.row <= b.row {
        (a.row, a.row_abs, b.row, b.row_abs)
    } else {
        (b.row, b.row_abs, a.row, a.row_abs)
    };
    // min/max put the sentinel on both corners; reset the near one to the axis origin.
    let (start_row, end_row) = if open_rows {
        (0, RangeNode::OPEN)
    } else {
        (start_row, end_row)
    };
    let (start_col, end_col) = if open_cols {
        (0, RangeNode::OPEN)
    } else {
        (start_col, end_col)
    };

    Ok(Expr::Range(RangeNode {
        start_col,
        start_row,
        end_col,
        end_row,
        start_col_abs,
        start_row_abs,
        end_col_abs,
        end_row_abs,
        sheet,
    }))
}

/// Higher binds tighter; a left-associative infix operator uses `(n, n+1)`. These numbers are the
/// one source of the ladder `schema::PRECEDENCE` publishes.
pub(crate) const PREFIX_BP: u8 = 70;
pub(crate) const AT_BP: u8 = 85;

pub(crate) fn infix_bp(kind: &TokenKind) -> Option<(u8, u8)> {
    Some(match kind {
        TokenKind::Eq
        | TokenKind::Ne
        | TokenKind::Lt
        | TokenKind::Le
        | TokenKind::Gt
        | TokenKind::Ge => (10, 11),
        TokenKind::Amp => (20, 21),
        TokenKind::Plus | TokenKind::Minus => (30, 31),
        TokenKind::Star | TokenKind::Slash => (40, 41),
        TokenKind::Caret => (50, 51),
        TokenKind::Colon => (90, 91),
        _ => return None,
    })
}

pub(crate) fn postfix_bp(kind: &TokenKind) -> Option<u8> {
    match kind {
        TokenKind::Percent => Some(60),
        TokenKind::Hash => Some(80),
        _ => None,
    }
}

fn starts_primary(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Num(_)
            | TokenKind::Str(_)
            | TokenKind::Bool(_)
            | TokenKind::Err(_)
            | TokenKind::CellRef { .. }
            | TokenKind::Name(_)
            | TokenKind::Func(_)
            | TokenKind::SheetBang(_)
            | TokenKind::LParen
            | TokenKind::LBrace
            | TokenKind::At
    )
}

/// Juxtaposing two reference-like primaries reads as the reserved intersection, not a syntax error.
fn ref_like(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::CellRef { .. }
            | TokenKind::Name(_)
            | TokenKind::Func(_)
            | TokenKind::SheetBang(_)
            | TokenKind::LParen
            | TokenKind::At
    )
}

fn arity_phrase(def: &func::FuncDef) -> String {
    match def.max_args {
        Some(max) if max == def.min_args => format!("{}", def.min_args),
        Some(max) => format!("{}–{max}", def.min_args),
        None => format!("{}+", def.min_args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::eval;
    use crate::test_support::Grid;
    use crate::value::{ErrKind, Value};

    fn run(formula: &str) -> Value {
        let g = Grid::new(1, vec![Value::Blank]);
        eval(&parse(formula).expect("should parse"), &g)
    }

    fn parse_err(formula: &str) -> DiagCode {
        parse(formula).unwrap_err().code
    }

    fn parse_diag(formula: &str) -> Diag {
        parse(formula).unwrap_err()
    }

    fn parse_ok(formula: &str) -> Expr {
        parse(formula).expect("should parse")
    }

    #[test]
    fn precedence_ladder_matches_excel() {
        assert_eq!(run("=1+2*3"), Value::Number(7.0));
        assert_eq!(run("=(1+2)*3"), Value::Number(9.0));
        assert_eq!(
            run("=-2^2"),
            Value::Number(4.0),
            "unary - binds tighter than ^"
        );
        assert_eq!(run("=2^3^2"), Value::Number(64.0), "^ is left-associative");
        assert_eq!(
            run("=2*50%"),
            Value::Number(1.0),
            "postfix % binds tightest"
        );
        assert_eq!(
            run("=1+2&\"x\""),
            Value::Text("3x".into()),
            "& is looser than +"
        );
        assert_eq!(
            run("=1+1=2"),
            Value::Bool(true),
            "comparisons are the loosest rung"
        );
    }

    #[test]
    fn leading_equals_optional_and_spans_align() {
        assert_eq!(run("=1+1"), Value::Number(2.0));
        assert_eq!(run("1+1"), Value::Number(2.0));
        let d = parse("=1+").unwrap_err();
        assert_eq!(d.code, DiagCode::UnexpectedEof);
        assert_eq!(d.span, Span::at(3), "the span indexes the original string");
    }

    #[test]
    fn static_range_folds_and_functions_resolve() {
        let g = Grid::new(
            2,
            vec![
                Value::Number(1.0),
                Value::Number(2.0),
                Value::Number(3.0),
                Value::Number(4.0),
            ],
        );
        assert_eq!(
            eval(&parse("=SUM(A1:B2)").unwrap(), &g),
            Value::Number(10.0)
        );
        assert_eq!(eval(&parse("=A1+B2").unwrap(), &g), Value::Number(5.0));
        assert_eq!(
            eval(&parse("=SUM(B2:A1)").unwrap(), &g),
            Value::Number(10.0),
            "a reversed spelling normalizes to the same rectangle"
        );
    }

    #[test]
    fn cross_sheet_refs_parse_and_carry_the_sheet_name() {
        match parse("=Sheet1!A1").unwrap() {
            Expr::Ref(r) => {
                assert_eq!((r.col, r.row), (0, 0));
                assert_eq!(r.sheet.as_ref().map(SheetName::as_str), Some("Sheet1"));
            }
            other => panic!("expected a Ref, got {other:?}"),
        }
        match parse("='My Sheet'!$B$3").unwrap() {
            Expr::Ref(r) => {
                assert_eq!((r.col, r.row, r.col_abs, r.row_abs), (1, 2, true, true));
                assert_eq!(r.sheet.as_ref().map(SheetName::as_str), Some("My Sheet"));
            }
            other => panic!("expected a Ref, got {other:?}"),
        }
        match parse("=Sheet1!A1:B2").unwrap() {
            Expr::Range(rn) => {
                assert_eq!(
                    (rn.start_col, rn.start_row, rn.end_col, rn.end_row),
                    (0, 0, 1, 1)
                );
                assert_eq!(rn.sheet.as_ref().map(SheetName::as_str), Some("Sheet1"));
            }
            other => panic!("expected a Range, got {other:?}"),
        }
    }

    #[test]
    fn reserved_at_and_hash_parse_and_preserve() {
        assert!(matches!(parse("=@A1").unwrap(), Expr::ImplicitIntersect(_)));
        assert!(matches!(parse("=A1#").unwrap(), Expr::SpillRef(_)));
        assert_eq!(
            run("=A1#"),
            Value::Error(ErrKind::Calc),
            "eval of a reserved node is deferred"
        );
    }

    #[test]
    fn located_refusals_across_the_categories() {
        assert_eq!(parse_err("=SUM("), DiagCode::UnexpectedEof);
        assert_eq!(parse_err("=SUM()"), DiagCode::BadArity);
        assert_eq!(parse_err("=NOPE(1)"), DiagCode::UnknownFunction);
        assert_eq!(parse_err("=1)"), DiagCode::UnbalancedParen);
        assert_eq!(parse_err("=(1"), DiagCode::UnclosedParen);
        assert_eq!(parse_err("=1,2"), DiagCode::ReservedUnion);
        assert_eq!(parse_err("=A1 B1"), DiagCode::ReservedIntersection);
        assert_eq!(
            parse_err("=Sheet1!A1:Sheet2!B2"),
            DiagCode::ReservedCrossSheet
        );
        assert_eq!(parse_err("=A1:Sheet2!B2"), DiagCode::ReservedCrossSheet);
        assert_eq!(parse_err("=Sheet1!5"), DiagCode::UnexpectedToken);
        assert_eq!(parse_err("=Sheet1!"), DiagCode::UnexpectedToken);
        assert_eq!(parse_err("=myname"), DiagCode::ReservedName);
        assert_eq!(parse_err("=SUM(A1):A2"), DiagCode::ReservedDynamicRange);
        assert_eq!(parse_err("="), DiagCode::EmptyFormula);
        assert_eq!(parse_err("=*3"), DiagCode::UnexpectedToken);
    }

    #[test]
    fn nesting_is_bounded_not_a_stack_overflow() {
        let deep = format!("={}1{}", "(".repeat(300), ")".repeat(300));
        assert_eq!(parse(&deep).unwrap_err().code, DiagCode::RecursionLimit);
        let deep_neg = format!("={}1", "-".repeat(300));
        assert_eq!(parse(&deep_neg).unwrap_err().code, DiagCode::RecursionLimit);
        let flat = format!("=1{}", "+1".repeat(MAX_TOKENS));
        assert_eq!(
            parse(&flat).unwrap_err().code,
            DiagCode::RecursionLimit,
            "a flat chain is refused before the tree is built or dropped"
        );
    }

    #[test]
    fn nested_calls_and_mixed_expression() {
        let g = Grid::new(
            2,
            vec![
                Value::Number(10.0),
                Value::Number(20.0),
                Value::Number(0.0),
                Value::Number(5.0),
            ],
        );
        assert_eq!(
            eval(
                &parse("=IFERROR(A1/B2, ROUND(AVERAGE(A1:B1),0))").unwrap(),
                &g
            ),
            Value::Number(2.0)
        );
        assert_eq!(
            eval(
                &parse("=IFERROR(A1/A2, ROUND(AVERAGE(A1:B1),0))").unwrap(),
                &g
            ),
            Value::Number(15.0),
            "A2 is 0, so the #DIV/0! branch is taken"
        );
    }
    #[test]
    fn an_open_axis_parses_as_a_range_with_the_open_sentinel() {
        let Expr::Range(r) = parse_ok("=A:A") else {
            panic!("A:A must fold to a Range")
        };
        assert_eq!((r.start_col, r.end_col), (0, 0));
        assert_eq!(r.start_row, 0);
        assert!(r.is_open_rows(), "the row axis is unbounded");

        let Expr::Range(r) = parse_ok("=1:1") else {
            panic!("1:1 must fold to a Range")
        };
        assert_eq!((r.start_row, r.end_row), (0, 0));
        assert!(r.is_open_cols(), "the column axis is unbounded");

        let Expr::Range(r) = parse_ok("=Data!B:B") else {
            panic!("Data!B:B must fold to a Range")
        };
        assert_eq!(r.sheet.as_ref().map(|s| s.as_str()), Some("Data"));
        assert!(r.is_open_rows());
    }

    #[test]
    fn a_mixed_open_and_bounded_axis_is_refused_not_invented() {
        assert_eq!(
            parse_err("=A:B2"),
            DiagCode::ReservedDynamicRange,
            "a half-open range must refuse rather than guess a corner"
        );
    }

    #[test]
    fn a_bare_column_word_or_number_away_from_a_colon_is_unchanged() {
        assert_eq!(parse_err("=Rate+1"), DiagCode::ReservedName);
        assert_eq!(parse_err("=A+1"), DiagCode::ReservedName);
        assert_eq!(run("=1+1"), Value::Number(2.0));
        assert_eq!(
            parse_err("=1.5:2"),
            DiagCode::ReservedDynamicRange,
            "a non-integer row is a number, never a row reference"
        );
    }

    #[test]
    fn an_unsupported_binding_form_names_itself_not_its_bound_identifier() {
        let d = parse_diag("=LET(x,A1,x*2)");
        assert_eq!(d.code, DiagCode::UnsupportedFunction);
        assert!(d.message.contains("LET"), "must name LET: {}", d.message);
        assert!(
            !d.message.contains("`x`"),
            "must not blame the bound identifier: {}",
            d.message
        );
        assert_eq!(parse_err("=NOSUCHFN(1)"), DiagCode::UnknownFunction);
    }
}
