// Concern: the information predicates that INSPECT an operand's kind | Non-concern: coercing an operand, the logical family | IO: (&mut EvalCtx, &[Expr]) -> Value
use super::*;

pub(crate) fn isblank(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    Value::Bool(matches!(collapse_1x1(ctx.eval(&args[0])), Value::Blank))
}

pub(crate) fn isnumber(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    Value::Bool(matches!(collapse_1x1(ctx.eval(&args[0])), Value::Number(_)))
}

pub(crate) fn istext(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    Value::Bool(matches!(collapse_1x1(ctx.eval(&args[0])), Value::Text(_)))
}

pub(crate) fn isnontext(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    Value::Bool(!matches!(collapse_1x1(ctx.eval(&args[0])), Value::Text(_)))
}

pub(crate) fn islogical(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    Value::Bool(matches!(collapse_1x1(ctx.eval(&args[0])), Value::Bool(_)))
}

pub(crate) fn iserror(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    Value::Bool(matches!(collapse_1x1(ctx.eval(&args[0])), Value::Error(_)))
}

pub(crate) fn iserr(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    Value::Bool(matches!(collapse_1x1(ctx.eval(&args[0])), Value::Error(k) if k != ErrKind::Na))
}

pub(crate) fn isna(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    Value::Bool(matches!(
        collapse_1x1(ctx.eval(&args[0])),
        Value::Error(ErrKind::Na)
    ))
}

pub(crate) fn na_fn(_ctx: &mut EvalCtx, _args: &[Expr]) -> Value {
    Value::Error(ErrKind::Na)
}

pub(crate) fn type_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let code = match collapse_1x1(ctx.eval(&args[0])) {
        Value::Number(_) | Value::Blank => 1.0,
        Value::Text(_) => 2.0,
        Value::Bool(_) => 4.0,
        Value::Error(_) => 16.0,
        Value::Array(..) => 64.0,
    };
    Value::Number(code)
}

pub(crate) fn error_type_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match collapse_1x1(ctx.eval(&args[0])) {
        Value::Error(k) => Value::Number(match k {
            ErrKind::Null => 1.0,
            ErrKind::Div0 => 2.0,
            ErrKind::Value => 3.0,
            ErrKind::Ref => 4.0,
            ErrKind::Name => 5.0,
            ErrKind::Num => 6.0,
            ErrKind::Na => 7.0,
            ErrKind::Spill => 9.0,
            ErrKind::Calc => 14.0,
        }),
        _ => Value::Error(ErrKind::Na),
    }
}

pub(crate) fn n_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match scalarize(ctx.eval(&args[0])) {
        Value::Number(n) => Value::Number(n),
        Value::Bool(b) => Value::Number(if b { 1.0 } else { 0.0 }),
        Value::Error(k) => Value::Error(k),
        Value::Text(_) | Value::Blank => Value::Number(0.0),
        Value::Array(..) => Value::Error(ErrKind::Value),
    }
}

/// Deliberately NOT [`coerce_num`]: a BOOLEAN must be `#VALUE!` here, not 1/0. The result is
/// integer-valued, so the callers test parity with an exact `% 2.0` and never cast.
fn parity_number(v: Value) -> Result<f64, ErrKind> {
    match scalarize(v) {
        Value::Number(n) => Ok(n.trunc()),
        Value::Blank => Ok(0.0),
        Value::Text(t) => match t.trim().parse::<f64>() {
            Ok(n) if n.is_finite() => Ok(n.trunc()),
            _ => Err(ErrKind::Value),
        },
        Value::Bool(_) => Err(ErrKind::Value),
        Value::Error(k) => Err(k),
        Value::Array(..) => Err(ErrKind::Value),
    }
}

pub(crate) fn iseven_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match parity_number(ctx.eval(&args[0])) {
        Ok(n) => Value::Bool(n % 2.0 == 0.0),
        Err(k) => Value::Error(k),
    }
}

pub(crate) fn isodd_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match parity_number(ctx.eval(&args[0])) {
        Ok(n) => Value::Bool(n % 2.0 != 0.0),
        Err(k) => Value::Error(k),
    }
}

pub(crate) fn isformula_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match &args[0] {
        Expr::Ref(r) => match ctx.ref_is_formula(r) {
            Some(b) => Value::Bool(b),
            None => Value::Error(ErrKind::Ref),
        },
        Expr::Range(rn) => match ctx.range_is_formula(rn) {
            Some(b) => Value::Bool(b),
            None => Value::Error(ErrKind::Ref),
        },
        _ => Value::Error(ErrKind::Value),
    }
}
