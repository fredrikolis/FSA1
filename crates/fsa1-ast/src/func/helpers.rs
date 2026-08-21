// Concern: materializes a call's arguments as numbers, text or blocks | Non-concern: any function's own semantics, the registry table | IO: (&mut EvalCtx, &Expr) -> f64, String or a block

use super::*;

/// [`block`] that BORROWS a bare range instead of copying it: every `*IF`/`*IFS` call reads its
/// ranges to scan them, never to keep them. Anything else falls through to [`block`]. The fast
/// path skips [`EvalCtx::eval`] and so takes no depth guard — a bare range is a leaf, and the
/// two paths yield the same cells.
pub(crate) fn block_cow<'r>(
    ctx: &mut EvalCtx<'r>,
    e: &Expr,
) -> Result<(u32, u32, Cow<'r, [Value]>), ErrKind> {
    if let Some((shape, cells)) = ctx.range_cells(e) {
        return Ok((shape.rows, shape.cols, Cow::Borrowed(cells)));
    }
    let (rows, cols, cells) = block(ctx, e)?;
    Ok((rows, cols, Cow::Owned(cells)))
}

/// A bare scalar presents as a 1x1 block, so a cell, a range and a literal all look alike here.
pub(crate) fn block(ctx: &mut EvalCtx, e: &Expr) -> Result<(u32, u32, Vec<Value>), ErrKind> {
    match ctx.eval(e) {
        Value::Array(shape, cells) => Ok((shape.rows, shape.cols, cells)),
        Value::Error(k) => Err(k),
        other => Ok((1, 1, vec![other])),
    }
}

pub(crate) fn one_num(ctx: &mut EvalCtx, e: &Expr) -> Result<f64, ErrKind> {
    coerce_num(&scalarize(ctx.eval(e)))
}

/// [`coerce_num`] widened by the ONE ISO reader, for a DATE-valued position: text a bare `f64` parse
/// refuses gets read as `yyyy-mm-dd[ hh:mm[:ss]]`, keeping the FRACTIONAL day so a clock survives.
/// Text that is neither still refuses, with the numeric coercion's own error.
pub(crate) fn coerce_date_num(v: &Value) -> Result<f64, ErrKind> {
    match coerce_num(v) {
        Ok(n) => Ok(n),
        Err(k) => match scalarize(v.clone()) {
            Value::Text(t) => parse_datetime_serial(&t).ok_or(k),
            _ => Err(k),
        },
    }
}

/// [`one_num`] for a DATE-valued argument: the same scalarization, read by [`coerce_date_num`].
pub(crate) fn one_date_num(ctx: &mut EvalCtx, e: &Expr) -> Result<f64, ErrKind> {
    coerce_date_num(&scalarize(ctx.eval(e)))
}

/// The ONE home of the aggregators' direct-vs-in-range asymmetry: a DIRECT boolean or numeric text
/// coerces and a direct non-numeric text is `#VALUE!`, while an IN-RANGE non-number is ignored.
pub(crate) fn collect_numbers(ctx: &mut EvalCtx, args: &[Expr]) -> Result<Vec<f64>, ErrKind> {
    let mut nums = Vec::new();
    for a in args {
        match ctx.eval(a) {
            Value::Array(_, cells) => {
                for c in &cells {
                    match c {
                        Value::Error(k) => return Err(*k),
                        Value::Number(n) => nums.push(*n),
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

pub(crate) fn opt_num(
    ctx: &mut EvalCtx,
    args: &[Expr],
    idx: usize,
    default: f64,
) -> Result<f64, ErrKind> {
    match args.get(idx) {
        Some(e) => one_num(ctx, e),
        None => Ok(default),
    }
}

pub(crate) fn opt_bool(
    ctx: &mut EvalCtx,
    args: &[Expr],
    idx: usize,
    default: bool,
) -> Result<bool, ErrKind> {
    match args.get(idx) {
        Some(e) => coerce_bool(&ctx.eval(e)),
        None => Ok(default),
    }
}

pub(crate) fn arg_text(ctx: &mut EvalCtx, e: &Expr) -> Result<String, ErrKind> {
    to_text(&ctx.eval(e))
}

/// Keeps `Value::Number` finite, and canonicalizes a signed zero: `Value`'s `Eq` is bit-exact, so a
/// stray computed `-0.0` (an empty `SUM`, `INT` of a small negative) must fold to `+0.0`.
pub(crate) fn finite_or_num(n: f64) -> Value {
    if n.is_finite() {
        Value::Number(if n == 0.0 { 0.0 } else { n })
    } else {
        Value::Error(ErrKind::Num)
    }
}
