// Concern: the TRANSCENDENTAL worksheet functions (PI EXP LN LOG LOG10 SIN COS TAN ASIN ACOS ATAN ATAN2 SINH COSH TANH RADIANS DEGREES) — the elementary exponential/logarithmic/trigonometric/hyperbolic built-ins and the degree↔radian conversions, each coercing its scalar argument through `one_num` and mapping the Excel domain errors (a non-positive LN/LOG/LOG10 is `#NUM!`, an out-of-[-1,1] ASIN/ACOS is `#NUM!`, ATAN2 at the origin is `#DIV/0!`, an overflowing EXP/SINH/COSH is `#NUM!`) to first-class error values, never a NaN/inf leak or a panic | Non-concern: the arithmetic/rounding functions (func/math.rs), the integer/combinatorial functions (func/combinatorics.rs), the registry table + dispatch (func/mod.rs), the coercion machinery (eval.rs), and the shared `one_num`/`finite_or_num` helpers (func/helpers.rs) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
use super::*;

// The elementary functions all coerce their scalar argument via `one_num` (so a boolean/numeric-text
// coerces exactly as an operator would) and wrap the result in `finite_or_num`, which is the single
// domain guard: any computation that produces a non-finite result — `LN` of a non-positive number,
// `ASIN` outside `[-1, 1]` (a NaN), an overflowing `EXP`/`COSH`/`SINH` (an ∞) — folds to `#NUM!`,
// matching Excel's raise-rather-than-return-NaN stance. The two special cases beyond that guard are
// spelled out at their sites: `LOG`'s base-`1` `#DIV/0!` and `ATAN2`'s origin `#DIV/0!`.

/// `PI()` — the constant π to `f64` precision (Excel's 15-significant-digit `3.14159265358979`).
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

/// `EXP(x)` — e raised to `x`. An overflowing magnitude is `#NUM!` (via `finite_or_num`).
pub(crate) fn exp_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::exp)
}

/// `LN(x)` — the natural logarithm. A non-positive `x` is `#NUM!` (`LN(0)` is `−∞`, `LN(<0)` is a
/// NaN — both fold to `#NUM!` through `finite_or_num`).
pub(crate) fn ln_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::ln)
}

/// `LOG10(x)` — the base-10 logarithm; a non-positive `x` is `#NUM!`.
pub(crate) fn log10_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::log10)
}

/// `LOG(number, [base])` — the logarithm of `number` to `base` (default `10`). A non-positive
/// `number` or `base` is `#NUM!`; a `base` of exactly `1` is `#DIV/0!` (division by `ln 1 = 0`).
/// The two common bases route through `f64::log10`/`log2` so `LOG(100)` and `LOG(8, 2)` land on the
/// exact integers Excel shows, rather than accumulating `ln`-ratio round-off.
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

/// `SIN(x)` — the sine of `x` radians.
pub(crate) fn sin_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::sin)
}

/// `COS(x)` — the cosine of `x` radians.
pub(crate) fn cos_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::cos)
}

/// `TAN(x)` — the tangent of `x` radians.
pub(crate) fn tan_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::tan)
}

/// `ASIN(x)` — the arcsine (in radians); `x` outside `[-1, 1]` is `#NUM!` (a NaN folded by
/// `finite_or_num`).
pub(crate) fn asin_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::asin)
}

/// `ACOS(x)` — the arccosine (in radians); `x` outside `[-1, 1]` is `#NUM!`.
pub(crate) fn acos_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::acos)
}

/// `ATAN(x)` — the arctangent (in radians), defined for every real `x`.
pub(crate) fn atan_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::atan)
}

/// `ATAN2(x_num, y_num)` — the arctangent of `y_num / x_num` in radians, using the signs of both to
/// place the angle in `(-π, π]`. NOTE the Excel argument order is `(x, y)` — the reverse of the usual
/// `atan2(y, x)`. The origin `(0, 0)` is `#DIV/0!` (Excel), where the angle is undefined.
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

/// `SINH(x)` — the hyperbolic sine; an overflowing magnitude is `#NUM!`.
pub(crate) fn sinh_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::sinh)
}

/// `COSH(x)` — the hyperbolic cosine; an overflowing magnitude is `#NUM!`.
pub(crate) fn cosh_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::cosh)
}

/// `TANH(x)` — the hyperbolic tangent (always finite, in `(-1, 1)`).
pub(crate) fn tanh_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::tanh)
}

/// `RADIANS(degrees)` — convert degrees to radians (`degrees · π / 180`).
pub(crate) fn radians_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::to_radians)
}

/// `DEGREES(radians)` — convert radians to degrees (`radians · 180 / π`).
pub(crate) fn degrees_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    unary_math(ctx, args, f64::to_degrees)
}
