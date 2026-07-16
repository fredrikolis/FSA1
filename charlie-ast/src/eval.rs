// Concern: the tree-walking EVALUATOR — `Expr` + a `&dyn Resolver` -> a first-class `Value`, with full left-to-right error propagation (an operand `Error` short-circuits to that error), the operator semantics (arithmetic/`%`/`^`/`&` concat/the six comparisons), the numeric/boolean/text COERCION rules, reference & range resolution through the `Resolver` (a range materializes its borrowed `ArrayView` into an owned `Value::Array`), and the deferred identity/`#CALC!` handling of the reserved `@`/`#` nodes; errors are values, never panics | Non-concern: PARSING text into an `Expr` (parser.rs) and the per-FUNCTION semantics (func.rs owns the registry impls; this module only dispatches a `Call` to them) | IO: (an `&Expr`, an `&dyn Resolver`) -> a `Value`
//! The evaluator: [`EvalCtx`] and [`eval`]. Synchronous over a pre-loaded [`Resolver`] (no lazy
//! per-cell I/O — see the resolver contract). Every failure is a first-class [`Value::Error`]; the
//! evaluator never panics and never returns a [`crate::Diag`] (refusals are a parse-time concern).

use crate::expr::{BinOp, Expr, UnOp};
use crate::func;
use crate::refs::{RangeNode, RefNode};
use crate::resolver::Resolver;
use crate::value::{ErrKind, Value};

/// A hard ceiling on evaluation recursion depth. The parser already bounds nesting (see
/// `parser::MAX_DEPTH`), so a parsed tree cannot reach this; it is defense-in-depth for a
/// *synthesized* tree handed straight to [`eval`] without going through the parser — such a tree
/// yields a `#NUM!` value rather than a stack overflow (ast-standards PART 9, "every later walk is
/// stack-safe"). Kept above the parser bound so the parser's limit is the one users normally meet.
const EVAL_DEPTH_LIMIT: u32 = 512;

/// The evaluator's working context: its entire view of the outside world ([`Resolver`]) plus the
/// live recursion depth. Threaded (`&mut`) through [`eval`] and every registry function so depth is
/// tracked across the whole walk.
pub struct EvalCtx<'r> {
    resolver: &'r dyn Resolver,
    depth: u32,
}

impl<'r> EvalCtx<'r> {
    pub fn new(resolver: &'r dyn Resolver) -> EvalCtx<'r> {
        EvalCtx { resolver, depth: 0 }
    }

    /// The "now" instant the VOLATILE `TODAY`/`NOW` built-ins read, as an Excel date-time serial, from
    /// the resolver's injectable [`Resolver::now_serial`] clock. Routed through the resolver — never
    /// `std::time` inline in a built-in — so conformance and tests PIN it deterministically.
    pub(crate) fn now_serial(&self) -> f64 {
        self.resolver.now_serial()
    }

    /// Evaluate one sub-expression, tracking depth. Registry functions call this on their argument
    /// `Expr`s (so lazy forms like `IF`/`IFERROR` control *which* arguments are evaluated).
    pub fn eval(&mut self, expr: &Expr) -> Value {
        if self.depth >= EVAL_DEPTH_LIMIT {
            return Value::Error(ErrKind::Num);
        }
        self.depth += 1;
        let v = self.eval_inner(expr);
        self.depth -= 1;
        v
    }

    fn eval_inner(&mut self, expr: &Expr) -> Value {
        match expr {
            Expr::Lit(v) => v.clone(),
            Expr::Ref(r) => self.eval_ref(r),
            Expr::Range(rn) => self.eval_range(rn),
            Expr::Unary(op, inner) => self.eval_unary(*op, inner),
            Expr::Binary(op, l, r) => self.eval_binary(*op, l, r),
            Expr::Call(fid, args) => func::dispatch(*fid, self, args),
            // RESERVED nodes (scope.md): evaluation is deferred in scalar-only v1. Implicit
            // intersection of an already-scalar value (including a 1×1 range) is the identity; on a
            // genuinely multi-cell array the real semantics are deferred -> `#CALC!`. The spill
            // operator has no v1 anchor -> `#CALC!`.
            Expr::ImplicitIntersect(inner) => match self.eval(inner) {
                Value::Array(shape, mut cells) if shape.rows == 1 && shape.cols == 1 => {
                    cells.pop().unwrap_or(Value::Blank)
                }
                Value::Array(..) => Value::Error(ErrKind::Calc),
                scalar => scalar,
            },
            Expr::SpillRef(_) => Value::Error(ErrKind::Calc),
        }
    }

    fn eval_ref(&self, r: &RefNode) -> Value {
        // Resolve the syntactic ref to a coordinate the resolver reads: its sheet NAME (if any) is
        // mapped to a `SheetId` through the resolver's `sheet_id` seam — the one place a parsed name
        // becomes a semantic handle (ast-standards PART 6). An unknown sheet is `#REF!`.
        match r.resolve(|name| self.resolver.sheet_id(name)) {
            Some(cell) => self.resolver.value(cell),
            None => Value::Error(ErrKind::Ref),
        }
    }

    fn eval_range(&self, rn: &RangeNode) -> Value {
        // Resolve the range's sheet NAME (if any) via the resolver, then materialize the borrowed
        // view into an owned array — the deliberate copy the architecture blesses (a view cannot
        // live in a returned `Value`). An unknown sheet is `#REF!`.
        match rn.resolve(|name| self.resolver.sheet_id(name)) {
            Some(rr) => {
                let view = self.resolver.range(rr);
                Value::Array(view.shape, view.cells.to_vec())
            }
            None => Value::Error(ErrKind::Ref),
        }
    }

    fn eval_unary(&mut self, op: UnOp, inner: &Expr) -> Value {
        let v = scalarize(self.eval(inner));
        if let Value::Error(k) = v {
            return Value::Error(k);
        }
        match coerce_num(&v) {
            Err(k) => Value::Error(k),
            Ok(n) => match op {
                UnOp::Plus => Value::Number(n),
                UnOp::Neg => Value::Number(-n),
                UnOp::Percent => Value::Number(n / 100.0),
            },
        }
    }

    fn eval_binary(&mut self, op: BinOp, l: &Expr, r: &Expr) -> Value {
        // Left-to-right error short-circuit: an explicit `Error` operand wins, leftmost first
        // (ast-standards: "an operand Error short-circuits to that Error"). Each operand is
        // collapsed to a scalar FIRST, before the other is even evaluated, so an error that only
        // surfaces on scalarize — a 1×1 range whose single cell is an error, or a multi-cell array
        // in scalar position (`#VALUE!`) — still preempts the right operand. Scalarizing the raw
        // value (a bare `Error` passes through unchanged) is what makes the leftmost rule hold for a
        // range/array operand, not only for a plain `Error` value.
        let lv = scalarize(self.eval(l));
        if let Value::Error(k) = lv {
            return Value::Error(k);
        }
        let rv = scalarize(self.eval(r));
        if let Value::Error(k) = rv {
            return Value::Error(k);
        }

        match op {
            BinOp::Concat => match (to_text(&lv), to_text(&rv)) {
                (Err(k), _) | (_, Err(k)) => Value::Error(k),
                (Ok(a), Ok(b)) => Value::Text(a + &b),
            },
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                compare(op, lv, rv)
            }
            // Arithmetic — coerce both to numbers (leftmost coercion error wins). Both operands are
            // already scalar here, so `coerce_num` sees a scalar directly.
            _ => {
                let a = match coerce_num(&lv) {
                    Ok(a) => a,
                    Err(k) => return Value::Error(k),
                };
                let b = match coerce_num(&rv) {
                    Ok(b) => b,
                    Err(k) => return Value::Error(k),
                };
                arith(op, a, b)
            }
        }
    }
}

/// Evaluate a whole formula tree against a resolver — the crate's top-level eval entry.
pub fn eval(expr: &Expr, resolver: &dyn Resolver) -> Value {
    EvalCtx::new(resolver).eval(expr)
}

/// Apply a scalar arithmetic operator to two already-coerced numbers, mapping the Excel
/// error conditions (`/0`, `0^-n`, an overflowing/complex power) to first-class errors.
fn arith(op: BinOp, a: f64, b: f64) -> Value {
    let r = match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        BinOp::Div => {
            if b == 0.0 {
                return Value::Error(ErrKind::Div0);
            }
            a / b
        }
        // The `^` operator and the `POWER` built-in share one exponentiation semantics.
        BinOp::Pow => return pow(a, b),
        // Non-arithmetic ops are handled before `arith` is reached.
        _ => unreachable!("arith called with a non-arithmetic operator"),
    };
    if r.is_finite() {
        Value::Number(r)
    } else {
        // A finite-operand computation that still overflowed (e.g. `1e300 * 1e300`) -> `#NUM!`.
        Value::Error(ErrKind::Num)
    }
}

/// Excel exponentiation, shared by the `^` operator and the `POWER` built-in so both map the error
/// conditions identically: `0` to a negative power is `#DIV/0!`; an overflowing or complex result
/// (e.g. `(-8)^0.5`) is `#NUM!`; otherwise the finite power.
pub(crate) fn pow(a: f64, b: f64) -> Value {
    if a == 0.0 && b < 0.0 {
        return Value::Error(ErrKind::Div0);
    }
    let p = a.powf(b);
    if p.is_finite() {
        Value::Number(p)
    } else {
        Value::Error(ErrKind::Num)
    }
}

/// Collapse a value to a scalar for a scalar operator position: a 1×1 array is its single cell;
/// a larger array cannot be used in scalar-only v1 (broadcasting is deferred to W3b) -> `#VALUE!`.
/// Non-array values pass through unchanged.
pub(crate) fn scalarize(v: Value) -> Value {
    match v {
        Value::Array(shape, mut cells) if shape.rows == 1 && shape.cols == 1 => {
            cells.pop().unwrap_or(Value::Blank)
        }
        Value::Array(..) => Value::Error(ErrKind::Value),
        other => other,
    }
}

/// Coerce a *scalar* value to a number (Excel arithmetic coercion). `Blank` is `0`; a boolean is
/// `1`/`0`; numeric-looking text parses (else `#VALUE!`); an error propagates; a multi-cell array is
/// `#VALUE!` (caller normally [`scalarize`]s first, but this stays total).
pub(crate) fn coerce_num(v: &Value) -> Result<f64, ErrKind> {
    match v {
        Value::Number(n) => Ok(*n),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::Blank => Ok(0.0),
        Value::Text(t) => match t.trim().parse::<f64>() {
            Ok(n) if n.is_finite() => Ok(n),
            _ => Err(ErrKind::Value),
        },
        Value::Error(k) => Err(*k),
        Value::Array(shape, cells) if shape.rows == 1 && shape.cols == 1 => {
            coerce_num(cells.first().unwrap_or(&Value::Blank))
        }
        Value::Array(..) => Err(ErrKind::Value),
    }
}

/// Coerce a scalar value to a boolean (Excel logical coercion). Numbers are truthy when non-zero;
/// text is `TRUE`/`FALSE` case-insensitively (else `#VALUE!`); `Blank` is `false`; an error
/// propagates. Used by the logical functions and `IF`.
pub(crate) fn coerce_bool(v: &Value) -> Result<bool, ErrKind> {
    match scalarize(v.clone()) {
        Value::Bool(b) => Ok(b),
        Value::Number(n) => Ok(n != 0.0),
        Value::Blank => Ok(false),
        Value::Text(t) if t.eq_ignore_ascii_case("TRUE") => Ok(true),
        Value::Text(t) if t.eq_ignore_ascii_case("FALSE") => Ok(false),
        Value::Text(_) => Err(ErrKind::Value),
        Value::Error(k) => Err(k),
        Value::Array(..) => Err(ErrKind::Value),
    }
}

/// Coerce a scalar value to its text form for `&` concatenation. `Blank` is the empty string; a
/// number uses a general format (`1`, not `1`.0); a boolean is `TRUE`/`FALSE`; an error propagates.
pub(crate) fn to_text(v: &Value) -> Result<String, ErrKind> {
    match scalarize(v.clone()) {
        Value::Text(t) => Ok(t),
        Value::Number(n) => Ok(num_to_text(n)),
        Value::Bool(b) => Ok(if b { "TRUE" } else { "FALSE" }.to_string()),
        Value::Blank => Ok(String::new()),
        Value::Error(k) => Err(k),
        Value::Array(..) => Err(ErrKind::Value),
    }
}

/// Render a number the way `&`-concat and `TEXT`-free contexts do: general format, no trailing `.0`,
/// and `0` for both `0.0` and `-0.0` (Excel shows an unsigned zero).
pub(crate) fn num_to_text(n: f64) -> String {
    if n == 0.0 {
        return "0".to_string();
    }
    // Rust's `Display` for `f64` already yields the shortest round-tripping form without a spurious
    // `.0` (e.g. `1.0 -> "1"`, `1.5 -> "1.5"`), which matches Excel's general format closely enough
    // for v1's non-`TEXT` concatenation.
    format!("{n}")
}

/// Compare two already-error-free values under a comparison operator, returning a `Bool` (or an
/// `#VALUE!` if an operand is a non-scalar array). Excel semantics: numbers numerically; text
/// case-*insensitively*; cross-type by the rank Number &lt; Text &lt; Bool; `Blank` takes the other
/// operand's zero (`0` / `""` / `FALSE`).
fn compare(op: BinOp, l: Value, r: Value) -> Value {
    use std::cmp::Ordering;
    let l = scalarize(l);
    let r = scalarize(r);
    if let Value::Error(k) = l {
        return Value::Error(k);
    }
    if let Value::Error(k) = r {
        return Value::Error(k);
    }

    let ord = compare_ord(&l, &r);
    let result = match op {
        BinOp::Eq => ord == Ordering::Equal,
        BinOp::Ne => ord != Ordering::Equal,
        BinOp::Lt => ord == Ordering::Less,
        BinOp::Le => ord != Ordering::Greater,
        BinOp::Gt => ord == Ordering::Greater,
        BinOp::Ge => ord != Ordering::Less,
        _ => unreachable!("compare called with a non-comparison operator"),
    };
    Value::Bool(result)
}

/// Excel `=` equality between two scalar values, the matcher `SWITCH` reads: an error operand
/// propagates (leftmost — the subject before the candidate), otherwise the two are equal iff they
/// rank `Equal` under [`compare_ord`] (numbers numerically, text case-*insensitively*, a `Blank`
/// against the other side's zero, cross-type never equal). Each operand is [`scalarize`]d first so a
/// 1×1 range compares as its single cell and a multi-cell array is `#VALUE!`.
pub(crate) fn value_eq(a: &Value, b: &Value) -> Result<bool, ErrKind> {
    let a = scalarize(a.clone());
    if let Value::Error(k) = a {
        return Err(k);
    }
    let b = scalarize(b.clone());
    if let Value::Error(k) = b {
        return Err(k);
    }
    Ok(compare_ord(&a, &b) == std::cmp::Ordering::Equal)
}

/// The engine's total ordering over two scalar values — the *same* order the comparison operators
/// read (numbers numerically, text case-*insensitively*, cross-type ranked Number &lt; Text &lt; Bool,
/// and a lone `Blank` resolved against the other operand's zero). Exposed so the approximate-match
/// lookup family reuses ONE ordering: `MATCH` modes `±1`, `VLOOKUP`'s sorted-first-column search, and
/// `XLOOKUP`'s next-smaller/next-larger all rank cells this way, so lookup ordering can never drift
/// from operator ordering (the batch's cross-type-ordering contract). Callers screen errors/arrays
/// first; an array/error defensively takes the top rank inside [`compare_ord`].
pub(crate) fn value_cmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    compare_ord(a, b)
}

/// The total order the comparison operators read. `Blank` is resolved against the *other* operand's
/// type before ranking, so `A1=0` is true for a blank `A1`.
fn compare_ord(l: &Value, r: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    // Resolve a lone Blank to the other side's zero so a blank compares as that type's identity.
    let (l, r) = match (l, r) {
        (Value::Blank, Value::Blank) => return Ordering::Equal,
        (Value::Blank, other) => (blank_as(other), other.clone()),
        (other, Value::Blank) => (other.clone(), blank_as(other)),
        (a, b) => (a.clone(), b.clone()),
    };
    match (&l, &r) {
        // `partial_cmp`, not `total_cmp`, so `-0.0 == 0.0` (Excel treats them as equal — e.g.
        // `(0*-1)=0` is TRUE); arith produces `Number(-0.0)`. NaN cannot arise (the lexer,
        // `coerce_num`, and `arith` all reject non-finite), so the `None` fallback is unreachable.
        (Value::Number(a), Value::Number(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        (Value::Text(a), Value::Text(b)) => {
            // Case-insensitive (Excel `"a"="A"` is TRUE), ASCII-fold.
            a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase())
        }
        // Cross-type: rank Number < Text < Bool.
        _ => type_rank(&l).cmp(&type_rank(&r)),
    }
}

/// A `Blank` seen next to `other` behaves as that type's zero value.
fn blank_as(other: &Value) -> Value {
    match other {
        Value::Number(_) => Value::Number(0.0),
        Value::Text(_) => Value::Text(String::new()),
        Value::Bool(_) => Value::Bool(false),
        _ => Value::Blank,
    }
}

/// The cross-type comparison rank (Excel): Number &lt; Text &lt; Bool. Errors/arrays/blanks never
/// reach here (handled earlier), so they take the top slot defensively.
fn type_rank(v: &Value) -> u8 {
    match v {
        Value::Number(_) => 0,
        Value::Text(_) => 1,
        Value::Bool(_) => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;
    use crate::refs::RangeNode;
    use crate::test_support::Grid;
    use crate::value::Shape;

    fn cell(col: u32, row: u32) -> Expr {
        Expr::Ref(RefNode {
            col,
            row,
            col_abs: false,
            row_abs: false,
            sheet: None,
        })
    }

    fn num(n: f64) -> Expr {
        Expr::Lit(Value::Number(n))
    }

    fn eval_on(expr: &Expr, grid: &Grid) -> Value {
        eval(expr, grid)
    }

    #[test]
    fn arithmetic_and_coercion() {
        let g = Grid::new(1, vec![Value::Blank]);
        // 1 + 2 * 3 (tree built to reflect precedence)
        let e = Expr::Binary(
            BinOp::Add,
            Box::new(num(1.0)),
            Box::new(Expr::Binary(
                BinOp::Mul,
                Box::new(num(2.0)),
                Box::new(num(3.0)),
            )),
        );
        assert_eq!(eval_on(&e, &g), Value::Number(7.0));
        // "5" + 1 -> 6 (numeric text coerces)
        let e = Expr::Binary(
            BinOp::Add,
            Box::new(Expr::Lit(Value::Text("5".into()))),
            Box::new(num(1.0)),
        );
        assert_eq!(eval_on(&e, &g), Value::Number(6.0));
        // "x" + 1 -> #VALUE!
        let e = Expr::Binary(
            BinOp::Add,
            Box::new(Expr::Lit(Value::Text("x".into()))),
            Box::new(num(1.0)),
        );
        assert_eq!(eval_on(&e, &g), Value::Error(ErrKind::Value));
    }

    #[test]
    fn division_and_power_errors() {
        let g = Grid::new(1, vec![Value::Blank]);
        let div0 = Expr::Binary(BinOp::Div, Box::new(num(1.0)), Box::new(num(0.0)));
        assert_eq!(eval_on(&div0, &g), Value::Error(ErrKind::Div0));
        // 0 ^ -1 -> #DIV/0!
        let e = Expr::Binary(BinOp::Pow, Box::new(num(0.0)), Box::new(num(-1.0)));
        assert_eq!(eval_on(&e, &g), Value::Error(ErrKind::Div0));
        // (-8) ^ 0.5 -> #NUM! (complex)
        let e = Expr::Binary(BinOp::Pow, Box::new(num(-8.0)), Box::new(num(0.5)));
        assert_eq!(eval_on(&e, &g), Value::Error(ErrKind::Num));
        // 2 ^ 3 ^ 2 is left-assoc at the tree level: (2^3)^2 = 64 — verify eval of that tree.
        let e = Expr::Binary(
            BinOp::Pow,
            Box::new(Expr::Binary(
                BinOp::Pow,
                Box::new(num(2.0)),
                Box::new(num(3.0)),
            )),
            Box::new(num(2.0)),
        );
        assert_eq!(eval_on(&e, &g), Value::Number(64.0));
    }

    #[test]
    fn error_propagation_is_leftmost() {
        let g = Grid::new(1, vec![Value::Blank]);
        let e = Expr::Binary(
            BinOp::Add,
            Box::new(Expr::Lit(Value::Error(ErrKind::Ref))),
            Box::new(Expr::Lit(Value::Error(ErrKind::Div0))),
        );
        assert_eq!(eval_on(&e, &g), Value::Error(ErrKind::Ref));
    }

    #[test]
    fn leftmost_error_wins_through_a_range_or_array_left_operand() {
        // A left RANGE operand that only becomes an error after scalarize must still preempt a
        // *different* explicit error on the right (Excel: leftmost error wins).
        let range = |sc: u32, sr: u32, ec: u32, er: u32| {
            Expr::Range(RangeNode {
                start_col: sc,
                start_row: sr,
                end_col: ec,
                end_row: er,
                sheet: None,
            })
        };
        // A1=#DIV/0!. =(A1:A1)+#REF! -> #DIV/0! (the left cell's error, NOT the right's #REF!).
        let g = Grid::new(1, vec![Value::Error(ErrKind::Div0)]);
        let e = Expr::Binary(
            BinOp::Add,
            Box::new(range(0, 0, 0, 0)),
            Box::new(Expr::Lit(Value::Error(ErrKind::Ref))),
        );
        assert_eq!(eval_on(&e, &g), Value::Error(ErrKind::Div0));
        // Same for a comparison operator: =(A1:A1)=#REF! -> #DIV/0!.
        let e = Expr::Binary(
            BinOp::Eq,
            Box::new(range(0, 0, 0, 0)),
            Box::new(Expr::Lit(Value::Error(ErrKind::Ref))),
        );
        assert_eq!(eval_on(&e, &g), Value::Error(ErrKind::Div0));

        // A MULTI-cell left array scalarizes to #VALUE!, which must win over a right #REF!.
        let g = Grid::new(1, vec![Value::Number(1.0), Value::Number(2.0)]);
        let e = Expr::Binary(
            BinOp::Add,
            Box::new(range(0, 0, 0, 1)),
            Box::new(Expr::Lit(Value::Error(ErrKind::Ref))),
        );
        assert_eq!(eval_on(&e, &g), Value::Error(ErrKind::Value));
    }

    #[test]
    fn concat_and_percent() {
        let g = Grid::new(1, vec![Value::Blank]);
        let e = Expr::Binary(
            BinOp::Concat,
            Box::new(num(1.0)),
            Box::new(Expr::Lit(Value::Text("x".into()))),
        );
        assert_eq!(eval_on(&e, &g), Value::Text("1x".into()));
        // 50% -> 0.5
        let e = Expr::Unary(UnOp::Percent, Box::new(num(50.0)));
        assert_eq!(eval_on(&e, &g), Value::Number(0.5));
        // TRUE & "!" -> "TRUE!"
        let e = Expr::Binary(
            BinOp::Concat,
            Box::new(Expr::Lit(Value::Bool(true))),
            Box::new(Expr::Lit(Value::Text("!".into()))),
        );
        assert_eq!(eval_on(&e, &g), Value::Text("TRUE!".into()));
    }

    #[test]
    fn comparisons_follow_excel_type_rules() {
        let g = Grid::new(1, vec![Value::Blank]);
        // case-insensitive text equality
        let e = Expr::Binary(
            BinOp::Eq,
            Box::new(Expr::Lit(Value::Text("a".into()))),
            Box::new(Expr::Lit(Value::Text("A".into()))),
        );
        assert_eq!(eval_on(&e, &g), Value::Bool(true));
        // number < text (cross-type rank)
        let e = Expr::Binary(
            BinOp::Lt,
            Box::new(num(999.0)),
            Box::new(Expr::Lit(Value::Text("a".into()))),
        );
        assert_eq!(eval_on(&e, &g), Value::Bool(true));
        // blank = 0
        let e = Expr::Binary(
            BinOp::Eq,
            Box::new(Expr::Lit(Value::Blank)),
            Box::new(num(0.0)),
        );
        assert_eq!(eval_on(&e, &g), Value::Bool(true));
        // signed zero: (0*-1) = 0 -> TRUE (Excel treats -0 and +0 as equal, unlike total_cmp).
        let neg_zero = Expr::Binary(BinOp::Mul, Box::new(num(0.0)), Box::new(num(-1.0)));
        let e = Expr::Binary(BinOp::Eq, Box::new(neg_zero), Box::new(num(0.0)));
        assert_eq!(eval_on(&e, &g), Value::Bool(true));
    }

    #[test]
    fn ref_and_range_resolution() {
        // 2x2 grid: A1=1 B1=2 / A2=3 B2=4
        let g = Grid::new(
            2,
            vec![
                Value::Number(1.0),
                Value::Number(2.0),
                Value::Number(3.0),
                Value::Number(4.0),
            ],
        );
        assert_eq!(eval_on(&cell(1, 1), &g), Value::Number(4.0)); // B2
        let range = Expr::Range(RangeNode {
            start_col: 0,
            start_row: 0,
            end_col: 1,
            end_row: 1,
            sheet: None,
        });
        match eval_on(&range, &g) {
            Value::Array(shape, cells) => {
                assert_eq!(shape, Shape { rows: 2, cols: 2 });
                assert_eq!(cells.len(), 4);
            }
            other => panic!("expected array, got {other:?}"),
        }
    }

    #[test]
    fn cross_sheet_refs_resolve_through_the_resolver() {
        // Default sheet `Sheet1` (A1=1), plus a named `Data` sheet (A1=10, A2=20, A3=30).
        let g = Grid::new(1, vec![Value::Number(1.0)]).with_sheet(
            "Data",
            1,
            vec![
                Value::Number(10.0),
                Value::Number(20.0),
                Value::Number(30.0),
            ],
        );

        // A cross-sheet single ref routes to the named sheet (10), NOT the default sheet (1).
        assert_eq!(eval(&parse("=Data!A1").unwrap(), &g), Value::Number(10.0));
        // The same address unqualified reads the default sheet — proving the name actually routes.
        assert_eq!(eval(&parse("=A1").unwrap(), &g), Value::Number(1.0));
        // `Sheet1!A1` names the default sheet explicitly and agrees with the bare form.
        assert_eq!(eval(&parse("=Sheet1!A1").unwrap(), &g), Value::Number(1.0));

        // A cross-sheet RANGE sums the named sheet's column.
        assert_eq!(
            eval(&parse("=SUM(Data!A1:A3)").unwrap(), &g),
            Value::Number(60.0)
        );

        // An UNKNOWN sheet name is `#REF!` (resolution failed) — for a ref and for a range.
        assert_eq!(
            eval(&parse("=Nope!A1").unwrap(), &g),
            Value::Error(ErrKind::Ref)
        );
        assert_eq!(
            eval(&parse("=SUM(Nope!A1:A3)").unwrap(), &g),
            Value::Error(ErrKind::Ref)
        );
    }

    #[test]
    fn reserved_nodes_defer_at_eval() {
        let g = Grid::new(1, vec![Value::Number(7.0)]);
        // @scalar is identity; @array is deferred (#CALC!).
        let ii_scalar = Expr::ImplicitIntersect(Box::new(num(7.0)));
        assert_eq!(eval_on(&ii_scalar, &g), Value::Number(7.0));
        let ii_arr = Expr::ImplicitIntersect(Box::new(Expr::Range(RangeNode {
            start_col: 0,
            start_row: 0,
            end_col: 0,
            end_row: 0,
            sheet: None,
        })));
        // A 1x1 range scalarizes -> identity of the single cell, NOT #CALC! (it is scalar).
        assert_eq!(eval_on(&ii_arr, &g), Value::Number(7.0));
        let spill = Expr::SpillRef(Box::new(num(7.0)));
        assert_eq!(eval_on(&spill, &g), Value::Error(ErrKind::Calc));
    }

    #[test]
    fn synthesized_deep_tree_does_not_overflow_eval() {
        // A tree deeper than EVAL_DEPTH_LIMIT (bypassing the parser's own bound) yields #NUM!, not a
        // stack overflow.
        let g = Grid::new(1, vec![Value::Blank]);
        let mut e = num(1.0);
        for _ in 0..2000 {
            e = Expr::Unary(UnOp::Plus, Box::new(e));
        }
        assert_eq!(eval_on(&e, &g), Value::Error(ErrKind::Num));
    }
}
