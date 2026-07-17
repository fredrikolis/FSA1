// Concern: the PRATT PARSER — a `&[Token]` stream -> an `Expr`, applying the exact Excel precedence ladder (`:` range > space-intersection > `,` union > unary `-`/`+` > `%` > `^` > `*`/`/` > `+`/`-` > `&` concat > the six comparisons), folding a STATIC `ref:ref` into `Expr::Range` (carrying a sheet qualifier onto the range), building a first-class SHEET-QUALIFIED reference from a `SheetBang` token (`Sheet1!A1` / `'Quoted'!A1` via `parse_sheet_qualified` → a sheet-tagged `RefNode`/`RangeNode`, while 3D/multi-sheet stays a reserved refusal), parsing-and-PRESERVING the reserved `@`(`ImplicitIntersect`)/`#`(`SpillRef`) nodes, resolving a function name + checking arity against the registry (so eval trusts arity — DbC) then running the row's OPTIONAL per-function static-argument `validate` (TEXT's unsupported-LITERAL-format `UnsupportedFormat` refusal — a non-literal format is accepted and deferred to eval), and turning every recognized-but-reserved or malformed construct into a LOCATED refusal; nesting past a depth bound (and a total-size bound) is a diagnostic, never a stack overflow | Non-concern: tokenizing (lexer.rs) and evaluating (eval.rs); this module builds the tree and never touches a `Resolver` | IO: (a formula `&str`, via `parse`) -> `Result<Expr, Diag>`
//! The Pratt (precedence-climbing) parser: [`parse`] a formula string into an [`Expr`].
//!
//! DbC: this is the one defended boundary (ast-standards PART 5). It never panics; a hole is a
//! located [`Diag`] returned *instead of* a tree (a formula is one parse unit — first-refusal stop,
//! no poison `Expr::Error` node). Two resource bounds keep hostile input safe: [`MAX_DEPTH`] caps
//! nesting recursion, and [`MAX_TOKENS`] caps total size so a huge *flat* left-associative chain can
//! neither be built nor recursively dropped into a stack overflow (ast-standards PART 9).

use crate::diag::{Diag, DiagCode, Span};
use crate::expr::{BinOp, Expr, UnOp};
use crate::func;
use crate::lexer::{Token, TokenKind, tokenize};
use crate::refs::{RangeNode, RefNode, SheetName};
use crate::value::Value;

/// Maximum nesting depth (parens / prefix operators / call arguments). Beyond this the parser
/// returns a `recursion-limit` refusal rather than recursing further.
pub const MAX_DEPTH: u32 = 128;

/// Maximum token count for one formula. A real cell formula is far smaller; the cap exists so a
/// hostile flat chain (`1+1+1+…` × 10⁵) can neither be parsed into, nor *dropped* as, a giant
/// left-leaning tree — the recursive `Box<Expr>` destructor would overflow the stack otherwise.
pub const MAX_TOKENS: usize = 4096;

/// Parse a formula (with or without a leading `=`) into an [`Expr`], or a located [`Diag`].
///
/// A leading `=` is consumed as the formula sigil (matching the on-disk `=formula` body), and byte
/// spans in any refusal are offsets into the **original** `formula` string (the `=` is skipped, not
/// stripped, so spans stay aligned and sliceable).
pub fn parse(formula: &str) -> Result<Expr, Diag> {
    let tokens = tokenize(formula)?;
    // Skip a single leading `=` sigil if present (it lexes as `Eq`).
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
    // Every token must be consumed. A leftover locates the specific failure.
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
    /// Byte length of the original formula — the anchor for an end-of-input refusal span.
    end: usize,
}

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

    /// The span to anchor an end-of-input refusal on (past the last token).
    fn eof_span(&self) -> Span {
        Span::at(self.tokens.last().map_or(self.end, |t| t.span.end))
    }

    /// Precedence-climbing core. `min_bp` is the caller's binding-power floor. Depth is bounded here
    /// (every nesting path routes through `parse_expr`), so the guard is single-sourced.
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

            // Postfix operators: `%` percent, `#` spill.
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

            // Infix binary operators and the `:` range operator.
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
                // A `,` / `)` ends this expression; the caller (call-arg loop or `parse`) decides
                // whether it is legal here.
                TokenKind::Comma | TokenKind::RParen => break,
                // Two juxtaposed operands with no operator: the reserved space-intersection, or a
                // plain syntax error for a non-reference operand.
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

    /// Parse a prefix position: a literal/reference/grouping/call, or a prefix operator (`-`, `+`,
    /// `@`). An operator with no operand, or end-of-input, is a located refusal.
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
            TokenKind::Func(name) => self.parse_call(name.clone(), span),
            // A bare defined-name is recognized but reserved.
            TokenKind::Name(n) => Err(Diag::new(
                DiagCode::ReservedName,
                span,
                format!("`{n}` is a defined name — reserved in v1"),
            )),
            // A cross-sheet prefix `Name!` qualifies the cell reference that must follow it. The
            // sheet NAME is carried as syntax onto the `RefNode`; resolving it to a `SheetId` is a
            // `Resolver` (eval-time) act (ast-standards PART 6: no semantics baked into syntax).
            TokenKind::SheetBang(name) => self.parse_sheet_qualified(name.clone()),
            // A closing paren or an operator with nothing to its left.
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

    /// Parse a cross-sheet reference whose `Name!` prefix (or `'Quoted Name'!`) was just consumed:
    /// the sheet name qualifies the cell reference that MUST follow. The name is attached to the
    /// resulting [`RefNode`] as syntax. A prefix on anything other than a cell reference (`Sheet1!5`,
    /// `Sheet1!SUM(..)`, `Sheet1!` at end-of-input) is a located refusal — the qualified target must
    /// be an A1 cell.
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
            other => {
                let span = other.map_or_else(|| self.eof_span(), |t| t.span);
                Err(Diag::new(
                    DiagCode::UnexpectedToken,
                    span,
                    format!("the sheet prefix `{name}!` must be followed by a cell reference"),
                ))
            }
        }
    }

    /// Parse the argument list of a call whose name token was just consumed (the next token is `(`),
    /// then resolve the name to a `FuncId` and check arity against the registry.
    fn parse_call(&mut self, name: String, name_span: Span) -> Result<Expr, Diag> {
        // The lexer only emits `Func` immediately before `(`, so this is guaranteed.
        debug_assert!(matches!(
            self.peek().map(|t| &t.kind),
            Some(TokenKind::LParen)
        ));
        self.advance(); // consume `(`

        let mut args = Vec::new();
        if matches!(self.peek().map(|t| &t.kind), Some(TokenKind::RParen)) {
            self.advance();
        } else {
            loop {
                args.push(self.parse_expr(0)?);
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

        let Some(fid) = func::lookup(&name) else {
            return Err(Diag::new(
                DiagCode::UnknownFunction,
                name_span,
                format!("`{name}` is not a recognized function"),
            ));
        };
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
        // An optional per-function static-argument check (registry data, not a hand-fork): TEXT vets
        // its format code here, refusing an unsupported LITERAL at parse rather than mis-rendering it
        // at eval; a non-literal (computed) format is accepted and deferred to eval (accept-under-
        // uncertainty, never a false-reject). A `None`-validate row is a no-op.
        def.validate_args(&args, name_span)?;
        Ok(Expr::Call(fid, args))
    }

    /// Build a binary/`:` node from an operator token and its two operands.
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

/// Fold `ref : ref` into a static [`Expr::Range`]. Both endpoints must be static cell references;
/// anything else (a dynamic range like `INDEX(...):B2`) is a `reserved-dynamic-range` refusal — the
/// dynamic reference operators have no v1 node (scope.md). Corners are normalized to
/// top-left..bottom-right so a reversed spelling still resolves.
///
/// A single sheet qualifier is carried onto the whole range (Excel: `Sheet1!A1:B2` reads A1:B2 on
/// Sheet1, so a qualifier on the *left* endpoint with an unqualified right endpoint is the normal
/// form). A range whose endpoints name *different* sheets, or whose qualifier sits on the right
/// endpoint only (`A1:Sheet2!B2`), is a 3D / multi-sheet reference — reserved in v1
/// (`reserved-cross-sheet`).
fn fold_range(lhs: Expr, rhs: Expr, span: Span) -> Result<Expr, Diag> {
    let (Expr::Ref(a), Expr::Ref(b)) = (&lhs, &rhs) else {
        return Err(Diag::new(
            DiagCode::ReservedDynamicRange,
            span,
            "a `:` range needs two static cell references (dynamic ranges are reserved in v1)",
        ));
    };
    // The sheet qualifier for the whole range: the left endpoint's, with an unqualified right
    // endpoint inheriting it. A qualifier that appears on the right (or a mismatched pair) is a 3D
    // reference — reserved in v1.
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
    // Normalize each axis to (min, max) independently, carrying each endpoint's `$`-anchor flag with
    // the corner it lands on, so a mixed range like `$E$2:E2` keeps "start absolute, end relative"
    // through the normalization — the AST faithfully represents each corner's anchor for source
    // round-trip, even though the engine resolves refs as absolute addresses (VAL1: no offsetting).
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

// --- the precedence ladder (binding powers) --------------------------------------------------
// Higher = binds tighter. Left-associative binary ops use (n, n+1); the whole ladder, tightest
// last, is: comparisons < & < +/- < */ < ^ < % (postfix) < unary -/+ (prefix) < : (range).
// (`,` union and space intersection sit above unary in Excel's ladder but are RESERVED — they never
// build a node, so they carry no binding power here; the parser refuses them positionally.)

/// Prefix binding power for unary `-`/`+` — above `^` (so `-2^2` is `(-2)^2`, Excel).
///
/// `pub(crate)` so `schema.rs` can bind the published precedence ladder to the parser's *actual*
/// binding powers (see `schema::tests::precedence_matches_the_parsers_binding_powers`) — the numbers
/// have exactly one source of truth, here.
pub(crate) const PREFIX_BP: u8 = 70;
/// Prefix binding power for `@` implicit-intersection — below `:` (so `@A1:B2` is `@(A1:B2)`) but
/// above arithmetic (so `@A1+1` is `(@A1)+1`). `pub(crate)` — see [`PREFIX_BP`].
pub(crate) const AT_BP: u8 = 85;

/// The (left, right) binding powers of a binary/`:` operator, or `None` if the token is not infix.
/// `pub(crate)` — see [`PREFIX_BP`].
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

/// The binding power of a postfix operator (`%`, `#`), or `None`. `pub(crate)` — see [`PREFIX_BP`].
pub(crate) fn postfix_bp(kind: &TokenKind) -> Option<u8> {
    match kind {
        TokenKind::Percent => Some(60),
        TokenKind::Hash => Some(80),
        _ => None,
    }
}

/// Whether a token can begin a primary (value/operand) — used to detect juxtaposition.
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
            | TokenKind::At
    )
}

/// Whether a juxtaposed primary is reference-like (so juxtaposition reads as reserved intersection
/// rather than a bare syntax error).
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

/// A human phrase for a function's arity, for a `bad-arity` message.
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

    /// Parse then evaluate against a blank single-cell grid (for pure-value formulas).
    fn run(formula: &str) -> Value {
        let g = Grid::new(1, vec![Value::Blank]);
        eval(&parse(formula).expect("should parse"), &g)
    }

    fn parse_err(formula: &str) -> DiagCode {
        parse(formula).unwrap_err().code
    }

    #[test]
    fn precedence_ladder_matches_excel() {
        assert_eq!(run("=1+2*3"), Value::Number(7.0));
        assert_eq!(run("=(1+2)*3"), Value::Number(9.0));
        // unary minus binds tighter than ^: -2^2 = (-2)^2 = 4
        assert_eq!(run("=-2^2"), Value::Number(4.0));
        // ^ is left-associative in Excel: 2^3^2 = (2^3)^2 = 64
        assert_eq!(run("=2^3^2"), Value::Number(64.0));
        // % is postfix, tighter than ^ and *: 50% = 0.5, 2*50% = 1
        assert_eq!(run("=2*50%"), Value::Number(1.0));
        // & concat is looser than +: 1+2&"x" = "3x"
        assert_eq!(run("=1+2&\"x\""), Value::Text("3x".into()));
        // comparisons are loosest: 1+1=2 -> TRUE
        assert_eq!(run("=1+1=2"), Value::Bool(true));
    }

    #[test]
    fn leading_equals_optional_and_spans_align() {
        assert_eq!(run("=1+1"), Value::Number(2.0));
        assert_eq!(run("1+1"), Value::Number(2.0));
        // With the leading `=`, a refusal span still indexes the original string.
        let d = parse("=1+").unwrap_err();
        assert_eq!(d.code, DiagCode::UnexpectedEof);
        assert_eq!(d.span, Span::at(3)); // just past the final `+`
    }

    #[test]
    fn static_range_folds_and_functions_resolve() {
        // A1:B2 folds to a Range; SUM over it works against a 2x2 grid.
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
        // reversed spelling normalizes to the same rectangle
        assert_eq!(
            eval(&parse("=SUM(B2:A1)").unwrap(), &g),
            Value::Number(10.0)
        );
    }

    #[test]
    fn cross_sheet_refs_parse_and_carry_the_sheet_name() {
        // `Sheet1!A1` -> a Ref carrying the parsed sheet NAME as syntax (not a resolved id).
        match parse("=Sheet1!A1").unwrap() {
            Expr::Ref(r) => {
                assert_eq!((r.col, r.row), (0, 0));
                assert_eq!(r.sheet.as_ref().map(SheetName::as_str), Some("Sheet1"));
            }
            other => panic!("expected a Ref, got {other:?}"),
        }
        // A quoted sheet name with a space survives verbatim.
        match parse("='My Sheet'!$B$3").unwrap() {
            Expr::Ref(r) => {
                assert_eq!((r.col, r.row, r.col_abs, r.row_abs), (1, 2, true, true));
                assert_eq!(r.sheet.as_ref().map(SheetName::as_str), Some("My Sheet"));
            }
            other => panic!("expected a Ref, got {other:?}"),
        }
        // `Sheet1!A1:B2` -> a Range whose sheet name qualifies the whole rectangle.
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
        // `@A1` -> ImplicitIntersect(Ref); `A1#` -> SpillRef(Ref). Parse must PRESERVE them.
        assert!(matches!(parse("=@A1").unwrap(), Expr::ImplicitIntersect(_)));
        assert!(matches!(parse("=A1#").unwrap(), Expr::SpillRef(_)));
        // eval is deferred: spill -> #CALC!
        assert_eq!(run("=A1#"), Value::Error(ErrKind::Calc));
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
        // A single-sheet cross-sheet ref now PARSES; only a 3D / multi-sheet range stays reserved.
        assert_eq!(
            parse_err("=Sheet1!A1:Sheet2!B2"),
            DiagCode::ReservedCrossSheet
        );
        assert_eq!(parse_err("=A1:Sheet2!B2"), DiagCode::ReservedCrossSheet);
        // A sheet prefix on a non-cell target is a plain unexpected-token refusal.
        assert_eq!(parse_err("=Sheet1!5"), DiagCode::UnexpectedToken);
        assert_eq!(parse_err("=Sheet1!"), DiagCode::UnexpectedToken);
        assert_eq!(parse_err("=myname"), DiagCode::ReservedName);
        // A `:` whose left endpoint is not a static ref (here a function result) is reserved.
        assert_eq!(parse_err("=SUM(A1):A2"), DiagCode::ReservedDynamicRange);
        assert_eq!(parse_err("="), DiagCode::EmptyFormula);
        assert_eq!(parse_err("=*3"), DiagCode::UnexpectedToken);
    }

    #[test]
    fn nesting_is_bounded_not_a_stack_overflow() {
        // Depth bound: many nested parens -> a located refusal, never a crash.
        let deep = format!("={}1{}", "(".repeat(300), ")".repeat(300));
        assert_eq!(parse(&deep).unwrap_err().code, DiagCode::RecursionLimit);
        // Prefix nesting is bounded too.
        let deep_neg = format!("={}1", "-".repeat(300));
        assert_eq!(parse(&deep_neg).unwrap_err().code, DiagCode::RecursionLimit);
        // Size bound: a huge flat chain is refused before a giant tree is built (or dropped).
        let flat = format!("=1{}", "+1".repeat(MAX_TOKENS));
        assert_eq!(parse(&flat).unwrap_err().code, DiagCode::RecursionLimit);
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
        // IFERROR(A1/B2, ROUND(AVERAGE(A1:B1), 0))  with A1=10 B2=5 -> 2
        assert_eq!(
            eval(
                &parse("=IFERROR(A1/B2, ROUND(AVERAGE(A1:B1),0))").unwrap(),
                &g
            ),
            Value::Number(2.0)
        );
        // Force the error branch: A1/A2 with A2=0 -> #DIV/0! -> ROUND(AVERAGE(A1:B1)=15, 0)=15
        assert_eq!(
            eval(
                &parse("=IFERROR(A1/A2, ROUND(AVERAGE(A1:B1),0))").unwrap(),
                &g
            ),
            Value::Number(15.0)
        );
    }
}
