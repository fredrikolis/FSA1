// Concern: the FUNCTION REGISTRY as data — the `FuncDef` record (`name`, arity bounds, an `eval` fn-pointer, an OPTIONAL parse-time `validate` argument-check, a `volatile` flag, and the `broadcast` scalar-argument positions that drive implicit array evaluation), the flat `FUNCS` table indexed by `FuncId`, name->`FuncId` lookup (case-insensitive, for the parser) and `FuncId`->`FuncDef` dispatch (for the evaluator); the built-ins THEMSELVES live in per-family sibling submodules (`aggregation` `logical` `criteria_agg` `math` `stats` `text` `date` `lookup` `info` `finance`), with the cross-family eval helpers in `func::helpers` and the IMPLICIT-ARRAY-EVALUATION broadcaster (mapping a call element-wise over an array in a scalar argument) in `func::array` — each row's `eval` points at its family's fn, and each built-in owns its own argument evaluation so lazy forms (IF/IFERROR), the direct-vs-in-range coercion asymmetry, and range-conformance checks stay expressible | Non-concern: each family's function bodies (the `func::*` submodules own them), the CRITERIA mini-language the `*IF(S)` built-ins depend on (criteria.rs owns `Criterion`/`parse_criterion`), and the operator/coercion machinery (eval.rs owns `coerce_num`/`coerce_bool`/`scalarize`/`pow`, which the built-ins reuse) | IO: none — a static dispatch table over the `EvalCtx`/`Value` contract
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

pub(crate) use crate::criteria::{Criterion, parse_criterion, parse_db_criterion, wildcard_match};
pub(crate) use crate::diag::{Diag, DiagCode, Span};
pub(crate) use crate::eval::{
    EvalCtx, coerce_bool, coerce_num, collapse_1x1, pow, scalarize, to_text, value_cmp, value_eq,
};
pub(crate) use crate::expr::{Expr, FuncId};
pub(crate) use crate::value::{ErrKind, Shape, Value};

mod aggregation;
mod array;
mod combinatorics;
mod criteria_agg;
mod database;
mod date;
mod engineering;
mod finance;
mod helpers;
mod info;
mod logical;
mod lookup;
mod math;
mod random;
mod registry_a;
mod registry_b;
mod spill;
mod stats;
mod stats_desc;
mod stats_dist;
mod stats_rank;
mod stats_reg;
mod subtotal;
mod text;
mod text_format;
mod trig;

use aggregation::*;
use combinatorics::*;
use criteria_agg::*;
use database::*;
use date::*;
use engineering::*;
use finance::*;
pub(crate) use helpers::*;
use info::*;
use logical::*;
use lookup::*;
use math::*;
use random::*;
use spill::*;
use stats::*;
use stats_desc::*;
use stats_dist::*;
use stats_rank::*;
use stats_reg::*;
use subtotal::*;
use text::*;
use text_format::*;
use trig::*;

// The Excel-serial date map is single-homed in `date`; re-export it crate-publicly so an out-of-crate
// deserializer (charlie-ingest) maps a date-typed cell to the SAME serial the engine's DATE/serial
// arithmetic uses, rather than re-deriving the 1900 leap-bug policy (DRY across the format firewall).
pub use date::serial_from_ymd;

#[cfg(test)]
mod tests;

/// One registry row. `min_args`/`max_args` bound the arity (`max_args = None` is unbounded/variadic);
/// `eval` receives the *unevaluated* argument `Expr`s and the [`EvalCtx`], so a function chooses what
/// to evaluate (lazy `IF`/`IFERROR`) and how to treat a datum by whether it arrived direct or inside
/// a range.
/// A parse-time argument validator: `(the call's args, the call name's [`Span`])` → `Ok(())` or a
/// located refusal [`Diag`]. The seam a [`FuncDef`] uses for a static-argument check beyond arity.
pub type ValidateFn = fn(&[Expr], Span) -> Result<(), Diag>;

/// `Copy` because [`concat_registry`] builds the flat [`FUNCS`] array from the two family-split
/// slices at compile time (`[row; N]` + element copies), which needs the element type to be `Copy`;
/// every field is already a `Copy` scalar/pointer/`&'static` reference, so the derive is free.
#[derive(Clone, Copy)]
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
    /// The SCALAR-expecting argument positions that participate in IMPLICIT ARRAY EVALUATION: an
    /// `array` handed to one of these maps the call element-wise (`func::array` owns the mapping
    /// LOGIC), while every other argument (a range/value-range a reducer or lookup consumes whole)
    /// is broadcast whole. An empty slice means "no broadcasting" — the function is dispatched
    /// unchanged. Two families broadcast in v1: (a) the single-criterion criteria-aggregation forms,
    /// whose CRITERION is arg 1 (`COUNTIF(range, criteria)`, `SUMIF(range, criteria, [sum_range])`,
    /// `AVERAGEIF(range, criteria, [avg_range])`), so an array criterion (the distinct-count idiom
    /// `SUMPRODUCT(1/COUNTIF(A1:A6,A1:A6))`) maps to an array of per-criterion results; and (b) the
    /// SCALAR TEXT functions (`LEFT`/`RIGHT`/`MID`/`LEN`/`FIND`/`SEARCH`/`SUBSTITUTE`/`TRIM`/`UPPER`/
    /// `LOWER`/`TEXT`/`VALUE`/`REPT`), each broadcasting ALL its scalar-typed positions so a
    /// range/array argument maps the call element-wise (the CSE-array idioms real sheets use, e.g.
    /// `SUMPRODUCT(LEN(A1:A3))` or `SUMPRODUCT(--(VALUE(TRIM(range))>0))`) — a genuinely multi-cell
    /// array in ANY marked position drives the map, every scalar broadcasts, and the reducer wrapping
    /// it collapses the result. `IF`'s array condition is NOT expressed here (it is lazy — arg 1/2 are
    /// evaluated only when the condition is an array — so `logical::if_fn` decides scalar-vs-array and
    /// delegates the map to `array::map_if`); the multi-criteria `*IFS` forms are a later batch.
    /// Recorded as
    /// registry DATA — like `validate`/`volatile` — so which functions broadcast which positions is
    /// single-sourced with the rest of the row and keyed by [`FuncId`], not a hand-forked name-match
    /// in `func::array`, keeping the "function is a row" invariant.
    pub broadcast: &'static [usize],
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
/// The number of registry rows: the two family-split slices summed. A `const` so the concatenated
/// array below is exactly sized (any drift between the slices and the array reddens compilation).
const REGISTRY_LEN: usize = registry_a::ROWS_A.len() + registry_b::ROWS_B.len();

/// The registry, indexed by [`FuncId`]`.0`. Order is the id assignment — appending is safe, but a
/// row's position is its stable id, so never reorder (a `self_consistency` test pins name↔index). The
/// rows live in [`registry_a`]/[`registry_b`] (a line-budget split only) and are CONCATENATED here —
/// `ROWS_A` then `ROWS_B` — into one contiguous slice, so `FUNCS[i]` is still the row with `FuncId(i)`
/// and the split is invisible to every reader of `FUNCS`.
pub static FUNCS: &[FuncDef] = &concat_registry();

/// Concatenate the two family-split registry slices, `ROWS_A` then `ROWS_B`, preserving order (and so
/// every `FuncId`). A `const fn` (hence [`FuncDef`]'s `Copy`) so `FUNCS` is built at compile time with
/// no runtime cost.
const fn concat_registry() -> [FuncDef; REGISTRY_LEN] {
    let mut out = [registry_a::ROWS_A[0]; REGISTRY_LEN];
    let mut i = 0;
    let mut j = 0;
    while j < registry_a::ROWS_A.len() {
        out[i] = registry_a::ROWS_A[j];
        i += 1;
        j += 1;
    }
    j = 0;
    while j < registry_b::ROWS_B.len() {
        out[i] = registry_b::ROWS_B[j];
        i += 1;
        j += 1;
    }
    out
}

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
        // Route through the implicit-array-evaluation home: it maps a function element-wise over an
        // array supplied to a scalar position and yields an array (`array::eval_call`), or — for a
        // function with no broadcasting positions — dispatches the row's `eval` unchanged.
        Some(f) if f.arity_ok(args.len()) => array::eval_call(f, ctx, args),
        Some(_) => Value::Error(ErrKind::Value),
        None => Value::Error(ErrKind::Name),
    }
}
