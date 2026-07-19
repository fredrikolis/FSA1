// Concern: the SHARED eval helpers reused by multiple function families — materializing an argument to a rectangular `block`, coercing a single argument to a number (`one_num`) or text (`arg_text`), reading an OPTIONAL numeric/boolean argument with a default (`opt_num`/`opt_bool`), gathering numbers under the direct-vs-in-range rule (`collect_numbers`), and the finite/overflow guard on a numeric result (`finite_or_num`) | Non-concern: the family function bodies that call these (func/<family>.rs) and the registry table + dispatch (func/mod.rs) | IO: (`EvalCtx`, an `Expr`/`Value`) -> an intermediate `Result`/`Value`
use super::*;

/// Evaluate an argument to a rectangular block: `(rows, cols, cells)`. A bare scalar is a `1×1`
/// block; an error value propagates (`Err`). This is the one materialization the family shares, so a
/// single cell, a range, or a literal all present the same shape/cell view to the conformance check.
pub(crate) fn block(ctx: &mut EvalCtx, e: &Expr) -> Result<(u32, u32, Vec<Value>), ErrKind> {
    match ctx.eval(e) {
        Value::Array(shape, cells) => Ok((shape.rows, shape.cols, cells)),
        Value::Error(k) => Err(k),
        other => Ok((1, 1, vec![other])),
    }
}

/// Evaluate one argument to a scalar number (Excel arithmetic coercion), or its propagated error.
pub(crate) fn one_num(ctx: &mut EvalCtx, e: &Expr) -> Result<f64, ErrKind> {
    coerce_num(&scalarize(ctx.eval(e)))
}

/// Gather the numeric data under SUM's direct-vs-in-range asymmetry: a direct boolean/numeric-text
/// coerces, an in-range non-number is ignored, a direct non-numeric text is `#VALUE!`, and any error
/// propagates (`Err`). The single materialization every plain numeric aggregator shares —
/// SUM/AVERAGE/PRODUCT and MIN/MAX/MEDIAN — so the coercion asymmetry lives in ONE place. (COUNT
/// differs: it ignores errors rather than propagating, so it stays a separate loop.)
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

/// An OPTIONAL numeric argument at `idx`, or `default` when the call omits it. An error propagates.
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

/// An OPTIONAL boolean flag argument at `idx`, or `default` when the call omits it. An error
/// propagates.
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

/// Coerce one argument to its Excel text form (general number format, `TRUE`/`FALSE`, `""` for blank),
/// propagating an error and rejecting a multi-cell array (`#VALUE!`). The shared front-door every text
/// function uses for a text-typed argument.
pub(crate) fn arg_text(ctx: &mut EvalCtx, e: &Expr) -> Result<String, ErrKind> {
    to_text(&ctx.eval(e))
}

/// Wrap a computed number, demoting a non-finite result (overflow) to `#NUM!` so a `Value::Number`
/// is always finite in the arithmetic domain (mirrors the lexer/`coerce_num` finiteness invariant),
/// and canonicalizing a signed zero to `+0.0`. Excel displays every zero as `0`, but `Value`'s `Eq`
/// is bit-exact (`-0.0 ≠ 0.0`), so a stray computed `-0.0` — an empty `SUM`/`SUMPRODUCT` aggregate
/// (`[].sum() == -0.0`), or `ROUNDDOWN`/`INT` of a small negative — must fold to `+0.0` or it would
/// spuriously Diverge from a `0`-expecting oracle.
pub(crate) fn finite_or_num(n: f64) -> Value {
    if n.is_finite() {
        // `n == 0.0` is true for BOTH `+0.0` and `-0.0` (IEEE), so this canonicalizes the sign.
        Value::Number(if n == 0.0 { 0.0 } else { n })
    } else {
        Value::Error(ErrKind::Num)
    }
}
