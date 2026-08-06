// Concern: computes a formula's Value: the tree walk plus the coercion, comparison and text rules | Non-concern: parsing, the built-in function bodies | IO: (&Expr, &Resolver) -> Value

use crate::expr::{BinOp, Expr, UnOp};
use crate::func;
use crate::refs::{RangeNode, RefNode};
use crate::resolver::Resolver;
use crate::value::{ErrKind, Value};

/// Defence in depth for a SYNTHESIZED tree that never met the parser's own nesting bound: such a
/// tree yields `#NUM!` rather than a stack overflow. Kept above `parser::MAX_DEPTH`.
const EVAL_DEPTH_LIMIT: u32 = 512;

pub struct EvalCtx<'r> {
    resolver: &'r dyn Resolver,
    depth: u32,
    /// `None` when there is no computing cell; no-arg `ROW()`/`COLUMN()` then anchor to A1.
    current_cell: Option<(u32, u32)>,
}

impl<'r> EvalCtx<'r> {
    pub fn new(resolver: &'r dyn Resolver) -> EvalCtx<'r> {
        EvalCtx {
            resolver,
            depth: 0,
            current_cell: None,
        }
    }

    pub fn at_cell(resolver: &'r dyn Resolver, row: u32, col: u32) -> EvalCtx<'r> {
        EvalCtx {
            resolver,
            depth: 0,
            current_cell: Some((row, col)),
        }
    }

    pub(crate) fn current_row(&self) -> u32 {
        self.current_cell.map_or(0, |(row, _)| row) + 1
    }

    pub(crate) fn current_col(&self) -> u32 {
        self.current_cell.map_or(0, |(_, col)| col) + 1
    }

    pub(crate) fn now_serial(&self) -> f64 {
        self.resolver.now_serial()
    }

    pub(crate) fn rand_unit(&self) -> f64 {
        self.resolver.rand_unit()
    }

    /// `None` when the sheet name is unknown — the caller turns that into `#REF!`.
    pub(crate) fn ref_is_formula(&self, r: &RefNode) -> Option<bool> {
        r.resolve(|name| self.resolver.sheet_id(name))
            .map(|cell| self.resolver.is_formula(cell))
    }

    /// `ISFORMULA` of a range applies to its normalized TOP-LEFT anchor.
    pub(crate) fn range_is_formula(&self, rn: &RangeNode) -> Option<bool> {
        rn.resolve(|name| self.resolver.sheet_id(name))
            .map(|rr| self.resolver.is_formula(rr.normalized().start))
    }

    /// A registry function calls this per argument `Expr`, so a lazy form like `IF` chooses which
    /// arguments are evaluated at all.
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
            // Reserved: `@` of a scalar (or a 1x1) is the identity; a genuinely multi-cell array defers.
            Expr::ImplicitIntersect(inner) => match collapse_1x1(self.eval(inner)) {
                Value::Array(..) => Value::Error(ErrKind::Calc),
                scalar => scalar,
            },
            Expr::SpillRef(_) => Value::Error(ErrKind::Calc),
        }
    }

    fn eval_ref(&self, r: &RefNode) -> Value {
        match r.resolve(|name| self.resolver.sheet_id(name)) {
            Some(cell) => self.resolver.value(cell),
            None => Value::Error(ErrKind::Ref),
        }
    }

    /// The borrowed view is copied into an owned array: a view cannot live in a returned `Value`.
    fn eval_range(&self, rn: &RangeNode) -> Value {
        match rn.resolve(|name| self.resolver.sheet_id(name)) {
            Some(rr) => {
                let view = self.resolver.range(rr);
                Value::Array(view.shape, view.cells.to_vec())
            }
            None => Value::Error(ErrKind::Ref),
        }
    }

    fn eval_unary(&mut self, op: UnOp, inner: &Expr) -> Value {
        // Element-wise over an array — the engine half of the `--(cond)` idiom.
        match collapse_1x1(self.eval(inner)) {
            Value::Error(k) => Value::Error(k),
            Value::Array(shape, cells) => {
                let mapped = cells.iter().map(|c| unop_scalar(op, c)).collect();
                Value::Array(shape, mapped)
            }
            scalar => unop_scalar(op, &scalar),
        }
    }

    fn eval_binary(&mut self, op: BinOp, l: &Expr, r: &Expr) -> Value {
        // Leftmost error wins; a multi-cell array survives to broadcast rather than becoming #VALUE!.
        let lv = collapse_1x1(self.eval(l));
        if let Value::Error(k) = lv {
            return Value::Error(k);
        }
        let rv = collapse_1x1(self.eval(r));
        if let Value::Error(k) = rv {
            return Value::Error(k);
        }
        binary_broadcast(op, lv, rv)
    }
}

/// A shape mismatch between two arrays is `#VALUE!`; a scalar broadcasts over every cell.
fn binary_broadcast(op: BinOp, lv: Value, rv: Value) -> Value {
    match (lv, rv) {
        (Value::Array(ls, lc), Value::Array(rs, rc)) => {
            if ls != rs {
                return Value::Error(ErrKind::Value);
            }
            let cells = lc
                .iter()
                .zip(rc.iter())
                .map(|(a, b)| binop_scalar(op, a, b))
                .collect();
            Value::Array(ls, cells)
        }
        (Value::Array(ls, lc), rv) => {
            let cells = lc.iter().map(|a| binop_scalar(op, a, &rv)).collect();
            Value::Array(ls, cells)
        }
        (lv, Value::Array(rs, rc)) => {
            let cells = rc.iter().map(|b| binop_scalar(op, &lv, b)).collect();
            Value::Array(rs, cells)
        }
        (lv, rv) => binop_scalar(op, &lv, &rv),
    }
}

/// The one home of the scalar operator semantics — every broadcast cell runs through here.
fn binop_scalar(op: BinOp, l: &Value, r: &Value) -> Value {
    match op {
        BinOp::Concat => match (to_text(l), to_text(r)) {
            (Err(k), _) | (_, Err(k)) => Value::Error(k),
            (Ok(a), Ok(b)) => Value::Text(a + &b),
        },
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            compare(op, l.clone(), r.clone())
        }
        _ => {
            let a = match coerce_num(l) {
                Ok(a) => a,
                Err(k) => return Value::Error(k),
            };
            let b = match coerce_num(r) {
                Ok(b) => b,
                Err(k) => return Value::Error(k),
            };
            arith(op, a, b)
        }
    }
}

fn unop_scalar(op: UnOp, v: &Value) -> Value {
    match coerce_num(v) {
        Err(k) => Value::Error(k),
        Ok(n) => match op {
            UnOp::Plus => Value::Number(n),
            UnOp::Neg => Value::Number(-n),
            UnOp::Percent => Value::Number(n / 100.0),
        },
    }
}

/// No computing cell, so no-arg `ROW()`/`COLUMN()` anchor to A1; [`eval_at`] supplies one.
pub fn eval(expr: &Expr, resolver: &dyn Resolver) -> Value {
    EvalCtx::new(resolver).eval(expr)
}

/// `row`/`col` are 0-based; no-arg `ROW()`/`COLUMN()` report them 1-based.
pub fn eval_at(expr: &Expr, resolver: &dyn Resolver, row: u32, col: u32) -> Value {
    EvalCtx::at_cell(resolver, row, col).eval(expr)
}

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
        BinOp::Pow => return pow(a, b),
        _ => unreachable!("arith called with a non-arithmetic operator"),
    };
    if r.is_finite() {
        Value::Number(r)
    } else {
        // Finite operands can still overflow (`1e300 * 1e300`).
        Value::Error(ErrKind::Num)
    }
}

/// Shared by the `^` operator and `POWER` so both map the error conditions identically.
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

/// Collapses ONLY a 1x1 array. Unlike [`scalarize`] a multi-cell array survives, because the
/// operator layer must still broadcast over it element-wise.
pub(crate) fn collapse_1x1(v: Value) -> Value {
    match v {
        Value::Array(shape, mut cells) if shape.rows == 1 && shape.cols == 1 => {
            cells.pop().unwrap_or(Value::Blank)
        }
        other => other,
    }
}

/// The scalar-POSITION collapse: a 1x1 array is its cell, a multi-cell array is `#VALUE!`. Public
/// because the filesystem model reuses this rule rather than re-deriving it. Operators do not route
/// through here; they collapse only a 1x1 array and broadcast instead.
pub fn scalarize(v: Value) -> Value {
    match collapse_1x1(v) {
        Value::Array(..) => Value::Error(ErrKind::Value),
        other => other,
    }
}

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
        Value::Array(..) => match scalarize(v.clone()) {
            Value::Error(k) => Err(k),
            scalar => coerce_num(&scalar),
        },
    }
}

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

/// Excel's General format, the C `%.15g` rule: up to 15 significant digits, trailing zeros trimmed,
/// scientific outside the decimal-exponent window `[-4, 15)` with an uppercase `E` and a signed
/// exponent of at least two digits; `0` and `-0` both print `0`. The ONE home for number-to-General
/// text, so `&`, `TEXT(…,"General")` and a consumer's display surface can never diverge.
pub fn num_to_text(n: f64) -> String {
    if n == 0.0 {
        return "0".to_string();
    }
    // A `Value::Number` is always finite; never panic on a synthesized non-finite.
    if !n.is_finite() {
        return format!("{n}");
    }
    let neg = n < 0.0;
    // Rust rounds and carries (`9.99…e0 -> 1.0e1`), so `exp` is the ROUNDED value's exponent.
    let sci = format!("{:.*e}", 14, n.abs());
    let (mantissa, exp_str) = match sci.split_once('e') {
        Some(parts) => parts,
        None => return format!("{n}"),
    };
    let exp: i32 = match exp_str.parse() {
        Ok(e) => e,
        Err(_) => return format!("{n}"),
    };
    let all_digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let trimmed = all_digits.trim_end_matches('0');
    let sig = if trimmed.is_empty() { "0" } else { trimmed };
    let body = if (-4..15).contains(&exp) {
        general_fixed(sig, exp)
    } else {
        general_scientific(sig, exp)
    };
    if neg { format!("-{body}") } else { body }
}

/// `("1", 20) -> "1E+20"`; `("123", -9) -> "1.23E-09"`.
fn general_scientific(sig: &str, exp: i32) -> String {
    let (lead, frac) = sig.split_at(1);
    let mantissa = if frac.is_empty() {
        lead.to_string()
    } else {
        format!("{lead}.{frac}")
    };
    let sign = if exp < 0 { '-' } else { '+' };
    format!("{mantissa}E{sign}{:02}", exp.abs())
}

/// `exp` is the leading digit's power of ten: `("314", 0) -> "3.14"`, `("1", -4) -> "0.0001"`.
fn general_fixed(sig: &str, exp: i32) -> String {
    if exp >= 0 {
        let int_len = (exp as usize) + 1;
        if sig.len() <= int_len {
            let mut s = String::with_capacity(int_len);
            s.push_str(sig);
            s.extend(std::iter::repeat_n('0', int_len - sig.len()));
            s
        } else {
            format!("{}.{}", &sig[..int_len], &sig[int_len..])
        }
    } else {
        let zeros = (-exp - 1) as usize;
        let mut s = String::from("0.");
        s.extend(std::iter::repeat_n('0', zeros));
        s.push_str(sig);
        s
    }
}

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

/// Excel `=` equality between two scalars; the leftmost error operand propagates.
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

/// The ONE ordering both the comparison operators and the approximate-match lookup family read, so
/// lookup ordering can never drift from operator ordering. Callers screen errors and arrays first.
pub(crate) fn value_cmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    compare_ord(a, b)
}

/// A lone `Blank` is resolved against the OTHER operand's type first, so `A1=0` holds for a blank.
fn compare_ord(l: &Value, r: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (l, r) = match (l, r) {
        (Value::Blank, Value::Blank) => return Ordering::Equal,
        (Value::Blank, other) => (blank_as(other), other.clone()),
        (other, Value::Blank) => (other.clone(), blank_as(other)),
        (a, b) => (a.clone(), b.clone()),
    };
    match (&l, &r) {
        // `partial_cmp`, not `total_cmp`, so `-0.0 == 0.0`; a Number is finite, so NaN cannot arise.
        (Value::Number(a), Value::Number(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        (Value::Text(a), Value::Text(b)) => a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()),
        _ => type_rank(&l).cmp(&type_rank(&r)),
    }
}

fn blank_as(other: &Value) -> Value {
    match other {
        Value::Number(_) => Value::Number(0.0),
        Value::Text(_) => Value::Text(String::new()),
        Value::Bool(_) => Value::Bool(false),
        _ => Value::Blank,
    }
}

/// Cross-type rank: Number &lt; Text &lt; Bool; anything else takes the top slot defensively.
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
        let e = Expr::Binary(
            BinOp::Add,
            Box::new(Expr::Lit(Value::Text("5".into()))),
            Box::new(num(1.0)),
        );
        assert_eq!(eval_on(&e, &g), Value::Number(6.0), "numeric text coerces");
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
        let e = Expr::Binary(BinOp::Pow, Box::new(num(0.0)), Box::new(num(-1.0)));
        assert_eq!(eval_on(&e, &g), Value::Error(ErrKind::Div0), "0 ^ -1");
        let e = Expr::Binary(BinOp::Pow, Box::new(num(-8.0)), Box::new(num(0.5)));
        assert_eq!(
            eval_on(&e, &g),
            Value::Error(ErrKind::Num),
            "(-8) ^ 0.5 is complex"
        );
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
        let range = |sc: u32, sr: u32, ec: u32, er: u32| {
            Expr::Range(RangeNode {
                start_col: sc,
                start_row: sr,
                end_col: ec,
                end_row: er,
                start_col_abs: false,
                start_row_abs: false,
                end_col_abs: false,
                end_row_abs: false,
                sheet: None,
            })
        };
        let g = Grid::new(1, vec![Value::Error(ErrKind::Div0)]);
        let e = Expr::Binary(
            BinOp::Add,
            Box::new(range(0, 0, 0, 0)),
            Box::new(Expr::Lit(Value::Error(ErrKind::Ref))),
        );
        assert_eq!(
            eval_on(&e, &g),
            Value::Error(ErrKind::Div0),
            "the left cell's error, not the right's #REF!"
        );
        let e = Expr::Binary(
            BinOp::Eq,
            Box::new(range(0, 0, 0, 0)),
            Box::new(Expr::Lit(Value::Error(ErrKind::Ref))),
        );
        assert_eq!(eval_on(&e, &g), Value::Error(ErrKind::Div0));

        let g = Grid::new(1, vec![Value::Number(1.0), Value::Number(2.0)]);
        let e = Expr::Binary(
            BinOp::Add,
            Box::new(range(0, 0, 0, 1)),
            Box::new(Expr::Lit(Value::Error(ErrKind::Ref))),
        );
        assert_eq!(
            eval_on(&e, &g),
            Value::Error(ErrKind::Ref),
            "a multi-cell left array broadcasts; the right error still wins"
        );
    }

    #[test]
    fn array_arithmetic_coerces_booleans_element_wise() {
        let g = Grid::new(
            1,
            vec![Value::Number(1.0), Value::Number(3.0), Value::Number(5.0)],
        );
        assert_eq!(
            eval(&parse("=A1:A3>2").unwrap(), &g),
            Value::Array(
                Shape { rows: 3, cols: 1 },
                vec![Value::Bool(false), Value::Bool(true), Value::Bool(true)]
            )
        );
        // `-FALSE` is `-0.0`, as the scalar unary always produced; the double `--` folds it to `+0.0`.
        assert_eq!(
            eval(&parse("=-(A1:A3>2)").unwrap(), &g),
            Value::Array(
                Shape { rows: 3, cols: 1 },
                vec![
                    Value::Number(-0.0),
                    Value::Number(-1.0),
                    Value::Number(-1.0)
                ]
            )
        );
        assert_eq!(
            eval(&parse("=SUMPRODUCT(--(A1:A3>2))").unwrap(), &g),
            Value::Number(2.0)
        );
        assert_eq!(eval(&parse("=-TRUE").unwrap(), &g), Value::Number(-1.0));
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
        let e = Expr::Unary(UnOp::Percent, Box::new(num(50.0)));
        assert_eq!(eval_on(&e, &g), Value::Number(0.5));
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
        let e = Expr::Binary(
            BinOp::Eq,
            Box::new(Expr::Lit(Value::Text("a".into()))),
            Box::new(Expr::Lit(Value::Text("A".into()))),
        );
        assert_eq!(
            eval_on(&e, &g),
            Value::Bool(true),
            "text equality folds case"
        );
        let e = Expr::Binary(
            BinOp::Lt,
            Box::new(num(999.0)),
            Box::new(Expr::Lit(Value::Text("a".into()))),
        );
        assert_eq!(
            eval_on(&e, &g),
            Value::Bool(true),
            "number ranks below text"
        );
        let e = Expr::Binary(
            BinOp::Eq,
            Box::new(Expr::Lit(Value::Blank)),
            Box::new(num(0.0)),
        );
        assert_eq!(eval_on(&e, &g), Value::Bool(true), "blank = 0");
        let neg_zero = Expr::Binary(BinOp::Mul, Box::new(num(0.0)), Box::new(num(-1.0)));
        let e = Expr::Binary(BinOp::Eq, Box::new(neg_zero), Box::new(num(0.0)));
        assert_eq!(eval_on(&e, &g), Value::Bool(true), "-0 equals +0");
    }

    #[test]
    fn ref_and_range_resolution() {
        let g = Grid::new(
            2,
            vec![
                Value::Number(1.0),
                Value::Number(2.0),
                Value::Number(3.0),
                Value::Number(4.0),
            ],
        );
        assert_eq!(eval_on(&cell(1, 1), &g), Value::Number(4.0), "B2");
        let range = Expr::Range(RangeNode {
            start_col: 0,
            start_row: 0,
            end_col: 1,
            end_row: 1,
            start_col_abs: false,
            start_row_abs: false,
            end_col_abs: false,
            end_row_abs: false,
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
        let g = Grid::new(1, vec![Value::Number(1.0)]).with_sheet(
            "Data",
            1,
            vec![
                Value::Number(10.0),
                Value::Number(20.0),
                Value::Number(30.0),
            ],
        );

        assert_eq!(eval(&parse("=Data!A1").unwrap(), &g), Value::Number(10.0));
        assert_eq!(
            eval(&parse("=A1").unwrap(), &g),
            Value::Number(1.0),
            "the unqualified address reads the default sheet"
        );
        assert_eq!(eval(&parse("=Sheet1!A1").unwrap(), &g), Value::Number(1.0));

        assert_eq!(
            eval(&parse("=SUM(Data!A1:A3)").unwrap(), &g),
            Value::Number(60.0)
        );

        assert_eq!(
            eval(&parse("=Nope!A1").unwrap(), &g),
            Value::Error(ErrKind::Ref),
            "an unknown sheet name is #REF!"
        );
        assert_eq!(
            eval(&parse("=SUM(Nope!A1:A3)").unwrap(), &g),
            Value::Error(ErrKind::Ref)
        );
    }

    #[test]
    fn reserved_nodes_defer_at_eval() {
        let g = Grid::new(1, vec![Value::Number(7.0)]);
        let ii_scalar = Expr::ImplicitIntersect(Box::new(num(7.0)));
        assert_eq!(eval_on(&ii_scalar, &g), Value::Number(7.0));
        let ii_arr = Expr::ImplicitIntersect(Box::new(Expr::Range(RangeNode {
            start_col: 0,
            start_row: 0,
            end_col: 0,
            end_row: 0,
            start_col_abs: false,
            start_row_abs: false,
            end_col_abs: false,
            end_row_abs: false,
            sheet: None,
        })));
        assert_eq!(
            eval_on(&ii_arr, &g),
            Value::Number(7.0),
            "a 1x1 range is scalar, so @ is the identity"
        );
        let spill = Expr::SpillRef(Box::new(num(7.0)));
        assert_eq!(eval_on(&spill, &g), Value::Error(ErrKind::Calc));
    }

    #[test]
    fn num_to_text_matches_excel_general_format() {
        assert_eq!(num_to_text(0.0), "0");
        assert_eq!(num_to_text(-0.0), "0");
        assert_eq!(num_to_text(1.0), "1");
        assert_eq!(num_to_text(2.0), "2");
        assert_eq!(num_to_text(5.0), "5");
        assert_eq!(num_to_text(10.0), "10");
        assert_eq!(num_to_text(2.75), "2.75");
        assert_eq!(num_to_text(-2.75), "-2.75");
        assert_eq!(num_to_text(1234.5), "1234.5");
        assert_eq!(num_to_text(0.5), "0.5");
        assert_eq!(num_to_text(0.0001), "0.0001", "exp -4 stays fixed");
        assert_eq!(num_to_text(123456789012345.0), "123456789012345");
        assert_eq!(num_to_text(1000000000000.0), "1000000000000");
        assert_eq!(num_to_text(1e20), "1E+20");
        assert_eq!(num_to_text(1e-9), "1E-09");
        assert_eq!(num_to_text(1e-7), "1E-07");
        assert_eq!(num_to_text(-1e20), "-1E+20");
        assert_eq!(
            num_to_text(1.0 / 3.0),
            "0.333333333333333",
            "15 threes, not Rust Display's 16"
        );
    }

    #[test]
    fn synthesized_deep_tree_does_not_overflow_eval() {
        let g = Grid::new(1, vec![Value::Blank]);
        let mut e = num(1.0);
        for _ in 0..2000 {
            e = Expr::Unary(UnOp::Plus, Box::new(e));
        }
        assert_eq!(eval_on(&e, &g), Value::Error(ErrKind::Num));
    }
}
