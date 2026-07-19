// Concern: the RANKING / POSITIONAL worksheet functions extending the order-statistics core — the average-rank tie rule RANK.AVG, the EXCLUSIVE percentile/quartile family (PERCENTILE.EXC QUARTILE.EXC), the percent-of-rank query PERCENTRANK (+ its .INC/.EXC spellings) with Excel's significant-digit truncation, the all-modes array MODE.MULT, and the binning array FREQUENCY — each Excel-exact in arg order, domain, and error value | Non-concern: RANK.EQ (an alias for the core `rank_fn` in func/stats.rs), the inclusive PERCENTILE/QUARTILE and MODE.SNGL (func/stats.rs), the registry table + dispatch (func/mod.rs), and the shared `collect_one`/`block`/`finite_or_num` helpers | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value` (a scalar, an array for MODE.MULT/FREQUENCY, or a located error value)
use super::*;

/// Gather the numeric cells of a `ref` argument for the ranking family: a `Number` is kept, an error
/// propagates (leftmost), every other cell (text/blank/logical) is ignored — matching RANK's rule.
fn rank_numbers(ctx: &mut EvalCtx, e: &Expr) -> Result<Vec<f64>, ErrKind> {
    let (_, _, cells) = block(ctx, e)?;
    let mut nums = Vec::new();
    for c in &cells {
        match c {
            Value::Error(k) => return Err(*k),
            Value::Number(n) => nums.push(*n),
            _ => {}
        }
    }
    Ok(nums)
}

/// `RANK.AVG(number, ref, [order])` — like RANK.EQ, but TIES SHARE THE AVERAGE of the ranks they span
/// (`(best + worst)/2`), so two values tied for rank 2 both report `2.5`. `order = 0`/omitted is
/// DESCENDING, any non-zero `order` ASCENDING. Non-numeric cells ignored, an error propagates, a
/// `number` absent from `ref` is `#N/A`.
pub(crate) fn rank_avg_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let number = match coerce_num(&scalarize(ctx.eval(&args[0]))) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let nums = match rank_numbers(ctx, &args[1]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
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
    let equal = nums.iter().filter(|&&x| x == number).count();
    // best rank = better + 1; worst = better + equal; average = better + 1 + (equal - 1)/2.
    Value::Number(better as f64 + 1.0 + (equal as f64 - 1.0) / 2.0)
}

/// The EXCLUSIVE percentile of `nums` at fraction `k` (`PERCENTILE.EXC`): sort, interpolate at the
/// 1-based `rank = k·(n+1)`. `k` must place the rank inside `[1, n]` (i.e. `1/(n+1) ≤ k ≤ n/(n+1)`);
/// empty data, or a `k` outside that open range, is `#NUM!` (Excel — the exclusive form cannot reach
/// the endpoints).
fn percentile_exclusive(mut nums: Vec<f64>, k: f64) -> Value {
    let n = nums.len();
    if n == 0 || !k.is_finite() {
        return Value::Error(ErrKind::Num);
    }
    let rank = k * (n as f64 + 1.0);
    if rank < 1.0 || rank > n as f64 {
        return Value::Error(ErrKind::Num);
    }
    nums.sort_by(f64::total_cmp);
    let lo = rank.floor() as usize; // 1-based lower rank
    let frac = rank - lo as f64;
    let idx = lo - 1; // 0-based
    let val = if idx + 1 < n {
        nums[idx] + frac * (nums[idx + 1] - nums[idx])
    } else {
        nums[idx]
    };
    finite_or_num(val)
}

/// `PERCENTILE.EXC(array, k)` — the EXCLUSIVE `k`-th percentile, `k` strictly inside `(0, 1)` and
/// within `[1/(n+1), n/(n+1)]`. Empty data or an out-of-domain `k` is `#NUM!`.
pub(crate) fn percentile_exc_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let nums = match collect_one(ctx, &args[0]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let k = match coerce_num(&scalarize(ctx.eval(&args[1]))) {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    percentile_exclusive(nums, k)
}

/// `QUARTILE.EXC(array, quart)` — the EXCLUSIVE quartile: `quart` (truncated) `1→25%`, `2→50%`,
/// `3→75%`, delegating to `PERCENTILE.EXC`. `quart` `0` or `4` (the endpoints the exclusive form
/// cannot reach), or anything outside `1..=3`, or empty data, is `#NUM!` (Excel).
pub(crate) fn quartile_exc_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let nums = match collect_one(ctx, &args[0]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let quart = match coerce_num(&scalarize(ctx.eval(&args[1]))) {
        Ok(n) => n.trunc(),
        Err(e) => return Value::Error(e),
    };
    if !(1.0..=3.0).contains(&quart) {
        return Value::Error(ErrKind::Num);
    }
    percentile_exclusive(nums, quart / 4.0)
}

/// Truncate `v` to `sig` significant digits (toward zero) — Excel's PERCENTRANK rounding: `0.16666…`
/// at 3 digits is `0.166`, at 5 is `0.16666`. Zero is returned unchanged (its magnitude is undefined).
fn trunc_significant(v: f64, sig: i32) -> f64 {
    if v == 0.0 || !v.is_finite() {
        return v;
    }
    // digits after which to cut: (sig - 1) beyond the leading digit's place.
    let place = sig as f64 - 1.0 - v.abs().log10().floor();
    let factor = 10f64.powf(place);
    (v * factor).trunc() / factor
}

/// Shared body of `PERCENTRANK` / `PERCENTRANK.INC` / `PERCENTRANK.EXC`: the rank of `x` as a fraction
/// of `array`. `inclusive` divides the interpolated position by `n-1` and maps the min→0/max→1;
/// `!inclusive` (the .EXC form) divides `position+1` by `n+1`. An `x` outside `[min, max]` is `#N/A`;
/// empty data or `significance < 1` is `#NUM!`. The result is truncated to `significance` (default 3)
/// significant digits.
fn percentrank(ctx: &mut EvalCtx, args: &[Expr], inclusive: bool) -> Value {
    let mut nums = match collect_one(ctx, &args[0]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let x = match coerce_num(&scalarize(ctx.eval(&args[1]))) {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let sig = match opt_num(ctx, args, 2, 3.0) {
        Ok(n) => n.trunc() as i32,
        Err(e) => return Value::Error(e),
    };
    if nums.is_empty() || sig < 1 {
        return Value::Error(ErrKind::Num);
    }
    nums.sort_by(f64::total_cmp);
    let n = nums.len();
    if x < nums[0] || x > nums[n - 1] {
        return Value::Error(ErrKind::Na);
    }
    // largest 0-based index `i` with nums[i] <= x (exists since x >= nums[0]).
    let i = nums.iter().rposition(|&v| v <= x).unwrap();
    // interpolated 0-based position of x within the sorted data.
    let pos = if nums[i] == x || i + 1 >= n {
        i as f64
    } else {
        i as f64 + (x - nums[i]) / (nums[i + 1] - nums[i])
    };
    let raw = if inclusive {
        pos / (n as f64 - 1.0)
    } else {
        (pos + 1.0) / (n as f64 + 1.0)
    };
    finite_or_num(trunc_significant(raw, sig))
}

/// `PERCENTRANK(array, x, [significance])` — the INCLUSIVE percent rank of `x` (legacy spelling of
/// `PERCENTRANK.INC`).
pub(crate) fn percentrank_inc_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    percentrank(ctx, args, true)
}

/// `PERCENTRANK.EXC(array, x, [significance])` — the EXCLUSIVE percent rank of `x` (divides by `n+1`).
pub(crate) fn percentrank_exc_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    percentrank(ctx, args, false)
}

/// `MODE.MULT(number1, …)` — the VERTICAL array of ALL values tied for the maximum frequency (each
/// occurring `≥ 2` times), in first-appearance order. Gathers under the SUM direct-vs-in-range
/// asymmetry (via `collect_numbers`). No repeated value is `#N/A` (Excel). A single-cell context keeps
/// the array's top-left element (the first-appearing mode).
pub(crate) fn mode_mult_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let nums = match collect_numbers(ctx, args) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    // First-appearance-ordered (value, count) tally.
    let mut seen: Vec<(f64, usize)> = Vec::new();
    for &x in &nums {
        match seen.iter_mut().find(|(v, _)| *v == x) {
            Some(e) => e.1 += 1,
            None => seen.push((x, 1)),
        }
    }
    let max_count = seen.iter().map(|&(_, c)| c).max().unwrap_or(0);
    if max_count < 2 {
        return Value::Error(ErrKind::Na);
    }
    let modes: Vec<Value> = seen
        .iter()
        .filter(|&&(_, c)| c == max_count)
        .map(|&(v, _)| Value::Number(v))
        .collect();
    Value::Array(
        Shape {
            rows: modes.len() as u32,
            cols: 1,
        },
        modes,
    )
}

/// `FREQUENCY(data_array, bins_array)` — a VERTICAL array of `bins+1` counts: element `0` is
/// `count(x ≤ bins[0])`, element `j` is `count(bins[j-1] < x ≤ bins[j])`, and the last is
/// `count(x > bins[last])`. Non-numeric cells in either array are ignored; bins are used in the order
/// given (not sorted). An error in either argument propagates. A single-cell context keeps the top
/// count. With no bins the result is the single count of all data.
pub(crate) fn frequency_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let data = match collect_one(ctx, &args[0]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let bins = match collect_one(ctx, &args[1]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let m = bins.len();
    let mut counts = vec![0u64; m + 1];
    for &x in &data {
        let mut placed = false;
        for (j, &b) in bins.iter().enumerate() {
            if x <= b {
                counts[j] += 1;
                placed = true;
                break;
            }
        }
        if !placed {
            counts[m] += 1;
        }
    }
    let cells: Vec<Value> = counts.iter().map(|&c| Value::Number(c as f64)).collect();
    Value::Array(
        Shape {
            rows: (m + 1) as u32,
            cols: 1,
        },
        cells,
    )
}
