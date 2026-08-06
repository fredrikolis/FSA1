// Concern: the order-statistic and counting built-ins | Non-concern: the criteria forms, the descriptive and distribution families | IO: (&mut EvalCtx, &[Expr]) -> Value
use super::*;

pub(crate) fn min_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    extreme(ctx, args, f64::min)
}

pub(crate) fn max_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    extreme(ctx, args, f64::max)
}

/// Shared body of `MIN`/`MAX`: gather numbers, reduce with `pick`; an empty set is `0` (the Excel
/// empty result, distinct from MEDIAN's `#NUM!`).
fn extreme(ctx: &mut EvalCtx, args: &[Expr], pick: fn(f64, f64) -> f64) -> Value {
    match collect_numbers(ctx, args) {
        Err(k) => Value::Error(k),
        Ok(nums) => match nums.into_iter().reduce(pick) {
            None => Value::Number(0.0),
            Some(x) => finite_or_num(x),
        },
    }
}

pub(crate) fn median_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let mut nums = match collect_numbers(ctx, args) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    if nums.is_empty() {
        return Value::Error(ErrKind::Num);
    }
    // `total_cmp` needs no unwrap; NaN cannot enter, because a non-finite spelling never becomes a Number.
    nums.sort_by(f64::total_cmp);
    let n = nums.len();
    let med = if n % 2 == 1 {
        nums[n / 2]
    } else {
        (nums[n / 2 - 1] + nums[n / 2]) / 2.0
    };
    finite_or_num(med)
}

pub(crate) fn rank_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let number = match coerce_num(&scalarize(ctx.eval(&args[0]))) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let (_, _, cells) = match block(ctx, &args[1]) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let mut nums = Vec::new();
    for c in &cells {
        match c {
            Value::Error(k) => return Value::Error(*k),
            Value::Number(n) => nums.push(*n),
            _ => {}
        }
    }
    let ascending = if args.len() == 3 {
        match coerce_num(&scalarize(ctx.eval(&args[2]))) {
            Ok(o) => o != 0.0,
            Err(k) => return Value::Error(k),
        }
    } else {
        false
    };
    if !nums.contains(&number) {
        return Value::Error(ErrKind::Na);
    }
    let better = if ascending {
        nums.iter().filter(|&&x| x < number).count()
    } else {
        nums.iter().filter(|&&x| x > number).count()
    };
    Value::Number((better + 1) as f64)
}

pub(crate) fn counta(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let mut n: u64 = 0;
    for a in args {
        match ctx.eval(a) {
            Value::Array(_, cells) => {
                n += cells.iter().filter(|c| !matches!(c, Value::Blank)).count() as u64
            }
            Value::Blank => {}
            _ => n += 1,
        }
    }
    Value::Number(n as f64)
}

pub(crate) fn countblank(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let mut n: u64 = 0;
    for a in args {
        match ctx.eval(a) {
            Value::Array(_, cells) => n += cells.iter().filter(|c| is_blankish(c)).count() as u64,
            other => {
                if is_blankish(&other) {
                    n += 1;
                }
            }
        }
    }
    Value::Number(n as f64)
}

/// Whether a value counts as "blank" for `COUNTBLANK`: a `Blank`, or an empty-string `Text("")`.
fn is_blankish(v: &Value) -> bool {
    matches!(v, Value::Blank) || matches!(v, Value::Text(t) if t.is_empty())
}

/// Gather the numeric data of a SINGLE array/range argument under the in-range rule (non-numbers
/// ignored, an error propagated) — the one-array front door LARGE/SMALL/PERCENTILE/QUARTILE
/// share. (A bare scalar coerces, as `collect_numbers` does for a direct datum.)
pub(crate) fn collect_one(ctx: &mut EvalCtx, e: &Expr) -> Result<Vec<f64>, ErrKind> {
    collect_numbers(ctx, std::slice::from_ref(e))
}

/// The variance of `nums`: `sample` divides by `n-1` (Bessel-corrected), else by `n`. Returns `None`
/// when the divisor is non-positive (a sample of `< 2`, or a population of `0`) — the caller's
/// Excel `#DIV/0!`. Numerically the sum-of-squared-deviations about the mean.
fn variance(nums: &[f64], sample: bool) -> Option<f64> {
    let n = nums.len() as f64;
    let denom = if sample { n - 1.0 } else { n };
    if denom <= 0.0 {
        return None;
    }
    let mean = nums.iter().sum::<f64>() / n;
    let ss: f64 = nums
        .iter()
        .map(|x| {
            let d = x - mean;
            d * d
        })
        .sum();
    Some(ss / denom)
}

/// Shared body of the dispersion four: gather all args (SUM asymmetry), compute the sample/population
/// variance, and optionally square-root it (STDEV vs VAR). An under-count is `#DIV/0!` (Excel).
fn dispersion(ctx: &mut EvalCtx, args: &[Expr], sample: bool, root: bool) -> Value {
    let nums = match collect_numbers(ctx, args) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    match variance(&nums, sample) {
        None => Value::Error(ErrKind::Div0),
        Some(v) => finite_or_num(if root { v.sqrt() } else { v }),
    }
}

pub(crate) fn stdev_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    dispersion(ctx, args, true, true)
}

pub(crate) fn stdevp_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    dispersion(ctx, args, false, true)
}

pub(crate) fn var_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    dispersion(ctx, args, true, false)
}

pub(crate) fn varp_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    dispersion(ctx, args, false, false)
}

pub(crate) fn mode_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let nums = match collect_numbers(ctx, args) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    // First-appearance-ordered (value, count) tally, so a count tie keeps the earliest value.
    let mut seen: Vec<(f64, usize)> = Vec::new();
    for &x in &nums {
        match seen.iter_mut().find(|(v, _)| *v == x) {
            Some(e) => e.1 += 1,
            None => seen.push((x, 1)),
        }
    }
    let mut best: Option<(f64, usize)> = None;
    for &(v, c) in &seen {
        if c >= 2 && best.is_none_or(|(_, bc)| c > bc) {
            best = Some((v, c));
        }
    }
    match best {
        Some((v, _)) => finite_or_num(v),
        None => Value::Error(ErrKind::Na),
    }
}

pub(crate) fn large_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    order_stat(ctx, args, true)
}

pub(crate) fn small_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    order_stat(ctx, args, false)
}

/// Shared body of `LARGE`/`SMALL`: gather the array, sort ascending, index the `k`-th from the
/// requested end. An empty array or an out-of-domain `k` is `#NUM!`.
fn order_stat(ctx: &mut EvalCtx, args: &[Expr], largest: bool) -> Value {
    let mut nums = match collect_one(ctx, &args[0]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let k = match coerce_num(&scalarize(ctx.eval(&args[1]))) {
        Ok(n) => n.trunc(),
        Err(e) => return Value::Error(e),
    };
    let n = nums.len();
    if n == 0 || k < 1.0 || k > n as f64 {
        return Value::Error(ErrKind::Num);
    }
    nums.sort_by(f64::total_cmp);
    let k = k as usize;
    // ascending sorted: k-th smallest is index k-1; k-th largest is index n-k.
    let idx = if largest { n - k } else { k - 1 };
    finite_or_num(nums[idx])
}

pub(crate) fn percentile_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let nums = match collect_one(ctx, &args[0]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let k = match coerce_num(&scalarize(ctx.eval(&args[1]))) {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    percentile_inclusive(nums, k)
}

pub(crate) fn quartile_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let nums = match collect_one(ctx, &args[0]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let quart = match coerce_num(&scalarize(ctx.eval(&args[1]))) {
        Ok(n) => n.trunc(),
        Err(e) => return Value::Error(e),
    };
    if !(0.0..=4.0).contains(&quart) {
        return Value::Error(ErrKind::Num);
    }
    percentile_inclusive(nums, quart / 4.0)
}

/// The inclusive percentile of `nums` at fraction `k` (`PERCENTILE.INC` / `QUARTILE.INC`): sort, then
/// interpolate at `rank = k*(n-1)`. Empty data or `k` outside `[0, 1]` is `#NUM!`.
fn percentile_inclusive(mut nums: Vec<f64>, k: f64) -> Value {
    let n = nums.len();
    if n == 0 || !(0.0..=1.0).contains(&k) {
        return Value::Error(ErrKind::Num);
    }
    nums.sort_by(f64::total_cmp);
    let rank = k * (n - 1) as f64;
    let lo = rank.floor() as usize;
    let frac = rank - lo as f64;
    let val = if lo + 1 < n {
        nums[lo] + frac * (nums[lo + 1] - nums[lo])
    } else {
        nums[lo]
    };
    finite_or_num(val)
}
