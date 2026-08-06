// Concern: the arithmetic and rounding built-ins | Non-concern: the operators (eval.rs owns them), statistics, trigonometry | IO: (&mut EvalCtx, &[Expr]) -> Value
use super::*;

pub(crate) fn abs_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match coerce_num(&scalarize(ctx.eval(&args[0]))) {
        Err(k) => Value::Error(k),
        Ok(n) => Value::Number(n.abs()),
    }
}

pub(crate) fn round_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let n = match coerce_num(&scalarize(ctx.eval(&args[0]))) {
        Ok(x) => x,
        Err(k) => return Value::Error(k),
    };
    let d = match coerce_num(&scalarize(ctx.eval(&args[1]))) {
        Ok(x) => x,
        Err(k) => return Value::Error(k),
    };
    let digits = d.trunc().clamp(-308.0, 308.0) as i32;
    let factor = 10f64.powi(digits);
    // `f64::round` is already round-half-away-from-zero, the tie rule ROUND needs.
    finite_or_num((n * factor).round() / factor)
}

/// Evaluate the first two arguments to scalar numbers, leftmost coercion error winning.
fn two_nums(ctx: &mut EvalCtx, args: &[Expr]) -> Result<(f64, f64), ErrKind> {
    let a = one_num(ctx, &args[0])?;
    let b = one_num(ctx, &args[1])?;
    Ok((a, b))
}

pub(crate) fn product(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match collect_numbers(ctx, args) {
        Err(k) => Value::Error(k),
        Ok(nums) if nums.is_empty() => Value::Number(0.0),
        Ok(nums) => finite_or_num(nums.iter().product()),
    }
}

pub(crate) fn sumproduct(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let mut base: Option<(u32, u32)> = None;
    let mut prod: Vec<f64> = Vec::new();
    for a in args {
        let (rows, cols, cells) = match block(ctx, a) {
            Ok(t) => t,
            Err(k) => return Value::Error(k),
        };
        match base {
            None => {
                base = Some((rows, cols));
                prod = vec![1.0; cells.len()];
            }
            Some(b) if b != (rows, cols) => return Value::Error(ErrKind::Value),
            Some(_) => {}
        }
        for (p, cell) in prod.iter_mut().zip(cells.iter()) {
            match cell {
                Value::Error(k) => return Value::Error(*k),
                Value::Number(n) => *p *= n,
                _ => *p = 0.0,
            }
        }
    }
    finite_or_num(prod.iter().sum())
}

/// The direction a magnitude-rounding takes to `digits` places: `Up` = away from zero (`ROUNDUP`),
/// `Down` = toward zero (`ROUNDDOWN`).
#[derive(Clone, Copy)]
enum RoundDir {
    Up,
    Down,
}

/// Shared body of `ROUNDUP`/`ROUNDDOWN`: scale by `10^digits`, round the magnitude in `dir`, unscale.
/// `digits` truncates toward zero and clamps to a sane exponent band (mirroring `ROUND`); a negative
/// `digits` rounds to the left of the decimal point.
fn round_dir(ctx: &mut EvalCtx, args: &[Expr], dir: RoundDir) -> Value {
    let (n, d) = match two_nums(ctx, args) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let digits = d.trunc().clamp(-308.0, 308.0) as i32;
    let factor = 10f64.powi(digits);
    let scaled = n * factor;
    let rounded = match dir {
        // Away from zero: ceil the magnitude, then restore the sign.
        RoundDir::Up => scaled.abs().ceil().copysign(scaled),
        // Toward zero: truncation is exactly round-toward-zero.
        RoundDir::Down => scaled.trunc(),
    };
    finite_or_num(rounded / factor)
}

pub(crate) fn roundup(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    round_dir(ctx, args, RoundDir::Up)
}

pub(crate) fn rounddown(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    round_dir(ctx, args, RoundDir::Down)
}

pub(crate) fn int_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match one_num(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok(n) => finite_or_num(n.floor()),
    }
}

pub(crate) fn mod_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (n, divisor) = match two_nums(ctx, args) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    if divisor == 0.0 {
        return Value::Error(ErrKind::Div0);
    }
    finite_or_num(n - divisor * (n / divisor).floor())
}

pub(crate) fn power_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match two_nums(ctx, args) {
        Ok((a, b)) => pow(a, b),
        Err(k) => Value::Error(k),
    }
}

pub(crate) fn sqrt_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match one_num(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok(n) if n < 0.0 => Value::Error(ErrKind::Num),
        Ok(n) => finite_or_num(n.sqrt()),
    }
}

pub(crate) fn ceiling_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (number, significance) = match two_nums(ctx, args) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    if significance == 0.0 {
        return Value::Number(0.0);
    }
    if number * significance < 0.0 {
        return Value::Error(ErrKind::Num);
    }
    finite_or_num(significance * (number / significance).ceil())
}

pub(crate) fn floor_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (number, significance) = match two_nums(ctx, args) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    if significance == 0.0 {
        return Value::Error(ErrKind::Div0);
    }
    if number * significance < 0.0 {
        return Value::Error(ErrKind::Num);
    }
    finite_or_num(significance * (number / significance).floor())
}

/// Truncate `digits` (the shared exponent handling of `ROUND`/`ROUNDUP`): trunc toward zero, clamp to
/// a sane exponent band, and return the power-of-ten factor and the integer digit count.
fn digit_factor(digits: f64) -> f64 {
    10f64.powi(digits.trunc().clamp(-308.0, 308.0) as i32)
}

pub(crate) fn trunc_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let n = match one_num(ctx, &args[0]) {
        Ok(x) => x,
        Err(k) => return Value::Error(k),
    };
    let digits = if args.len() == 2 {
        match one_num(ctx, &args[1]) {
            Ok(d) => d,
            Err(k) => return Value::Error(k),
        }
    } else {
        0.0
    };
    let factor = digit_factor(digits);
    finite_or_num((n * factor).trunc() / factor)
}

pub(crate) fn sign_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match one_num(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok(n) if n > 0.0 => Value::Number(1.0),
        Ok(n) if n < 0.0 => Value::Number(-1.0),
        Ok(_) => Value::Number(0.0),
    }
}

pub(crate) fn mround_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (number, multiple) = match two_nums(ctx, args) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    if multiple == 0.0 {
        return Value::Number(0.0);
    }
    if number * multiple < 0.0 {
        return Value::Error(ErrKind::Num);
    }
    // `f64::round` is round-half-away-from-zero, MROUND's tie rule, and the quotient shares `number`'s sign, so no `copysign` is needed.
    finite_or_num(multiple * (number / multiple).round())
}

/// Shared body of the `.MATH` rounders. `dir_up` selects `CEILING.MATH` (round toward +∞ by default)
/// vs `FLOOR.MATH` (toward −∞). Significance defaults to `1`, its sign is ignored (|significance|),
/// and a zero significance is `0`. A nonzero `mode` flips the direction for a NEGATIVE number so it
/// rounds AWAY FROM ZERO instead of toward it.
fn round_math(ctx: &mut EvalCtx, args: &[Expr], dir_up: bool) -> Value {
    let number = match one_num(ctx, &args[0]) {
        Ok(x) => x,
        Err(k) => return Value::Error(k),
    };
    let significance = if args.len() >= 2 {
        match one_num(ctx, &args[1]) {
            Ok(s) => s,
            Err(k) => return Value::Error(k),
        }
    } else {
        1.0
    };
    let mode = if args.len() == 3 {
        match one_num(ctx, &args[2]) {
            Ok(m) => m,
            Err(k) => return Value::Error(k),
        }
    } else {
        0.0
    };
    if significance == 0.0 {
        return Value::Number(0.0);
    }
    let sig = significance.abs();
    let ratio = number / sig;
    // A nonzero `mode` reverses ONLY the negative side, turning "toward zero" into "away from zero".
    let flip = mode != 0.0 && number < 0.0;
    let ceil = if dir_up { !flip } else { flip };
    let rounded = if ceil { ratio.ceil() } else { ratio.floor() };
    finite_or_num(sig * rounded)
}

pub(crate) fn ceiling_math_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    round_math(ctx, args, true)
}

pub(crate) fn floor_math_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    round_math(ctx, args, false)
}

pub(crate) fn even_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match one_num(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        // At n = 0 this is `⌈0⌉·2 = 0`, so `EVEN(0) = 0` falls out with no special case.
        Ok(n) => finite_or_num(((n.abs() / 2.0).ceil() * 2.0).copysign(n)),
    }
}

pub(crate) fn odd_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match one_num(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        // `2*ceil((|n|-1)/2) + 1` then restore the sign; at n = 0 this already yields 1, so there is no zero special case.
        Ok(n) => {
            let a = n.abs();
            let odd = 2.0 * ((a - 1.0) / 2.0).ceil() + 1.0;
            finite_or_num(odd.copysign(n))
        }
    }
}

pub(crate) fn sumsq(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match collect_numbers(ctx, args) {
        Err(k) => Value::Error(k),
        Ok(nums) => finite_or_num(nums.iter().map(|x| x * x).sum()),
    }
}

pub(crate) fn quotient_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (numerator, denominator) = match two_nums(ctx, args) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    if denominator == 0.0 {
        return Value::Error(ErrKind::Div0);
    }
    finite_or_num((numerator / denominator).trunc())
}
