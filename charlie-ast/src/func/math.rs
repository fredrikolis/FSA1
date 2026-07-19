// Concern: the MATH worksheet functions — arithmetic/rounding/sign built-ins (ABS ROUND PRODUCT SUMPRODUCT ROUNDUP ROUNDDOWN INT MOD POWER SQRT CEILING FLOOR TRUNC SIGN MROUND CEILING.MATH FLOOR.MATH EVEN ODD SUMSQ QUOTIENT) — reusing the operator-level `coerce_num`/`pow` so a function form and its operator agree, and pinning the Excel domain calls (MOD's sign follows the divisor, INT floors toward −∞, SQRT of a negative is `#NUM!`, legacy CEILING/FLOOR sign rules, TRUNC truncates toward zero, MROUND ties away from zero and rejects opposite signs, the .MATH rounders take a mode flag and treat zero significance as `0`, EVEN/ODD round away from zero, QUOTIENT truncates the quotient toward zero) | Non-concern: the transcendental functions (func/trig.rs), the integer/combinatorial functions (func/combinatorics.rs), the volatile random functions (func/random.rs), the meta-aggregators SUBTOTAL/AGGREGATE (func/subtotal.rs), the registry table + dispatch (func/mod.rs), the coercion machinery (eval.rs owns `coerce_num`/`pow`/`scalarize`), and the shared `one_num`/`block`/`collect_numbers`/`finite_or_num` helpers (func/helpers.rs) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
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
/// static-conformance stance as the `*IFS` family). A non-numeric cell (text /
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

// --- Rounding / sign / integer extensions (the Math/Trig parity batch). TRUNC/SIGN/EVEN/ODD are the
//     rounding-and-sign scalars; MROUND rounds to a multiple (ties away from zero, opposite signs are
//     `#NUM!`); CEILING.MATH/FLOOR.MATH are the modern rounders (a `mode` flag flips the direction for
//     negatives, |significance| is used, and a zero significance is `0` — no legacy `#DIV/0!`
//     asymmetry); SUMSQ sums squares under SUM's gathering asymmetry; QUOTIENT is the truncated
//     integer quotient (a zero divisor is `#DIV/0!`).

/// Truncate `digits` (the shared exponent handling of `ROUND`/`ROUNDUP`): trunc toward zero, clamp to
/// a sane exponent band, and return the power-of-ten factor and the integer digit count.
fn digit_factor(digits: f64) -> f64 {
    10f64.powi(digits.trunc().clamp(-308.0, 308.0) as i32)
}

/// `TRUNC(number, [num_digits])` — truncate `number` toward zero to `num_digits` decimal places
/// (default `0`). Unlike `INT` (which floors toward −∞), `TRUNC(-8.9)` is `-8`. A negative
/// `num_digits` truncates to the left of the decimal point (`TRUNC(123.45, -1) = 120`).
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

/// `SIGN(number)` — the sign of `number`: `1` if positive, `-1` if negative, `0` if zero.
pub(crate) fn sign_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match one_num(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok(n) if n > 0.0 => Value::Number(1.0),
        Ok(n) if n < 0.0 => Value::Number(-1.0),
        Ok(_) => Value::Number(0.0),
    }
}

/// `MROUND(number, multiple)` — round `number` to the nearest multiple of `multiple`, ties AWAY FROM
/// ZERO (Excel). `number` and `multiple` must share a sign (opposite signs are `#NUM!`); a zero
/// `multiple` is `0`.
///
/// Accept-under-uncertainty (a documented divergence outside the parity corpus): charlie rounds the
/// raw IEEE quotient, whereas Excel first snaps the intermediate to 15 significant digits, so a
/// binary-FP-edge input can differ by one step — e.g. `MROUND(6.05, 0.1)` computes `6.05/0.1 =
/// 60.4999…` → `6.0`, where Excel's guard-digit rounding yields `6.1`. This 15-digit guard rule is
/// systemic to the whole ROUND family (ROUND/ROUNDUP/ROUNDDOWN/TRUNC), not to MROUND alone, so it is
/// noted here rather than patched piecemeal in one member.
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
    // `f64::round` is round-half-away-from-zero — MROUND's tie rule — and the quotient shares
    // `number`'s sign (the opposite-sign case is already rejected), so no `copysign` is needed.
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
    // Default direction: CEILING.MATH ceils toward +∞, FLOOR.MATH floors toward −∞. A nonzero `mode`
    // reverses ONLY the negative side, turning "toward zero" into "away from zero".
    let flip = mode != 0.0 && number < 0.0;
    let ceil = if dir_up { !flip } else { flip };
    let rounded = if ceil { ratio.ceil() } else { ratio.floor() };
    finite_or_num(sig * rounded)
}

/// `CEILING.MATH(number, [significance], [mode])` — round `number` toward +∞ to a multiple of
/// `significance` (default `1`, sign ignored). A nonzero `mode` rounds a NEGATIVE number away from
/// zero instead; a zero significance is `0`.
pub(crate) fn ceiling_math_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    round_math(ctx, args, true)
}

/// `FLOOR.MATH(number, [significance], [mode])` — round `number` toward −∞ to a multiple of
/// `significance` (default `1`, sign ignored). A nonzero `mode` rounds a NEGATIVE number toward zero
/// instead; a zero significance is `0`.
pub(crate) fn floor_math_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    round_math(ctx, args, false)
}

/// `EVEN(number)` — round `number` AWAY FROM ZERO to the nearest even integer (`EVEN(1.5) = 2`,
/// `EVEN(-1) = -2`, `EVEN(0) = 0`).
pub(crate) fn even_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match one_num(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        // At n = 0 this is `⌈0⌉·2 = 0`, so `EVEN(0) = 0` falls out with no special case.
        Ok(n) => finite_or_num(((n.abs() / 2.0).ceil() * 2.0).copysign(n)),
    }
}

/// `ODD(number)` — round `number` AWAY FROM ZERO to the nearest odd integer (`ODD(1.5) = 3`,
/// `ODD(2) = 3`, `ODD(0) = 1`, `ODD(-2) = -3`).
pub(crate) fn odd_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match one_num(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        // Round |n| up to the next odd integer: 2·⌈(|n|−1)/2⌉ + 1, then restore the sign. At n = 0
        // this yields 1 (Excel's `ODD(0) = 1`), so no zero special-case is needed.
        Ok(n) => {
            let a = n.abs();
            let odd = 2.0 * ((a - 1.0) / 2.0).ceil() + 1.0;
            finite_or_num(odd.copysign(n))
        }
    }
}

/// `SUMSQ(a, b, …)` — the sum of the squares of the numeric data. Shares SUM's direct-vs-in-range
/// gathering ([`collect_numbers`]): a direct boolean/numeric-text coerces, an in-range non-number is
/// ignored, any error propagates. No numeric datum sums to `0`.
pub(crate) fn sumsq(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match collect_numbers(ctx, args) {
        Err(k) => Value::Error(k),
        Ok(nums) => finite_or_num(nums.iter().map(|x| x * x).sum()),
    }
}

/// `QUOTIENT(numerator, denominator)` — the integer portion of the division, truncated TOWARD ZERO
/// (`QUOTIENT(-5, 2) = -2`). A zero denominator is `#DIV/0!`.
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
