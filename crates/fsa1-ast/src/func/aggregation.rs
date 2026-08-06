// Concern: SUM AVERAGE COUNT over a call's numeric data | Non-concern: the coercion asymmetry (helpers owns it), the criteria forms | IO: (&mut EvalCtx, &[Expr]) -> Value

use super::*;

pub(crate) fn sum(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match collect_numbers(ctx, args) {
        Err(k) => Value::Error(k),
        Ok(nums) => finite_or_num(nums.iter().sum()),
    }
}

pub(crate) fn average(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match collect_numbers(ctx, args) {
        Err(k) => Value::Error(k),
        Ok(nums) if nums.is_empty() => Value::Error(ErrKind::Div0),
        Ok(nums) => finite_or_num(nums.iter().sum::<f64>() / nums.len() as f64),
    }
}

/// Its own loop, not [`collect_numbers`]: COUNT IGNORES an error datum rather than propagating it.
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
