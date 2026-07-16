// Concern: the FUNCTION REGISTRY as data — the `FuncDef` record (`name`, arity bounds, an `eval` fn-pointer), the flat `FUNCS` table indexed by `FuncId`, name->`FuncId` lookup (case-insensitive, for the parser) and `FuncId`->`FuncDef` dispatch (for the evaluator), plus a FEW foundational built-ins across categories (SUM AVERAGE COUNT · IF IFERROR AND OR · ABS ROUND) proving the mechanism end-to-end; each built-in owns its own argument evaluation so lazy forms (IF/IFERROR) and the direct-vs-in-range coercion asymmetry are expressible | Non-concern: the ~70-function grind (criteria/lookup/spill land in W3b) and the operator/coercion machinery (eval.rs owns `coerce_num`/`coerce_bool`/`scalarize`, which the built-ins reuse) | IO: none — a static dispatch table over the `EvalCtx`/`Value` contract
//! The function registry: [`FuncDef`], the [`FUNCS`] table, [`lookup`], [`def`], [`dispatch`].
//!
//! Registry-as-data (ast-standards PART 7, "one engine, N behaviors as data"): a function is a row,
//! not a hand-forked code path. The parser resolves a name to a [`crate::FuncId`] and checks arity
//! against the row (so eval trusts the arity — DbC); the evaluator dispatches the row's `eval`. The
//! v1 set here is deliberately small — enough to prove aggregation, laziness, error-catching, logic,
//! and pure-math all route through the same table.

use crate::eval::{EvalCtx, coerce_bool, coerce_num, scalarize};
use crate::expr::{Expr, FuncId};
use crate::value::{ErrKind, Value};

/// One registry row. `min_args`/`max_args` bound the arity (`max_args = None` is unbounded/variadic);
/// `eval` receives the *unevaluated* argument `Expr`s and the [`EvalCtx`], so a function chooses what
/// to evaluate (lazy `IF`/`IFERROR`) and how to treat a datum by whether it arrived direct or inside
/// a range.
pub struct FuncDef {
    pub name: &'static str,
    pub min_args: usize,
    pub max_args: Option<usize>,
    pub eval: fn(&mut EvalCtx, &[Expr]) -> Value,
}

impl FuncDef {
    /// Whether `n` arguments satisfy this function's arity bounds.
    pub fn arity_ok(&self, n: usize) -> bool {
        n >= self.min_args && self.max_args.is_none_or(|max| n <= max)
    }
}

/// The registry, indexed by [`FuncId`]`.0`. Order is the id assignment — appending is safe, but a
/// row's position is its stable id, so never reorder (a `self_consistency` test pins name↔index).
pub static FUNCS: &[FuncDef] = &[
    FuncDef {
        name: "SUM",
        min_args: 1,
        max_args: None,
        eval: sum,
    },
    FuncDef {
        name: "AVERAGE",
        min_args: 1,
        max_args: None,
        eval: average,
    },
    FuncDef {
        name: "COUNT",
        min_args: 1,
        max_args: None,
        eval: count,
    },
    FuncDef {
        name: "IF",
        min_args: 2,
        max_args: Some(3),
        eval: if_fn,
    },
    FuncDef {
        name: "IFERROR",
        min_args: 2,
        max_args: Some(2),
        eval: iferror,
    },
    FuncDef {
        name: "AND",
        min_args: 1,
        max_args: None,
        eval: and_fn,
    },
    FuncDef {
        name: "OR",
        min_args: 1,
        max_args: None,
        eval: or_fn,
    },
    FuncDef {
        name: "ABS",
        min_args: 1,
        max_args: Some(1),
        eval: abs_fn,
    },
    FuncDef {
        name: "ROUND",
        min_args: 2,
        max_args: Some(2),
        eval: round_fn,
    },
];

/// Resolve a function name (case-insensitive — Excel names fold case) to its [`FuncId`], or `None`
/// if unknown. The parser turns `None` into an `unknown-function` located refusal.
pub fn lookup(name: &str) -> Option<FuncId> {
    FUNCS
        .iter()
        .position(|f| f.name.eq_ignore_ascii_case(name))
        .map(|i| FuncId(i as u32))
}

/// The registry row for a [`FuncId`], or `None` if the id is out of range (only possible for a
/// hand-synthesized `Call` — the parser never mints an out-of-range id).
pub fn def(id: FuncId) -> Option<&'static FuncDef> {
    FUNCS.get(id.0 as usize)
}

/// Evaluate a `Call`. Two synthesized-only faults are turned into first-class errors rather than a
/// panic (the parser never mints either — it gates the id via `lookup` and the arity via
/// `BadArity` — but `eval` is a total public API over *any* `Expr`, so it must defend a
/// hand-built tree): an out-of-range id is `#NAME?`, and an off-arity call is `#VALUE!`. The arity
/// gate is essential because the positional built-ins (`IF`/`IFERROR`/`ABS`/`ROUND`) index `args`
/// directly and would otherwise panic on an under-arity `Call` — mirroring the bad-id guard so eval
/// stays panic-free, as [`crate::eval`]'s contract promises.
pub fn dispatch(id: FuncId, ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match def(id) {
        Some(f) if f.arity_ok(args.len()) => (f.eval)(ctx, args),
        Some(_) => Value::Error(ErrKind::Value),
        None => Value::Error(ErrKind::Name),
    }
}

// ---------------------------------------------------------------------------------------------
// Built-ins. The "direct vs in-range" asymmetry is deliberate and Excel-faithful: a boolean/text
// datum coerces when passed *directly* as an argument, but is ignored when it rides *inside* a
// range. Errors propagate for SUM/AVERAGE (aggregation over an error is an error) but are *ignored*
// by COUNT (COUNT never returns an error from its data).
// ---------------------------------------------------------------------------------------------

/// `SUM(a, b, …)` — total the numbers. Direct booleans/numeric-text coerce; in-range non-numbers are
/// ignored; any error propagates.
fn sum(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let mut acc = 0.0;
    for a in args {
        match ctx.eval(a) {
            Value::Array(_, cells) => {
                for c in &cells {
                    match c {
                        Value::Error(k) => return Value::Error(*k),
                        Value::Number(n) => acc += n,
                        _ => {}
                    }
                }
            }
            Value::Error(k) => return Value::Error(k),
            Value::Number(n) => acc += n,
            Value::Bool(b) => acc += if b { 1.0 } else { 0.0 },
            Value::Blank => {}
            Value::Text(t) => match t.trim().parse::<f64>() {
                Ok(n) if n.is_finite() => acc += n,
                _ => return Value::Error(ErrKind::Value),
            },
        }
    }
    finite_or_num(acc)
}

/// `AVERAGE(a, b, …)` — the arithmetic mean of the numeric data. Empty (no numbers) is `#DIV/0!`.
fn average(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let mut sum = 0.0;
    let mut count: u64 = 0;
    for a in args {
        match ctx.eval(a) {
            Value::Array(_, cells) => {
                for c in &cells {
                    match c {
                        Value::Error(k) => return Value::Error(*k),
                        Value::Number(n) => {
                            sum += n;
                            count += 1;
                        }
                        _ => {}
                    }
                }
            }
            Value::Error(k) => return Value::Error(k),
            Value::Number(n) => {
                sum += n;
                count += 1;
            }
            Value::Bool(b) => {
                sum += if b { 1.0 } else { 0.0 };
                count += 1;
            }
            Value::Blank => {}
            Value::Text(t) => match t.trim().parse::<f64>() {
                Ok(n) if n.is_finite() => {
                    sum += n;
                    count += 1;
                }
                _ => return Value::Error(ErrKind::Value),
            },
        }
    }
    if count == 0 {
        Value::Error(ErrKind::Div0)
    } else {
        finite_or_num(sum / count as f64)
    }
}

/// `COUNT(a, b, …)` — how many data are numbers. Never propagates an error; direct booleans and
/// numeric-text count, in-range non-numbers do not.
fn count(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
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

/// `IF(cond, then [, else])` — lazily evaluates only the selected branch (so `IF(TRUE, 1, 1/0)` is
/// `1`, never `#DIV/0!`). A two-arg false yields `FALSE` (Excel).
fn if_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let cond = ctx.eval(&args[0]);
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
fn iferror(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match ctx.eval(&args[0]) {
        Value::Error(_) => ctx.eval(&args[1]),
        v => v,
    }
}

/// `AND(a, …)` — true iff every logical datum is true. `OR` is the dual.
fn and_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    logical_reduce(ctx, args, true)
}

fn or_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
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

/// `ABS(x)` — magnitude. Coerces its scalar argument; propagates an error.
fn abs_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match coerce_num(&scalarize(ctx.eval(&args[0]))) {
        Err(k) => Value::Error(k),
        Ok(n) => Value::Number(n.abs()),
    }
}

/// `ROUND(x, digits)` — round to `digits` decimal places, ties away from zero (Excel). Negative
/// `digits` rounds to the left of the decimal point.
fn round_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let n = match coerce_num(&scalarize(ctx.eval(&args[0]))) {
        Ok(x) => x,
        Err(k) => return Value::Error(k),
    };
    let d = match coerce_num(&scalarize(ctx.eval(&args[1]))) {
        Ok(x) => x,
        Err(k) => return Value::Error(k),
    };
    // Excel truncates the digit count toward zero and clamps the exponent to a sane band.
    let digits = d.trunc().clamp(-308.0, 308.0) as i32;
    let factor = 10f64.powi(digits);
    // `f64::round` is already round-half-away-from-zero, matching Excel's ROUND tie rule.
    finite_or_num((n * factor).round() / factor)
}

/// Wrap a computed number, demoting a non-finite result (overflow) to `#NUM!` so a `Value::Number`
/// is always finite in the arithmetic domain (mirrors the lexer/`coerce_num` finiteness invariant).
fn finite_or_num(n: f64) -> Value {
    if n.is_finite() {
        Value::Number(n)
    } else {
        Value::Error(ErrKind::Num)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::eval;
    use crate::refs::{CellRef, RangeRef};
    use crate::test_support::Grid;

    fn num(n: f64) -> Expr {
        Expr::Lit(Value::Number(n))
    }
    fn call(name: &str, args: Vec<Expr>) -> Expr {
        Expr::Call(lookup(name).expect("known function"), args)
    }
    /// A full-column range A1:A{rows} over a 1-wide grid (contiguous in the stub).
    fn col_range(rows: u32) -> Expr {
        Expr::Range(RangeRef {
            start: CellRef {
                col: 0,
                row: 0,
                sheet: None,
            },
            end: CellRef {
                col: 0,
                row: rows - 1,
                sheet: None,
            },
        })
    }

    #[test]
    fn registry_is_self_consistent() {
        // Names unique (case-insensitively), index == FuncId, arity bounds well-formed.
        for (i, f) in FUNCS.iter().enumerate() {
            assert_eq!(
                lookup(f.name),
                Some(FuncId(i as u32)),
                "name maps to its index"
            );
            assert_eq!(def(FuncId(i as u32)).unwrap().name, f.name);
            if let Some(max) = f.max_args {
                assert!(max >= f.min_args, "{}: max >= min", f.name);
            }
            assert!(
                f.name.chars().all(|c| c.is_ascii_uppercase()),
                "UPPERCASE name"
            );
        }
        let mut names: Vec<String> = FUNCS.iter().map(|f| f.name.to_ascii_uppercase()).collect();
        let before = names.len();
        names.sort();
        names.dedup();
        assert_eq!(before, names.len(), "function names are unique");
        // case-insensitive lookup
        assert_eq!(lookup("sum"), lookup("SUM"));
        assert_eq!(lookup("NoSuchFn"), None);
    }

    #[test]
    fn sum_average_count_over_a_range_with_mixed_cells() {
        // A1..A5 = 1, "x"(text), TRUE(bool), <blank>, 4  -> numbers are {1, 4}
        let g = Grid::new(
            1,
            vec![
                Value::Number(1.0),
                Value::Text("x".into()),
                Value::Bool(true),
                Value::Blank,
                Value::Number(4.0),
            ],
        );
        assert_eq!(
            eval(&call("SUM", vec![col_range(5)]), &g),
            Value::Number(5.0)
        );
        assert_eq!(
            eval(&call("AVERAGE", vec![col_range(5)]), &g),
            Value::Number(2.5)
        );
        // COUNT counts only the two numbers (in-range bool/text ignored).
        assert_eq!(
            eval(&call("COUNT", vec![col_range(5)]), &g),
            Value::Number(2.0)
        );
    }

    #[test]
    fn direct_vs_in_range_coercion_asymmetry() {
        let g = Grid::new(1, vec![Value::Blank]);
        // Direct booleans/numeric-text coerce and count.
        assert_eq!(
            eval(
                &call(
                    "SUM",
                    vec![
                        num(1.0),
                        Expr::Lit(Value::Bool(true)),
                        Expr::Lit(Value::Text("2".into()))
                    ]
                ),
                &g
            ),
            Value::Number(4.0)
        );
        assert_eq!(
            eval(
                &call(
                    "COUNT",
                    vec![
                        Expr::Lit(Value::Bool(true)),
                        Expr::Lit(Value::Text("3".into()))
                    ]
                ),
                &g
            ),
            Value::Number(2.0)
        );
        // A direct non-numeric text is #VALUE! for SUM.
        assert_eq!(
            eval(&call("SUM", vec![Expr::Lit(Value::Text("x".into()))]), &g),
            Value::Error(ErrKind::Value)
        );
    }

    #[test]
    fn sum_propagates_but_count_ignores_errors() {
        let g = Grid::new(
            1,
            vec![
                Value::Number(1.0),
                Value::Error(ErrKind::Div0),
                Value::Number(2.0),
            ],
        );
        assert_eq!(
            eval(&call("SUM", vec![col_range(3)]), &g),
            Value::Error(ErrKind::Div0)
        );
        // COUNT never returns an error from its data.
        assert_eq!(
            eval(&call("COUNT", vec![col_range(3)]), &g),
            Value::Number(2.0)
        );
    }

    #[test]
    fn if_is_lazy_and_iferror_catches() {
        let g = Grid::new(1, vec![Value::Blank]);
        // IF(TRUE, 1, 1/0) -> 1 (else branch not evaluated).
        let div0 = Expr::Binary(
            crate::expr::BinOp::Div,
            Box::new(num(1.0)),
            Box::new(num(0.0)),
        );
        let e = call(
            "IF",
            vec![Expr::Lit(Value::Bool(true)), num(1.0), div0.clone()],
        );
        assert_eq!(eval(&e, &g), Value::Number(1.0));
        // Two-arg false -> FALSE.
        let e = call("IF", vec![Expr::Lit(Value::Bool(false)), num(1.0)]);
        assert_eq!(eval(&e, &g), Value::Bool(false));
        // IFERROR(1/0, 99) -> 99.
        let e = call("IFERROR", vec![div0, num(99.0)]);
        assert_eq!(eval(&e, &g), Value::Number(99.0));
        // IFERROR passes a non-error through.
        let e = call("IFERROR", vec![num(7.0), num(99.0)]);
        assert_eq!(eval(&e, &g), Value::Number(7.0));
    }

    #[test]
    fn and_or_semantics() {
        let g = Grid::new(1, vec![Value::Blank]);
        assert_eq!(
            eval(
                &call("AND", vec![Expr::Lit(Value::Bool(true)), num(1.0)]),
                &g
            ),
            Value::Bool(true)
        );
        assert_eq!(
            eval(
                &call("AND", vec![Expr::Lit(Value::Bool(true)), num(0.0)]),
                &g
            ),
            Value::Bool(false)
        );
        assert_eq!(
            eval(
                &call(
                    "OR",
                    vec![num(0.0), Expr::Lit(Value::Bool(false)), num(1.0)]
                ),
                &g
            ),
            Value::Bool(true)
        );
        // error propagates
        assert_eq!(
            eval(
                &call(
                    "AND",
                    vec![
                        Expr::Lit(Value::Error(ErrKind::Ref)),
                        Expr::Lit(Value::Bool(true))
                    ]
                ),
                &g
            ),
            Value::Error(ErrKind::Ref)
        );
    }

    #[test]
    fn abs_and_round() {
        let g = Grid::new(1, vec![Value::Blank]);
        assert_eq!(eval(&call("ABS", vec![num(-5.0)]), &g), Value::Number(5.0));
        assert_eq!(
            eval(&call("ROUND", vec![num(1.2345), num(2.0)]), &g),
            Value::Number(1.23)
        );
        // ties away from zero
        assert_eq!(
            eval(&call("ROUND", vec![num(2.5), num(0.0)]), &g),
            Value::Number(3.0)
        );
        assert_eq!(
            eval(&call("ROUND", vec![num(-2.5), num(0.0)]), &g),
            Value::Number(-3.0)
        );
        // negative digits round left of the point
        assert_eq!(
            eval(&call("ROUND", vec![num(1234.0), num(-2.0)]), &g),
            Value::Number(1200.0)
        );
    }

    #[test]
    fn dispatch_guards_synthesized_off_arity_and_bad_id_without_panicking() {
        // A synthesized off-arity Call (the parser would refuse these via BadArity) must NOT panic
        // the positional built-ins — dispatch's arity gate turns each into #VALUE!.
        let g = Grid::new(1, vec![Value::Blank]);
        // IF/IFERROR/ROUND handed too few args; ABS handed too many.
        assert_eq!(eval(&call("IF", vec![]), &g), Value::Error(ErrKind::Value));
        assert_eq!(
            eval(&call("IFERROR", vec![num(1.0)]), &g),
            Value::Error(ErrKind::Value)
        );
        assert_eq!(
            eval(&call("ROUND", vec![num(1.0)]), &g),
            Value::Error(ErrKind::Value)
        );
        assert_eq!(
            eval(&call("ABS", vec![num(1.0), num(2.0)]), &g),
            Value::Error(ErrKind::Value)
        );
        // An out-of-range (synthesized) FuncId stays #NAME? — the sibling guard.
        assert_eq!(
            eval(&Expr::Call(FuncId(9999), vec![]), &g),
            Value::Error(ErrKind::Name)
        );
    }

    #[test]
    fn arity_bounds() {
        let sum = def(lookup("SUM").unwrap()).unwrap();
        assert!(!sum.arity_ok(0));
        assert!(sum.arity_ok(1) && sum.arity_ok(99));
        let iff = def(lookup("IF").unwrap()).unwrap();
        assert!(!iff.arity_ok(1) && iff.arity_ok(2) && iff.arity_ok(3) && !iff.arity_ok(4));
    }
}
