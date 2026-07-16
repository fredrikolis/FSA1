// Concern: the FUNCTION REGISTRY as data — the `FuncDef` record (`name`, arity bounds, an `eval` fn-pointer), the flat `FUNCS` table indexed by `FuncId`, name->`FuncId` lookup (case-insensitive, for the parser) and `FuncId`->`FuncDef` dispatch (for the evaluator), plus the built-ins landed so far across categories (aggregation SUM AVERAGE COUNT · logical IF IFERROR AND OR IFS NOT IFNA SWITCH · the `*IF(S)` criteria family SUMIF SUMIFS COUNTIF COUNTIFS AVERAGEIF AVERAGEIFS MINIFS MAXIFS · math ABS ROUND PRODUCT SUMPRODUCT ROUNDUP ROUNDDOWN INT MOD POWER SQRT CEILING FLOOR · stats MIN MAX MEDIAN RANK COUNTA COUNTBLANK); each built-in owns its own argument evaluation so lazy forms (IF/IFERROR), the direct-vs-in-range coercion asymmetry, and range-conformance checks are expressible | Non-concern: the remaining ~70-function grind (lookup/spill land later), the CRITERIA mini-language the `*IF(S)` built-ins depend on (criteria.rs owns `Criterion`/`parse_criterion` — the "does this cell match this criterion" grammar), and the operator/coercion machinery (eval.rs owns `coerce_num`/`coerce_bool`/`scalarize`/`pow`, which the built-ins reuse) | IO: none — a static dispatch table over the `EvalCtx`/`Value` contract
//! The function registry: [`FuncDef`], the [`FUNCS`] table, [`lookup`], [`def`], [`dispatch`].
//!
//! Registry-as-data (ast-standards PART 7, "one engine, N behaviors as data"): a function is a row,
//! not a hand-forked code path. The parser resolves a name to a [`crate::FuncId`] and checks arity
//! against the row (so eval trusts the arity — DbC); the evaluator dispatches the row's `eval`. The
//! v1 set here is deliberately small — enough to prove aggregation, laziness, error-catching, logic,
//! and pure-math all route through the same table.

use crate::criteria::{Criterion, parse_criterion};
use crate::eval::{EvalCtx, coerce_bool, coerce_num, pow, scalarize, value_eq};
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
    // --- Criteria-aggregation family (the `*IF(S)` reporting workhorse) ---
    FuncDef {
        name: "SUMIF",
        min_args: 2,
        max_args: Some(3),
        eval: sumif,
    },
    FuncDef {
        name: "SUMIFS",
        min_args: 3,
        max_args: None,
        eval: sumifs,
    },
    FuncDef {
        name: "COUNTIF",
        min_args: 2,
        max_args: Some(2),
        eval: countif,
    },
    FuncDef {
        name: "COUNTIFS",
        min_args: 2,
        max_args: None,
        eval: countifs,
    },
    FuncDef {
        name: "AVERAGEIF",
        min_args: 2,
        max_args: Some(3),
        eval: averageif,
    },
    FuncDef {
        name: "AVERAGEIFS",
        min_args: 3,
        max_args: None,
        eval: averageifs,
    },
    FuncDef {
        name: "MINIFS",
        min_args: 3,
        max_args: None,
        eval: minifs,
    },
    FuncDef {
        name: "MAXIFS",
        min_args: 3,
        max_args: None,
        eval: maxifs,
    },
    // --- Pure scalar / vector math (the v1 math batch) ---
    FuncDef {
        name: "PRODUCT",
        min_args: 1,
        max_args: None,
        eval: product,
    },
    FuncDef {
        name: "SUMPRODUCT",
        min_args: 1,
        max_args: None,
        eval: sumproduct,
    },
    FuncDef {
        name: "ROUNDUP",
        min_args: 2,
        max_args: Some(2),
        eval: roundup,
    },
    FuncDef {
        name: "ROUNDDOWN",
        min_args: 2,
        max_args: Some(2),
        eval: rounddown,
    },
    FuncDef {
        name: "INT",
        min_args: 1,
        max_args: Some(1),
        eval: int_fn,
    },
    FuncDef {
        name: "MOD",
        min_args: 2,
        max_args: Some(2),
        eval: mod_fn,
    },
    FuncDef {
        name: "POWER",
        min_args: 2,
        max_args: Some(2),
        eval: power_fn,
    },
    FuncDef {
        name: "SQRT",
        min_args: 1,
        max_args: Some(1),
        eval: sqrt_fn,
    },
    FuncDef {
        name: "CEILING",
        min_args: 2,
        max_args: Some(2),
        eval: ceiling_fn,
    },
    FuncDef {
        name: "FLOOR",
        min_args: 2,
        max_args: Some(2),
        eval: floor_fn,
    },
    // --- Statistical extremes / order / counting (the v1 stats batch) ---
    FuncDef {
        name: "MIN",
        min_args: 1,
        max_args: None,
        eval: min_fn,
    },
    FuncDef {
        name: "MAX",
        min_args: 1,
        max_args: None,
        eval: max_fn,
    },
    FuncDef {
        name: "MEDIAN",
        min_args: 1,
        max_args: None,
        eval: median_fn,
    },
    FuncDef {
        name: "RANK",
        min_args: 2,
        max_args: Some(3),
        eval: rank_fn,
    },
    FuncDef {
        name: "COUNTA",
        min_args: 1,
        max_args: None,
        eval: counta,
    },
    FuncDef {
        name: "COUNTBLANK",
        min_args: 1,
        max_args: Some(1),
        eval: countblank,
    },
    // --- Logical batch v1: IFS NOT IFNA SWITCH (IF/IFERROR/AND/OR are the earlier logical batch) ---
    FuncDef {
        name: "IFS",
        min_args: 2,
        max_args: None,
        eval: ifs_fn,
    },
    FuncDef {
        name: "NOT",
        min_args: 1,
        max_args: Some(1),
        eval: not_fn,
    },
    FuncDef {
        name: "IFNA",
        min_args: 2,
        max_args: Some(2),
        eval: ifna,
    },
    FuncDef {
        name: "SWITCH",
        min_args: 3,
        max_args: None,
        eval: switch_fn,
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
/// ignored; any error propagates. Shares [`collect_numbers`]' direct-vs-in-range gathering.
fn sum(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match collect_numbers(ctx, args) {
        Err(k) => Value::Error(k),
        Ok(nums) => finite_or_num(nums.iter().sum()),
    }
}

/// `AVERAGE(a, b, …)` — the arithmetic mean of the numeric data. Empty (no numbers) is `#DIV/0!`.
/// Shares [`collect_numbers`]' direct-vs-in-range gathering.
fn average(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match collect_numbers(ctx, args) {
        Err(k) => Value::Error(k),
        Ok(nums) if nums.is_empty() => Value::Error(ErrKind::Div0),
        Ok(nums) => finite_or_num(nums.iter().sum::<f64>() / nums.len() as f64),
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

// ---------------------------------------------------------------------------------------------
// Criteria-aggregation family. Two argument SHAPES share one criteria mini-language (`crate::criteria`)
// and one range-conformance rule:
//   * the `*IF` forms take `(criteria_range, criteria, [value_range])` — a single criterion, and the
//     value range defaults to the criteria range when omitted;
//   * the `*IFS` forms take `(value_range, criteria_range1, criteria1, …)` — an AND across pairs,
//     with COUNTIFS having no value range (it counts matching cells directly).
// CONFORMANCE (an Excel-semantics call worth a reviewer's eye): every criteria range and the value
// range must share the SAME on-disk shape (rows × cols) — a mismatch is a STATIC `#VALUE!`, not
// Excel's lenient legacy reshape-from-the-value-range's-corner. This is the same "static conformance
// beats runtime guessing" stance the encoding layer takes (format.md §6). A blank/text cell in a
// value range is ignored (only numbers aggregate); an error in a value range at a MATCHING position
// propagates; an error IN a criteria range never matches; an error-valued criterion propagates.
// ---------------------------------------------------------------------------------------------

/// The reduction a masked aggregation performs over the numeric matching cells of a value range.
#[derive(Clone, Copy)]
enum Reduce {
    Sum,
    Avg,
    Min,
    Max,
}

/// Evaluate an argument to a rectangular block: `(rows, cols, cells)`. A bare scalar is a `1×1`
/// block; an error value propagates (`Err`). This is the one materialization the family shares, so a
/// single cell, a range, or a literal all present the same shape/cell view to the conformance check.
fn block(ctx: &mut EvalCtx, e: &Expr) -> Result<(u32, u32, Vec<Value>), ErrKind> {
    match ctx.eval(e) {
        Value::Array(shape, cells) => Ok((shape.rows, shape.cols, cells)),
        Value::Error(k) => Err(k),
        other => Ok((1, 1, vec![other])),
    }
}

/// Parse a criteria argument: evaluate it, collapse to a scalar, and parse the mini-language. An
/// error criterion (or a multi-cell array in criteria position) propagates.
fn criterion(ctx: &mut EvalCtx, e: &Expr) -> Result<Criterion, ErrKind> {
    parse_criterion(&scalarize(ctx.eval(e)))
}

/// Reduce the numeric cells of `value_cells` at the positions `mask` marks true. An error cell at a
/// matching position propagates; non-numbers are ignored. `Avg` over no numbers is `#DIV/0!`; `Min`/
/// `Max` over no numbers is `0` (Excel's `MINIFS`/`MAXIFS` empty result).
fn reduce_masked(value_cells: &[Value], mask: &[bool], reduce: Reduce) -> Value {
    let mut sum = 0.0;
    let mut count: u64 = 0;
    let mut extreme: Option<f64> = None;
    for (m, v) in mask.iter().zip(value_cells.iter()) {
        if !*m {
            continue;
        }
        match v {
            Value::Error(k) => return Value::Error(*k),
            Value::Number(n) => {
                sum += n;
                count += 1;
                extreme = Some(match reduce {
                    Reduce::Min => extreme.map_or(*n, |e| e.min(*n)),
                    Reduce::Max => extreme.map_or(*n, |e| e.max(*n)),
                    _ => *n,
                });
            }
            _ => {}
        }
    }
    match reduce {
        Reduce::Sum => finite_or_num(sum),
        Reduce::Avg => {
            if count == 0 {
                Value::Error(ErrKind::Div0)
            } else {
                finite_or_num(sum / count as f64)
            }
        }
        Reduce::Min | Reduce::Max => Value::Number(extreme.unwrap_or(0.0)),
    }
}

/// Build the AND-combined match mask for the `*IFS` pair list `(criteria_range, criteria)…`,
/// enforcing that every criteria range shares the first one's shape. Returns the shared shape and
/// the per-cell mask (true = every criterion matched). An empty pair list is a caller bug (arity
/// guarantees ≥1 pair), guarded as `#VALUE!` rather than a panic.
fn build_mask(
    ctx: &mut EvalCtx,
    pairs: &[(&Expr, &Expr)],
) -> Result<((u32, u32), Vec<bool>), ErrKind> {
    let mut base: Option<(u32, u32)> = None;
    let mut mask: Vec<bool> = Vec::new();
    for (crange, cexpr) in pairs {
        let (rows, cols, cells) = block(ctx, crange)?;
        match base {
            None => {
                base = Some((rows, cols));
                mask = vec![true; cells.len()];
            }
            Some(b) if b != (rows, cols) => return Err(ErrKind::Value),
            Some(_) => {}
        }
        let crit = criterion(ctx, cexpr)?;
        for (m, cell) in mask.iter_mut().zip(cells.iter()) {
            if *m && !crit.matches(cell) {
                *m = false;
            }
        }
    }
    base.map(|b| (b, mask)).ok_or(ErrKind::Value)
}

/// Shared body of `SUMIF`/`AVERAGEIF`: a single criterion over `range`, reducing the value range
/// (`value_range` arg, or `range` itself when omitted) at the matching positions.
fn single_if(ctx: &mut EvalCtx, args: &[Expr], reduce: Reduce) -> Value {
    let (rrows, rcols, rcells) = match block(ctx, &args[0]) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let crit = match criterion(ctx, &args[1]) {
        Ok(c) => c,
        Err(k) => return Value::Error(k),
    };
    let value_cells = if args.len() == 3 {
        let (vrows, vcols, vcells) = match block(ctx, &args[2]) {
            Ok(t) => t,
            Err(k) => return Value::Error(k),
        };
        if (vrows, vcols) != (rrows, rcols) {
            return Value::Error(ErrKind::Value);
        }
        vcells
    } else {
        rcells.clone()
    };
    let mask: Vec<bool> = rcells.iter().map(|c| crit.matches(c)).collect();
    reduce_masked(&value_cells, &mask, reduce)
}

/// Shared body of `SUMIFS`/`AVERAGEIFS`/`MINIFS`/`MAXIFS`: value range is `args[0]`, then
/// `(criteria_range, criteria)` pairs. Enforces an odd arity (value + whole pairs) and that the value
/// range conforms to the criteria ranges' shape.
fn multi_if(ctx: &mut EvalCtx, args: &[Expr], reduce: Reduce) -> Value {
    // args[0] is the value range; the rest must be whole (criteria_range, criteria) pairs.
    if !(args.len() - 1).is_multiple_of(2) {
        return Value::Error(ErrKind::Value);
    }
    let pairs = pair_up(&args[1..]);
    let ((brows, bcols), mask) = match build_mask(ctx, &pairs) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let (vrows, vcols, vcells) = match block(ctx, &args[0]) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    if (vrows, vcols) != (brows, bcols) {
        return Value::Error(ErrKind::Value);
    }
    reduce_masked(&vcells, &mask, reduce)
}

/// Chunk a flat argument slice into `(range, criteria)` pairs. The caller has already checked the
/// slice length is even.
fn pair_up(args: &[Expr]) -> Vec<(&Expr, &Expr)> {
    args.chunks_exact(2).map(|c| (&c[0], &c[1])).collect()
}

/// `SUMIF(range, criteria, [sum_range])` — total the numbers at the matching positions.
fn sumif(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    single_if(ctx, args, Reduce::Sum)
}

/// `AVERAGEIF(range, criteria, [average_range])` — mean of the matching numbers; no match is
/// `#DIV/0!`.
fn averageif(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    single_if(ctx, args, Reduce::Avg)
}

/// `SUMIFS(sum_range, criteria_range1, criteria1, …)` — total where EVERY criterion matches.
fn sumifs(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    multi_if(ctx, args, Reduce::Sum)
}

/// `AVERAGEIFS(average_range, criteria_range1, criteria1, …)` — mean where every criterion matches;
/// no match is `#DIV/0!`.
fn averageifs(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    multi_if(ctx, args, Reduce::Avg)
}

/// `MINIFS(min_range, criteria_range1, criteria1, …)` — smallest matching number; no match is `0`.
fn minifs(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    multi_if(ctx, args, Reduce::Min)
}

/// `MAXIFS(max_range, criteria_range1, criteria1, …)` — largest matching number; no match is `0`.
fn maxifs(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    multi_if(ctx, args, Reduce::Max)
}

/// `COUNTIF(range, criteria)` — how many cells match. Counts a matching cell of ANY type (unlike the
/// summing forms, `COUNTIF` does not require a number), and never returns an error from its data — but
/// an error-valued CRITERION propagates.
fn countif(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (_, _, cells) = match block(ctx, &args[0]) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let crit = match criterion(ctx, &args[1]) {
        Ok(c) => c,
        Err(k) => return Value::Error(k),
    };
    let n = cells.iter().filter(|c| crit.matches(c)).count();
    Value::Number(n as f64)
}

/// `COUNTIFS(criteria_range1, criteria1, …)` — how many positions match EVERY criterion. Requires an
/// even arity (whole pairs) and conforming criteria-range shapes.
fn countifs(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    if !args.len().is_multiple_of(2) {
        return Value::Error(ErrKind::Value);
    }
    let pairs = pair_up(args);
    match build_mask(ctx, &pairs) {
        Ok((_, mask)) => Value::Number(mask.iter().filter(|m| **m).count() as f64),
        Err(k) => Value::Error(k),
    }
}

// ---------------------------------------------------------------------------------------------
// Pure scalar / vector math (the v1 math batch). These reuse eval.rs's scalar `coerce_num`/
// `scalarize` (so a boolean/numeric-text argument coerces exactly as it does for an operator) and
// `pow` (so `POWER` and the `^` operator share one error mapping). Excel-semantics calls worth a
// reviewer's eye are flagged at each site: MOD's sign follows the DIVISOR; INT floors toward −∞;
// SQRT of a negative is `#NUM!`; and legacy CEILING/FLOOR reject different-signed args with `#NUM!`
// while treating a zero significance ASYMMETRICALLY (CEILING → 0, FLOOR → `#DIV/0!`).
// ---------------------------------------------------------------------------------------------

/// Evaluate one argument to a scalar number (Excel arithmetic coercion), or its propagated error.
fn one_num(ctx: &mut EvalCtx, e: &Expr) -> Result<f64, ErrKind> {
    coerce_num(&scalarize(ctx.eval(e)))
}

/// Evaluate the first two arguments to scalar numbers, leftmost coercion error winning.
fn two_nums(ctx: &mut EvalCtx, args: &[Expr]) -> Result<(f64, f64), ErrKind> {
    let a = one_num(ctx, &args[0])?;
    let b = one_num(ctx, &args[1])?;
    Ok((a, b))
}

/// `PRODUCT(a, b, …)` — multiply the numbers. Mirrors `SUM`'s coercion asymmetry via
/// [`collect_numbers`]: a direct boolean/numeric-text coerces, an in-range non-number is ignored, any
/// error propagates. With NO numeric datum the product is `0` (Excel's empty-product result, not the
/// `1` identity).
fn product(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match collect_numbers(ctx, args) {
        Err(k) => Value::Error(k),
        Ok(nums) if nums.is_empty() => Value::Number(0.0),
        Ok(nums) => finite_or_num(nums.iter().product()),
    }
}

/// `SUMPRODUCT(array1, [array2], …)` — multiply the arrays element-for-element, then sum. Every
/// argument must share ONE shape (rows × cols); a mismatch is a static `#VALUE!` (the same
/// static-conformance stance as the `*IFS` family and format.md §6). A non-numeric cell (text /
/// blank / boolean) contributes `0` — Excel's rule, so an unfiltered boolean is `0`, not `1`; an
/// error at ANY position propagates (leftmost array, leftmost cell).
fn sumproduct(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let mut base: Option<(u32, u32)> = None;
    let mut prod: Vec<f64> = Vec::new();
    for a in args {
        let (rows, cols, cells) = match block(ctx, a) {
            Ok(t) => t,
            Err(k) => return Value::Error(k),
        };
        match base {
            None => {
                base = Some((rows, cols));
                prod = vec![1.0; cells.len()];
            }
            Some(b) if b != (rows, cols) => return Value::Error(ErrKind::Value),
            Some(_) => {}
        }
        for (p, cell) in prod.iter_mut().zip(cells.iter()) {
            match cell {
                Value::Error(k) => return Value::Error(*k),
                Value::Number(n) => *p *= n,
                _ => *p = 0.0,
            }
        }
    }
    finite_or_num(prod.iter().sum())
}

/// The direction a magnitude-rounding takes to `digits` places: `Up` = away from zero (`ROUNDUP`),
/// `Down` = toward zero (`ROUNDDOWN`).
#[derive(Clone, Copy)]
enum RoundDir {
    Up,
    Down,
}

/// Shared body of `ROUNDUP`/`ROUNDDOWN`: scale by `10^digits`, round the magnitude in `dir`, unscale.
/// `digits` truncates toward zero and clamps to a sane exponent band (mirroring `ROUND`); a negative
/// `digits` rounds to the left of the decimal point.
fn round_dir(ctx: &mut EvalCtx, args: &[Expr], dir: RoundDir) -> Value {
    let (n, d) = match two_nums(ctx, args) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let digits = d.trunc().clamp(-308.0, 308.0) as i32;
    let factor = 10f64.powi(digits);
    let scaled = n * factor;
    let rounded = match dir {
        // Away from zero: ceil the magnitude, then restore the sign.
        RoundDir::Up => scaled.abs().ceil().copysign(scaled),
        // Toward zero: truncation is exactly round-toward-zero.
        RoundDir::Down => scaled.trunc(),
    };
    finite_or_num(rounded / factor)
}

/// `ROUNDUP(x, digits)` — round the magnitude UP (away from zero) to `digits` places.
fn roundup(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    round_dir(ctx, args, RoundDir::Up)
}

/// `ROUNDDOWN(x, digits)` — round the magnitude DOWN (toward zero) to `digits` places.
fn rounddown(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    round_dir(ctx, args, RoundDir::Down)
}

/// `INT(x)` — round DOWN to the nearest integer, flooring toward −∞ (so `INT(-2.5) = -3`, NOT the
/// toward-zero `-2`). This is the load-bearing distinction from truncation.
fn int_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match one_num(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok(n) => finite_or_num(n.floor()),
    }
}

/// `MOD(n, divisor)` — the remainder, whose SIGN FOLLOWS THE DIVISOR (Excel), computed as
/// `n − divisor·⌊n/divisor⌋`. So `MOD(-3, 2) = 1` and `MOD(3, -2) = -1`. A zero divisor is `#DIV/0!`.
fn mod_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (n, divisor) = match two_nums(ctx, args) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    if divisor == 0.0 {
        return Value::Error(ErrKind::Div0);
    }
    finite_or_num(n - divisor * (n / divisor).floor())
}

/// `POWER(x, y)` — `x` raised to `y`, sharing `eval::pow` with the `^` operator (so `0^-1` is
/// `#DIV/0!` and a complex/overflowing power is `#NUM!`, identically to the operator).
fn power_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match two_nums(ctx, args) {
        Ok((a, b)) => pow(a, b),
        Err(k) => Value::Error(k),
    }
}

/// `SQRT(x)` — the non-negative square root; a NEGATIVE argument is `#NUM!` (Excel raises, rather
/// than returning a complex or `NaN`).
fn sqrt_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match one_num(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok(n) if n < 0.0 => Value::Error(ErrKind::Num),
        Ok(n) => finite_or_num(n.sqrt()),
    }
}

/// `CEILING(number, significance)` — round `number` AWAY FROM ZERO to the nearest multiple of
/// `significance` (legacy Excel). If the two arguments have DIFFERENT signs it is `#NUM!`; a zero
/// significance returns `0` (the asymmetric counterpart to `FLOOR`'s `#DIV/0!`).
fn ceiling_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (number, significance) = match two_nums(ctx, args) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    if significance == 0.0 {
        return Value::Number(0.0);
    }
    if number * significance < 0.0 {
        return Value::Error(ErrKind::Num);
    }
    finite_or_num(significance * (number / significance).ceil())
}

/// `FLOOR(number, significance)` — round `number` TOWARD ZERO to the nearest multiple of
/// `significance` (legacy Excel). Different-signed arguments are `#NUM!`; a zero significance is
/// `#DIV/0!` (the asymmetric counterpart to `CEILING`'s `0`).
fn floor_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (number, significance) = match two_nums(ctx, args) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    if significance == 0.0 {
        return Value::Error(ErrKind::Div0);
    }
    if number * significance < 0.0 {
        return Value::Error(ErrKind::Num);
    }
    finite_or_num(significance * (number / significance).floor())
}

// ---------------------------------------------------------------------------------------------
// Statistical extremes / order / counting (the v1 stats batch). MIN/MAX/MEDIAN share the SAME
// data-gathering asymmetry as SUM (`collect_numbers`): a datum passed DIRECTLY coerces (a boolean →
// 1/0, numeric-text → its number, a non-numeric direct text → `#VALUE!`), but a boolean/text/blank
// riding INSIDE a range is IGNORED (only in-range numbers aggregate). This "range vs direct-arg
// asymmetry" is the load-bearing Excel-semantics call for the whole batch. Errors propagate
// (leftmost). Empty-result calls differ by function: MIN/MAX over no numbers is `0` (Excel), MEDIAN
// over no numbers is `#NUM!`. COUNTA/COUNTBLANK count cells (never propagate an error from their
// data): COUNTA counts every non-empty datum (text, number, boolean, AND an error all count — only a
// `Blank` does not); COUNTBLANK counts the empty ones — a `Blank` OR an empty-string `""` (Excel
// counts a formula's `""` as blank), and nothing else.
// ---------------------------------------------------------------------------------------------

/// Gather the numeric data under SUM's direct-vs-in-range asymmetry: a direct boolean/numeric-text
/// coerces, an in-range non-number is ignored, a direct non-numeric text is `#VALUE!`, and any error
/// propagates (`Err`). The single materialization every plain numeric aggregator shares —
/// SUM/AVERAGE/PRODUCT and MIN/MAX/MEDIAN — so the coercion asymmetry lives in ONE place. (COUNT
/// differs: it ignores errors rather than propagating, so it stays a separate loop.)
fn collect_numbers(ctx: &mut EvalCtx, args: &[Expr]) -> Result<Vec<f64>, ErrKind> {
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

/// `MIN(a, b, …)` — the smallest number among the data; in-range text/blanks/logicals are ignored,
/// direct booleans/numeric-text coerce, errors propagate, and NO numeric datum yields `0` (Excel).
fn min_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    extreme(ctx, args, f64::min)
}

/// `MAX(a, b, …)` — the largest number among the data; same gathering rules and empty-`0` result as
/// [`min_fn`], reduced with `f64::max` instead.
fn max_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    extreme(ctx, args, f64::max)
}

/// Shared body of `MIN`/`MAX`: gather numbers, reduce with `pick`; an empty set is `0` (the Excel
/// empty result, distinct from MEDIAN's `#NUM!`).
fn extreme(ctx: &mut EvalCtx, args: &[Expr], pick: fn(f64, f64) -> f64) -> Value {
    match collect_numbers(ctx, args) {
        Err(k) => Value::Error(k),
        Ok(nums) => match nums.into_iter().reduce(pick) {
            None => Value::Number(0.0),
            Some(x) => finite_or_num(x),
        },
    }
}

/// `MEDIAN(a, b, …)` — the middle of the sorted numeric data, AVERAGING THE TWO MIDDLES for an even
/// count (`(lo + hi) / 2`). Gathering matches MIN/MAX (in-range non-numbers ignored, direct coerce,
/// errors propagate); NO numeric datum is `#NUM!` (distinct from MIN/MAX's empty-`0`).
fn median_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let mut nums = match collect_numbers(ctx, args) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    if nums.is_empty() {
        return Value::Error(ErrKind::Num);
    }
    // total_cmp is a total order over the finite f64s gathered here (no NaN can enter — non-finite
    // text never parses to a Number), so no unwrap and no panic.
    nums.sort_by(f64::total_cmp);
    let n = nums.len();
    let med = if n % 2 == 1 {
        nums[n / 2]
    } else {
        (nums[n / 2 - 1] + nums[n / 2]) / 2.0
    };
    finite_or_num(med)
}

/// `RANK(number, ref, [order])` — the position of `number` within the numeric cells of `ref`,
/// `order = 0`/omitted DESCENDING (largest is rank `1`), any non-zero `order` ASCENDING. TIES SHARE
/// THE LOWEST RANK (computed as `1 + count of strictly-better values`, so equal values necessarily
/// get the same, best, rank). Non-numeric cells in `ref` are ignored; an error in `ref` (or a
/// non-numeric `number`/`order`) propagates; a `number` absent from `ref` is `#N/A`.
fn rank_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let number = match coerce_num(&scalarize(ctx.eval(&args[0]))) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let (_, _, cells) = match block(ctx, &args[1]) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let mut nums = Vec::new();
    for c in &cells {
        match c {
            Value::Error(k) => return Value::Error(*k),
            Value::Number(n) => nums.push(*n),
            _ => {}
        }
    }
    let ascending = if args.len() == 3 {
        match coerce_num(&scalarize(ctx.eval(&args[2]))) {
            Ok(o) => o != 0.0,
            Err(k) => return Value::Error(k),
        }
    } else {
        false
    };
    if !nums.contains(&number) {
        return Value::Error(ErrKind::Na);
    }
    let better = if ascending {
        nums.iter().filter(|&&x| x < number).count()
    } else {
        nums.iter().filter(|&&x| x > number).count()
    };
    Value::Number((better + 1) as f64)
}

/// `COUNTA(a, b, …)` — how many data are NON-EMPTY. Everything but a `Blank` counts (text, numbers,
/// booleans, AND errors), whether passed directly or riding in a range; a `Blank` (direct or
/// in-range) never counts. Never propagates an error — an error datum is just a counted non-empty.
fn counta(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let mut n: u64 = 0;
    for a in args {
        match ctx.eval(a) {
            Value::Array(_, cells) => {
                n += cells.iter().filter(|c| !matches!(c, Value::Blank)).count() as u64
            }
            Value::Blank => {}
            _ => n += 1,
        }
    }
    Value::Number(n as f64)
}

/// `COUNTBLANK(range)` — how many cells are EMPTY: a `Blank` or an empty-string `""` (Excel counts a
/// formula's `""` result as blank). Nothing else counts — a zero, an error, or any non-empty text is
/// not blank. Never propagates an error.
fn countblank(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let mut n: u64 = 0;
    for a in args {
        match ctx.eval(a) {
            Value::Array(_, cells) => n += cells.iter().filter(|c| is_blankish(c)).count() as u64,
            other => {
                if is_blankish(&other) {
                    n += 1;
                }
            }
        }
    }
    Value::Number(n as f64)
}

/// Whether a value counts as "blank" for `COUNTBLANK`: a `Blank`, or an empty-string `Text("")`.
fn is_blankish(v: &Value) -> bool {
    matches!(v, Value::Blank) || matches!(v, Value::Text(t) if t.is_empty())
}

// ---------------------------------------------------------------------------------------------
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
// ---------------------------------------------------------------------------------------------

/// `IFS(test1, value1, test2, value2, …)` — the value paired with the FIRST TRUE test, evaluating
/// only that value (lazy). No TRUE test is `#N/A`; a test that errors or is non-coercible propagates;
/// a dangling test with no value (odd arity) is a structural `#VALUE!`.
fn ifs_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
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
fn not_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match coerce_bool(&ctx.eval(&args[0])) {
        Err(k) => Value::Error(k),
        Ok(b) => Value::Bool(!b),
    }
}

/// `IFNA(value, value_if_na)` — the fallback replaces `value` ONLY when it is `#N/A`; every other
/// error and any normal value (arrays preserved) passes through unchanged. The fallback is evaluated
/// lazily, only on a genuine `#N/A`.
fn ifna(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
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
fn switch_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
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

/// Wrap a computed number, demoting a non-finite result (overflow) to `#NUM!` so a `Value::Number`
/// is always finite in the arithmetic domain (mirrors the lexer/`coerce_num` finiteness invariant),
/// and canonicalizing a signed zero to `+0.0`. Excel displays every zero as `0`, but `Value`'s `Eq`
/// is bit-exact (`-0.0 ≠ 0.0`), so a stray computed `-0.0` — an empty `SUM`/`SUMPRODUCT` aggregate
/// (`[].sum() == -0.0`), or `ROUNDDOWN`/`INT` of a small negative — must fold to `+0.0` or it would
/// spuriously Diverge from a `0`-expecting oracle.
fn finite_or_num(n: f64) -> Value {
    if n.is_finite() {
        // `n == 0.0` is true for BOTH `+0.0` and `-0.0` (IEEE), so this canonicalizes the sign.
        Value::Number(if n == 0.0 { 0.0 } else { n })
    } else {
        Value::Error(ErrKind::Num)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::eval;
    use crate::refs::RangeNode;
    use crate::test_support::Grid;

    fn num(n: f64) -> Expr {
        Expr::Lit(Value::Number(n))
    }
    fn call(name: &str, args: Vec<Expr>) -> Expr {
        Expr::Call(lookup(name).expect("known function"), args)
    }
    /// A full-column range A1:A{rows} over a 1-wide grid (contiguous in the stub).
    fn col_range(rows: u32) -> Expr {
        Expr::Range(RangeNode {
            start_col: 0,
            start_row: 0,
            end_col: 0,
            end_row: rows - 1,
            sheet: None,
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
    fn math_batch_scalar_semantics() {
        let g = Grid::new(1, vec![Value::Blank]);
        // MOD sign follows the divisor.
        assert_eq!(
            eval(&call("MOD", vec![num(7.0), num(3.0)]), &g),
            Value::Number(1.0)
        );
        assert_eq!(
            eval(&call("MOD", vec![num(7.0), num(-3.0)]), &g),
            Value::Number(-2.0)
        );
        assert_eq!(
            eval(&call("MOD", vec![num(-7.0), num(3.0)]), &g),
            Value::Number(2.0)
        );
        assert_eq!(
            eval(&call("MOD", vec![num(5.0), num(0.0)]), &g),
            Value::Error(ErrKind::Div0)
        );
        // INT floors toward −∞ (not toward zero).
        assert_eq!(eval(&call("INT", vec![num(-2.5)]), &g), Value::Number(-3.0));
        assert_eq!(eval(&call("INT", vec![num(2.9)]), &g), Value::Number(2.0));
        // SQRT of a negative is #NUM!.
        assert_eq!(eval(&call("SQRT", vec![num(16.0)]), &g), Value::Number(4.0));
        assert_eq!(
            eval(&call("SQRT", vec![num(-4.0)]), &g),
            Value::Error(ErrKind::Num)
        );
        // POWER shares the operator's error mapping.
        assert_eq!(
            eval(&call("POWER", vec![num(2.0), num(10.0)]), &g),
            Value::Number(1024.0)
        );
        assert_eq!(
            eval(&call("POWER", vec![num(0.0), num(-1.0)]), &g),
            Value::Error(ErrKind::Div0)
        );
        assert_eq!(
            eval(&call("POWER", vec![num(-8.0), num(0.5)]), &g),
            Value::Error(ErrKind::Num)
        );
        // ROUNDUP away from zero; ROUNDDOWN toward zero; negative digits shift left.
        assert_eq!(
            eval(&call("ROUNDUP", vec![num(1.234), num(2.0)]), &g),
            Value::Number(1.24)
        );
        assert_eq!(
            eval(&call("ROUNDUP", vec![num(-1.234), num(2.0)]), &g),
            Value::Number(-1.24)
        );
        assert_eq!(
            eval(&call("ROUNDDOWN", vec![num(1.789), num(2.0)]), &g),
            Value::Number(1.78)
        );
        assert_eq!(
            eval(&call("ROUNDDOWN", vec![num(3.99999), num(0.0)]), &g),
            Value::Number(3.0)
        );
    }

    #[test]
    fn ceiling_floor_sign_and_zero_asymmetry() {
        let g = Grid::new(1, vec![Value::Blank]);
        // Away-from-zero / toward-zero to a multiple.
        assert_eq!(
            eval(&call("CEILING", vec![num(2.5), num(1.0)]), &g),
            Value::Number(3.0)
        );
        assert_eq!(
            eval(&call("FLOOR", vec![num(2.5), num(1.0)]), &g),
            Value::Number(2.0)
        );
        assert_eq!(
            eval(&call("CEILING", vec![num(-2.5), num(-2.0)]), &g),
            Value::Number(-4.0)
        );
        assert_eq!(
            eval(&call("FLOOR", vec![num(-2.5), num(-2.0)]), &g),
            Value::Number(-2.0)
        );
        // Different-signed args are #NUM! for both.
        assert_eq!(
            eval(&call("CEILING", vec![num(2.5), num(-1.0)]), &g),
            Value::Error(ErrKind::Num)
        );
        assert_eq!(
            eval(&call("FLOOR", vec![num(2.5), num(-1.0)]), &g),
            Value::Error(ErrKind::Num)
        );
        // Zero significance: CEILING → 0, FLOOR → #DIV/0! (the legacy asymmetry).
        assert_eq!(
            eval(&call("CEILING", vec![num(5.0), num(0.0)]), &g),
            Value::Number(0.0)
        );
        assert_eq!(
            eval(&call("FLOOR", vec![num(5.0), num(0.0)]), &g),
            Value::Error(ErrKind::Div0)
        );
    }

    #[test]
    fn product_and_sumproduct_semantics() {
        // PRODUCT over a range multiplies the numbers; a range with no numbers is 0.
        let g = Grid::new(
            1,
            vec![Value::Number(2.0), Value::Number(3.0), Value::Number(4.0)],
        );
        assert_eq!(
            eval(&call("PRODUCT", vec![col_range(3)]), &g),
            Value::Number(24.0)
        );
        // Direct-arg coercion mirrors SUM (bool → 1/0, numeric-text parses).
        assert_eq!(
            eval(
                &call(
                    "PRODUCT",
                    vec![
                        num(2.0),
                        Expr::Lit(Value::Bool(true)),
                        Expr::Lit(Value::Text("3".into()))
                    ]
                ),
                &g
            ),
            Value::Number(6.0)
        );
        // Empty product (no numeric datum) is 0, not the 1 identity.
        let blank = Grid::new(1, vec![Value::Text("x".into()), Value::Blank]);
        assert_eq!(
            eval(&call("PRODUCT", vec![col_range(2)]), &blank),
            Value::Number(0.0)
        );
        // SUMPRODUCT multiplies aligned arrays then sums (array literals sidestep the whole-row
        // stub, which cannot window a single column of a multi-column grid).
        let col3 = |a: f64, b: f64, c: f64| {
            Expr::Lit(Value::Array(
                crate::value::Shape { rows: 3, cols: 1 },
                vec![Value::Number(a), Value::Number(b), Value::Number(c)],
            ))
        };
        assert_eq!(
            eval(
                &call("SUMPRODUCT", vec![col3(1.0, 2.0, 3.0), col3(4.0, 5.0, 6.0)]),
                &g
            ),
            Value::Number(32.0)
        );
        // A shape mismatch (3×1 vs 2×1) is a static #VALUE!.
        let col2 = Expr::Lit(Value::Array(
            crate::value::Shape { rows: 2, cols: 1 },
            vec![Value::Number(4.0), Value::Number(5.0)],
        ));
        assert_eq!(
            eval(&call("SUMPRODUCT", vec![col3(1.0, 2.0, 3.0), col2]), &g),
            Value::Error(ErrKind::Value)
        );
        // A non-numeric cell counts as 0 (so an unfiltered text zeroes its product term).
        let with_text = Expr::Lit(Value::Array(
            crate::value::Shape { rows: 3, cols: 1 },
            vec![
                Value::Number(2.0),
                Value::Text("x".into()),
                Value::Number(4.0),
            ],
        ));
        assert_eq!(
            eval(
                &call("SUMPRODUCT", vec![with_text, col3(5.0, 5.0, 5.0)]),
                &g
            ),
            Value::Number(30.0)
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
    fn min_max_range_vs_direct_arg_asymmetry() {
        // In a RANGE, text/blank/logical are ignored (only numbers) — so TRUE does NOT count as 1.
        let g = Grid::new(
            1,
            vec![
                Value::Number(-5.0),
                Value::Bool(true),
                Value::Blank,
                Value::Number(-2.0),
            ],
        );
        assert_eq!(
            eval(&call("MIN", vec![col_range(4)]), &g),
            Value::Number(-5.0)
        );
        assert_eq!(
            eval(&call("MAX", vec![col_range(4)]), &g),
            Value::Number(-2.0)
        );
        // DIRECT booleans/numeric-text coerce (TRUE -> 1), the asymmetry's other half.
        let b = Grid::new(1, vec![Value::Blank]);
        assert_eq!(
            eval(
                &call(
                    "MAX",
                    vec![num(-5.0), Expr::Lit(Value::Bool(true)), num(-2.0)]
                ),
                &b
            ),
            Value::Number(1.0)
        );
        assert_eq!(
            eval(
                &call("MIN", vec![num(3.0), Expr::Lit(Value::Text("2".into()))]),
                &b
            ),
            Value::Number(2.0)
        );
        // No numeric datum -> 0 (Excel), and an in-range error propagates.
        let empty = Grid::new(1, vec![Value::Text("x".into()), Value::Blank]);
        assert_eq!(
            eval(&call("MIN", vec![col_range(2)]), &empty),
            Value::Number(0.0)
        );
        let with_err = Grid::new(1, vec![Value::Number(5.0), Value::Error(ErrKind::Div0)]);
        assert_eq!(
            eval(&call("MAX", vec![col_range(2)]), &with_err),
            Value::Error(ErrKind::Div0)
        );
    }

    #[test]
    fn median_even_averages_two_middles_and_empty_is_num() {
        // Even count {1,2,3,4} -> (2+3)/2 = 2.5 (in-range text ignored).
        let g = Grid::new(
            1,
            vec![
                Value::Number(1.0),
                Value::Number(2.0),
                Value::Text("x".into()),
                Value::Number(3.0),
                Value::Number(4.0),
            ],
        );
        assert_eq!(
            eval(&call("MEDIAN", vec![col_range(5)]), &g),
            Value::Number(2.5)
        );
        // Odd count -> the exact middle.
        let odd = Grid::new(
            1,
            vec![
                Value::Number(5.0),
                Value::Number(3.0),
                Value::Number(1.0),
                Value::Number(4.0),
                Value::Number(2.0),
            ],
        );
        assert_eq!(
            eval(&call("MEDIAN", vec![col_range(5)]), &odd),
            Value::Number(3.0)
        );
        // No numeric datum -> #NUM! (distinct from MIN/MAX's 0); an error propagates.
        let empty = Grid::new(1, vec![Value::Text("x".into()), Value::Blank]);
        assert_eq!(
            eval(&call("MEDIAN", vec![col_range(2)]), &empty),
            Value::Error(ErrKind::Num)
        );
        let with_err = Grid::new(1, vec![Value::Number(1.0), Value::Error(ErrKind::Ref)]);
        assert_eq!(
            eval(&call("MEDIAN", vec![col_range(2)]), &with_err),
            Value::Error(ErrKind::Ref)
        );
    }

    #[test]
    fn rank_descending_default_ties_share_lowest_and_missing_is_na() {
        // {10,8,8,5}: RANK(8) descending -> 2 (one value strictly greater); both 8s share rank 2.
        let g = Grid::new(
            1,
            vec![
                Value::Number(10.0),
                Value::Number(8.0),
                Value::Number(8.0),
                Value::Number(5.0),
            ],
        );
        assert_eq!(
            eval(&call("RANK", vec![num(8.0), col_range(4)]), &g),
            Value::Number(2.0)
        );
        // Ascending (non-zero order): RANK(10, …, 1) -> 4 (three strictly less).
        assert_eq!(
            eval(&call("RANK", vec![num(10.0), col_range(4), num(1.0)]), &g),
            Value::Number(4.0)
        );
        // A number not present in ref is #N/A.
        assert_eq!(
            eval(&call("RANK", vec![num(7.0), col_range(4)]), &g),
            Value::Error(ErrKind::Na)
        );
        // A non-numeric `number` argument is #VALUE!.
        assert_eq!(
            eval(
                &call(
                    "RANK",
                    vec![Expr::Lit(Value::Text("x".into())), col_range(4)]
                ),
                &g
            ),
            Value::Error(ErrKind::Value)
        );
    }

    #[test]
    fn counta_and_countblank_over_a_range() {
        // A1..A5 = 1, "", "x", <blank>, #N/A. COUNTA counts non-empty: 1, "", "x", #N/A = 4
        // (the empty-string "" is non-empty; error counts; only the blank does not).
        let g = Grid::new(
            1,
            vec![
                Value::Number(1.0),
                Value::Text(String::new()),
                Value::Text("x".into()),
                Value::Blank,
                Value::Error(ErrKind::Na),
            ],
        );
        assert_eq!(
            eval(&call("COUNTA", vec![col_range(5)]), &g),
            Value::Number(4.0)
        );
        // COUNTBLANK counts the empty: the "" AND the <blank> = 2 (error/number/text not blank).
        assert_eq!(
            eval(&call("COUNTBLANK", vec![col_range(5)]), &g),
            Value::Number(2.0)
        );
        // COUNTA of a direct blank does not count it; a direct value does.
        let b = Grid::new(1, vec![Value::Blank]);
        assert_eq!(
            eval(&call("COUNTA", vec![Expr::Lit(Value::Blank), num(1.0)]), &b),
            Value::Number(1.0)
        );
    }

    #[test]
    fn ifs_first_true_wins_lazily_and_none_is_na() {
        let g = Grid::new(1, vec![Value::Blank]);
        let t = || Expr::Lit(Value::Bool(true));
        let f = || Expr::Lit(Value::Bool(false));
        let div0 = || {
            Expr::Binary(
                crate::expr::BinOp::Div,
                Box::new(num(1.0)),
                Box::new(num(0.0)),
            )
        };
        // First TRUE test's value wins; the earlier FALSE pair's value is skipped.
        assert_eq!(
            eval(
                &call("IFS", vec![f(), num(1.0), t(), num(2.0), t(), num(3.0)]),
                &g
            ),
            Value::Number(2.0)
        );
        // Lazy: the unreached value (1/0) after the first match is never evaluated.
        assert_eq!(
            eval(&call("IFS", vec![t(), num(1.0), t(), div0()]), &g),
            Value::Number(1.0)
        );
        // No TRUE test -> #N/A.
        assert_eq!(
            eval(&call("IFS", vec![f(), num(1.0), f(), num(2.0)]), &g),
            Value::Error(ErrKind::Na)
        );
        // A test that errors propagates (evaluated before any match).
        assert_eq!(
            eval(&call("IFS", vec![div0(), num(1.0), t(), num(2.0)]), &g),
            Value::Error(ErrKind::Div0)
        );
        // An odd argument count (dangling test) is a structural #VALUE!.
        assert_eq!(
            eval(&call("IFS", vec![f(), num(1.0), t()]), &g),
            Value::Error(ErrKind::Value)
        );
    }

    #[test]
    fn not_coerces_and_propagates() {
        let g = Grid::new(1, vec![Value::Blank]);
        assert_eq!(
            eval(&call("NOT", vec![Expr::Lit(Value::Bool(true))]), &g),
            Value::Bool(false)
        );
        // A non-zero number coerces to TRUE -> NOT is FALSE; zero -> TRUE.
        assert_eq!(eval(&call("NOT", vec![num(5.0)]), &g), Value::Bool(false));
        assert_eq!(eval(&call("NOT", vec![num(0.0)]), &g), Value::Bool(true));
        // A non-logical text is #VALUE!; an error propagates.
        assert_eq!(
            eval(&call("NOT", vec![Expr::Lit(Value::Text("x".into()))]), &g),
            Value::Error(ErrKind::Value)
        );
        assert_eq!(
            eval(&call("NOT", vec![Expr::Lit(Value::Error(ErrKind::Na))]), &g),
            Value::Error(ErrKind::Na)
        );
    }

    #[test]
    fn ifna_catches_only_na() {
        let g = Grid::new(1, vec![Value::Blank]);
        // Catches #N/A.
        assert_eq!(
            eval(
                &call(
                    "IFNA",
                    vec![Expr::Lit(Value::Error(ErrKind::Na)), num(99.0)]
                ),
                &g
            ),
            Value::Number(99.0)
        );
        // Passes a normal value through.
        assert_eq!(
            eval(&call("IFNA", vec![num(42.0), num(99.0)]), &g),
            Value::Number(42.0)
        );
        // Does NOT catch a different error (the distinction from IFERROR).
        assert_eq!(
            eval(
                &call(
                    "IFNA",
                    vec![Expr::Lit(Value::Error(ErrKind::Div0)), num(99.0)]
                ),
                &g
            ),
            Value::Error(ErrKind::Div0)
        );
    }

    #[test]
    fn switch_matches_first_with_optional_default() {
        let g = Grid::new(1, vec![Value::Blank]);
        let txt = |s: &str| Expr::Lit(Value::Text(s.into()));
        // Matches the second value.
        assert_eq!(
            eval(
                &call(
                    "SWITCH",
                    vec![
                        num(2.0),
                        num(1.0),
                        txt("one"),
                        num(2.0),
                        txt("two"),
                        num(3.0),
                        txt("three")
                    ]
                ),
                &g
            ),
            Value::Text("two".into())
        );
        // No match + trailing default -> the default; no match + no default -> #N/A.
        assert_eq!(
            eval(
                &call("SWITCH", vec![num(9.0), num(1.0), txt("one"), txt("none")]),
                &g
            ),
            Value::Text("none".into())
        );
        assert_eq!(
            eval(&call("SWITCH", vec![num(9.0), num(1.0), txt("one")]), &g),
            Value::Error(ErrKind::Na)
        );
        // Text matching is case-insensitive (Excel `=`).
        assert_eq!(
            eval(
                &call("SWITCH", vec![txt("hello"), txt("HELLO"), num(1.0)]),
                &g
            ),
            Value::Number(1.0)
        );
        // The expression's error propagates.
        let div0 = Expr::Binary(
            crate::expr::BinOp::Div,
            Box::new(num(1.0)),
            Box::new(num(0.0)),
        );
        assert_eq!(
            eval(
                &call("SWITCH", vec![div0, num(1.0), txt("one"), txt("def")]),
                &g
            ),
            Value::Error(ErrKind::Div0)
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
