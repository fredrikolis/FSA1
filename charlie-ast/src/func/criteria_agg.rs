// Concern: the CRITERIA-AGGREGATION worksheet functions (SUMIF SUMIFS · AVERAGEIF AVERAGEIFS · MINIFS MAXIFS · COUNTIF COUNTIFS) — the `*IF(S)` family, building a boolean match mask over criteria ranges (one criterion for `*IF`, an AND across pairs for `*IFS`) and reducing the value range under it, with the STATIC range-shape-conformance `#VALUE!` rule | Non-concern: the registry table + dispatch (func/mod.rs), the CRITERIA mini-language (criteria.rs owns `Criterion`/`parse_criterion`), and the shared `block`/`finite_or_num` helpers (func/helpers.rs) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
use super::*;

// Criteria-aggregation family. Two argument SHAPES share one criteria mini-language (`crate::criteria`)
// and one range-conformance rule:
//   * the `*IF` forms take `(criteria_range, criteria, [value_range])` — a single criterion, and the
//     value range defaults to the criteria range when omitted;
//   * the `*IFS` forms take `(value_range, criteria_range1, criteria1, …)` — an AND across pairs,
//     with COUNTIFS having no value range (it counts matching cells directly).
// CONFORMANCE (an Excel-semantics call worth a reviewer's eye): every criteria range and the value
// range must share the SAME on-disk shape (rows × cols) — a mismatch is a STATIC `#VALUE!`, not
// Excel's lenient legacy reshape-from-the-value-range's-corner. This is the same "static conformance
// beats runtime guessing" stance the encoding layer takes (a static structural refusal, cf.
// charlie-model's FT-8 dimension check). A blank/text cell in a
// value range is ignored (only numbers aggregate); an error in a value range at a MATCHING position
// propagates; an error IN a criteria range never matches; an error-valued criterion propagates.
/// The reduction a masked aggregation performs over the numeric matching cells of a value range.
#[derive(Clone, Copy)]
enum Reduce {
    Sum,
    Avg,
    Min,
    Max,
}

/// Parse a criteria argument: evaluate it, collapse to a scalar, and parse the mini-language. An
/// error criterion (or a multi-cell array in criteria position) propagates.
fn criterion(ctx: &mut EvalCtx, e: &Expr) -> Result<Criterion, ErrKind> {
    parse_criterion(&scalarize(ctx.eval(e)))
}

/// Reduce the numeric cells of `value_cells` at the positions `mask` marks true. An error cell at a
/// matching position propagates; non-numbers are ignored. `Avg` over no numbers is `#DIV/0!`; `Min`/
/// `Max` over no numbers is `0` (Excel's `MINIFS`/`MAXIFS` empty result).
fn reduce_masked(value_cells: &[Value], mask: &[bool], reduce: Reduce) -> Value {
    let mut sum = 0.0;
    let mut count: u64 = 0;
    let mut extreme: Option<f64> = None;
    for (m, v) in mask.iter().zip(value_cells.iter()) {
        if !*m {
            continue;
        }
        match v {
            Value::Error(k) => return Value::Error(*k),
            Value::Number(n) => {
                sum += n;
                count += 1;
                extreme = Some(match reduce {
                    Reduce::Min => extreme.map_or(*n, |e| e.min(*n)),
                    Reduce::Max => extreme.map_or(*n, |e| e.max(*n)),
                    _ => *n,
                });
            }
            _ => {}
        }
    }
    match reduce {
        Reduce::Sum => finite_or_num(sum),
        Reduce::Avg => {
            if count == 0 {
                Value::Error(ErrKind::Div0)
            } else {
                finite_or_num(sum / count as f64)
            }
        }
        Reduce::Min | Reduce::Max => Value::Number(extreme.unwrap_or(0.0)),
    }
}

/// Build the AND-combined match mask for the `*IFS` pair list `(criteria_range, criteria)…`,
/// enforcing that every criteria range shares the first one's shape. Returns the shared shape and
/// the per-cell mask (true = every criterion matched). An empty pair list is a caller bug (arity
/// guarantees ≥1 pair), guarded as `#VALUE!` rather than a panic.
fn build_mask(
    ctx: &mut EvalCtx,
    pairs: &[(&Expr, &Expr)],
) -> Result<((u32, u32), Vec<bool>), ErrKind> {
    let mut base: Option<(u32, u32)> = None;
    let mut mask: Vec<bool> = Vec::new();
    for (crange, cexpr) in pairs {
        let (rows, cols, cells) = block(ctx, crange)?;
        match base {
            None => {
                base = Some((rows, cols));
                mask = vec![true; cells.len()];
            }
            Some(b) if b != (rows, cols) => return Err(ErrKind::Value),
            Some(_) => {}
        }
        let crit = criterion(ctx, cexpr)?;
        for (m, cell) in mask.iter_mut().zip(cells.iter()) {
            if *m && !crit.matches(cell) {
                *m = false;
            }
        }
    }
    base.map(|b| (b, mask)).ok_or(ErrKind::Value)
}

/// Shared body of `SUMIF`/`AVERAGEIF`: a single criterion over `range`, reducing the value range
/// (`value_range` arg, or `range` itself when omitted) at the matching positions.
fn single_if(ctx: &mut EvalCtx, args: &[Expr], reduce: Reduce) -> Value {
    let (rrows, rcols, rcells) = match block(ctx, &args[0]) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let crit = match criterion(ctx, &args[1]) {
        Ok(c) => c,
        Err(k) => return Value::Error(k),
    };
    let value_cells = if args.len() == 3 {
        let (vrows, vcols, vcells) = match block(ctx, &args[2]) {
            Ok(t) => t,
            Err(k) => return Value::Error(k),
        };
        if (vrows, vcols) != (rrows, rcols) {
            return Value::Error(ErrKind::Value);
        }
        vcells
    } else {
        rcells.clone()
    };
    let mask: Vec<bool> = rcells.iter().map(|c| crit.matches(c)).collect();
    reduce_masked(&value_cells, &mask, reduce)
}

/// Shared body of `SUMIFS`/`AVERAGEIFS`/`MINIFS`/`MAXIFS`: value range is `args[0]`, then
/// `(criteria_range, criteria)` pairs. Enforces an odd arity (value + whole pairs) and that the value
/// range conforms to the criteria ranges' shape.
fn multi_if(ctx: &mut EvalCtx, args: &[Expr], reduce: Reduce) -> Value {
    // args[0] is the value range; the rest must be whole (criteria_range, criteria) pairs.
    if !(args.len() - 1).is_multiple_of(2) {
        return Value::Error(ErrKind::Value);
    }
    let pairs = pair_up(&args[1..]);
    let ((brows, bcols), mask) = match build_mask(ctx, &pairs) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let (vrows, vcols, vcells) = match block(ctx, &args[0]) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    if (vrows, vcols) != (brows, bcols) {
        return Value::Error(ErrKind::Value);
    }
    reduce_masked(&vcells, &mask, reduce)
}

/// Chunk a flat argument slice into `(range, criteria)` pairs. The caller has already checked the
/// slice length is even.
fn pair_up(args: &[Expr]) -> Vec<(&Expr, &Expr)> {
    args.chunks_exact(2).map(|c| (&c[0], &c[1])).collect()
}

/// `SUMIF(range, criteria, [sum_range])` — total the numbers at the matching positions.
pub(crate) fn sumif(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    single_if(ctx, args, Reduce::Sum)
}

/// `AVERAGEIF(range, criteria, [average_range])` — mean of the matching numbers; no match is
/// `#DIV/0!`.
pub(crate) fn averageif(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    single_if(ctx, args, Reduce::Avg)
}

/// `SUMIFS(sum_range, criteria_range1, criteria1, …)` — total where EVERY criterion matches.
pub(crate) fn sumifs(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    multi_if(ctx, args, Reduce::Sum)
}

/// `AVERAGEIFS(average_range, criteria_range1, criteria1, …)` — mean where every criterion matches;
/// no match is `#DIV/0!`.
pub(crate) fn averageifs(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    multi_if(ctx, args, Reduce::Avg)
}

/// `MINIFS(min_range, criteria_range1, criteria1, …)` — smallest matching number; no match is `0`.
pub(crate) fn minifs(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    multi_if(ctx, args, Reduce::Min)
}

/// `MAXIFS(max_range, criteria_range1, criteria1, …)` — largest matching number; no match is `0`.
pub(crate) fn maxifs(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    multi_if(ctx, args, Reduce::Max)
}

/// `COUNTIF(range, criteria)` — how many cells match. Counts a matching cell of ANY type (unlike the
/// summing forms, `COUNTIF` does not require a number), and never returns an error from its data — but
/// an error-valued CRITERION propagates.
pub(crate) fn countif(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (_, _, cells) = match block(ctx, &args[0]) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let crit = match criterion(ctx, &args[1]) {
        Ok(c) => c,
        Err(k) => return Value::Error(k),
    };
    let n = cells.iter().filter(|c| crit.matches(c)).count();
    Value::Number(n as f64)
}

/// `COUNTIFS(criteria_range1, criteria1, …)` — how many positions match EVERY criterion. Requires an
/// even arity (whole pairs) and conforming criteria-range shapes.
pub(crate) fn countifs(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    if !args.len().is_multiple_of(2) {
        return Value::Error(ErrKind::Value);
    }
    let pairs = pair_up(args);
    match build_mask(ctx, &pairs) {
        Ok((_, mask)) => Value::Number(mask.iter().filter(|m| **m).count() as f64),
        Err(k) => Value::Error(k),
    }
}
