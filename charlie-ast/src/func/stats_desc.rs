// Concern: the DESCRIPTIVE-STATISTIC worksheet functions beyond the extremes/dispersion core — the "A"-suffix aggregators that COUNT in-range text/logicals (AVERAGEA MAXA MINA), the alternative means (GEOMEAN HARMEAN), the mean-absolute deviation AVEDEV, and the shape moments SKEW/KURT — each Excel-exact in its coercion rule, empty/degenerate error value, and moment formula | Non-concern: the extremes/order/counting/dispersion core (func/stats.rs), the ranking/positional and regression/distribution extensions (func/stats_rank.rs, func/stats_reg.rs, func/stats_dist.rs), the registry table + dispatch (func/mod.rs), and the shared `collect_numbers`/`finite_or_num` helpers (func/helpers.rs) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
use super::*;

// The "A"-suffix aggregators (AVERAGEA/MAXA/MINA) differ from their plain siblings ONLY in the
// in-range branch: an in-range boolean COUNTS (TRUE→1, FALSE→0) and an in-range text COUNTS AS 0
// (Excel: "text evaluates as 0"), where the plain forms ignore both. The direct-argument branch is
// identical to [`collect_numbers`] (a direct boolean/numeric-text coerces, a direct non-numeric text
// is `#VALUE!`), so the direct-vs-in-range asymmetry is preserved — only the in-range treatment of
// non-numbers changes. A `Blank` is ignored in both branches; an error propagates (leftmost).
/// Gather numeric data under the "A" coercion rule: in-range booleans and text COUNT (a bool → 1/0,
/// any text → 0), unlike the plain aggregators that ignore them; direct arguments coerce exactly as
/// [`collect_numbers`] does. A `Blank` is ignored; an error propagates.
fn collect_numbers_a(ctx: &mut EvalCtx, args: &[Expr]) -> Result<Vec<f64>, ErrKind> {
    let mut nums = Vec::new();
    for a in args {
        match ctx.eval(a) {
            Value::Array(_, cells) => {
                for c in &cells {
                    match c {
                        Value::Error(k) => return Err(*k),
                        Value::Number(n) => nums.push(*n),
                        Value::Bool(b) => nums.push(if *b { 1.0 } else { 0.0 }),
                        Value::Text(_) => nums.push(0.0),
                        // A Blank (and the unreachable nested array) is ignored.
                        _ => {}
                    }
                }
            }
            Value::Error(k) => return Err(k),
            Value::Number(n) => nums.push(n),
            Value::Bool(b) => nums.push(if b { 1.0 } else { 0.0 }),
            Value::Blank => {}
            Value::Text(t) => match t.trim().parse::<f64>() {
                Ok(n) if n.is_finite() => nums.push(n),
                _ => return Err(ErrKind::Value),
            },
        }
    }
    Ok(nums)
}

/// `AVERAGEA(a, b, …)` — arithmetic mean of the "A"-gathered data (in-range text/logicals count, text
/// as 0). Empty (no counted datum) is `#DIV/0!` (Excel), matching AVERAGE.
pub(crate) fn averagea_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match collect_numbers_a(ctx, args) {
        Err(k) => Value::Error(k),
        Ok(nums) if nums.is_empty() => Value::Error(ErrKind::Div0),
        Ok(nums) => finite_or_num(nums.iter().sum::<f64>() / nums.len() as f64),
    }
}

/// `MAXA(a, b, …)` — the largest of the "A"-gathered data (an in-range `FALSE`/text counts as 0, so it
/// can win over negatives); no counted datum is `0` (Excel), matching MAX.
pub(crate) fn maxa_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    extreme_a(ctx, args, f64::max)
}

/// `MINA(a, b, …)` — the smallest of the "A"-gathered data (an in-range `FALSE`/text counts as 0, so
/// it can win over positives); no counted datum is `0` (Excel), matching MIN.
pub(crate) fn mina_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    extreme_a(ctx, args, f64::min)
}

/// Shared body of `MAXA`/`MINA`: gather under the "A" rule, reduce with `pick`; an empty set is `0`.
fn extreme_a(ctx: &mut EvalCtx, args: &[Expr], pick: fn(f64, f64) -> f64) -> Value {
    match collect_numbers_a(ctx, args) {
        Err(k) => Value::Error(k),
        Ok(nums) => match nums.into_iter().reduce(pick) {
            None => Value::Number(0.0),
            Some(x) => finite_or_num(x),
        },
    }
}

/// `GEOMEAN(a, b, …)` — the geometric mean `(∏xᵢ)^(1/n)`, computed as `exp(mean(ln xᵢ))` for numerical
/// stability. Gathers under the plain SUM asymmetry (in-range non-numbers ignored). Every datum must
/// be strictly positive; a value `≤ 0`, or no numeric datum, is `#NUM!` (Excel).
pub(crate) fn geomean_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let nums = match collect_numbers(ctx, args) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    if nums.is_empty() || nums.iter().any(|&x| x <= 0.0) {
        return Value::Error(ErrKind::Num);
    }
    let sum_ln: f64 = nums.iter().map(|x| x.ln()).sum();
    finite_or_num((sum_ln / nums.len() as f64).exp())
}

/// `HARMEAN(a, b, …)` — the harmonic mean `n / Σ(1/xᵢ)`. Gathers under the plain SUM asymmetry. Every
/// datum must be strictly positive; a value `≤ 0`, or no numeric datum, is `#NUM!` (Excel).
pub(crate) fn harmean_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let nums = match collect_numbers(ctx, args) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    if nums.is_empty() || nums.iter().any(|&x| x <= 0.0) {
        return Value::Error(ErrKind::Num);
    }
    let sum_recip: f64 = nums.iter().map(|x| 1.0 / x).sum();
    finite_or_num(nums.len() as f64 / sum_recip)
}

/// `AVEDEV(a, b, …)` — the average of the absolute deviations from the mean, `Σ|xᵢ − mean| / n`.
/// Gathers under the plain SUM asymmetry; no numeric datum is `#NUM!` (Excel).
pub(crate) fn avedev_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let nums = match collect_numbers(ctx, args) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    if nums.is_empty() {
        return Value::Error(ErrKind::Num);
    }
    let mean = nums.iter().sum::<f64>() / nums.len() as f64;
    let ad: f64 = nums.iter().map(|x| (x - mean).abs()).sum();
    finite_or_num(ad / nums.len() as f64)
}

/// The SAMPLE standard deviation of `nums` (divisor `n-1`) as `Some`, or `None` when `n < 2` — the
/// shared prerequisite of the standardized shape moments SKEW/KURT (each divides by it).
fn sample_stdev(nums: &[f64], mean: f64) -> Option<f64> {
    let n = nums.len();
    if n < 2 {
        return None;
    }
    let ss: f64 = nums.iter().map(|x| (x - mean) * (x - mean)).sum();
    Some((ss / (n - 1) as f64).sqrt())
}

/// `SKEW(a, b, …)` — the SAMPLE skewness `n/((n-1)(n-2)) · Σ((xᵢ−mean)/s)³`, where `s` is the sample
/// standard deviation. Requires `n ≥ 3` and `s > 0`; fewer than 3 numbers, or a zero spread, is
/// `#DIV/0!` (Excel). Gathers under the plain SUM asymmetry.
pub(crate) fn skew_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let nums = match collect_numbers(ctx, args) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let n = nums.len();
    if n < 3 {
        return Value::Error(ErrKind::Div0);
    }
    let mean = nums.iter().sum::<f64>() / n as f64;
    let s = match sample_stdev(&nums, mean) {
        Some(s) if s > 0.0 => s,
        _ => return Value::Error(ErrKind::Div0),
    };
    let m3: f64 = nums.iter().map(|x| ((x - mean) / s).powi(3)).sum();
    let nf = n as f64;
    finite_or_num(nf / ((nf - 1.0) * (nf - 2.0)) * m3)
}

/// `KURT(a, b, …)` — the SAMPLE excess kurtosis
/// `{n(n+1)/((n-1)(n-2)(n-3)) · Σ((xᵢ−mean)/s)⁴} − 3(n-1)²/((n-2)(n-3))`, where `s` is the sample
/// standard deviation. Requires `n ≥ 4` and `s > 0`; fewer than 4 numbers, or a zero spread, is
/// `#DIV/0!` (Excel). Gathers under the plain SUM asymmetry.
pub(crate) fn kurt_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let nums = match collect_numbers(ctx, args) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let n = nums.len();
    if n < 4 {
        return Value::Error(ErrKind::Div0);
    }
    let mean = nums.iter().sum::<f64>() / n as f64;
    let s = match sample_stdev(&nums, mean) {
        Some(s) if s > 0.0 => s,
        _ => return Value::Error(ErrKind::Div0),
    };
    let m4: f64 = nums.iter().map(|x| ((x - mean) / s).powi(4)).sum();
    let nf = n as f64;
    let lead = nf * (nf + 1.0) / ((nf - 1.0) * (nf - 2.0) * (nf - 3.0));
    let corr = 3.0 * (nf - 1.0) * (nf - 1.0) / ((nf - 2.0) * (nf - 3.0));
    finite_or_num(lead * m4 - corr)
}
