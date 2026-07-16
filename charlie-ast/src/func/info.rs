// Concern: the INFORMATION worksheet functions (ISBLANK ISNUMBER ISTEXT ISERROR NA TYPE) — the ONE error-TRANSPARENT family: each evaluates its argument with a bare `ctx.eval` and matches the RAW `Value` (never `scalarize`/`coerce_*`), so `ISERROR(1/0)` is TRUE and `TYPE(NA())` is 16 rather than propagating | Non-concern: the registry table + dispatch (func/mod.rs) and the coercion machinery every OTHER family routes through (eval.rs) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
use super::*;

// Information family — the ONE error-TRANSPARENT group in the engine. EVERY other built-in routes
// its operands through `scalarize`/`coerce_num`/`coerce_bool`, so an `Error` operand SHORT-CIRCUITS
// to that error and a multi-cell array collapses to a propagated `#VALUE!`. The IS*/`TYPE` predicates
// must do the OPPOSITE: their whole purpose is to REPORT on such an operand, so each evaluates its
// argument with a bare `ctx.eval` and matches the RAW `Value` — never `scalarize`, never `coerce_*`.
// Consequences that a review should hold to exactly: `=ISERROR(1/0)` is TRUE (the `#DIV/0!` is caught,
// not returned); `=TYPE(NA())` is 16 (not `#N/A`); `=ISBLANK(<empty ref>)` is TRUE; and an ARRAY
// operand is inspected AS an array — `TYPE` -> 64, and the IS-predicates each see "not a blank / not a
// number / not text / not an error" and return FALSE — never a scalarize-into-`#VALUE!` propagation.
// `NA()` is the lone error-PRODUCING member: it mints the `#N/A` value on demand.
/// `ISBLANK(value)` — TRUE iff the operand is an empty cell / blank. Error-transparent: an error or a
/// (multi-cell) array operand is simply "not blank" -> FALSE, never propagated.
pub(crate) fn isblank(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    Value::Bool(matches!(ctx.eval(&args[0]), Value::Blank))
}

/// `ISNUMBER(value)` — TRUE iff the operand is a number. Error-transparent (an error operand -> FALSE).
pub(crate) fn isnumber(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    Value::Bool(matches!(ctx.eval(&args[0]), Value::Number(_)))
}

/// `ISTEXT(value)` — TRUE iff the operand is text. Error-transparent.
pub(crate) fn istext(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    Value::Bool(matches!(ctx.eval(&args[0]), Value::Text(_)))
}

/// `ISERROR(value)` — TRUE iff the operand is ANY error value (`#N/A` INCLUDED — cf. `ISERR`, which
/// excludes `#N/A`, deferred). The defining error-transparent case: the operand's error is CAUGHT and
/// reported, never propagated, so `=ISERROR(1/0)` is TRUE rather than `#DIV/0!`.
pub(crate) fn iserror(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    Value::Bool(matches!(ctx.eval(&args[0]), Value::Error(_)))
}

/// `NA()` — the `#N/A` ("value not available") error, produced on demand. The one error-PRODUCING
/// member of the information family; arity 0 (checked at parse).
pub(crate) fn na_fn(_ctx: &mut EvalCtx, _args: &[Expr]) -> Value {
    Value::Error(ErrKind::Na)
}

/// `TYPE(value)` — the operand's Excel type code: 1 number, 2 text, 4 logical, 16 error, 64 array. An
/// empty cell reports as a number (1), matching Excel (a blank is a numeric zero in value context).
/// Error-transparent: an error operand reports 16, and an array operand reports 64 — neither is
/// propagated.
pub(crate) fn type_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let code = match ctx.eval(&args[0]) {
        Value::Number(_) | Value::Blank => 1.0,
        Value::Text(_) => 2.0,
        Value::Bool(_) => 4.0,
        Value::Error(_) => 16.0,
        Value::Array(..) => 64.0,
    };
    Value::Number(code)
}
