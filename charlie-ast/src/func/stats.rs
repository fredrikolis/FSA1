// Concern: the STATISTICS worksheet functions (MIN MAX MEDIAN RANK COUNTA COUNTBLANK; the dispersion family STDEV/STDEVP/VAR/VARP; the order-statistics LARGE/SMALL/PERCENTILE/QUARTILE; MODE) — statistical extremes / order / counting / dispersion, sharing SUM's direct-vs-in-range data-gathering asymmetry and pinning the empty-result calls (MIN/MAX over no numbers is 0, MEDIAN is `#NUM!`, STDEV/VAR under-count is `#DIV/0!`, MODE with no repeat is `#N/A`) and the COUNTA/COUNTBLANK cell-counting rules | Non-concern: the registry table + dispatch (func/mod.rs) and the shared `collect_numbers`/`block`/`finite_or_num` helpers (func/helpers.rs) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
use super::*;

// Statistical extremes / order / counting (the v1 stats batch). MIN/MAX/MEDIAN share the SAME
// data-gathering asymmetry as SUM (`collect_numbers`): a datum passed DIRECTLY coerces (a boolean →
// 1/0, numeric-text → its number, a non-numeric direct text → `#VALUE!`), but a boolean/text/blank
// riding INSIDE a range is IGNORED (only in-range numbers aggregate). This "range vs direct-arg
// asymmetry" is the load-bearing Excel-semantics call for the whole batch. Errors propagate
// (leftmost). Empty-result calls differ by function: MIN/MAX over no numbers is `0` (Excel), MEDIAN
// over no numbers is `#NUM!`. COUNTA/COUNTBLANK count cells (never propagate an error from their
// data): COUNTA counts every non-empty datum (text, number, boolean, AND an error all count — only a
// `Blank` does not); COUNTBLANK counts the empty ones — a `Blank` OR an empty-string `""` (Excel
// counts a formula's `""` as blank), and nothing else.
/// `MIN(a, b, …)` — the smallest number among the data; in-range text/blanks/logicals are ignored,
/// direct booleans/numeric-text coerce, errors propagate, and NO numeric datum yields `0` (Excel).
pub(crate) fn min_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    extreme(ctx, args, f64::min)
}

/// `MAX(a, b, …)` — the largest number among the data; same gathering rules and empty-`0` result as
/// [`min_fn`], reduced with `f64::max` instead.
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

/// `MEDIAN(a, b, …)` — the middle of the sorted numeric data, AVERAGING THE TWO MIDDLES for an even
/// count (`(lo + hi) / 2`). Gathering matches MIN/MAX (in-range non-numbers ignored, direct coerce,
/// errors propagate); NO numeric datum is `#NUM!` (distinct from MIN/MAX's empty-`0`).
pub(crate) fn median_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let mut nums = match collect_numbers(ctx, args) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    if nums.is_empty() {
        return Value::Error(ErrKind::Num);
    }
    // total_cmp is a total order over the finite f64s gathered here (no NaN can enter — non-finite
    // text never parses to a Number), so no unwrap and no panic.
    nums.sort_by(f64::total_cmp);
    let n = nums.len();
    let med = if n % 2 == 1 {
        nums[n / 2]
    } else {
        (nums[n / 2 - 1] + nums[n / 2]) / 2.0
    };
    finite_or_num(med)
}

/// `RANK(number, ref, [order])` — the position of `number` within the numeric cells of `ref`,
/// `order = 0`/omitted DESCENDING (largest is rank `1`), any non-zero `order` ASCENDING. TIES SHARE
/// THE LOWEST RANK (computed as `1 + count of strictly-better values`, so equal values necessarily
/// get the same, best, rank). Non-numeric cells in `ref` are ignored; an error in `ref` (or a
/// non-numeric `number`/`order`) propagates; a `number` absent from `ref` is `#N/A`.
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

/// `COUNTA(a, b, …)` — how many data are NON-EMPTY. Everything but a `Blank` counts (text, numbers,
/// booleans, AND errors), whether passed directly or riding in a range; a `Blank` (direct or
/// in-range) never counts. Never propagates an error — an error datum is just a counted non-empty.
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

/// `COUNTBLANK(range)` — how many cells are EMPTY: a `Blank` or an empty-string `""` (Excel counts a
/// formula's `""` result as blank). Nothing else counts — a zero, an error, or any non-empty text is
/// not blank. Never propagates an error.
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

// --- Dispersion + order statistics (the P3 stats batch: STDEV STDEVP VAR VARP MODE LARGE SMALL
//     PERCENTILE QUARTILE). STDEV/STDEVP/VAR/VARP/MODE are VARIADIC and reuse SUM's `collect_numbers`
//     direct-vs-in-range asymmetry (an in-range non-number is ignored, a direct boolean/numeric-text
//     coerces, an error propagates) — matching Excel's variadic non-`A`-suffixed dispersion functions
//     and Excel's `MODE(number1, [number2], …)`.
//     LARGE/SMALL/PERCENTILE/QUARTILE take ONE array/range argument gathered under the
//     in-range rule (via `collect_one`); a degenerate input (empty data, or an out-of-domain k /
//     quart / no-repeat) is the documented Excel error value, never a panic.

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

/// `STDEV(a, b, …)` — the SAMPLE standard deviation (divisor `n-1`); `< 2` numbers is `#DIV/0!`.
pub(crate) fn stdev_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    dispersion(ctx, args, true, true)
}

/// `STDEVP(a, b, …)` — the POPULATION standard deviation (divisor `n`); no numbers is `#DIV/0!`.
pub(crate) fn stdevp_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    dispersion(ctx, args, false, true)
}

/// `VAR(a, b, …)` — the SAMPLE variance (divisor `n-1`); `< 2` numbers is `#DIV/0!`.
pub(crate) fn var_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    dispersion(ctx, args, true, false)
}

/// `VARP(a, b, …)` — the POPULATION variance (divisor `n`); no numbers is `#DIV/0!`.
pub(crate) fn varp_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    dispersion(ctx, args, false, false)
}

/// `MODE(number1, [number2], …)` — the most frequently occurring number, TIES BROKEN by first
/// appearance; if NO value repeats (or there is no numeric datum) the result is `#N/A` (Excel).
/// Equality is exact `f64`. VARIADIC like its dispersion siblings — gathers ALL args under SUM's
/// direct-vs-in-range asymmetry (via `collect_numbers`), so `MODE(1,2,2,3)` and `MODE(A1:A5,B1:B5)`
/// both tally across every datum, not just the first argument.
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

/// `LARGE(array, k)` — the `k`-th LARGEST number (1-based). `k` is truncated toward zero; a `k` below
/// `1` or above the count, or an empty array, is `#NUM!` (Excel).
pub(crate) fn large_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    order_stat(ctx, args, true)
}

/// `SMALL(array, k)` — the `k`-th SMALLEST number (1-based); same `k`-domain and `#NUM!` rules as
/// [`large_fn`].
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

/// `PERCENTILE(array, k)` — the INCLUSIVE `k`-th percentile (`PERCENTILE.INC`), `k` in `[0, 1]`. Linear
/// interpolation between the two closest ranks: `rank = k*(n-1)`. Empty data, or `k` outside `[0, 1]`,
/// is `#NUM!` (Excel).
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

/// `QUARTILE(array, quart)` — the INCLUSIVE quartile (`QUARTILE.INC`): `quart` (truncated) maps
/// `0→0%`, `1→25%`, `2→50%`, `3→75%`, `4→100%`. A `quart` outside `0..=4`, or empty data, is `#NUM!`.
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
