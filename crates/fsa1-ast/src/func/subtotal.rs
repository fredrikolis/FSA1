// Concern: SUBTOTAL and AGGREGATE, dispatching a function number to a reducer | Non-concern: the reducers themselves | IO: (&mut EvalCtx, &[Expr]) -> Value
use super::*;

/// A family reducer over a variadic argument list — the shared signature SUBTOTAL/AGGREGATE dispatch
/// their reference-form function-number to.
type Reducer = fn(&mut EvalCtx, &[Expr]) -> Value;

/// Map a SUBTOTAL/AGGREGATE reference-form function number (`1..=13`) to its family reducer. SUBTOTAL
/// uses `1..=11`; AGGREGATE extends it with `12` (MEDIAN) and `13` (MODE.SNGL). The dispersion codes
/// map to the SAMPLE forms (`7 → STDEV`, `10 → VAR`) and the population forms (`8 → STDEVP`,
/// `11 → VARP`), matching Excel. `None` for an out-of-range code (the caller returns `#VALUE!`).
fn reference_reducer(code: i64) -> Option<Reducer> {
    Some(match code {
        1 => average,
        2 => count,
        3 => counta,
        4 => max_fn,
        5 => min_fn,
        6 => product,
        7 => stdev_fn,
        8 => stdevp_fn,
        9 => sum,
        10 => var_fn,
        11 => varp_fn,
        12 => median_fn,
        13 => mode_fn,
        _ => return None,
    })
}

pub(crate) fn subtotal_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let raw = match one_num(ctx, &args[0]) {
        Ok(n) => n.trunc(),
        Err(k) => return Value::Error(k),
    };
    // Fold the 100-series (ignore-hidden) onto its 1-series reducer; anything else is out of range.
    let code = if (101.0..=111.0).contains(&raw) {
        (raw - 100.0) as i64
    } else if (1.0..=11.0).contains(&raw) {
        raw as i64
    } else {
        return Value::Error(ErrKind::Value);
    };
    match reference_reducer(code) {
        Some(reducer) => reducer(ctx, &args[1..]),
        None => Value::Error(ErrKind::Value),
    }
}

pub(crate) fn aggregate_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let fnum = match one_num(ctx, &args[0]) {
        Ok(n) => n.trunc(),
        Err(k) => return Value::Error(k),
    };
    let options = match one_num(ctx, &args[1]) {
        Ok(n) => n.trunc(),
        Err(k) => return Value::Error(k),
    };
    if !(1.0..=19.0).contains(&fnum) || !(0.0..=7.0).contains(&options) {
        return Value::Error(ErrKind::Value);
    }
    let fnum = fnum as i64;
    // Only these options ignore error values; the hidden-row options are a no-op against a materialized block.
    let ignore_errors = matches!(options as i64, 2 | 3 | 6 | 7);

    if fnum <= 13 {
        let reducer = reference_reducer(fnum).expect("fnum 1..=13 checked above");
        let data = materialize(ctx, &args[2..], ignore_errors);
        reducer(ctx, &data)
    } else {
        // Array form (14..=19): exactly (array, k). Fewer args is a #VALUE! (the k is required).
        if args.len() != 4 {
            return Value::Error(ErrKind::Value);
        }
        let arr = Expr::Lit(prep_value(ctx.eval(&args[2]), ignore_errors));
        match fnum {
            14 => large_fn(ctx, &[arr, args[3].clone()]),
            15 => small_fn(ctx, &[arr, args[3].clone()]),
            16 => percentile_fn(ctx, &[arr, args[3].clone()]),
            17 => quartile_fn(ctx, &[arr, args[3].clone()]),
            18 | 19 => {
                let nums = match collect_numbers(ctx, std::slice::from_ref(&arr)) {
                    Ok(v) => v,
                    Err(k) => return Value::Error(k),
                };
                let arg = match one_num(ctx, &args[3]) {
                    Ok(v) => v,
                    Err(k) => return Value::Error(k),
                };
                if fnum == 18 {
                    percentile_exclusive(nums, arg)
                } else {
                    quartile_exclusive(nums, arg)
                }
            }
            _ => Value::Error(ErrKind::Value),
        }
    }
}

/// Materialize each data argument to a `Lit` value, [`prep_value`]-processing it so a downstream
/// reducer sees exactly the data AGGREGATE's `options` intends (errors stripped when ignored).
fn materialize(ctx: &mut EvalCtx, args: &[Expr], ignore_errors: bool) -> Vec<Expr> {
    args.iter()
        .map(|a| Expr::Lit(prep_value(ctx.eval(a), ignore_errors)))
        .collect()
}

/// When errors are ignored, an error CELL is dropped and re-packed as a flat row (reducers read
/// range data position-agnostically) and a bare error SCALAR becomes `Blank`, which every reducer skips.
fn prep_value(v: Value, ignore_errors: bool) -> Value {
    if !ignore_errors {
        return v;
    }
    match v {
        Value::Array(_, cells) => {
            let kept: Vec<Value> = cells
                .into_iter()
                .filter(|c| !matches!(c, Value::Error(_)))
                .collect();
            Value::Array(
                Shape {
                    rows: 1,
                    cols: kept.len() as u32,
                },
                kept,
            )
        }
        Value::Error(_) => Value::Blank,
        other => other,
    }
}

/// `PERCENTILE.EXC` of `nums` at fraction `k` (AGGREGATE function 18): the EXCLUSIVE percentile, where
/// the interpolation rank is `k·(n+1)` and `k` must lie strictly inside `(0, 1)` with a rank in
/// `[1, n]`; otherwise `#NUM!`. Distinct from the inclusive `PERCENTILE`/`percentile_inclusive`
/// (rank `k·(n−1)`, `k ∈ [0, 1]`).
fn percentile_exclusive(mut nums: Vec<f64>, k: f64) -> Value {
    let n = nums.len();
    if n == 0 || k <= 0.0 || k >= 1.0 {
        return Value::Error(ErrKind::Num);
    }
    let rank = k * (n as f64 + 1.0); // 1-based interpolation rank
    if rank < 1.0 || rank > n as f64 {
        return Value::Error(ErrKind::Num);
    }
    nums.sort_by(f64::total_cmp);
    let lo = rank.floor() as usize; // 1-based lower rank
    let frac = rank - lo as f64;
    let lo_idx = lo - 1; // 0-based
    let val = if lo_idx + 1 < n {
        nums[lo_idx] + frac * (nums[lo_idx + 1] - nums[lo_idx])
    } else {
        nums[lo_idx]
    };
    finite_or_num(val)
}

/// `QUARTILE.EXC` of `nums` at `quart` (AGGREGATE function 19): the EXCLUSIVE quartile. Only `quart`
/// `1, 2, 3` are valid (mapping to the `25/50/75%` exclusive percentiles); `0` and `4` — the min/max —
/// are `#NUM!` under the exclusive definition.
fn quartile_exclusive(nums: Vec<f64>, quart: f64) -> Value {
    let quart = quart.trunc();
    if !(1.0..=3.0).contains(&quart) {
        return Value::Error(ErrKind::Num);
    }
    percentile_exclusive(nums, quart / 4.0)
}
