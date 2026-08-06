// Concern: the exponential, logarithmic and trigonometric built-ins | Non-concern: arithmetic rounding, statistics | IO: (&mut EvalCtx, &[Expr]) -> Value
use super::*;

pub(crate) fn pi_fn(_ctx: &mut EvalCtx, _args: &[Expr]) -> Value {
    Value::Number(std::f64::consts::PI)
}

/// Evaluate the sole scalar argument, then apply `f`, folding a non-finite result to `#NUM!`.
fn unary_math(ctx: &mut EvalCtx, args: &[Expr], f: fn(f64) -> f64) -> Value {
    match one_num(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok(n) => finite_or_num(f(n)),
    }
}

pub(crate) fn exp_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::exp)
}

pub(crate) fn ln_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::ln)
}

pub(crate) fn log10_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::log10)
}

pub(crate) fn log_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let number = match one_num(ctx, &args[0]) {
        Ok(x) => x,
        Err(k) => return Value::Error(k),
    };
    let base = if args.len() == 2 {
        match one_num(ctx, &args[1]) {
            Ok(b) => b,
            Err(k) => return Value::Error(k),
        }
    } else {
        10.0
    };
    if number <= 0.0 || base <= 0.0 {
        return Value::Error(ErrKind::Num);
    }
    if base == 1.0 {
        return Value::Error(ErrKind::Div0);
    }
    let r = if base == 10.0 {
        number.log10()
    } else if base == 2.0 {
        number.log2()
    } else {
        number.ln() / base.ln()
    };
    finite_or_num(r)
}

pub(crate) fn sin_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::sin)
}

pub(crate) fn cos_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::cos)
}

pub(crate) fn tan_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::tan)
}

pub(crate) fn asin_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::asin)
}

pub(crate) fn acos_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::acos)
}

pub(crate) fn atan_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::atan)
}

pub(crate) fn atan2_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let x = match one_num(ctx, &args[0]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let y = match one_num(ctx, &args[1]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    if x == 0.0 && y == 0.0 {
        return Value::Error(ErrKind::Div0);
    }
    finite_or_num(y.atan2(x))
}

pub(crate) fn sinh_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::sinh)
}

pub(crate) fn cosh_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::cosh)
}

pub(crate) fn tanh_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::tanh)
}

pub(crate) fn radians_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::to_radians)
}

pub(crate) fn degrees_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::to_degrees)
}
