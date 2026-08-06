// Concern: RAND and RANDBETWEEN over the resolver's entropy seam | Non-concern: the entropy source (the Resolver seam owns it) | IO: (&mut EvalCtx, &[Expr]) -> Value

use super::*;

pub(crate) fn rand_fn(ctx: &mut EvalCtx, _args: &[Expr]) -> Value {
    Value::Number(ctx.rand_unit())
}

/// `ceil`/`floor` keeps every draw inside the literal `[bottom, top]` band, even for non-integer
/// bounds; an empty band is `#NUM!`.
pub(crate) fn randbetween_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let bottom = match one_num(ctx, &args[0]) {
        Ok(v) => v.ceil(),
        Err(k) => return Value::Error(k),
    };
    let top = match one_num(ctx, &args[1]) {
        Ok(v) => v.floor(),
        Err(k) => return Value::Error(k),
    };
    if bottom > top {
        return Value::Error(ErrKind::Num);
    }
    // `rand_unit()` is in [0, 1), so the floored offset is in `0..=span-1` and never reaches `top + 1`.
    let span = top - bottom + 1.0;
    let offset = (ctx.rand_unit() * span).floor();
    finite_or_num(bottom + offset)
}
