// Concern: the VOLATILE random worksheet functions (RAND RANDBETWEEN) — the two built-ins whose value changes between evaluations of the same tree, reading their entropy through the engine's ONE injectable randomness seam (`EvalCtx::rand_unit` → `Resolver::rand_unit`, the randomness analogue of the `now_serial` clock) so a deterministic resolver can pin the stream; RAND returns a draw in [0,1) directly, RANDBETWEEN maps a draw onto the inclusive integer band [⌈bottom⌉, ⌊top⌋] and refuses an empty band with `#NUM!` | Non-concern: WHERE the entropy comes from (resolver.rs owns the default `system_rand_unit` and the pin seam), the non-volatile math functions (func/math.rs, func/trig.rs, func/combinatorics.rs), and the registry table + dispatch (func/mod.rs — these two rows carry `volatile: true`) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
use super::*;

/// `RAND()` — a uniformly-distributed random number in the half-open interval `[0, 1)`. VOLATILE: it
/// reads the resolver's injectable [`crate::Resolver::rand_unit`] seam, so two evaluations diverge
/// (and a deterministic resolver can pin the sequence).
pub(crate) fn rand_fn(ctx: &mut EvalCtx, _args: &[Expr]) -> Value {
    Value::Number(ctx.rand_unit())
}

/// `RANDBETWEEN(bottom, top)` — a random INTEGER in the inclusive band `[⌈bottom⌉, ⌊top⌋]`. Rounding
/// `bottom` up and `top` down keeps every result within the literal `[bottom, top]` interval even for
/// non-integer bounds; an empty band (`⌈bottom⌉ > ⌊top⌋`) is `#NUM!`. VOLATILE (reads the same seam
/// as [`rand_fn`]).
///
/// Accept-under-uncertainty (a documented divergence outside the parity corpus — RANDBETWEEN is
/// volatile, so it is uncorpusable): Excel's exact handling of NON-INTEGER bounds is undocumented and
/// may differ from this `⌈bottom⌉..⌊top⌋` choice. The choice is defensible — every draw stays inside
/// the literal `[bottom, top]` interval — so it is noted rather than second-guessed against an
/// unknown reference.
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
    // `rand_unit()` is in [0, 1), so `⌊u · span⌋` is in `0..=span-1` — the offset never reaches `top +
    // 1`. `span` is computed as an f64; for the spreadsheet-scale bands RANDBETWEEN is used with it is
    // exact.
    let span = top - bottom + 1.0;
    let offset = (ctx.rand_unit() * span).floor();
    finite_or_num(bottom + offset)
}
