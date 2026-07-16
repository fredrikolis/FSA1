// Concern: the STATISTICS worksheet functions (MIN MAX MEDIAN RANK COUNTA COUNTBLANK) — statistical extremes / order / counting, sharing SUM's direct-vs-in-range data-gathering asymmetry and pinning the empty-result calls (MIN/MAX over no numbers is 0, MEDIAN is `#NUM!`) and the COUNTA/COUNTBLANK cell-counting rules | Non-concern: the registry table + dispatch (func/mod.rs) and the shared `collect_numbers`/`block`/`finite_or_num` helpers (func/helpers.rs) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
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
