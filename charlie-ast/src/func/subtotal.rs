// Concern: the META-AGGREGATOR worksheet functions (SUBTOTAL AGGREGATE) — the two built-ins that pick an aggregate by a leading FUNCTION-NUMBER and apply it to the remaining ranges, DELEGATING to the already-correct family reducers (SUM/AVERAGE/COUNT/COUNTA/MAX/MIN/PRODUCT/STDEV*/VAR*/MEDIAN/MODE and the LARGE/SMALL/PERCENTILE/QUARTILE order-statistics) so the arithmetic lives in ONE place; SUBTOTAL maps 1–11 and 101–111 to the same reducer (charlie has no hidden-row concept, so the 100-series behaves identically), and AGGREGATE additionally honours its options flag by materializing the data and STRIPPING error cells when the option ignores errors, plus the two exclusive order statistics (PERCENTILE.EXC/QUARTILE.EXC) it alone needs | Non-concern: the reducers themselves (func/aggregation.rs, func/stats.rs, func/math.rs own the arithmetic), nested-SUBTOTAL exclusion and manually-hidden rows (charlie's filesystem grid has neither — accept-under-uncertainty, a documented divergence outside the parity corpus), and the registry table + dispatch (func/mod.rs) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
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

/// `SUBTOTAL(function_num, ref1, [ref2], …)` — aggregate the referenced data with the reducer named by
/// `function_num`. `1..=11` and `101..=111` select the same reducer (AVERAGE/COUNT/COUNTA/MAX/MIN/
/// PRODUCT/STDEV/STDEVP/SUM/VAR/VARP); the 100-series means "ignore manually-hidden rows", which a
/// filesystem grid has no notion of, so both series behave identically here. An out-of-range
/// `function_num` is `#VALUE!`.
///
/// NOTE: real Excel SUBTOTAL also ignores the results of any nested SUBTOTAL/AGGREGATE cells inside
/// its refs; charlie's engine sees only materialized values (not which cell was itself a SUBTOTAL), so
/// that exclusion is not modelled — a deliberate, documented divergence (accept-under-uncertainty),
/// outside the ENG6 parity corpus.
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

/// `AGGREGATE(function_num, options, ref1, [ref2], …)` (reference form, `function_num 1..=13`) or
/// `AGGREGATE(function_num, options, array, k)` (array form, `function_num 14..=19`). Applies the
/// reducer named by `function_num`; `options` selects what to ignore. charlie has no hidden rows and
/// cannot see nested aggregates, so only the ERROR-ignoring dimension of `options` is observable:
/// options `2, 3, 6, 7` strip error cells from the data (so the reducer never propagates them); the
/// rest leave errors in place (so an erroring datum propagates as it normally would). An out-of-range
/// `function_num` or `options`, or the array form without its `k`, is `#VALUE!`.
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
    // Options 2, 3, 6, 7 ignore error values; the rest do not (1/5 ignore only hidden rows — a no-op
    // here — and 0/4 ignore only nested aggregates / nothing).
    let ignore_errors = matches!(options as i64, 2 | 3 | 6 | 7);

    if fnum <= 13 {
        // Reference form: the data are args[2..]. materialize (stripping errors if the option asks)
        // then delegate to the family reducer, which owns the arithmetic.
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

/// Prepare one evaluated datum for a reducer under AGGREGATE's ignore-errors option. When errors are
/// ignored, an error cell is dropped from an array (the surviving cells are re-packed as a flat row —
/// reducers treat range data positionally-agnostically) and a bare error scalar becomes `Blank` (which
/// every reducer skips). When errors are NOT ignored the value passes through unchanged, so an
/// erroring datum propagates exactly as it would to the reducer called directly.
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
