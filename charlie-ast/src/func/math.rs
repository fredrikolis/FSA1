// Concern: the MATH worksheet functions (ABS ROUND PRODUCT SUMPRODUCT ROUNDUP ROUNDDOWN INT MOD POWER SQRT CEILING FLOOR) — the pure scalar/vector arithmetic built-ins, reusing the operator-level `coerce_num`/`pow` so a function form and its operator agree, and pinning the Excel domain calls (MOD's sign follows the divisor, INT floors toward −∞, SQRT of a negative is `#NUM!`, legacy CEILING/FLOOR sign rules) | Non-concern: the registry table + dispatch (func/mod.rs), the coercion machinery (eval.rs owns `coerce_num`/`pow`/`scalarize`), and the shared `one_num`/`block`/`collect_numbers`/`finite_or_num` helpers (func/helpers.rs) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
use super::*;

/// `ABS(x)` — magnitude. Coerces its scalar argument; propagates an error.
pub(crate) fn abs_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match coerce_num(&scalarize(ctx.eval(&args[0]))) {
        Err(k) => Value::Error(k),
        Ok(n) => Value::Number(n.abs()),
    }
}

/// `ROUND(x, digits)` — round to `digits` decimal places, ties away from zero (Excel). Negative
/// `digits` rounds to the left of the decimal point.
pub(crate) fn round_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let n = match coerce_num(&scalarize(ctx.eval(&args[0]))) {
        Ok(x) => x,
        Err(k) => return Value::Error(k),
    };
    let d = match coerce_num(&scalarize(ctx.eval(&args[1]))) {
        Ok(x) => x,
        Err(k) => return Value::Error(k),
    };
    // Excel truncates the digit count toward zero and clamps the exponent to a sane band.
    let digits = d.trunc().clamp(-308.0, 308.0) as i32;
    let factor = 10f64.powi(digits);
    // `f64::round` is already round-half-away-from-zero, matching Excel's ROUND tie rule.
    finite_or_num((n * factor).round() / factor)
}

// Pure scalar / vector math (the v1 math batch). These reuse eval.rs's scalar `coerce_num`/
// `scalarize` (so a boolean/numeric-text argument coerces exactly as it does for an operator) and
// `pow` (so `POWER` and the `^` operator share one error mapping). Excel-semantics calls worth a
// reviewer's eye are flagged at each site: MOD's sign follows the DIVISOR; INT floors toward −∞;
// SQRT of a negative is `#NUM!`; and legacy CEILING/FLOOR reject different-signed args with `#NUM!`
// while treating a zero significance ASYMMETRICALLY (CEILING → 0, FLOOR → `#DIV/0!`).
/// Evaluate the first two arguments to scalar numbers, leftmost coercion error winning.
fn two_nums(ctx: &mut EvalCtx, args: &[Expr]) -> Result<(f64, f64), ErrKind> {
    let a = one_num(ctx, &args[0])?;
    let b = one_num(ctx, &args[1])?;
    Ok((a, b))
}

/// `PRODUCT(a, b, …)` — multiply the numbers. Mirrors `SUM`'s coercion asymmetry via
/// [`collect_numbers`]: a direct boolean/numeric-text coerces, an in-range non-number is ignored, any
/// error propagates. With NO numeric datum the product is `0` (Excel's empty-product result, not the
/// `1` identity).
pub(crate) fn product(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match collect_numbers(ctx, args) {
        Err(k) => Value::Error(k),
        Ok(nums) if nums.is_empty() => Value::Number(0.0),
        Ok(nums) => finite_or_num(nums.iter().product()),
    }
}

/// `SUMPRODUCT(array1, [array2], …)` — multiply the arrays element-for-element, then sum. Every
/// argument must share ONE shape (rows × cols); a mismatch is a static `#VALUE!` (the same
/// static-conformance stance as the `*IFS` family and format.md §6). A non-numeric cell (text /
/// blank / boolean) contributes `0` — Excel's rule, so an unfiltered boolean is `0`, not `1`; an
/// error at ANY position propagates (leftmost array, leftmost cell).
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

/// `ROUNDUP(x, digits)` — round the magnitude UP (away from zero) to `digits` places.
pub(crate) fn roundup(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    round_dir(ctx, args, RoundDir::Up)
}

/// `ROUNDDOWN(x, digits)` — round the magnitude DOWN (toward zero) to `digits` places.
pub(crate) fn rounddown(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    round_dir(ctx, args, RoundDir::Down)
}

/// `INT(x)` — round DOWN to the nearest integer, flooring toward −∞ (so `INT(-2.5) = -3`, NOT the
/// toward-zero `-2`). This is the load-bearing distinction from truncation.
pub(crate) fn int_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match one_num(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok(n) => finite_or_num(n.floor()),
    }
}

/// `MOD(n, divisor)` — the remainder, whose SIGN FOLLOWS THE DIVISOR (Excel), computed as
/// `n − divisor·⌊n/divisor⌋`. So `MOD(-3, 2) = 1` and `MOD(3, -2) = -1`. A zero divisor is `#DIV/0!`.
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

/// `POWER(x, y)` — `x` raised to `y`, sharing `eval::pow` with the `^` operator (so `0^-1` is
/// `#DIV/0!` and a complex/overflowing power is `#NUM!`, identically to the operator).
pub(crate) fn power_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match two_nums(ctx, args) {
        Ok((a, b)) => pow(a, b),
        Err(k) => Value::Error(k),
    }
}

/// `SQRT(x)` — the non-negative square root; a NEGATIVE argument is `#NUM!` (Excel raises, rather
/// than returning a complex or `NaN`).
pub(crate) fn sqrt_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match one_num(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok(n) if n < 0.0 => Value::Error(ErrKind::Num),
        Ok(n) => finite_or_num(n.sqrt()),
    }
}

/// `CEILING(number, significance)` — round `number` AWAY FROM ZERO to the nearest multiple of
/// `significance` (legacy Excel). If the two arguments have DIFFERENT signs it is `#NUM!`; a zero
/// significance returns `0` (the asymmetric counterpart to `FLOOR`'s `#DIV/0!`).
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

/// `FLOOR(number, significance)` — round `number` TOWARD ZERO to the nearest multiple of
/// `significance` (legacy Excel). Different-signed arguments are `#NUM!`; a zero significance is
/// `#DIV/0!` (the asymmetric counterpart to `CEILING`'s `0`).
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
