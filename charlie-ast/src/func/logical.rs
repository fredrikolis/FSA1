// Concern: the LOGICAL worksheet functions (IF IFERROR AND OR · IFS NOT IFNA SWITCH) — the lazy/short-circuiting control-flow built-ins, evaluating only the branch/arm a value actually reaches (an unreached `1/0` never surfaces) and pinning each function's error-catching contract (IFERROR catches every error, IFNA only `#N/A`) | Non-concern: the registry table + dispatch (func/mod.rs) and the boolean coercion machinery (eval.rs owns `coerce_bool`) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
use super::*;

/// `IF(cond, then [, else])` — lazily evaluates only the selected branch for a SCALAR condition (so
/// `IF(TRUE, 1, 1/0)` is `1`, never `#DIV/0!`); a two-arg false yields `FALSE` (Excel). When `cond` is
/// a genuinely multi-cell ARRAY, `IF` maps element-wise (the CSE array idiom
/// `IF({1;0;1},{"a";"b";"c"},"")` -> `{"a";"";"c"}`): both branches are evaluated (array `IF` is not
/// lazy) and each cell picks its `then`/`else` element via `array::map_if`, scalar branches
/// broadcasting. A 1×1 condition collapses to its scalar and keeps the lazy path.
pub(crate) fn if_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let cond = collapse_1x1(ctx.eval(&args[0]));
    if let Value::Array(shape, cells) = cond {
        // Array condition: evaluate BOTH branches (a false element needs the else value; a two-arg
        // false element is `FALSE`, as Excel yields) and select element-wise in `array::map_if`.
        let then_v = ctx.eval(&args[1]);
        let else_v = if args.len() == 3 {
            ctx.eval(&args[2])
        } else {
            Value::Bool(false)
        };
        return array::map_if(shape, &cells, &then_v, &else_v);
    }
    match coerce_bool(&cond) {
        Err(k) => Value::Error(k),
        Ok(true) => ctx.eval(&args[1]),
        Ok(false) => {
            if args.len() == 3 {
                ctx.eval(&args[2])
            } else {
                Value::Bool(false)
            }
        }
    }
}

/// `IFERROR(value, fallback)` — the fallback is evaluated only when `value` is an error; a non-error
/// value passes through unchanged (arrays preserved).
pub(crate) fn iferror(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match ctx.eval(&args[0]) {
        Value::Error(_) => ctx.eval(&args[1]),
        v => v,
    }
}

/// `AND(a, …)` — true iff every logical datum is true. `OR` is the dual.
pub(crate) fn and_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    logical_reduce(ctx, args, true)
}

pub(crate) fn or_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    logical_reduce(ctx, args, false)
}

/// Shared reduction for `AND`/`OR`. `is_and` picks the identity and combinator. Booleans and numbers
/// (non-zero = true) contribute; in-range text/blank is ignored; a *direct* non-logical text is
/// `#VALUE!`; a direct blank is ignored; any error propagates. No logical datum at all is `#VALUE!`.
fn logical_reduce(ctx: &mut EvalCtx, args: &[Expr], is_and: bool) -> Value {
    let mut acc = is_and;
    let mut seen = false;
    let combine = |b: bool, acc: &mut bool| {
        *acc = if is_and { *acc && b } else { *acc || b };
    };
    for a in args {
        match ctx.eval(a) {
            Value::Array(_, cells) => {
                for c in &cells {
                    match c {
                        Value::Error(k) => return Value::Error(*k),
                        Value::Bool(b) => {
                            seen = true;
                            combine(*b, &mut acc);
                        }
                        Value::Number(n) => {
                            seen = true;
                            combine(*n != 0.0, &mut acc);
                        }
                        _ => {}
                    }
                }
            }
            Value::Error(k) => return Value::Error(k),
            Value::Blank => {}
            other => match coerce_bool(&other) {
                Err(k) => return Value::Error(k),
                Ok(b) => {
                    seen = true;
                    combine(b, &mut acc);
                }
            },
        }
    }
    if seen {
        Value::Bool(acc)
    } else {
        Value::Error(ErrKind::Value)
    }
}

// Logical batch v1: IFS NOT IFNA SWITCH. (IF/IFERROR/AND/OR are the earlier logical batch above.)
// The Excel-semantics calls pinned here, each worth a reviewer's eye:
//   * IFS is LAZY — tests are evaluated left-to-right and it returns the value paired with the FIRST
//     TRUE test (only THAT value is evaluated, so an unreached `1/0` never surfaces). NO true test is
//     `#N/A`; a test that errors or is non-coercible text propagates. A dangling test with no value
//     (an ODD argument count) is a structural `#VALUE!` — the same "static structure beats runtime
//     guessing" stance the `*IFS` pair-count check takes.
//   * NOT COERCES a non-boolean argument (a non-zero number → TRUE, "TRUE"/"FALSE" text folds, a
//     blank → FALSE) then negates; a non-logical text is `#VALUE!`; an error propagates.
//   * IFNA catches ONLY `#N/A` — every OTHER error (and any normal value/array) passes through
//     unchanged; the fallback is evaluated lazily, only on a genuine `#N/A`. This is the load-bearing
//     distinction from `IFERROR` (which catches every error).
//   * SWITCH matches the expression against each value with Excel `=` equality (numbers numerically,
//     text case-INSENSITIVELY, cross-type never equal) and returns the FIRST match's result (lazy).
//     An optional trailing DEFAULT (an odd tail after the expression) is returned when nothing
//     matches, else `#N/A`. The expression's error — or a compared value's error reached BEFORE a
//     match — propagates.
/// `IFS(test1, value1, test2, value2, …)` — the value paired with the FIRST TRUE test, evaluating
/// only that value (lazy). No TRUE test is `#N/A`; a test that errors or is non-coercible propagates;
/// a dangling test with no value (odd arity) is a structural `#VALUE!`.
pub(crate) fn ifs_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    if !args.len().is_multiple_of(2) {
        return Value::Error(ErrKind::Value);
    }
    for pair in args.chunks_exact(2) {
        match coerce_bool(&ctx.eval(&pair[0])) {
            Err(k) => return Value::Error(k),
            Ok(true) => return ctx.eval(&pair[1]),
            Ok(false) => {}
        }
    }
    Value::Error(ErrKind::Na)
}

/// `NOT(logical)` — the boolean negation of its argument under Excel logical coercion (a non-zero
/// number is TRUE, "TRUE"/"FALSE" text folds, a blank is FALSE); a non-logical text is `#VALUE!` and
/// an error propagates.
pub(crate) fn not_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match coerce_bool(&ctx.eval(&args[0])) {
        Err(k) => Value::Error(k),
        Ok(b) => Value::Bool(!b),
    }
}

/// `IFNA(value, value_if_na)` — the fallback replaces `value` ONLY when it is `#N/A`; every other
/// error and any normal value (arrays preserved) passes through unchanged. The fallback is evaluated
/// lazily, only on a genuine `#N/A`.
pub(crate) fn ifna(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match ctx.eval(&args[0]) {
        Value::Error(ErrKind::Na) => ctx.eval(&args[1]),
        v => v,
    }
}

/// `SWITCH(expression, value1, result1, [value2, result2], …, [default])` — the result paired with
/// the FIRST value equal to `expression` (Excel `=` equality), evaluating only that result (lazy). An
/// optional trailing DEFAULT (an odd tail after the expression) is returned when nothing matches,
/// else `#N/A`. The expression's error — or a compared value's error reached before a match —
/// propagates.
pub(crate) fn switch_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let subject = scalarize(ctx.eval(&args[0]));
    if let Value::Error(k) = subject {
        return Value::Error(k);
    }
    // After the expression, the arguments are (value, result) pairs with an optional lone DEFAULT.
    let rest = &args[1..];
    let it = rest.chunks_exact(2);
    let default = it.remainder();
    for pair in it {
        match value_eq(&subject, &ctx.eval(&pair[0])) {
            Err(k) => return Value::Error(k),
            Ok(true) => return ctx.eval(&pair[1]),
            Ok(false) => {}
        }
    }
    match default {
        [d] => ctx.eval(d),
        _ => Value::Error(ErrKind::Na),
    }
}
