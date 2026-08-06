// Concern: declares the registry row and dispatches a Call to it | Non-concern: the function bodies, parsing a call | IO: (FuncId, &mut EvalCtx, &[Expr]) -> Value
//! A function is a ROW, never a hand-forked code path: adding one means adding its `fn` to a family
//! file and its row here.

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

/// Re-exported so a deserializer outside this crate maps a date cell to the SAME serial the engine
/// uses, rather than re-deriving the 1900 leap-bug policy.
pub use date::serial_from_ymd;

/// The crate-public façade over the engine's ONE numFmt renderer, so a consumer's format-aware
/// render reuses it. `code` must be a bare Excel format code: a colour or condition bracket is
/// outside the `TEXT()` subset and renders as `#VALUE!`.
pub fn format_value(value: &Value, code: &str) -> Value {
    render_text_format(value, code)
}

/// The crate-public façade over the engine's ONE ISO reader, so a consumer's date-literal recovery
/// is BIT-EXACT with the engine's own reading — this wrapper adds no arithmetic.
pub fn parse_iso_serial(s: &str) -> Option<f64> {
    parse_datetime_serial(s)
}

#[cfg(test)]
mod tests;

pub type ValidateFn = fn(&[Expr], Span) -> Result<(), Diag>;

/// `Copy` so [`FUNCS`] can be concatenated at compile time; every field is already `Copy`.
#[derive(Clone, Copy)]
pub struct FuncDef {
    pub name: &'static str,
    pub min_args: usize,
    /// `None` is unbounded/variadic.
    pub max_args: Option<usize>,
    /// Receives the UNEVALUATED argument `Expr`s, so a lazy form chooses what to evaluate.
    pub eval: fn(&mut EvalCtx, &[Expr]) -> Value,
    /// A static-shape check beyond arity, run by the parser after the arity gate. A row-level datum
    /// rather than a hand-fork in the parser, so the "function is a row" invariant holds.
    pub validate: Option<ValidateFn>,
    /// VOLATILE: the value can change between two evaluations of the same tree against the same
    /// resolver, because it reads a mutable seam of the outside world rather than its arguments.
    pub volatile: bool,
    /// The SCALAR-expecting positions that participate in implicit array evaluation: an array in a
    /// marked position maps the call element-wise, and every other argument broadcasts whole. Empty
    /// means the row is dispatched unchanged — the classification that separates a scalar-mapping
    /// function from an array-CONSUMING reducer, lookup or aggregator.
    pub broadcast: &'static [usize],
}

impl FuncDef {
    pub fn arity_ok(&self, n: usize) -> bool {
        n >= self.min_args && self.max_args.is_none_or(|max| n <= max)
    }

    /// `span` locates a refusal on the call's name token; a `validate = None` row is a no-op.
    pub fn validate_args(&self, args: &[Expr], span: Span) -> Result<(), Diag> {
        match self.validate {
            Some(check) => check(args, span),
            None => Ok(()),
        }
    }
}

const REGISTRY_LEN: usize = registry_a::ROWS_A.len() + registry_b::ROWS_B.len();

/// Indexed by [`FuncId`]`.0`, so a row's POSITION is its stable id: append freely, never reorder.
/// `ROWS_A`/`ROWS_B` is a line-budget split only and is invisible here.
pub static FUNCS: &[FuncDef] = &concat_registry();

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

/// Case-insensitive; the parser turns `None` into an `unknown-function` located refusal.
pub fn lookup(name: &str) -> Option<FuncId> {
    FUNCS
        .iter()
        .position(|f| f.name.eq_ignore_ascii_case(name))
        .map(|i| FuncId(i as u32))
}

pub fn def(id: FuncId) -> Option<&'static FuncDef> {
    FUNCS.get(id.0 as usize)
}

/// The parser mints neither a bad id nor an off-arity call, but `eval` is total over ANY `Expr`, so
/// a hand-built tree must yield an error value here rather than panic in a positional built-in.
pub fn dispatch(id: FuncId, ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match def(id) {
        Some(f) if f.arity_ok(args.len()) => array::eval_call(f, ctx, args),
        Some(_) => Value::Error(ErrKind::Value),
        None => Value::Error(ErrKind::Name),
    }
}
