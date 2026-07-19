// Concern: the INFORMATION worksheet functions — three CONTENT-INSPECTING sub-families sharing this file: (a) the ERROR-TRANSPARENT predicates ISBLANK ISNUMBER ISTEXT ISERROR ISERR ISNA ISLOGICAL ISNONTEXT plus TYPE and ERROR.TYPE, each evaluating its argument with a bare `ctx.eval` and matching the RAW `Value` after a degenerate 1×1 collapse (via `collapse_1x1`, never `scalarize`/`coerce_*`), so `ISERROR(1/0)` is TRUE, `ISERROR(A1:A1)` of a `#DIV/0!` cell is TRUE, and `ERROR.TYPE(1/0)` is 2 rather than propagating the `#DIV/0!`; (b) the ERROR-PROPAGATING numeric coercers N ISEVEN ISODD, which DO surface an operand error and coerce text/blank to a number (ISEVEN/ISODD rejecting a BOOLEAN as `#VALUE!`, unlike arithmetic coercion); and (c) the REFERENCE-inspecting ISFORMULA, which reads whether the referenced cell's CONTENT is a formula via the `EvalCtx`/`Resolver::is_formula` seam, never the cell's value. `NA()` is the lone error-PRODUCING member | Non-concern: the registry table + dispatch (func/mod.rs), the coercion machinery every OTHER family routes through (eval.rs `scalarize`/`coerce_num`), and the concrete formula/literal distinction ISFORMULA queries (charlie-model owns the `Resolver::is_formula` impl over its loaded grid) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
use super::*;

// Information family — three content-inspecting sub-families (see the file annotation above).
//
// (a) The ERROR-TRANSPARENT predicates (ISBLANK/ISNUMBER/ISTEXT/ISERROR/ISERR/ISNA/ISLOGICAL/
// ISNONTEXT + TYPE/ERROR.TYPE) INSPECT their operand's kind and REPORT on it: EVERY other built-in
// routes its operands through `scalarize`/`coerce_num`/`coerce_bool`, so an `Error` operand
// SHORT-CIRCUITS to that error and a multi-cell array collapses to a propagated `#VALUE!`. These
// predicates must do the OPPOSITE — evaluate with a bare `ctx.eval` and match the RAW `Value`, never
// `scalarize`/`coerce_*` — collapsing ONLY a degenerate 1×1 array/range to its single cell first (via
// `collapse_1x1`, matching Excel implicit-intersection and the sibling `ERROR.TYPE`). Consequences a
// review should hold to exactly: `=ISERROR(1/0)` is TRUE (the `#DIV/0!` is caught, not returned);
// `=ISERROR(A1:A1)` of a `#DIV/0!` cell is TRUE and `=ISNUMBER(B1:B1)` of `5` is TRUE (the 1×1
// collapses); `=TYPE(NA())` is 16 (not `#N/A`); `=ERROR.TYPE(1/0)` is 2; `=ISBLANK(<empty ref>)` is
// TRUE; and a genuinely MULTI-cell array operand is inspected AS an array — `TYPE` -> 64, and the
// IS-predicates each see "not a blank / not a number / not text / not an error" and return FALSE.
//
// (b) The ERROR-PROPAGATING coercers (N/ISEVEN/ISODD) are the EXCEPTION: they convert their operand
// to a number, so an operand error PROPAGATES (they are NOT transparent) and non-numeric text is
// `#VALUE!`. `ISEVEN`/`ISODD` truncate toward zero then test parity and REJECT a boolean operand as
// `#VALUE!` (Excel-exact — verified against the formulas-lib oracle), which is why they use a private
// [`parity_number`] rather than the arithmetic `coerce_num` (which would coerce a boolean to 1/0).
//
// (c) The REFERENCE-inspecting `ISFORMULA` reads whether the CELL its argument references holds a
// formula, via the [`EvalCtx::ref_is_formula`] / [`Resolver::is_formula`] seam — it never evaluates
// the cell's value, so `ISFORMULA` of a cell whose formula errors is still TRUE. A non-reference
// argument is `#VALUE!`; an unresolvable sheet name is `#REF!`.
//
// `NA()` is the lone error-PRODUCING member: it mints the `#N/A` value on demand.

/// `ISBLANK(value)` — TRUE iff the operand is an empty cell / blank. Error-transparent: an error or a
/// genuinely multi-cell array operand is simply "not blank" -> FALSE, never propagated; a degenerate
/// 1×1 array/range collapses to its single cell first (via [`collapse_1x1`], matching Excel's
/// implicit-intersection and the sibling `ERROR.TYPE`).
pub(crate) fn isblank(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    Value::Bool(matches!(collapse_1x1(ctx.eval(&args[0])), Value::Blank))
}

/// `ISNUMBER(value)` — TRUE iff the operand is a number. Error-transparent (an error operand -> FALSE);
/// a 1×1 array/range collapses to its cell first (see [`isblank`]).
pub(crate) fn isnumber(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    Value::Bool(matches!(collapse_1x1(ctx.eval(&args[0])), Value::Number(_)))
}

/// `ISTEXT(value)` — TRUE iff the operand is text. Error-transparent; a 1×1 array/range collapses to
/// its cell first (see [`isblank`]).
pub(crate) fn istext(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    Value::Bool(matches!(collapse_1x1(ctx.eval(&args[0])), Value::Text(_)))
}

/// `ISNONTEXT(value)` — TRUE iff the operand is NOT text (a number, boolean, error, blank, or genuinely
/// multi-cell array; notably `ISNONTEXT(<blank>)` and `ISNONTEXT(<error>)` are both TRUE).
/// Error-transparent: the operand's kind is reported, never propagated; a 1×1 array/range collapses to
/// its cell first (see [`isblank`]).
pub(crate) fn isnontext(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    Value::Bool(!matches!(collapse_1x1(ctx.eval(&args[0])), Value::Text(_)))
}

/// `ISLOGICAL(value)` — TRUE iff the operand is a boolean. Error-transparent; a number `1`, or the
/// TEXT `"TRUE"`, is NOT logical -> FALSE (no coercion). A 1×1 array/range collapses to its cell first
/// (see [`isblank`]).
pub(crate) fn islogical(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    Value::Bool(matches!(collapse_1x1(ctx.eval(&args[0])), Value::Bool(_)))
}

/// `ISERROR(value)` — TRUE iff the operand is ANY error value (`#N/A` INCLUDED — cf. [`iserr`], which
/// excludes `#N/A`). The defining error-transparent case: the operand's error is CAUGHT and reported,
/// never propagated, so `=ISERROR(1/0)` is TRUE rather than `#DIV/0!`. A 1×1 array/range collapses to
/// its cell first (see [`isblank`]) — `ISERROR(A1:A1)` of a cell holding `#DIV/0!` is TRUE.
pub(crate) fn iserror(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    Value::Bool(matches!(collapse_1x1(ctx.eval(&args[0])), Value::Error(_)))
}

/// `ISERR(value)` — TRUE iff the operand is any error EXCEPT `#N/A` (the complement of [`isna`] within
/// the errors; cf. [`iserror`], which includes `#N/A`). Error-transparent; a 1×1 array/range collapses
/// to its cell first (see [`isblank`]).
pub(crate) fn iserr(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    Value::Bool(matches!(collapse_1x1(ctx.eval(&args[0])), Value::Error(k) if k != ErrKind::Na))
}

/// `ISNA(value)` — TRUE iff the operand is the `#N/A` error specifically (any OTHER error, or a
/// non-error, is FALSE). Error-transparent — the `#N/A` is caught, not propagated; a 1×1 array/range
/// collapses to its cell first (see [`isblank`]).
pub(crate) fn isna(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    Value::Bool(matches!(
        collapse_1x1(ctx.eval(&args[0])),
        Value::Error(ErrKind::Na)
    ))
}

/// `NA()` — the `#N/A` ("value not available") error, produced on demand. The one error-PRODUCING
/// member of the information family; arity 0 (checked at parse).
pub(crate) fn na_fn(_ctx: &mut EvalCtx, _args: &[Expr]) -> Value {
    Value::Error(ErrKind::Na)
}

/// `TYPE(value)` — the operand's Excel type code: 1 number, 2 text, 4 logical, 16 error, 64 array. An
/// empty cell reports as a number (1), matching Excel (a blank is a numeric zero in value context).
/// Error-transparent: an error operand reports 16, and a genuinely multi-cell array reports 64 —
/// neither is propagated. A degenerate 1×1 array/range collapses to its single cell first (via
/// [`collapse_1x1`], matching Excel implicit-intersection and the sibling `ERROR.TYPE`), so
/// `TYPE(A1:A1)` reports the type of `A1`, not 64.
pub(crate) fn type_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let code = match collapse_1x1(ctx.eval(&args[0])) {
        Value::Number(_) | Value::Blank => 1.0,
        Value::Text(_) => 2.0,
        Value::Bool(_) => 4.0,
        Value::Error(_) => 16.0,
        Value::Array(..) => 64.0,
    };
    Value::Number(code)
}

/// `ERROR.TYPE(value)` — the numeric CODE of an error operand (`#NULL!`=1, `#DIV/0!`=2, `#VALUE!`=3,
/// `#REF!`=4, `#NAME?`=5, `#NUM!`=6, `#N/A`=7, `#SPILL!`=9, `#CALC!`=14), or `#N/A` if the operand is
/// not an error. Error-transparent (the error is inspected, not propagated); a 1×1 array of an error
/// collapses to that error's code, a genuinely multi-cell array is not an error -> `#N/A`.
pub(crate) fn error_type_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match collapse_1x1(ctx.eval(&args[0])) {
        Value::Error(k) => Value::Number(match k {
            ErrKind::Null => 1.0,
            ErrKind::Div0 => 2.0,
            ErrKind::Value => 3.0,
            ErrKind::Ref => 4.0,
            ErrKind::Name => 5.0,
            ErrKind::Num => 6.0,
            ErrKind::Na => 7.0,
            ErrKind::Spill => 9.0,
            ErrKind::Calc => 14.0,
        }),
        _ => Value::Error(ErrKind::Na),
    }
}

/// `N(value)` — coerce a value to a number: a number is itself, `TRUE`->1 / `FALSE`->0, an empty cell
/// ->0, and ANY text (even numeric-looking `"123"`) ->0 (N does NOT parse text). An error PROPAGATES
/// (N is NOT error-transparent — unlike the IS-predicates), and a genuinely multi-cell array in this
/// scalar slot is `#VALUE!`.
pub(crate) fn n_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match scalarize(ctx.eval(&args[0])) {
        Value::Number(n) => Value::Number(n),
        Value::Bool(b) => Value::Number(if b { 1.0 } else { 0.0 }),
        Value::Error(k) => Value::Error(k),
        Value::Text(_) | Value::Blank => Value::Number(0.0),
        // `scalarize` already demoted a genuinely multi-cell array to `#VALUE!`; this arm is for
        // exhaustiveness and mirrors that verdict.
        Value::Array(..) => Value::Error(ErrKind::Value),
    }
}

/// Coerce an `ISEVEN`/`ISODD` operand to the integer whose parity Excel tests: the value TRUNCATED
/// toward zero. A number/blank/numeric-text coerces (blank->0, `"4"`->4); a BOOLEAN is REJECTED as
/// `#VALUE!` (Excel-exact: `ISEVEN(TRUE)` is `#VALUE!`, NOT `ISEVEN(1)` — so this deliberately does
/// NOT use the arithmetic [`coerce_num`], which coerces a boolean to 1/0); an error PROPAGATES;
/// non-numeric text and a genuinely multi-cell array are `#VALUE!`. The result is integer-valued, so
/// the callers test parity with an exact `% 2.0` (no `as i64` truncation/overflow).
fn parity_number(v: Value) -> Result<f64, ErrKind> {
    match scalarize(v) {
        Value::Number(n) => Ok(n.trunc()),
        Value::Blank => Ok(0.0),
        Value::Text(t) => match t.trim().parse::<f64>() {
            Ok(n) if n.is_finite() => Ok(n.trunc()),
            _ => Err(ErrKind::Value),
        },
        // A boolean is NOT a valid ISEVEN/ISODD operand in Excel (-> `#VALUE!`).
        Value::Bool(_) => Err(ErrKind::Value),
        Value::Error(k) => Err(k),
        // `scalarize` already demoted a genuinely multi-cell array to `#VALUE!`.
        Value::Array(..) => Err(ErrKind::Value),
    }
}

/// `ISEVEN(number)` — TRUE iff `number`, truncated toward zero, is even. Coerces text/blank to a
/// number and REJECTS a boolean (`#VALUE!`); an operand error PROPAGATES (see [`parity_number`]).
pub(crate) fn iseven_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match parity_number(ctx.eval(&args[0])) {
        Ok(n) => Value::Bool(n % 2.0 == 0.0),
        Err(k) => Value::Error(k),
    }
}

/// `ISODD(number)` — TRUE iff `number`, truncated toward zero, is odd. The parity dual of
/// [`iseven_fn`], sharing its coercion (text/blank coerce, a boolean is `#VALUE!`, an error
/// propagates).
pub(crate) fn isodd_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match parity_number(ctx.eval(&args[0])) {
        Ok(n) => Value::Bool(n % 2.0 != 0.0),
        Err(k) => Value::Error(k),
    }
}

/// `ISFORMULA(reference)` — TRUE iff `reference` names a cell whose CONTENT is a formula (an `=…`),
/// FALSE for a literal/blank/gap. It inspects the referenced cell's content KIND through the
/// [`EvalCtx::ref_is_formula`] / [`Resolver::is_formula`] seam — never the cell's VALUE — so a cell
/// whose formula evaluates to an error still reports TRUE. A range reference anchors on its top-left
/// cell (implicit intersection). A non-reference argument (a literal, an operator expression) is
/// `#VALUE!`; an unresolvable sheet name is `#REF!`.
///
/// Known scalar-v1 gap vs Excel: only a SYNTACTIC `Expr::Ref`/`Expr::Range` argument is inspected —
/// a reference PRODUCED by a reference-returning function (`INDEX`, `OFFSET`, …) would yield `#VALUE!`
/// rather than inspecting the target cell. This engine ships no reference-returning functions, so the
/// gap is unreachable today; revisit when one lands.
pub(crate) fn isformula_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match &args[0] {
        Expr::Ref(r) => match ctx.ref_is_formula(r) {
            Some(b) => Value::Bool(b),
            None => Value::Error(ErrKind::Ref),
        },
        Expr::Range(rn) => match ctx.range_is_formula(rn) {
            Some(b) => Value::Bool(b),
            None => Value::Error(ErrKind::Ref),
        },
        _ => Value::Error(ErrKind::Value),
    }
}
