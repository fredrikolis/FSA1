// Concern: the ranking, percentile and frequency built-ins | Non-concern: the inclusive percentile forms (stats.rs holds them) | IO: (&mut EvalCtx, &[Expr]) -> Value
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

/// `inclusive` divides the interpolated position by `n-1`, mapping min to 0 and max to 1; the
/// exclusive form divides `position+1` by `n+1`.
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

pub(crate) fn percentrank_inc_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    percentrank(ctx, args, true)
}

pub(crate) fn percentrank_exc_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    percentrank(ctx, args, false)
}

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
