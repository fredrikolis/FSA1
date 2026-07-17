// Concern: the FUNCTION REGISTRY as data — the `FuncDef` record (`name`, arity bounds, an `eval` fn-pointer, an OPTIONAL parse-time `validate` argument-check, and a `volatile` flag), the flat `FUNCS` table indexed by `FuncId`, name->`FuncId` lookup (case-insensitive, for the parser) and `FuncId`->`FuncDef` dispatch (for the evaluator); the built-ins THEMSELVES live in per-family sibling submodules (`aggregation` `logical` `criteria_agg` `math` `stats` `text` `date` `lookup` `info` `finance`), with the cross-family eval helpers in `func::helpers` — each row's `eval` points at its family's fn, and each built-in owns its own argument evaluation so lazy forms (IF/IFERROR), the direct-vs-in-range coercion asymmetry, and range-conformance checks stay expressible | Non-concern: each family's function bodies (the `func::*` submodules own them), the CRITERIA mini-language the `*IF(S)` built-ins depend on (criteria.rs owns `Criterion`/`parse_criterion`), and the operator/coercion machinery (eval.rs owns `coerce_num`/`coerce_bool`/`scalarize`/`pow`, which the built-ins reuse) | IO: none — a static dispatch table over the `EvalCtx`/`Value` contract
//! The function registry: [`FuncDef`], the [`FUNCS`] table, [`lookup`], [`def`], [`dispatch`].
//!
//! Registry-as-data (ast-standards PART 7, "one engine, N behaviors as data"): a function is a row,
//! not a hand-forked code path. The parser resolves a name to a [`crate::FuncId`] and checks arity
//! against the row (so eval trusts the arity — DbC); the evaluator dispatches the row's `eval`. This
//! module owns the TABLE and its lookup/dispatch machinery; the function bodies live in per-family
//! submodules (`aggregation`/`logical`/`criteria_agg`/`math`/`stats`/`text`/`date`/`lookup`/`info`/
//! `finance`) and the eval helpers reused across families live in [`helpers`], so the table stays a
//! readable index and each family grows in its own file. A row's `eval`/`validate` fields point at
//! the owning submodule's `pub(crate)` fns (glob-imported below), so appending a function is: add
//! its fn to the family file, add its row here.

pub(crate) use crate::criteria::{Criterion, parse_criterion, wildcard_match};
pub(crate) use crate::diag::{Diag, DiagCode, Span};
pub(crate) use crate::eval::{
    EvalCtx, coerce_bool, coerce_num, pow, scalarize, to_text, value_cmp, value_eq,
};
pub(crate) use crate::expr::{Expr, FuncId};
pub(crate) use crate::value::{ErrKind, Shape, Value};

mod aggregation;
mod criteria_agg;
mod date;
mod finance;
mod helpers;
mod info;
mod logical;
mod lookup;
mod math;
mod stats;
mod text;

use aggregation::*;
use criteria_agg::*;
use date::*;
use finance::*;
pub(crate) use helpers::*;
use info::*;
use logical::*;
use lookup::*;
use math::*;
use stats::*;
use text::*;

#[cfg(test)]
mod tests;

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
    //     in tests + conformance. The serial↔date maps: the forward `serial_to_ymd` lives in
    //     `func::text` (TEXT's date render needs it), its inverse `serial_from_ymd` in `func::date`. ---
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
    // --- Lookup & reference (v1): XLOOKUP INDEX MATCH VLOOKUP CHOOSE ROW COLUMN, plus the reserved,
    // always-refused reference-returning INDIRECT / OFFSET (see the block comment below the defs). ---
    FuncDef {
        name: "XLOOKUP",
        min_args: 3,
        max_args: Some(6),
        eval: xlookup,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "INDEX",
        min_args: 2,
        max_args: Some(3),
        eval: index_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "MATCH",
        min_args: 2,
        max_args: Some(3),
        eval: match_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "VLOOKUP",
        min_args: 3,
        max_args: Some(4),
        eval: vlookup,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "CHOOSE",
        min_args: 2,
        max_args: None,
        eval: choose,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "ROW",
        min_args: 1,
        max_args: Some(1),
        eval: row_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "COLUMN",
        min_args: 1,
        max_args: Some(1),
        eval: column_fn,
        validate: None,
        volatile: false,
    },
    // The two RESERVED reference-returning functions. Arity is left wide open (`0..`) on purpose so the
    // arity gate never fires FIRST — every call, whatever its argument count, reaches the always-refuse
    // `validate` seam and is turned into a located `reserved-ref-function` refusal on the call name
    // (never a wrong value, never the generic `unknown-function` path — the name IS recognized).
    FuncDef {
        name: "INDIRECT",
        min_args: 0,
        max_args: None,
        eval: reserved_ref_eval,
        validate: Some(refuse_reserved_ref_function),
        volatile: false,
    },
    FuncDef {
        name: "OFFSET",
        min_args: 0,
        max_args: None,
        eval: reserved_ref_eval,
        validate: Some(refuse_reserved_ref_function),
        volatile: false,
    },
    // --- Information (v1): the ONE error-TRANSPARENT family — ISBLANK ISNUMBER ISTEXT ISERROR NA TYPE.
    // These INSPECT their operand's kind (a blank, a number, an error, an array) and REPORT on it; they
    // must NOT route it through `scalarize`/`coerce_*`, which would propagate (see the block comment on
    // the impls below). `NA` is the sole error-PRODUCING member — arity 0, returns `#N/A`. ---
    FuncDef {
        name: "ISBLANK",
        min_args: 1,
        max_args: Some(1),
        eval: isblank,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "ISNUMBER",
        min_args: 1,
        max_args: Some(1),
        eval: isnumber,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "ISTEXT",
        min_args: 1,
        max_args: Some(1),
        eval: istext,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "ISERROR",
        min_args: 1,
        max_args: Some(1),
        eval: iserror,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "NA",
        min_args: 0,
        max_args: Some(0),
        eval: na_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "TYPE",
        min_args: 1,
        max_args: Some(1),
        eval: type_fn,
        validate: None,
        volatile: false,
    },
    // --- Financial (v1): PMT NPV IRR. Closed-form annuity (PMT) + discounting (NPV) + an ITERATIVE
    // root-find (IRR) that is GUARANTEED to halt — Newton under a hard iteration cap, then a bounded
    // bisection fallback, then `#NUM!`, never a hang or a panic (see the block comment below the defs
    // + `pow_int`, whose deterministic multiply order the conformance oracle mirrors bit-for-bit). ---
    FuncDef {
        name: "PMT",
        min_args: 3,
        max_args: Some(5),
        eval: pmt_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "NPV",
        min_args: 2,
        max_args: None,
        eval: npv_fn,
        validate: None,
        volatile: false,
    },
    FuncDef {
        name: "IRR",
        min_args: 1,
        max_args: Some(2),
        eval: irr_fn,
        validate: None,
        volatile: false,
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
