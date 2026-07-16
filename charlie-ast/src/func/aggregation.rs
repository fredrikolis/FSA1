// Concern: the AGGREGATION worksheet functions (SUM AVERAGE COUNT) — total / arithmetic-mean / count over numeric data, each owning its argument evaluation so the direct-vs-in-range coercion asymmetry (a direct boolean/numeric-text coerces; the same datum inside a range is ignored) is expressible | Non-concern: the registry table + dispatch (func/mod.rs), the shared eval helpers `collect_numbers`/`finite_or_num` (func/helpers.rs), and how a range materializes into cells (eval.rs) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
use super::*;

// Built-ins. The "direct vs in-range" asymmetry is deliberate and Excel-faithful: a boolean/text
// datum coerces when passed *directly* as an argument, but is ignored when it rides *inside* a
// range. Errors propagate for SUM/AVERAGE (aggregation over an error is an error) but are *ignored*
// by COUNT (COUNT never returns an error from its data).
/// `SUM(a, b, …)` — total the numbers. Direct booleans/numeric-text coerce; in-range non-numbers are
/// ignored; any error propagates. Shares [`collect_numbers`]' direct-vs-in-range gathering.
pub(crate) fn sum(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match collect_numbers(ctx, args) {
        Err(k) => Value::Error(k),
        Ok(nums) => finite_or_num(nums.iter().sum()),
    }
}

/// `AVERAGE(a, b, …)` — the arithmetic mean of the numeric data. Empty (no numbers) is `#DIV/0!`.
/// Shares [`collect_numbers`]' direct-vs-in-range gathering.
pub(crate) fn average(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match collect_numbers(ctx, args) {
        Err(k) => Value::Error(k),
        Ok(nums) if nums.is_empty() => Value::Error(ErrKind::Div0),
        Ok(nums) => finite_or_num(nums.iter().sum::<f64>() / nums.len() as f64),
    }
}

/// `COUNT(a, b, …)` — how many data are numbers. Never propagates an error; direct booleans and
/// numeric-text count, in-range non-numbers do not.
pub(crate) fn count(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let mut n: u64 = 0;
    for a in args {
        match ctx.eval(a) {
            Value::Array(_, cells) => {
                for c in &cells {
                    if matches!(c, Value::Number(_)) {
                        n += 1;
                    }
                }
            }
            Value::Number(_) | Value::Bool(_) => n += 1,
            Value::Text(t) => {
                if matches!(t.trim().parse::<f64>(), Ok(x) if x.is_finite()) {
                    n += 1;
                }
            }
            Value::Blank | Value::Error(_) => {}
        }
    }
    Value::Number(n as f64)
}
