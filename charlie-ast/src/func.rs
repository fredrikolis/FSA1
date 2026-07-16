// Concern: the FUNCTION REGISTRY as data — the `FuncDef` record (`name`, arity bounds, an `eval` fn-pointer, an OPTIONAL parse-time `validate` argument-check, and a `volatile` flag), the flat `FUNCS` table indexed by `FuncId`, name->`FuncId` lookup (case-insensitive, for the parser) and `FuncId`->`FuncDef` dispatch (for the evaluator), plus the built-ins landed so far across categories (aggregation SUM AVERAGE COUNT · logical IF IFERROR AND OR IFS NOT IFNA SWITCH · the `*IF(S)` criteria family SUMIF SUMIFS COUNTIF COUNTIFS AVERAGEIF AVERAGEIFS MINIFS MAXIFS · math ABS ROUND PRODUCT SUMPRODUCT ROUNDUP ROUNDDOWN INT MOD POWER SQRT CEILING FLOOR · stats MIN MAX MEDIAN RANK COUNTA COUNTBLANK · text CONCAT TEXTJOIN LEFT RIGHT MID LEN FIND SEARCH SUBSTITUTE REPLACE TRIM UPPER LOWER TEXT · date/time DATE YEAR MONTH DAY EDATE DATEDIF TODAY NOW, the last two VOLATILE via the resolver's injectable clock); each built-in owns its own argument evaluation so lazy forms (IF/IFERROR), the direct-vs-in-range coercion asymmetry, and range-conformance checks are expressible | Non-concern: the remaining ~70-function grind (lookup/spill land later), the CRITERIA mini-language the `*IF(S)` built-ins depend on (criteria.rs owns `Criterion`/`parse_criterion` — the "does this cell match this criterion" grammar), and the operator/coercion machinery (eval.rs owns `coerce_num`/`coerce_bool`/`scalarize`/`pow`, which the built-ins reuse) | IO: none — a static dispatch table over the `EvalCtx`/`Value` contract
//! The function registry: [`FuncDef`], the [`FUNCS`] table, [`lookup`], [`def`], [`dispatch`].
//!
//! Registry-as-data (ast-standards PART 7, "one engine, N behaviors as data"): a function is a row,
//! not a hand-forked code path. The parser resolves a name to a [`crate::FuncId`] and checks arity
//! against the row (so eval trusts the arity — DbC); the evaluator dispatches the row's `eval`. The
//! v1 set here is deliberately small — enough to prove aggregation, laziness, error-catching, logic,
//! and pure-math all route through the same table.

use crate::criteria::{Criterion, parse_criterion};
use crate::diag::{Diag, DiagCode, Span};
use crate::eval::{EvalCtx, coerce_bool, coerce_num, pow, scalarize, to_text, value_eq};
use crate::expr::{Expr, FuncId};
use crate::value::{ErrKind, Value};

/// One registry row. `min_args`/`max_args` bound the arity (`max_args = None` is unbounded/variadic);
/// `eval` receives the *unevaluated* argument `Expr`s and the [`EvalCtx`], so a function chooses what
/// to evaluate (lazy `IF`/`IFERROR`) and how to treat a datum by whether it arrived direct or inside
/// a range.
/// A parse-time argument validator: `(the call's args, the call name's [`Span`])` → `Ok(())` or a
/// located refusal [`Diag`]. The seam a [`FuncDef`] uses for a static-argument check beyond arity.
pub type ValidateFn = fn(&[Expr], Span) -> Result<(), Diag>;

pub struct FuncDef {
    pub name: &'static str,
    pub min_args: usize,
    pub max_args: Option<usize>,
    pub eval: fn(&mut EvalCtx, &[Expr]) -> Value,
    /// An OPTIONAL parse-time argument check, run by the parser AFTER the arity gate, for a function
    /// that constrains its arguments' *static shape* beyond arity — the one such case today is `TEXT`,
    /// which refuses a format code that is a STATICALLY-KNOWN-UNSUPPORTED literal (a wrong-format guess
    /// caught up front, not mis-rendered at eval; refusals are a parse-time concern, and eval never
    /// returns a [`Diag`]). A non-literal format is accepted and deferred to eval — accept-under-
    /// uncertainty, never a false-reject. Stays a registry-row datum (not a hand-fork in the parser)
    /// so the "function is a row" invariant holds. `None` for every function whose only static contract
    /// is arity.
    pub validate: Option<ValidateFn>,
    /// Whether this function is VOLATILE — its value can change between evaluations of the SAME tree
    /// against the SAME resolver, because it reads a mutable seam of the outside world rather than only
    /// its arguments. The clock functions `TODAY`/`NOW` are the v1 volatiles: they read the resolver's
    /// injectable [`crate::Resolver::now_serial`] clock (pinned in tests/conformance, system time in
    /// production), so two evals a second apart can Diverge. Recorded as registry data so a later
    /// recalc engine can find the volatile cells without a hand-forked name list; `false` for every
    /// pure function (whose value is a function of its arguments alone).
    pub volatile: bool,
}

impl FuncDef {
    /// Whether `n` arguments satisfy this function's arity bounds.
    pub fn arity_ok(&self, n: usize) -> bool {
        n >= self.min_args && self.max_args.is_none_or(|max| n <= max)
    }

    /// Run this row's optional parse-time argument check (a no-op when the row sets `validate = None`).
    /// The parser calls this after the arity gate; `span` locates a refusal on the call's name token.
    pub fn validate_args(&self, args: &[Expr], span: Span) -> Result<(), Diag> {
        match self.validate {
            Some(check) => check(args, span),
            None => Ok(()),
        }
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
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "AVERAGE",
        min_args: 1,
        max_args: None,
        eval: average,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "COUNT",
        min_args: 1,
        max_args: None,
        eval: count,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "IF",
        min_args: 2,
        max_args: Some(3),
        eval: if_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "IFERROR",
        min_args: 2,
        max_args: Some(2),
        eval: iferror,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "AND",
        min_args: 1,
        max_args: None,
        eval: and_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "OR",
        min_args: 1,
        max_args: None,
        eval: or_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "ABS",
        min_args: 1,
        max_args: Some(1),
        eval: abs_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "ROUND",
        min_args: 2,
        max_args: Some(2),
        eval: round_fn,
        validate: None,
        volatile: false,
    },
    // --- Criteria-aggregation family (the `*IF(S)` reporting workhorse) ---
    FuncDef {
        name: "SUMIF",
        min_args: 2,
        max_args: Some(3),
        eval: sumif,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "SUMIFS",
        min_args: 3,
        max_args: None,
        eval: sumifs,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "COUNTIF",
        min_args: 2,
        max_args: Some(2),
        eval: countif,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "COUNTIFS",
        min_args: 2,
        max_args: None,
        eval: countifs,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "AVERAGEIF",
        min_args: 2,
        max_args: Some(3),
        eval: averageif,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "AVERAGEIFS",
        min_args: 3,
        max_args: None,
        eval: averageifs,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "MINIFS",
        min_args: 3,
        max_args: None,
        eval: minifs,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "MAXIFS",
        min_args: 3,
        max_args: None,
        eval: maxifs,
        validate: None,
        volatile: false,
    },
    // --- Pure scalar / vector math (the v1 math batch) ---
    FuncDef {
        name: "PRODUCT",
        min_args: 1,
        max_args: None,
        eval: product,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "SUMPRODUCT",
        min_args: 1,
        max_args: None,
        eval: sumproduct,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "ROUNDUP",
        min_args: 2,
        max_args: Some(2),
        eval: roundup,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "ROUNDDOWN",
        min_args: 2,
        max_args: Some(2),
        eval: rounddown,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "INT",
        min_args: 1,
        max_args: Some(1),
        eval: int_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "MOD",
        min_args: 2,
        max_args: Some(2),
        eval: mod_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "POWER",
        min_args: 2,
        max_args: Some(2),
        eval: power_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "SQRT",
        min_args: 1,
        max_args: Some(1),
        eval: sqrt_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "CEILING",
        min_args: 2,
        max_args: Some(2),
        eval: ceiling_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "FLOOR",
        min_args: 2,
        max_args: Some(2),
        eval: floor_fn,
        validate: None,
        volatile: false,
    },
    // --- Statistical extremes / order / counting (the v1 stats batch) ---
    FuncDef {
        name: "MIN",
        min_args: 1,
        max_args: None,
        eval: min_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "MAX",
        min_args: 1,
        max_args: None,
        eval: max_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "MEDIAN",
        min_args: 1,
        max_args: None,
        eval: median_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "RANK",
        min_args: 2,
        max_args: Some(3),
        eval: rank_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "COUNTA",
        min_args: 1,
        max_args: None,
        eval: counta,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "COUNTBLANK",
        min_args: 1,
        max_args: Some(1),
        eval: countblank,
        validate: None,
        volatile: false,
    },
    // --- Logical batch v1: IFS NOT IFNA SWITCH (IF/IFERROR/AND/OR are the earlier logical batch) ---
    FuncDef {
        name: "IFS",
        min_args: 2,
        max_args: None,
        eval: ifs_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "NOT",
        min_args: 1,
        max_args: Some(1),
        eval: not_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "IFNA",
        min_args: 2,
        max_args: Some(2),
        eval: ifna,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "SWITCH",
        min_args: 3,
        max_args: None,
        eval: switch_fn,
        validate: None,
        volatile: false,
    },
    // --- Text batch v1: CONCAT TEXTJOIN LEFT RIGHT MID LEN FIND SEARCH SUBSTITUTE REPLACE TRIM
    //     UPPER LOWER TEXT. (The `&` concat OPERATOR is eval.rs's BinOp::Concat — CONCAT/TEXTJOIN are
    //     the function forms; TEXTJOIN adds a delimiter + an ignore-empty flag.) ---
    FuncDef {
        name: "CONCAT",
        min_args: 1,
        max_args: None,
        eval: concat_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "TEXTJOIN",
        min_args: 3,
        max_args: None,
        eval: textjoin_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "LEFT",
        min_args: 1,
        max_args: Some(2),
        eval: left_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "RIGHT",
        min_args: 1,
        max_args: Some(2),
        eval: right_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "MID",
        min_args: 3,
        max_args: Some(3),
        eval: mid_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "LEN",
        min_args: 1,
        max_args: Some(1),
        eval: len_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "FIND",
        min_args: 2,
        max_args: Some(3),
        eval: find_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "SEARCH",
        min_args: 2,
        max_args: Some(3),
        eval: search_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "SUBSTITUTE",
        min_args: 3,
        max_args: Some(4),
        eval: substitute_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "REPLACE",
        min_args: 4,
        max_args: Some(4),
        eval: replace_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "TRIM",
        min_args: 1,
        max_args: Some(1),
        eval: trim_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "UPPER",
        min_args: 1,
        max_args: Some(1),
        eval: upper_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "LOWER",
        min_args: 1,
        max_args: Some(1),
        eval: lower_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "TEXT",
        min_args: 2,
        max_args: Some(2),
        eval: text_fn,
        validate: Some(validate_text_format),
        volatile: false,
    },
    // --- Date/time batch v1: DATE YEAR MONTH DAY EDATE DATEDIF TODAY NOW. The Excel 1900 date-serial
    //     system (with the leap-year bug replicated — see `serial_to_ymd`/`serial_from_ymd`); TODAY/NOW
    //     are VOLATILE and read the resolver's injectable clock (`now_serial`), pinned deterministically
    //     in tests + conformance. See docs/format.md §14. ---
    FuncDef {
        name: "DATE",
        min_args: 3,
        max_args: Some(3),
        eval: date_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "YEAR",
        min_args: 1,
        max_args: Some(1),
        eval: year_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "MONTH",
        min_args: 1,
        max_args: Some(1),
        eval: month_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "DAY",
        min_args: 1,
        max_args: Some(1),
        eval: day_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "EDATE",
        min_args: 2,
        max_args: Some(2),
        eval: edate_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "DATEDIF",
        min_args: 3,
        max_args: Some(3),
        eval: datedif_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "TODAY",
        min_args: 0,
        max_args: Some(0),
        eval: today_fn,
        validate: None,
        volatile: true,
    },
    FuncDef {
        name: "NOW",
        min_args: 0,
        max_args: Some(0),
        eval: now_fn,
        validate: None,
        volatile: true,
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

// ---------------------------------------------------------------------------------------------
// Text batch v1: CONCAT TEXTJOIN LEFT RIGHT MID LEN FIND SEARCH SUBSTITUTE REPLACE TRIM UPPER LOWER
// TEXT. Every function coerces a text argument through eval.rs's `to_text` (so a number takes its
// GENERAL form, a boolean → TRUE/FALSE, a blank → "", an error PROPAGATES) — the exact rule the `&`
// operator uses, so the function forms and the operator agree. The Excel-semantics calls pinned here,
// each worth a reviewer's eye:
//   * POSITIONS ARE 1-BASED and count CHARACTERS (Unicode scalar values, `char`s — not bytes); an
//     ASCII fixture is byte==char so the distinction is invisible there, but a multi-byte string
//     indexes by char. LEFT/RIGHT/MID CLAMP an out-of-range count to the string's edge (never panic);
//     a NEGATIVE count/start is `#VALUE!`.
//   * FIND is CASE-SENSITIVE with no wildcards; SEARCH is CASE-INSENSITIVE (ASCII fold, matching the
//     rest of the engine's text equality) and honours the `?`(one char) / `*`(any run) wildcards with
//     `~` escaping. Both return the 1-based START position of the match; a miss is `#VALUE!`; an empty
//     needle returns `start_num`. A `start_num` past `len+1` is `#VALUE!`.
//   * SUBSTITUTE replaces the Nth (with `instance_num`) or ALL (without) NON-OVERLAPPING occurrences,
//     CASE-SENSITIVELY; an EMPTY `old_text` returns the text unchanged (Excel); `instance_num < 1` is
//     `#VALUE!`; an Nth that does not exist returns the text unchanged.
//   * REPLACE is POSITIONAL — it splices out `num_chars` chars from `start_num` and inserts `new_text`
//     (clamping a `start_num` past the end to an append, and `num_chars` past the end to "to the end").
//   * TRIM removes leading/trailing ASCII spaces and COLLAPSES interior runs to a single space (Excel
//     TRIM touches only 0x20, never a tab).
//   * TEXT renders a value through a SUPPORTED format-code subset (docs/format.md §13); an unsupported
//     LITERAL format is refused at PARSE (`validate_text_format` → `unsupported-format`), while a
//     NON-LITERAL (computed) format is accepted and deferred — `text_fn`'s `None` arm returns `#VALUE!`
//     iff the RESOLVED format is unsupported (accept-under-uncertainty, never a false-reject). The
//     1900 date system with Excel's leap-year bug is the epoch call (see `serial_to_ymd`).
// ---------------------------------------------------------------------------------------------

/// Coerce one argument to its Excel text form (general number format, `TRUE`/`FALSE`, `""` for blank),
/// propagating an error and rejecting a multi-cell array (`#VALUE!`). The shared front-door every text
/// function uses for a text-typed argument.
fn arg_text(ctx: &mut EvalCtx, e: &Expr) -> Result<String, ErrKind> {
    to_text(&ctx.eval(e))
}

/// `CONCAT(text1, …)` — concatenate the text of every datum, FLATTENING ranges row-major (a blank cell
/// contributes `""`, a number its general text); an error at ANY position propagates. This is the
/// function form of the `&` operator, minus the delimiter/skip logic `TEXTJOIN` adds.
fn concat_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let mut out = String::new();
    for a in args {
        match ctx.eval(a) {
            Value::Array(_, cells) => {
                for c in &cells {
                    match to_text(c) {
                        Ok(s) => out.push_str(&s),
                        Err(k) => return Value::Error(k),
                    }
                }
            }
            other => match to_text(&other) {
                Ok(s) => out.push_str(&s),
                Err(k) => return Value::Error(k),
            },
        }
    }
    Value::Text(out)
}

/// `TEXTJOIN(delimiter, ignore_empty, text1, …)` — join the text of every datum (ranges flattened)
/// with `delimiter`; when `ignore_empty` is TRUE, a piece that renders to `""` (a blank cell or an
/// empty string) is dropped BEFORE joining (so no doubled delimiter). An error in any piece — or in
/// the delimiter / flag — propagates.
fn textjoin_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let delim = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let ignore_empty = match coerce_bool(&ctx.eval(&args[1])) {
        Ok(b) => b,
        Err(k) => return Value::Error(k),
    };
    let mut pieces: Vec<String> = Vec::new();
    let push = |s: String, out: &mut Vec<String>| {
        if !(ignore_empty && s.is_empty()) {
            out.push(s);
        }
    };
    for a in &args[2..] {
        match ctx.eval(a) {
            Value::Array(_, cells) => {
                for c in &cells {
                    match to_text(c) {
                        Ok(s) => push(s, &mut pieces),
                        Err(k) => return Value::Error(k),
                    }
                }
            }
            other => match to_text(&other) {
                Ok(s) => push(s, &mut pieces),
                Err(k) => return Value::Error(k),
            },
        }
    }
    Value::Text(pieces.join(&delim))
}

/// Evaluate an argument to a NON-NEGATIVE character count, truncating a fractional value toward zero
/// (Excel) and mapping a negative to `#VALUE!`. A huge value saturates on the `as usize` cast and is
/// clamped by the caller against the string length.
fn count_arg(ctx: &mut EvalCtx, e: &Expr) -> Result<usize, ErrKind> {
    let n = one_num(ctx, e)?.trunc();
    if n < 0.0 {
        return Err(ErrKind::Value);
    }
    Ok(n as usize)
}

/// `LEFT(text, [num_chars])` — the leftmost `num_chars` (default 1) characters, clamped to the string
/// length; a negative count is `#VALUE!`.
fn left_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let s = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let n = if args.len() == 2 {
        match count_arg(ctx, &args[1]) {
            Ok(n) => n,
            Err(k) => return Value::Error(k),
        }
    } else {
        1
    };
    let chars: Vec<char> = s.chars().collect();
    let take = n.min(chars.len());
    Value::Text(chars[..take].iter().collect())
}

/// `RIGHT(text, [num_chars])` — the rightmost `num_chars` (default 1) characters, clamped; a negative
/// count is `#VALUE!`.
fn right_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let s = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let n = if args.len() == 2 {
        match count_arg(ctx, &args[1]) {
            Ok(n) => n,
            Err(k) => return Value::Error(k),
        }
    } else {
        1
    };
    let chars: Vec<char> = s.chars().collect();
    let take = n.min(chars.len());
    Value::Text(chars[chars.len() - take..].iter().collect())
}

/// `MID(text, start_num, num_chars)` — up to `num_chars` characters from the 1-based `start_num`. A
/// `start_num < 1` or a `num_chars < 0` is `#VALUE!`; a `start_num` past the end yields `""`; the take
/// is clamped to the remaining length.
fn mid_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let s = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let start = match one_num(ctx, &args[1]) {
        Ok(x) => x.trunc(),
        Err(k) => return Value::Error(k),
    };
    let count = match count_arg(ctx, &args[2]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    if start < 1.0 {
        return Value::Error(ErrKind::Value);
    }
    let chars: Vec<char> = s.chars().collect();
    let start_idx = (start as usize) - 1;
    if start_idx >= chars.len() {
        return Value::Text(String::new());
    }
    let take = count.min(chars.len() - start_idx);
    Value::Text(chars[start_idx..start_idx + take].iter().collect())
}

/// `LEN(text)` — the number of CHARACTERS in the value's text form (`LEN(TRUE) = 4`, `LEN(12.5) = 4`).
fn len_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match arg_text(ctx, &args[0]) {
        Ok(s) => Value::Number(s.chars().count() as f64),
        Err(k) => Value::Error(k),
    }
}

/// Shared start-position resolution for `FIND`/`SEARCH`: evaluate the optional `start_num` (default
/// 1), reject `< 1` as `#VALUE!`, and return the 0-based start index. `hay_len` is the haystack's char
/// count; a start past `len + 1` is `#VALUE!` (Excel), while `len + 1` itself is legal (it only
/// matches an empty needle at the end).
fn find_start(ctx: &mut EvalCtx, args: &[Expr], hay_len: usize) -> Result<usize, ErrKind> {
    let start = if args.len() == 3 {
        one_num(ctx, &args[2])?.trunc()
    } else {
        1.0
    };
    if start < 1.0 {
        return Err(ErrKind::Value);
    }
    let idx = (start as usize) - 1;
    if idx > hay_len {
        return Err(ErrKind::Value);
    }
    Ok(idx)
}

/// `FIND(find_text, within_text, [start_num])` — the 1-based char position of the first CASE-SENSITIVE
/// occurrence of `find_text` in `within_text` at/after `start_num`; a miss is `#VALUE!`. An empty
/// `find_text` returns `start_num`. No wildcards.
fn find_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let needle = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let hay = match arg_text(ctx, &args[1]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let hay_chars: Vec<char> = hay.chars().collect();
    let start_idx = match find_start(ctx, args, hay_chars.len()) {
        Ok(i) => i,
        Err(k) => return Value::Error(k),
    };
    let needle_chars: Vec<char> = needle.chars().collect();
    match find_sub(&hay_chars, &needle_chars, start_idx) {
        Some(i) => Value::Number((i + 1) as f64),
        None => Value::Error(ErrKind::Value),
    }
}

/// The first index `>= from` at which `needle` occurs verbatim (case-sensitive) in `hay`. An empty
/// needle matches at `from` when `from <= hay.len()`.
fn find_sub(hay: &[char], needle: &[char], from: usize) -> Option<usize> {
    if needle.is_empty() {
        return (from <= hay.len()).then_some(from);
    }
    if needle.len() > hay.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

/// `SEARCH(find_text, within_text, [start_num])` — like `FIND` but CASE-INSENSITIVE (ASCII fold) and
/// honouring the `?`(one char) / `*`(any run) wildcards, with `~` escaping a literal `?`/`*`/`~`.
/// Returns the 1-based START position of the first match; a miss is `#VALUE!`.
fn search_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let pattern = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let hay = match arg_text(ctx, &args[1]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let hay_chars: Vec<char> = hay.chars().collect();
    let start_idx = match find_start(ctx, args, hay_chars.len()) {
        Ok(i) => i,
        Err(k) => return Value::Error(k),
    };
    let toks = parse_wild(&pattern);
    for p in start_idx..=hay_chars.len() {
        if wild_prefix(&toks, &hay_chars[p..]) {
            return Value::Number((p + 1) as f64);
        }
    }
    Value::Error(ErrKind::Value)
}

/// A wildcard-pattern token for `SEARCH`.
enum Wild {
    /// `*` — any run of characters (including empty).
    Star,
    /// `?` — exactly one character.
    Any,
    /// A literal character (case-folded on compare).
    Lit(char),
}

/// Tokenize a `SEARCH` pattern: `*`/`?` are wildcards, `~` escapes the next char to a literal (a
/// trailing `~` is itself a literal `~`).
fn parse_wild(pattern: &str) -> Vec<Wild> {
    let mut toks = Vec::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => toks.push(Wild::Star),
            '?' => toks.push(Wild::Any),
            '~' => toks.push(Wild::Lit(chars.next().unwrap_or('~'))),
            other => toks.push(Wild::Lit(other)),
        }
    }
    toks
}

/// Whether `toks` matches a PREFIX of `text` (the match need not consume all of `text` — `SEARCH`
/// finds a substring anchored at the current start). Case-insensitive (ASCII fold) on a literal,
/// matching the engine's text-equality convention.
///
/// ITERATIVE, single-star-backtrack (the classic greedy wildcard matcher, `O(text·pattern)`). A `*`
/// records its position and the text index it began at; on a later mismatch we rewind to that star and
/// let it swallow one more character. This deliberately replaces a recursive `*`-splits-every-way
/// walk, whose branching made a multi-star pattern EXPONENTIAL in time (a ReDoS: `SEARCH("*a*a*…*b",
/// <run of 'a's>)` could run unbounded) — the greedy form only ever advances the saved star's text
/// index, so total work is bounded by `text.len() · toks.len()`.
fn wild_prefix(toks: &[Wild], text: &[char]) -> bool {
    let n = text.len();
    let mut ti = 0; // text cursor
    let mut pi = 0; // pattern cursor
    // The last `*` we passed and the text index it started matching at (for backtracking).
    let mut star: Option<(usize, usize)> = None;
    while pi < toks.len() {
        match &toks[pi] {
            Wild::Star => {
                star = Some((pi, ti));
                pi += 1;
            }
            Wild::Any if ti < n => {
                pi += 1;
                ti += 1;
            }
            Wild::Lit(c) if ti < n && text[ti].eq_ignore_ascii_case(c) => {
                pi += 1;
                ti += 1;
            }
            // Mismatch (or text exhausted for a non-star token): rewind to the last `*` and let it
            // consume one more character; with no `*` to fall back on, the prefix cannot match.
            _ => match star {
                Some((sp, st)) if st < n => {
                    ti = st + 1;
                    star = Some((sp, st + 1));
                    pi = sp + 1;
                }
                _ => return false,
            },
        }
    }
    // Pattern fully consumed — a prefix matched; any leftover `text` is fine (this is a prefix match).
    true
}

/// `SUBSTITUTE(text, old_text, new_text, [instance_num])` — replace the Nth (with `instance_num`) or
/// ALL (without) non-overlapping CASE-SENSITIVE occurrences of `old_text` with `new_text`. An empty
/// `old_text`, or an `instance_num` past the last occurrence, returns `text` unchanged; `instance_num
/// < 1` is `#VALUE!`.
fn substitute_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let text = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let old = match arg_text(ctx, &args[1]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let new = match arg_text(ctx, &args[2]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    if old.is_empty() {
        return Value::Text(text);
    }
    if args.len() == 4 {
        let n = match one_num(ctx, &args[3]) {
            Ok(x) => x.trunc(),
            Err(k) => return Value::Error(k),
        };
        if n < 1.0 {
            return Value::Error(ErrKind::Value);
        }
        let target = n as usize;
        let mut result = String::new();
        let mut last = 0;
        let mut count = 0usize;
        for (idx, m) in text.match_indices(&old) {
            count += 1;
            if count == target {
                result.push_str(&text[last..idx]);
                result.push_str(&new);
                last = idx + m.len();
                break;
            }
        }
        result.push_str(&text[last..]);
        Value::Text(result)
    } else {
        Value::Text(text.replace(&old, &new))
    }
}

/// `REPLACE(old_text, start_num, num_chars, new_text)` — splice out `num_chars` characters starting at
/// the 1-based `start_num` and insert `new_text`. `start_num < 1` or `num_chars < 0` is `#VALUE!`; a
/// `start_num` past the end appends, and `num_chars` past the end deletes to the end.
fn replace_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let old = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let start = match one_num(ctx, &args[1]) {
        Ok(x) => x.trunc(),
        Err(k) => return Value::Error(k),
    };
    let num = match count_arg(ctx, &args[2]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let new = match arg_text(ctx, &args[3]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    if start < 1.0 {
        return Value::Error(ErrKind::Value);
    }
    let chars: Vec<char> = old.chars().collect();
    let start_idx = ((start as usize) - 1).min(chars.len());
    let take = num.min(chars.len() - start_idx);
    let mut result: String = chars[..start_idx].iter().collect();
    result.push_str(&new);
    result.extend(chars[start_idx + take..].iter());
    Value::Text(result)
}

/// `TRIM(text)` — strip leading/trailing ASCII spaces and collapse each interior run of spaces to a
/// single space (Excel TRIM touches only 0x20 — a tab or other whitespace rides through untouched).
fn trim_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match arg_text(ctx, &args[0]) {
        Ok(s) => {
            let collapsed = s
                .split(' ')
                .filter(|w| !w.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            Value::Text(collapsed)
        }
        Err(k) => Value::Error(k),
    }
}

/// `UPPER(text)` — the value's text form upper-cased (full Unicode case mapping).
fn upper_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match arg_text(ctx, &args[0]) {
        Ok(s) => Value::Text(s.to_uppercase()),
        Err(k) => Value::Error(k),
    }
}

/// `LOWER(text)` — the value's text form lower-cased (full Unicode case mapping).
fn lower_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match arg_text(ctx, &args[0]) {
        Ok(s) => Value::Text(s.to_lowercase()),
        Err(k) => Value::Error(k),
    }
}

// --- TEXT() and its format-code subset ------------------------------------------------------
// The supported subset is documented in docs/format.md §13 and single-sourced in `classify_format`,
// which BOTH the parser's `validate_text_format` (refuse an unsupported LITERAL format up front) and
// `text_fn` (render a resolved format, `#VALUE!` on an unsupported one) consult — so what parses and
// what renders can never drift.

/// The supported TEXT format-code kinds (docs/format.md §13). Anything else classifies to `None` and
/// is refused at parse.
#[derive(Clone, Copy)]
enum Fmt {
    /// `General` — the value's general text form.
    General,
    /// A `0`/`0.00` fixed-decimal mask: `int_min` leading-zero-padded integer digits, `decimals`
    /// fractional places.
    Fixed { int_min: usize, decimals: usize },
    /// A `#,##0`/`#,##0.00` thousands-grouped mask with `decimals` fractional places.
    Thousands { decimals: usize },
    /// A `0%`/`0.00%` percent mask with `decimals` fractional places (value ×100, trailing `%`).
    Percent { decimals: usize },
    /// The `yyyy-mm-dd` date mask (1900 date system — see `serial_to_ymd`).
    DateYmd,
}

/// Classify a TEXT format string into the supported subset, or `None` if unsupported. The ONE source
/// of truth for both the parse-time gate and the render path.
fn classify_format(fmt: &str) -> Option<Fmt> {
    if fmt.eq_ignore_ascii_case("General") {
        return Some(Fmt::General);
    }
    if fmt.eq_ignore_ascii_case("yyyy-mm-dd") {
        return Some(Fmt::DateYmd);
    }
    // Percent: a `0`-mask followed by a single trailing `%`.
    if let Some(mask) = fmt.strip_suffix('%') {
        if let Some((int_min, decimals)) = parse_zero_mask(mask) {
            // The integer part of a percent mask is a plain `0…` run (no grouping).
            if int_min >= 1 {
                return Some(Fmt::Percent { decimals });
            }
        }
        return None;
    }
    // Thousands: the literal `#,##0` integer group, optionally `.0…` fractional places.
    if let Some(rest) = fmt.strip_prefix("#,##0") {
        return parse_decimals(rest).map(|decimals| Fmt::Thousands { decimals });
    }
    // Fixed: a plain `0…`(`.0…`) mask.
    parse_zero_mask(fmt).map(|(int_min, decimals)| Fmt::Fixed { int_min, decimals })
}

/// Parse a `0`-only mask like `0`, `00`, `0.00` into `(int_min_digits, decimals)`. The integer part
/// must be a non-empty run of `0`; an optional `.` introduces a non-empty run of `0` decimals. Any
/// other character (or an empty part) is unsupported (`None`).
fn parse_zero_mask(mask: &str) -> Option<(usize, usize)> {
    let (int_part, frac_part) = match mask.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (mask, None),
    };
    if int_part.is_empty() || !int_part.bytes().all(|b| b == b'0') {
        return None;
    }
    let decimals = match frac_part {
        None => 0,
        Some(f) if !f.is_empty() && f.bytes().all(|b| b == b'0') => f.len(),
        Some(_) => return None,
    };
    Some((int_part.len(), decimals))
}

/// Parse the fractional tail of a thousands mask (`""` → 0 places, or `.0…` → that many), rejecting
/// anything else.
fn parse_decimals(rest: &str) -> Option<usize> {
    if rest.is_empty() {
        return Some(0);
    }
    let frac = rest.strip_prefix('.')?;
    (!frac.is_empty() && frac.bytes().all(|b| b == b'0')).then_some(frac.len())
}

/// `TEXT(value, format)` — render `value` through the supported format subset (docs/format.md §13).
/// A LITERAL format was vetted by `validate_text_format` at parse; a NON-LITERAL (computed) format
/// reaches here unvetted, so the `None` arm is a LIVE path — an unsupported RESOLVED format (e.g.
/// `TEXT(A1, B1)` where `B1` is a currency mask) is `#VALUE!`, never a wrong guess (accept-under-
/// uncertainty: the parse-time gate deferred to this eval-time check). An error `value` propagates; a
/// value that a numeric/date format cannot coerce to a number is `#VALUE!`.
fn text_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let value = scalarize(ctx.eval(&args[0]));
    if let Value::Error(k) = value {
        return Value::Error(k);
    }
    let fmt = match arg_text(ctx, &args[1]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let Some(kind) = classify_format(&fmt) else {
        return Value::Error(ErrKind::Value);
    };
    match render_format(&value, kind) {
        Ok(s) => Value::Text(s),
        Err(k) => Value::Error(k),
    }
}

/// Render a (non-error) scalar value through a vetted format kind.
fn render_format(value: &Value, kind: Fmt) -> Result<String, ErrKind> {
    match kind {
        Fmt::General => Ok(to_text(value)?),
        Fmt::Fixed { int_min, decimals } => {
            Ok(format_number(coerce_num(value)?, decimals, int_min, false))
        }
        Fmt::Thousands { decimals } => Ok(format_number(coerce_num(value)?, decimals, 1, true)),
        Fmt::Percent { decimals } => {
            Ok(format_number(coerce_num(value)? * 100.0, decimals, 1, false) + "%")
        }
        Fmt::DateYmd => format_date_ymd(coerce_num(value)?),
    }
}

/// Format `n` with `decimals` fractional places (half-away-from-zero), a minimum of `int_min` integer
/// digits (leading-zero padded), and optional thousands grouping. The workhorse behind the fixed /
/// thousands / percent masks.
fn format_number(n: f64, decimals: usize, int_min: usize, grouping: bool) -> String {
    let (neg, mut int_digits, frac_digits) = split_scaled(n, decimals);
    while int_digits.len() < int_min {
        int_digits.insert(0, '0');
    }
    if grouping {
        int_digits = group_thousands(&int_digits);
    }
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    out.push_str(&int_digits);
    if decimals > 0 {
        out.push('.');
        out.push_str(&frac_digits);
    }
    out
}

/// Scale `|n|` by `10^decimals`, round half-away-from-zero, and split back into `(is_negative,
/// integer_digits, fractional_digits)`. The sign is dropped when the rounded magnitude is zero (Excel
/// shows `0.00`, never `-0.00`).
fn split_scaled(n: f64, decimals: usize) -> (bool, String, String) {
    let factor = 10f64.powi(decimals as i32);
    let scaled = (n.abs() * factor).round();
    let neg = n < 0.0 && scaled != 0.0;
    let mut digits = format!("{scaled:.0}");
    while digits.len() < decimals + 1 {
        digits.insert(0, '0');
    }
    let split = digits.len() - decimals;
    let int_digits = digits[..split].to_string();
    let frac_digits = digits[split..].to_string();
    (neg, int_digits, frac_digits)
}

/// Insert `,` thousands separators into a run of ASCII digits.
fn group_thousands(int_digits: &str) -> String {
    let n = int_digits.len();
    let mut out = String::with_capacity(n + n / 3);
    for (i, ch) in int_digits.chars().enumerate() {
        if i > 0 && (n - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Render an Excel date serial as `yyyy-mm-dd`. The integer day is `floor`ed from the serial, then
/// gated to the valid Excel date band `[1, MAX_SERIAL]` (1900-01-01 … 9999-12-31) — the SAME band the
/// sibling reader [`date_serial_arg`] enforces for YEAR/MONTH/DAY/EDATE/DATEDIF. A serial `< 1` (before
/// the 1900 epoch, rather than Excel's fictional `1900-01-00`), a serial past 9999-12-31, or a `NaN`
/// is `#VALUE!` — one located refusal consistent with every other TEXT format failure. The upper gate
/// is load-bearing: without it a large serial (`=TEXT(1e300,"yyyy-mm-dd")`) flows into `serial_to_ymd`
/// → `civil_from_days` and OVERFLOWS `i64` at `z + 719_468` — a panic under overflow-checks, or a
/// silently-wrapped nonsense date in release. The refusal replaces both with the correct located hole.
fn format_date_ymd(serial: f64) -> Result<String, ErrKind> {
    let day = serial.floor();
    if !(1.0..=MAX_SERIAL as f64).contains(&day) {
        return Err(ErrKind::Value);
    }
    let (y, m, d) = serial_to_ymd(day as i64);
    Ok(format!("{y:04}-{m:02}-{d:02}"))
}

/// Unix day index of 1899-12-31 (Excel "serial 0" in the pre-bug half); the shared epoch anchor for
/// both directions of the serial↔date map (`serial_to_ymd` and its inverse `serial_from_ymd`).
const EPOCH_1899_12_31: i64 = -25568;

/// Convert an Excel date serial (integer, `>= 1`) to a proleptic-Gregorian `(year, month, day)` in the
/// **1900 date system, WITH Excel's leap-year bug replicated** (serial 60 is the fictional
/// `1900-02-29`; serials `>= 61` are shifted back one day to skip it, so serial 61 is `1900-03-01`).
/// This epoch/bug fidelity is the load-bearing date call — a real Excel-authored serial round-trips.
fn serial_to_ymd(serial: i64) -> (i64, u32, u32) {
    // The phantom leap day Excel invented has no real civil date; report it verbatim.
    if serial == 60 {
        return (1900, 2, 29);
    }
    // Serials 1..59 add straight through (serial 1 = 1900-01-01); serials > 60 lose one day (the
    // phantom 1900-02-29) so the calendar re-aligns with reality (serial 61 = 1900-03-01).
    let unix_days = if serial < 60 {
        EPOCH_1899_12_31 + serial
    } else {
        EPOCH_1899_12_31 + serial - 1
    };
    civil_from_days(unix_days)
}

/// Howard Hinnant's `civil_from_days`: days since the Unix epoch (1970-01-01) → proleptic-Gregorian
/// `(year, month, day)`. Exact integer arithmetic, valid across the whole date range v1 cares about.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (y + i64::from(m <= 2), m, d)
}

/// Parse-time gate for `TEXT`: refuse ONLY an UNSUPPORTED STRING LITERAL format code (docs/format.md
/// §13) — a statically-known-wrong format is caught up front rather than mis-rendered at eval. A
/// non-literal format (a reference/computed string, e.g. `TEXT(A1, B1)`) is ACCEPTED and deferred to
/// `text_fn`, which returns `#VALUE!` iff the RESOLVED format turns out unsupported. This is
/// accept-under-uncertainty (ast-standards PART 6): a false-reject is the cardinal sin, so a dynamic
/// format that RESOLVES to a supported code (`B1="0.00"`) — which real Excel accepts and computes —
/// must not be rejected up front; the only deferred gap is a false-*negative* (an unsupported dynamic
/// format becomes eval's `#VALUE!`, not a parse refusal). Registered as `TEXT`'s `validate` row so the
/// check stays registry data, not a hand-fork in the parser.
fn validate_text_format(args: &[Expr], span: Span) -> Result<(), Diag> {
    // Arity (exactly 2) is already checked; guard defensively so a synthesized short call can't panic.
    match args.get(1) {
        // The one static-certainty case: a literal format string that is NOT in the supported subset.
        Some(Expr::Lit(Value::Text(fmt))) if classify_format(fmt).is_none() => Err(Diag::new(
            DiagCode::UnsupportedFormat,
            span,
            format!(
                "TEXT format code {fmt:?} is not in the supported v1 subset (docs/format.md §13)"
            ),
        )),
        // A supported literal, OR any non-literal format v1 cannot vet statically: accept and defer to
        // eval's resolved-format `#VALUE!` rather than false-reject a call Excel would compute.
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------------------------
// Date/time batch v1: DATE YEAR MONTH DAY EDATE DATEDIF TODAY NOW.
//
// EPOCH (the load-bearing call, worth a reviewer's eye — docs/format.md §14, and §13.2 for the
// shared serial↔date mapping): the Excel 1900 date-serial system, WITH Excel's 1900 leap-year bug
// REPLICATED (serial 60 = the fictional 1900-02-29; serials ≥ 61 shift back one day), so a serial an
// xlsx round-trip authored in Excel maps to the same civil date later. `serial_to_ymd` (the forward
// map, already used by TEXT's date render) and `serial_from_ymd` (its inverse) are the one place that
// bug lives; DATE/EDATE build a serial by day-offset arithmetic in the CONTIGUOUS serial space, so
// DATE(1900,2,29) reproduces the phantom serial 60 with no special case.
//
// VOLATILITY: TODAY/NOW read the resolver's INJECTABLE clock (`EvalCtx::now_serial` →
// `Resolver::now_serial`), never `std::time` from here — so conformance/tests PIN "now" to a fixed
// instant (`Resolver`'s `PINNED_NOW_SERIAL` = 2023-01-01T12:00, serial 44927.5) and every fixture is
// reproducible, while production's default impl returns real system time. Both rows are `volatile:
// true` in the registry.
//
// COERCION/DOMAIN: DATE truncates its year/month/day toward zero (Excel) and NORMALIZES an
// out-of-range month/day (DATE(2008,14,2) = 2009-02-02; DATE(2023,3,0) = 2023-02-28), folding a year
// in 0..=1899 by +1900. The serial readers (YEAR/MONTH/DAY/EDATE/DATEDIF) `floor` their serial
// argument (matching TEXT's date render) and accept the valid band [1, 2958465] (1900-01-01 …
// 9999-12-31); a serial or a DATE/EDATE result outside it is #NUM!. DATEDIF with start > end is
// #NUM! and an unknown unit is #NUM!. A non-coercible argument is #VALUE!; an error propagates.
// ---------------------------------------------------------------------------------------------

/// The largest valid Excel date serial: 9999-12-31.
const MAX_SERIAL: i64 = 2_958_465;

/// Whether `y` is a leap year in the proleptic-Gregorian calendar. (The 1900 leap-year *bug* is NOT
/// applied here — it lives only in the serial↔date mapping; normalization arithmetic uses the real
/// calendar, and v1's date fixtures stay clear of Jan/Feb 1900 where the two would disagree.)
fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// The number of days in month `m` (1-based) of year `y`.
fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        // Every caller passes a month already normalized into 1..=12 (EDATE's `rem_euclid(12) + 1`,
        // DATEDIF's `serial_to_ymd`-derived month), so no other arm is reachable; assert it so an
        // impossible month is a located panic rather than a plausible-but-wrong 30.
        _ => unreachable!("days_in_month expects a normalized 1..=12 month, got {m}"),
    }
}

/// Howard Hinnant's `days_from_civil`: a proleptic-Gregorian `(year, month, day)` → days since the
/// Unix epoch (1970-01-01). Exact integer arithmetic; the inverse of [`civil_from_days`].
const fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 }; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// Convert a proleptic-Gregorian `(year, month, day)` to an Excel 1900-system date serial, replicating
/// Excel's leap-year bug: a date on/after 1900-03-01 is shifted `+1` (Excel counts the phantom
/// 1900-02-29 in the run-up, so serial 61 = 1900-03-01), while an earlier date passes straight through
/// (serial 1 = 1900-01-01). The inverse of [`serial_to_ymd`] for every real civil date (the phantom
/// serial 60 has no civil pre-image — it is produced only by the forward map / by day-offset
/// arithmetic). The returned serial may fall outside the valid band for a pre-epoch input; callers
/// gate the range.
fn serial_from_ymd(y: i64, m: u32, d: u32) -> i64 {
    // On/after 1900-03-01 the phantom leap day sits in the count → shift +1. The threshold is a
    // compile-time constant (const-folded, not recomputed per call).
    const LEAP_BUG_SHIFT_THRESHOLD: i64 = days_from_civil(1900, 3, 1);
    let unix_days = days_from_civil(y, m as i64, d as i64);
    let serial = unix_days - EPOCH_1899_12_31;
    if unix_days >= LEAP_BUG_SHIFT_THRESHOLD {
        serial + 1
    } else {
        serial
    }
}

/// Evaluate a DATE year/month/day argument to an integer, TRUNCATING toward zero (Excel) and rejecting
/// a value outside a safe band (`|x| ≤ 1e15`) as #NUM! so the later `as i64` never saturates. A
/// non-coercible value is #VALUE!; an error propagates.
fn date_int_arg(ctx: &mut EvalCtx, e: &Expr) -> Result<i64, ErrKind> {
    let n = one_num(ctx, e)?.trunc();
    if n.abs() > 1e15 {
        return Err(ErrKind::Num);
    }
    Ok(n as i64)
}

/// Evaluate a serial-valued argument (YEAR/MONTH/DAY/EDATE/DATEDIF): coerce, `floor` to the integer day
/// (matching TEXT's date render), and gate the valid serial band [1, MAX_SERIAL] — a serial before
/// 1900-01-01 or after 9999-12-31 is #NUM!. A non-coercible value is #VALUE!; an error propagates.
fn date_serial_arg(ctx: &mut EvalCtx, e: &Expr) -> Result<i64, ErrKind> {
    let n = one_num(ctx, e)?.floor();
    if !(1.0..=MAX_SERIAL as f64).contains(&n) {
        return Err(ErrKind::Num);
    }
    Ok(n as i64)
}

/// `DATE(year, month, day)` — build a date serial, NORMALIZING an out-of-range month/day (they roll
/// over into adjacent years/months) and folding a year in 0..=1899 by +1900 (Excel). A year outside
/// 0..=9999, or a normalized serial outside [1, MAX_SERIAL], is #NUM!.
fn date_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let y = match date_int_arg(ctx, &args[0]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let m = match date_int_arg(ctx, &args[1]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let d = match date_int_arg(ctx, &args[2]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    // Excel year rule: 0..=1899 folds into the 1900s; a year outside 0..=9999 is #NUM!.
    let year = if (0..=1899).contains(&y) { y + 1900 } else { y };
    if !(0..=9999).contains(&year) {
        return Value::Error(ErrKind::Num);
    }
    // Normalize the (possibly out-of-range) month into a (year, month) with month in 1..=12, then
    // build the serial of that month's day 1 and add (day − 1) days — day roll-over is just serial
    // arithmetic in the contiguous serial space (which includes the phantom serial 60).
    let total = year * 12 + (m - 1);
    let ny = total.div_euclid(12);
    let nm = (total.rem_euclid(12) + 1) as u32;
    if !(0..=9999).contains(&ny) {
        return Value::Error(ErrKind::Num);
    }
    let serial = serial_from_ymd(ny, nm, 1) + (d - 1);
    if !(1..=MAX_SERIAL).contains(&serial) {
        return Value::Error(ErrKind::Num);
    }
    Value::Number(serial as f64)
}

/// `YEAR(serial)` — the Gregorian year of a date serial (1900 system, leap-bug faithful:
/// `YEAR(60) = 1900`).
fn year_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match date_serial_arg(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok(s) => Value::Number(serial_to_ymd(s).0 as f64),
    }
}

/// `MONTH(serial)` — the 1-based month of a date serial (`MONTH(60) = 2`, the phantom 1900-02-29).
fn month_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match date_serial_arg(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok(s) => Value::Number(serial_to_ymd(s).1 as f64),
    }
}

/// `DAY(serial)` — the 1-based day-of-month of a date serial (`DAY(60) = 29`, the phantom day).
fn day_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match date_serial_arg(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok(s) => Value::Number(serial_to_ymd(s).2 as f64),
    }
}

/// `EDATE(start_date, months)` — the date `months` months from `start_date` (a serial), CLAMPING the
/// day to the target month's last day (`EDATE(2020-01-31, 1) = 2020-02-29`). `months` truncates toward
/// zero; a result outside [1, MAX_SERIAL] is #NUM!.
fn edate_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let start = match date_serial_arg(ctx, &args[0]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let months = match date_int_arg(ctx, &args[1]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (y, m, d) = serial_to_ymd(start);
    let total = y * 12 + (m as i64 - 1) + months;
    let ny = total.div_euclid(12);
    let nm = (total.rem_euclid(12) + 1) as u32;
    if !(0..=9999).contains(&ny) {
        return Value::Error(ErrKind::Num);
    }
    let nd = d.min(days_in_month(ny, nm));
    let serial = serial_from_ymd(ny, nm, nd);
    if !(1..=MAX_SERIAL).contains(&serial) {
        return Value::Error(ErrKind::Num);
    }
    Value::Number(serial as f64)
}

/// `DATEDIF(start_date, end_date, unit)` — the elapsed time between two serials in `unit`:
/// `"Y"`/`"M"`/`"D"` (complete years / months / days) plus `"MD"`/`"YM"`/`"YD"` (day/month/day
/// remainders ignoring the larger units). The unit folds case (Excel accepts either). `start > end`
/// is #NUM!; an unknown unit is #NUM!.
fn datedif_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let s = match date_serial_arg(ctx, &args[0]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let e = match date_serial_arg(ctx, &args[1]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let unit = match arg_text(ctx, &args[2]) {
        Ok(u) => u,
        Err(k) => return Value::Error(k),
    };
    if s > e {
        return Value::Error(ErrKind::Num);
    }
    let (y1, m1, d1) = serial_to_ymd(s);
    let (y2, m2, d2) = serial_to_ymd(e);
    let result: i64 = match unit.to_ascii_uppercase().as_str() {
        "D" => e - s,
        "Y" => {
            let mut yr = y2 - y1;
            if (m2, d2) < (m1, d1) {
                yr -= 1;
            }
            yr
        }
        "M" => {
            let mut mo = (y2 - y1) * 12 + (m2 as i64 - m1 as i64);
            if d2 < d1 {
                mo -= 1;
            }
            mo
        }
        // Days ignoring months and years. When the end day is on/after the start day it is a plain
        // difference; otherwise borrow the previous month's length (computed in i64 so the known
        // Excel `MD` borrow corner cannot underflow-panic — v1 fixtures use the clean branch).
        "MD" => {
            if d2 >= d1 {
                (d2 - d1) as i64
            } else {
                let (py, pm) = if m2 == 1 { (y2 - 1, 12) } else { (y2, m2 - 1) };
                days_in_month(py, pm) as i64 - d1 as i64 + d2 as i64
            }
        }
        // Months ignoring years (and the day remainder): the month gap, less one if the end day has
        // not yet reached the start day, folded into 0..=11.
        "YM" => {
            let mut mo = m2 as i64 - m1 as i64;
            if d2 < d1 {
                mo -= 1;
            }
            mo.rem_euclid(12)
        }
        // Days ignoring years: re-home the end's month/day into the start's year (or the next year if
        // it falls before the start's month/day) and take the serial difference.
        "YD" => {
            let (ey, em, ed) = if (m2, d2) >= (m1, d1) {
                (y1, m2, d2)
            } else {
                (y1 + 1, m2, d2)
            };
            serial_from_ymd(ey, em, ed) - serial_from_ymd(y1, m1, d1)
        }
        _ => return Value::Error(ErrKind::Num),
    };
    Value::Number(result as f64)
}

/// `TODAY()` — the current date as an integer serial (the time-of-day fraction FLOORed off). VOLATILE:
/// reads the resolver's injectable clock (pinned in tests/conformance, system time in production), and
/// returns a `Value::Number` usable in arithmetic (`TODAY()+7`).
fn today_fn(ctx: &mut EvalCtx, _args: &[Expr]) -> Value {
    finite_or_num(ctx.now_serial().floor())
}

/// `NOW()` — the current date AND time as a serial: the integer date plus a fractional time-of-day
/// (noon = 0.5). VOLATILE, same injectable clock as [`today_fn`].
fn now_fn(ctx: &mut EvalCtx, _args: &[Expr]) -> Value {
    finite_or_num(ctx.now_serial())
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

    // ---- Text batch v1 ------------------------------------------------------------------------

    fn txt(s: &str) -> Expr {
        Expr::Lit(Value::Text(s.into()))
    }
    fn text(v: Value) -> String {
        match v {
            Value::Text(t) => t,
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn concat_and_textjoin() {
        let g = Grid::new(1, vec![Value::Blank]);
        // CONCAT stringifies each arg (number → general text, bool → TRUE/FALSE).
        assert_eq!(
            eval(
                &call(
                    "CONCAT",
                    vec![txt("a"), num(1.0), Expr::Lit(Value::Bool(true))]
                ),
                &g
            ),
            Value::Text("a1TRUE".into())
        );
        // CONCAT flattens a range (in-range blank → "").
        let r = Grid::new(
            1,
            vec![Value::Text("x".into()), Value::Blank, Value::Number(2.0)],
        );
        assert_eq!(
            eval(&call("CONCAT", vec![col_range(3)]), &r),
            Value::Text("x2".into())
        );
        // TEXTJOIN with ignore_empty=TRUE drops the blank; delimiter between kept pieces.
        assert_eq!(
            eval(
                &call(
                    "TEXTJOIN",
                    vec![txt("-"), Expr::Lit(Value::Bool(true)), col_range(3)]
                ),
                &r
            ),
            Value::Text("x-2".into())
        );
        // ignore_empty=FALSE keeps the empty slot (doubled delimiter).
        assert_eq!(
            eval(
                &call(
                    "TEXTJOIN",
                    vec![txt("-"), Expr::Lit(Value::Bool(false)), col_range(3)]
                ),
                &r
            ),
            Value::Text("x--2".into())
        );
        // An error anywhere propagates.
        let e = Grid::new(1, vec![Value::Number(1.0), Value::Error(ErrKind::Div0)]);
        assert_eq!(
            eval(&call("CONCAT", vec![col_range(2)]), &e),
            Value::Error(ErrKind::Div0)
        );
    }

    #[test]
    fn left_right_mid_len_clamp_and_coerce() {
        let g = Grid::new(1, vec![Value::Blank]);
        assert_eq!(
            text(eval(&call("LEFT", vec![txt("hello"), num(2.0)]), &g)),
            "he"
        );
        assert_eq!(text(eval(&call("LEFT", vec![txt("hi")]), &g)), "h"); // default 1
        assert_eq!(
            text(eval(&call("RIGHT", vec![txt("hello"), num(3.0)]), &g)),
            "llo"
        );
        // Out-of-range count clamps to the whole string.
        assert_eq!(
            text(eval(&call("LEFT", vec![txt("hi"), num(99.0)]), &g)),
            "hi"
        );
        // Negative count is #VALUE!.
        assert_eq!(
            eval(&call("LEFT", vec![txt("hi"), num(-1.0)]), &g),
            Value::Error(ErrKind::Value)
        );
        // MID from 1-based start, clamped take; start past end → "".
        assert_eq!(
            text(eval(
                &call("MID", vec![txt("hello"), num(2.0), num(3.0)]),
                &g
            )),
            "ell"
        );
        assert_eq!(
            text(eval(
                &call("MID", vec![txt("hello"), num(10.0), num(3.0)]),
                &g
            )),
            ""
        );
        assert_eq!(
            eval(&call("MID", vec![txt("hi"), num(0.0), num(1.0)]), &g),
            Value::Error(ErrKind::Value)
        );
        // LEN over the general text of non-text values.
        assert_eq!(
            eval(&call("LEN", vec![txt("hello")]), &g),
            Value::Number(5.0)
        );
        assert_eq!(
            eval(&call("LEN", vec![Expr::Lit(Value::Bool(true))]), &g),
            Value::Number(4.0)
        );
    }

    #[test]
    fn find_is_case_sensitive_search_is_wildcard() {
        let g = Grid::new(1, vec![Value::Blank]);
        // FIND: 1-based, case-SENSITIVE.
        assert_eq!(
            eval(&call("FIND", vec![txt("l"), txt("hello")]), &g),
            Value::Number(3.0)
        );
        assert_eq!(
            eval(&call("FIND", vec![txt("l"), txt("hello"), num(4.0)]), &g),
            Value::Number(4.0)
        );
        // Case mismatch → not found → #VALUE!.
        assert_eq!(
            eval(&call("FIND", vec![txt("H"), txt("hello")]), &g),
            Value::Error(ErrKind::Value)
        );
        // Empty needle returns start_num.
        assert_eq!(
            eval(&call("FIND", vec![txt(""), txt("abc")]), &g),
            Value::Number(1.0)
        );
        // SEARCH: case-INSENSITIVE and wildcards.
        assert_eq!(
            eval(&call("SEARCH", vec![txt("H"), txt("hello")]), &g),
            Value::Number(1.0)
        );
        assert_eq!(
            eval(&call("SEARCH", vec![txt("l?o"), txt("hello")]), &g),
            Value::Number(3.0)
        );
        assert_eq!(
            eval(&call("SEARCH", vec![txt("e*o"), txt("hello")]), &g),
            Value::Number(2.0)
        );
        // A literal `?` via `~?`.
        assert_eq!(
            eval(&call("SEARCH", vec![txt("~?"), txt("a?b")]), &g),
            Value::Number(2.0)
        );
        // start_num past len+1 is #VALUE!.
        assert_eq!(
            eval(&call("FIND", vec![txt("a"), txt("abc"), num(5.0)]), &g),
            Value::Error(ErrKind::Value)
        );
    }

    #[test]
    fn search_multi_star_matches_and_terminates_fast() {
        // Regression: the wildcard matcher is iterative (single-star backtrack), so a MULTI-star
        // pattern is O(text·pattern), not the old exponential recursive backtracking (a ReDoS). Both
        // arms complete instantly; the no-match arm is the one that used to blow up.
        let g = Grid::new(1, vec![Value::Blank]);
        // Multi-star, leftmost anchored match at position 1.
        assert_eq!(
            eval(&call("SEARCH", vec![txt("h*o*d"), txt("hello world")]), &g),
            Value::Number(1.0)
        );
        // The pathological shape: many stars over a long run with NO final match → #VALUE!, fast.
        let hay = "a".repeat(64);
        assert_eq!(
            eval(&call("SEARCH", vec![txt("*a*a*a*a*a*z"), txt(&hay)]), &g),
            Value::Error(ErrKind::Value)
        );
    }

    #[test]
    fn substitute_and_replace() {
        let g = Grid::new(1, vec![Value::Blank]);
        // SUBSTITUTE all occurrences.
        assert_eq!(
            text(eval(
                &call("SUBSTITUTE", vec![txt("a-b-c"), txt("-"), txt("+")]),
                &g
            )),
            "a+b+c"
        );
        // SUBSTITUTE the Nth only.
        assert_eq!(
            text(eval(
                &call(
                    "SUBSTITUTE",
                    vec![txt("a-b-c"), txt("-"), txt("+"), num(2.0)]
                ),
                &g
            )),
            "a-b+c"
        );
        // Empty old_text returns text unchanged.
        assert_eq!(
            text(eval(
                &call("SUBSTITUTE", vec![txt("abc"), txt(""), txt("X")]),
                &g
            )),
            "abc"
        );
        // instance_num < 1 is #VALUE!.
        assert_eq!(
            eval(
                &call("SUBSTITUTE", vec![txt("a-b"), txt("-"), txt("+"), num(0.0)]),
                &g
            ),
            Value::Error(ErrKind::Value)
        );
        // REPLACE positional splice.
        assert_eq!(
            text(eval(
                &call("REPLACE", vec![txt("abcdef"), num(2.0), num(3.0), txt("X")]),
                &g
            )),
            "aXef"
        );
        // start past end appends.
        assert_eq!(
            text(eval(
                &call("REPLACE", vec![txt("ab"), num(9.0), num(0.0), txt("!")]),
                &g
            )),
            "ab!"
        );
    }

    #[test]
    fn trim_upper_lower() {
        let g = Grid::new(1, vec![Value::Blank]);
        assert_eq!(text(eval(&call("TRIM", vec![txt("  a   b  ")]), &g)), "a b");
        assert_eq!(text(eval(&call("UPPER", vec![txt("aBc")]), &g)), "ABC");
        assert_eq!(text(eval(&call("LOWER", vec![txt("aBc")]), &g)), "abc");
    }

    #[test]
    fn text_format_subset_and_error_paths() {
        let g = Grid::new(1, vec![Value::Blank]);
        assert_eq!(
            text(eval(&call("TEXT", vec![num(12.5), txt("0.00")]), &g)),
            "12.50"
        );
        assert_eq!(
            text(eval(&call("TEXT", vec![num(-7.0), txt("0.00")]), &g)),
            "-7.00"
        );
        assert_eq!(
            text(eval(&call("TEXT", vec![num(1234567.0), txt("#,##0")]), &g)),
            "1,234,567"
        );
        assert_eq!(
            text(eval(&call("TEXT", vec![num(0.5), txt("0%")]), &g)),
            "50%"
        );
        assert_eq!(
            text(eval(&call("TEXT", vec![num(0.1234), txt("0.00%")]), &g)),
            "12.34%"
        );
        assert_eq!(
            text(eval(&call("TEXT", vec![num(5.0), txt("General")]), &g)),
            "5"
        );
        // The 1900 date system with the leap-year bug: serial 60 is the phantom 1900-02-29,
        // serial 61 is 1900-03-01, serial 44927 is 2023-01-01.
        assert_eq!(
            text(eval(
                &call("TEXT", vec![num(44927.0), txt("yyyy-mm-dd")]),
                &g
            )),
            "2023-01-01"
        );
        assert_eq!(
            text(eval(&call("TEXT", vec![num(60.0), txt("yyyy-mm-dd")]), &g)),
            "1900-02-29"
        );
        assert_eq!(
            text(eval(&call("TEXT", vec![num(61.0), txt("yyyy-mm-dd")]), &g)),
            "1900-03-01"
        );
        // Serial-band gate (regression: a large serial used to overflow `civil_from_days` — a panic
        // under overflow-checks, a wrapped nonsense date in release — instead of a located refusal).
        // The band `[1, MAX_SERIAL]` is refused as `#VALUE!` on BOTH edges plus `NaN`, never rendered.
        assert_eq!(
            eval(&call("TEXT", vec![num(1e300), txt("yyyy-mm-dd")]), &g),
            Value::Error(ErrKind::Value)
        );
        assert_eq!(
            eval(&call("TEXT", vec![num(2_958_466.0), txt("yyyy-mm-dd")]), &g),
            Value::Error(ErrKind::Value)
        );
        assert_eq!(
            eval(&call("TEXT", vec![num(0.0), txt("yyyy-mm-dd")]), &g),
            Value::Error(ErrKind::Value)
        );
        // The exact upper edge (9999-12-31 = serial 2958465) still renders.
        assert_eq!(
            text(eval(
                &call("TEXT", vec![num(2_958_465.0), txt("yyyy-mm-dd")]),
                &g
            )),
            "9999-12-31"
        );
        // Error propagation and a non-numeric value into a numeric format.
        assert_eq!(
            eval(
                &call(
                    "TEXT",
                    vec![Expr::Lit(Value::Error(ErrKind::Div0)), txt("0.00")]
                ),
                &g
            ),
            Value::Error(ErrKind::Div0)
        );
        assert_eq!(
            eval(&call("TEXT", vec![txt("abc"), txt("0.00")]), &g),
            Value::Error(ErrKind::Value)
        );
    }

    #[test]
    fn text_unsupported_literal_format_is_a_parse_refusal() {
        use crate::parse;
        // A supported literal format parses.
        assert!(parse("=TEXT(1,\"0.00\")").is_ok());
        // An unsupported literal format is a located `unsupported-format` refusal, not a wrong guess.
        let d = parse("=TEXT(1,\"$#,##0.00\")").unwrap_err();
        assert_eq!(d.code, crate::DiagCode::UnsupportedFormat);
    }

    #[test]
    fn text_nonliteral_format_is_accepted_and_deferred_to_eval() {
        use crate::parse;
        // Accept-under-uncertainty (ast-standards PART 6): a computed format v1 cannot vet statically
        // is NOT refused at parse — a false-reject is the cardinal sin, since it RESOLVES-to-supported
        // at runtime (real Excel accepts and computes `=TEXT(A1, B1)`).
        let expr = parse("=TEXT(1,A1)").expect("a non-literal format parses (deferred to eval)");
        // A1 resolves to a SUPPORTED format → computes (the false-reject the old blanket refusal made).
        let supported = Grid::new(1, vec![Value::Text("0.00".to_string())]);
        assert_eq!(eval(&expr, &supported), Value::Text("1.00".to_string()));
        // A1 resolves to an UNSUPPORTED format → the deferred `#VALUE!` (a false-NEGATIVE, allowed).
        let unsupported = Grid::new(1, vec![Value::Text("$#,##0.00".to_string())]);
        assert_eq!(eval(&expr, &unsupported), Value::Error(ErrKind::Value));
    }

    #[test]
    fn date_builds_and_normalizes_a_serial() {
        let g = Grid::new(1, vec![Value::Blank]);
        // A plain in-range date (44927 = 2023-01-01, cross-checked against the TEXT date anchor).
        assert_eq!(
            eval(&call("DATE", vec![num(2023.0), num(1.0), num(1.0)]), &g),
            Value::Number(44927.0)
        );
        // Month roll-over: DATE(2008,14,2) = 2009-02-02 (independently 39846).
        assert_eq!(
            eval(&call("DATE", vec![num(2008.0), num(14.0), num(2.0)]), &g),
            Value::Number(39846.0)
        );
        // Day 0 rolls back to the last day of the previous month: DATE(2023,3,0) = 2023-02-28 (44985).
        assert_eq!(
            eval(&call("DATE", vec![num(2023.0), num(3.0), num(0.0)]), &g),
            Value::Number(44985.0)
        );
        // The two-digit year rule folds 0..=1899 by +1900: DATE(108,1,2) = 2008-01-02 (39449).
        assert_eq!(
            eval(&call("DATE", vec![num(108.0), num(1.0), num(2.0)]), &g),
            Value::Number(39449.0)
        );
        // A year past 9999 is #NUM!.
        assert_eq!(
            eval(&call("DATE", vec![num(10000.0), num(1.0), num(1.0)]), &g),
            Value::Error(ErrKind::Num)
        );
    }

    #[test]
    fn year_month_day_read_a_serial_with_the_leap_bug() {
        let g = Grid::new(1, vec![Value::Blank]);
        // 44927 = 2023-01-01.
        assert_eq!(
            eval(&call("YEAR", vec![num(44927.0)]), &g),
            Value::Number(2023.0)
        );
        assert_eq!(
            eval(&call("MONTH", vec![num(44927.0)]), &g),
            Value::Number(1.0)
        );
        assert_eq!(
            eval(&call("DAY", vec![num(44957.0)]), &g),
            Value::Number(31.0) // 2023-01-31
        );
        // The replicated leap-year bug: serial 60 is the fictional 1900-02-29.
        assert_eq!(
            eval(&call("YEAR", vec![num(60.0)]), &g),
            Value::Number(1900.0)
        );
        assert_eq!(
            eval(&call("MONTH", vec![num(60.0)]), &g),
            Value::Number(2.0)
        );
        assert_eq!(eval(&call("DAY", vec![num(60.0)]), &g), Value::Number(29.0));
        // A serial before the epoch (< 1) is out of the supported domain → #NUM!.
        assert_eq!(
            eval(&call("YEAR", vec![num(0.0)]), &g),
            Value::Error(ErrKind::Num)
        );
    }

    #[test]
    fn edate_clamps_to_end_of_month() {
        let g = Grid::new(1, vec![Value::Blank]);
        // One month forward from 2023-01-01 (44927) = 2023-02-01 (44958).
        assert_eq!(
            eval(&call("EDATE", vec![num(44927.0), num(1.0)]), &g),
            Value::Number(44958.0)
        );
        // Clamp: one month from 2020-01-31 (43861) lands on 2020-02-29 (43890, a leap February).
        assert_eq!(
            eval(&call("EDATE", vec![num(43861.0), num(1.0)]), &g),
            Value::Number(43890.0)
        );
        // Negative months go back: two months before 2023-01-01 = 2022-11-01 (44866).
        assert_eq!(
            eval(&call("EDATE", vec![num(44927.0), num(-2.0)]), &g),
            Value::Number(44866.0)
        );
        // A non-numeric start is #VALUE!.
        assert_eq!(
            eval(
                &call("EDATE", vec![Expr::Lit(Value::Text("x".into())), num(1.0)]),
                &g
            ),
            Value::Error(ErrKind::Value)
        );
    }

    #[test]
    fn datedif_units() {
        let g = Grid::new(1, vec![Value::Blank]);
        let dd = |a: f64, b: f64, u: &str| {
            call(
                "DATEDIF",
                vec![num(a), num(b), Expr::Lit(Value::Text(u.into()))],
            )
        };
        // Whole days.
        assert_eq!(eval(&dd(44927.0, 44957.0, "D"), &g), Value::Number(30.0));
        // Complete years / months between 2020-01-01 (43831) and 2023-06-01 (45078).
        assert_eq!(eval(&dd(43831.0, 45078.0, "Y"), &g), Value::Number(3.0));
        assert_eq!(eval(&dd(43831.0, 45078.0, "M"), &g), Value::Number(41.0));
        // MD: 2020-01-15 (43845) → 2020-03-20 (43910), day remainder = 5.
        assert_eq!(eval(&dd(43845.0, 43910.0, "MD"), &g), Value::Number(5.0));
        // YM: 2020-01-15 → 2023-06-20 (45097), month remainder = 5.
        assert_eq!(eval(&dd(43845.0, 45097.0, "YM"), &g), Value::Number(5.0));
        // YD: 2020-01-15 → 2023-03-20 (45005), day-of-year remainder = 65.
        assert_eq!(eval(&dd(43845.0, 45005.0, "YD"), &g), Value::Number(65.0));
        // The unit folds case.
        assert_eq!(eval(&dd(44927.0, 44957.0, "d"), &g), Value::Number(30.0));
        // start > end is #NUM!.
        assert_eq!(
            eval(&dd(44957.0, 44927.0, "D"), &g),
            Value::Error(ErrKind::Num)
        );
        // An unknown unit is #NUM!.
        assert_eq!(
            eval(&dd(44927.0, 44957.0, "Q"), &g),
            Value::Error(ErrKind::Num)
        );
    }

    #[test]
    fn today_and_now_read_the_pinned_clock() {
        // The test grid pins the clock to PINNED_NOW_SERIAL (44927.5 = 2023-01-01T12:00).
        let g = Grid::new(1, vec![Value::Blank]);
        assert_eq!(eval(&call("TODAY", vec![]), &g), Value::Number(44927.0));
        assert_eq!(eval(&call("NOW", vec![]), &g), Value::Number(44927.5));
        // NOW carries the time-of-day fraction TODAY floors off.
        let frac = Expr::Binary(
            crate::expr::BinOp::Sub,
            Box::new(call("NOW", vec![])),
            Box::new(call("TODAY", vec![])),
        );
        assert_eq!(eval(&frac, &g), Value::Number(0.5));
    }

    #[test]
    fn today_and_now_are_the_registry_volatiles() {
        // Exactly TODAY and NOW carry `volatile: true`; every other row is pure.
        for f in FUNCS {
            let expect = matches!(f.name, "TODAY" | "NOW");
            assert_eq!(f.volatile, expect, "{} volatility", f.name);
        }
    }
}
