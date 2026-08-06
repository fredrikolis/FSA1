// Concern: IF IFS NOT IFERROR IFNA SWITCH AND OR XOR and the TRUE/FALSE literals | Non-concern: the element-wise mapping (func::array owns it) | IO: (&mut EvalCtx, &[Expr]) -> Value

use super::*;

/// A SCALAR condition takes the LAZY path, so `IF(TRUE, 1, 1/0)` never evaluates the division; an
/// ARRAY condition must evaluate both branches before mapping.
pub(crate) fn if_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let cond = collapse_1x1(ctx.eval(&args[0]));
    if let Value::Array(shape, cells) = cond {
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

pub(crate) fn iferror(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    catch_errors(ctx, args, is_any_error)
}

fn is_any_error(v: &Value) -> bool {
    matches!(v, Value::Error(_))
}

fn is_na(v: &Value) -> bool {
    matches!(v, Value::Error(ErrKind::Na))
}

/// An ARRAY value is caught PER CELL, not whole; the fallback stays lazy either way.
fn catch_errors(ctx: &mut EvalCtx, args: &[Expr], caught: fn(&Value) -> bool) -> Value {
    match ctx.eval(&args[0]) {
        Value::Array(shape, cells) => {
            if !cells.iter().any(caught) {
                return Value::Array(shape, cells);
            }
            let fallback = ctx.eval(&args[1]);
            array::map_catch(shape, &cells, &fallback, caught)
        }
        v if caught(&v) => ctx.eval(&args[1]),
        v => v,
    }
}

pub(crate) fn and_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    logical_reduce(ctx, args, LogicalReduce::And)
}

pub(crate) fn or_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    logical_reduce(ctx, args, LogicalReduce::Or)
}

pub(crate) fn xor_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    logical_reduce(ctx, args, LogicalReduce::Xor)
}

#[derive(Clone, Copy)]
enum LogicalReduce {
    And,
    Or,
    Xor,
}

/// The direct-vs-in-range asymmetry again: in-range text and blanks are ignored, but a DIRECT
/// non-logical text is `#VALUE!`. No logical datum at all is `#VALUE!` too.
fn logical_reduce(ctx: &mut EvalCtx, args: &[Expr], op: LogicalReduce) -> Value {
    let mut acc = matches!(op, LogicalReduce::And);
    let mut seen = false;
    let combine = |b: bool, acc: &mut bool| {
        *acc = match op {
            LogicalReduce::And => *acc && b,
            LogicalReduce::Or => *acc || b,
            LogicalReduce::Xor => *acc ^ b,
        };
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

/// A dangling test with no value (odd arity) is a structural `#VALUE!`, never a runtime guess.
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

pub(crate) fn not_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match coerce_bool(&ctx.eval(&args[0])) {
        Err(k) => Value::Error(k),
        Ok(b) => Value::Bool(!b),
    }
}

/// Catches ONLY `#N/A` — the load-bearing difference from [`iferror`], which catches every error.
pub(crate) fn ifna(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    catch_errors(ctx, args, is_na)
}

/// The zero-arg CALL form; the bare word `TRUE` is a literal the lexer handles.
pub(crate) fn true_fn(_ctx: &mut EvalCtx, _args: &[Expr]) -> Value {
    Value::Bool(true)
}

pub(crate) fn false_fn(_ctx: &mut EvalCtx, _args: &[Expr]) -> Value {
    Value::Bool(false)
}

/// An optional trailing DEFAULT (an odd tail after the expression) answers when nothing matches.
pub(crate) fn switch_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let subject = scalarize(ctx.eval(&args[0]));
    if let Value::Error(k) = subject {
        return Value::Error(k);
    }
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
