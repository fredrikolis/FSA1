// Concern: the *IF and *IFS aggregating built-ins | Non-concern: the criteria grammar (criteria.rs owns it), plain aggregation | IO: (&mut EvalCtx, &[Expr]) -> Value
use super::*;

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

pub(crate) fn sumif(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    single_if(ctx, args, Reduce::Sum)
}

pub(crate) fn averageif(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    single_if(ctx, args, Reduce::Avg)
}

pub(crate) fn sumifs(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    multi_if(ctx, args, Reduce::Sum)
}

pub(crate) fn averageifs(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    multi_if(ctx, args, Reduce::Avg)
}

pub(crate) fn minifs(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    multi_if(ctx, args, Reduce::Min)
}

pub(crate) fn maxifs(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    multi_if(ctx, args, Reduce::Max)
}

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
