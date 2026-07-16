// Concern: the FINANCIAL worksheet functions (PMT NPV IRR) — the cash-flow time-value built-ins over one deterministic annuity/present-value model, with IRR's Newton-then-bisection root find bounded to terminate (never a hang or a panic) and `pow_int`'s deterministic multiply order the conformance oracle mirrors bit-for-bit | Non-concern: the registry table + dispatch (func/mod.rs) and the shared `one_num`/`collect_numbers`/`finite_or_num` helpers (func/helpers.rs) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
use super::*;

// Financial (v1): PMT NPV IRR. Three cash-flow-time-value functions sharing ONE deterministic
// integer-power helper (`pow_int`) and the engine's numeric-gathering asymmetry (`collect_numbers`
// for the ordered value streams NPV/IRR consume). The Excel-semantics calls pinned here, each worth
// a reviewer's eye:
//   * PMT(rate, nper, pv, [fv], [type]) solves the standard annuity balance equation
//         pv·(1+rate)^nper + pmt·(1 + rate·type)·((1+rate)^nper − 1)/rate + fv = 0
//     for `pmt` (money OUT is negative, Excel's sign convention). `rate == 0` degenerates to the
//     LINEAR `pmt = −(pv + fv)/nper`. A zero denominator (`nper == 0`, or `1 + rate·type == 0`) is
//     `#DIV/0!` (never a silent ∞). `type` is 0 (end of period, default) or 1 (beginning).
//   * NPV(rate, value1, …) discounts EACH value one period further out — value1 is divided by
//     (1+rate)^1, NOT ^0 (the classic "NPV starts at period 1" rule; a period-0 outlay is added
//     OUTSIDE NPV). Values flatten row-major through `collect_numbers` (a direct boolean/numeric-text
//     coerces, an in-range non-number is IGNORED — so it never consumes a period slot — and an error
//     propagates), matching Excel's "ignore text/blank/logical inside a reference" rule.
//   * IRR(values, [guess]) finds the rate making the period-0-anchored NPV zero
//         Σ_i cf_i/(1+r)^i = 0   (cf_0 undiscounted)
//     by NEWTON from `guess` (default 0.1), CAPPED at `IRR_NEWTON_MAX` iterations; on non-convergence
//     it falls back to a BRACKETING bisection (scan for a sign change over a bounded rate grid, then
//     bisect, itself capped at `IRR_BISECT_MAX`); if neither finds a root — e.g. all-positive flows
//     with no sign change — the result is `#NUM!`. TERMINATION IS GUARANTEED: both loops have hard
//     integer caps, so no cash flow (convergent or not) can hang or panic. `pow_int` (a left-to-right
//     multiply loop, NOT `f64::powi`, whose lowering can differ across toolchains) makes the produced
//     f64 reproducible bit-for-bit by an independent oracle using the same op order.
/// `base` raised to a NON-NEGATIVE integer power via a deterministic left-to-right `f64` multiply
/// loop. Deliberately NOT [`f64::powi`]: a plain multiply chain is one fixed IEEE-754 round-to-nearest
/// sequence that a from-scratch oracle can reproduce EXACTLY (so a bit-exact conformance literal is
/// authorable), whereas `powi`/`llvm.powi` may pick a different (equally-valid) rounding per toolchain.
fn pow_int(base: f64, exp: u32) -> f64 {
    let mut acc = 1.0;
    for _ in 0..exp {
        acc *= base;
    }
    acc
}

/// `PMT(rate, nper, pv, [fv], [type])` — the periodic annuity payment. `rate == 0` uses the LINEAR
/// `−(pv+fv)/nper`; otherwise the closed-form annuity solution. A zero denominator (`nper == 0`, or
/// `1 + rate·type == 0`) is `#DIV/0!`. `nper` truncates toward zero for the integer power; a
/// non-integer `nper` uses its truncated period count (Excel accepts a fractional nper via `^`, but v1
/// pins the integer-period annuity — documented in PROVENANCE). Errors propagate leftmost.
pub(crate) fn pmt_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let rate = match one_num(ctx, &args[0]) {
        Ok(x) => x,
        Err(k) => return Value::Error(k),
    };
    let nper = match one_num(ctx, &args[1]) {
        Ok(x) => x,
        Err(k) => return Value::Error(k),
    };
    let pv = match one_num(ctx, &args[2]) {
        Ok(x) => x,
        Err(k) => return Value::Error(k),
    };
    let fv = match args.get(3) {
        Some(e) => match one_num(ctx, e) {
            Ok(x) => x,
            Err(k) => return Value::Error(k),
        },
        None => 0.0,
    };
    let typ = match args.get(4) {
        Some(e) => match one_num(ctx, e) {
            Ok(x) => x,
            Err(k) => return Value::Error(k),
        },
        None => 0.0,
    };
    if rate == 0.0 {
        if nper == 0.0 {
            return Value::Error(ErrKind::Div0);
        }
        return finite_or_num(-(pv + fv) / nper);
    }
    // nper truncates toward zero for the integer-period power (a negative nper is degenerate — its
    // truncated magnitude drives the loop; the annuity identity still holds for the pinned integer case).
    let periods = nper.trunc().abs().min(u32::MAX as f64) as u32;
    let temp = pow_int(1.0 + rate, periods);
    let denom = (1.0 + rate * typ) * (temp - 1.0);
    if denom == 0.0 {
        return Value::Error(ErrKind::Div0);
    }
    finite_or_num(-(fv + pv * temp) * rate / denom)
}

/// `NPV(rate, value1, …)` — the net present value, discounting value1 from period ONE
/// (`value1/(1+rate)^1`). Values flatten row-major via [`collect_numbers`] (direct booleans/
/// numeric-text coerce, in-range non-numbers are ignored, an error propagates); an ignored cell does
/// NOT consume a period. A `rate == −1` (division by zero) surfaces as `#NUM!` via [`finite_or_num`].
pub(crate) fn npv_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let rate = match one_num(ctx, &args[0]) {
        Ok(x) => x,
        Err(k) => return Value::Error(k),
    };
    let values = match collect_numbers(ctx, &args[1..]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let base = 1.0 + rate;
    let mut acc = 0.0;
    for (i, v) in values.iter().enumerate() {
        acc += v / pow_int(base, i as u32 + 1);
    }
    finite_or_num(acc)
}

/// The IRR objective: the period-0-anchored NPV `Σ cf_i/(1+rate)^i` (cf_0 UNDISCOUNTED — the IRR
/// convention, distinct from NPV's period-1 start). Its root is the internal rate of return.
fn irr_npv(rate: f64, cf: &[f64]) -> f64 {
    let base = 1.0 + rate;
    let mut acc = 0.0;
    for (i, c) in cf.iter().enumerate() {
        acc += c / pow_int(base, i as u32);
    }
    acc
}

/// The derivative of [`irr_npv`] in `rate`: `Σ_{i≥1} −i·cf_i/(1+rate)^{i+1}`. Drives the Newton step.
fn irr_npv_deriv(rate: f64, cf: &[f64]) -> f64 {
    let base = 1.0 + rate;
    let mut acc = 0.0;
    for (i, c) in cf.iter().enumerate() {
        if i == 0 {
            continue;
        }
        acc += -(i as f64) * c / pow_int(base, i as u32 + 1);
    }
    acc
}

/// The HARD cap on Newton iterations — the primary guarantee IRR can never spin forever.
const IRR_NEWTON_MAX: usize = 50;

/// The HARD cap on bisection halvings in the fallback bracket search.
const IRR_BISECT_MAX: usize = 200;

/// The relative step size below which Newton is declared converged.
const IRR_STEP_TOL: f64 = 1e-12;

/// The residual `|NPV|` a converged rate must satisfy — guards a spurious tiny Newton step at a
/// non-root (huge derivative) from being mistaken for a solution.
const IRR_RESID_TOL: f64 = 1e-6;

/// `IRR(values, [guess])` — the rate making [`irr_npv`] zero. NEWTON from `guess` (default 0.1) under
/// [`IRR_NEWTON_MAX`], then a bounded [`irr_bisect`] fallback; a cash flow with no sign change (no real
/// root) — or any non-convergence — is `#NUM!`. Termination is guaranteed by the two integer caps.
pub(crate) fn irr_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let cf = match collect_numbers(ctx, &args[0..1]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    // A well-posed IRR needs at least two flows (one in, one out); fewer can never have a sign change.
    if cf.len() < 2 {
        return Value::Error(ErrKind::Num);
    }
    let guess = match args.get(1) {
        Some(e) => match one_num(ctx, e) {
            Ok(x) => x,
            Err(k) => return Value::Error(k),
        },
        None => 0.1,
    };
    // Newton needs a start strictly inside the domain (rate > −1); a bad guess falls back to 0.1.
    let start = if guess > -1.0 && guess.is_finite() {
        guess
    } else {
        0.1
    };
    if let Some(r) = irr_newton(&cf, start) {
        return finite_or_num(r);
    }
    match irr_bisect(&cf) {
        Some(r) => finite_or_num(r),
        None => Value::Error(ErrKind::Num),
    }
}

/// Newton's method for the IRR root, capped at [`IRR_NEWTON_MAX`] steps. Returns the converged rate
/// (relative step below [`IRR_STEP_TOL`] AND residual below [`IRR_RESID_TOL`]), or `None` on a zero/
/// non-finite derivative, a step out of the `rate > −1` domain, or exhausting the cap — the caller
/// then tries bracketing. Cannot loop forever: the `for` bound is a hard integer cap.
fn irr_newton(cf: &[f64], guess: f64) -> Option<f64> {
    let mut rate = guess;
    for _ in 0..IRR_NEWTON_MAX {
        let f = irr_npv(rate, cf);
        let d = irr_npv_deriv(rate, cf);
        if d == 0.0 || !f.is_finite() || !d.is_finite() {
            return None;
        }
        let step = f / d;
        let next = rate - step;
        if !next.is_finite() || next <= -1.0 {
            return None;
        }
        if step.abs() <= IRR_STEP_TOL * next.abs().max(1.0) {
            rate = next;
            return (irr_npv(rate, cf).abs() <= IRR_RESID_TOL).then_some(rate);
        }
        rate = next;
    }
    None
}

/// Bracketing bisection fallback: scan a bounded ascending rate grid (all strictly `> −1`) for a sign
/// change in [`irr_npv`], then bisect that bracket to [`IRR_STEP_TOL`], capped at [`IRR_BISECT_MAX`]
/// halvings. `None` when no sign change exists on the grid (e.g. all-positive flows) — the honest
/// "no real IRR" answer the caller turns into `#NUM!`. Cannot loop forever: both loops have hard caps.
fn irr_bisect(cf: &[f64]) -> Option<f64> {
    // A fixed grid from just above −1 up through large positive rates. 0.005 steps over [−0.999, 4]
    // resolve any economically-meaningful root; the search is O(GRID) and purely for a sign change.
    const GRID: usize = 1000;
    const LO: f64 = -0.999;
    const STEP: f64 = 0.005;
    let mut prev_r = LO;
    let mut prev_f = irr_npv(prev_r, cf);
    let mut bracket: Option<(f64, f64)> = None;
    for k in 1..=GRID {
        let r = LO + STEP * k as f64;
        let f = irr_npv(r, cf);
        if f.is_finite() && prev_f.is_finite() && f * prev_f < 0.0 {
            bracket = Some((prev_r, r));
            break;
        }
        prev_r = r;
        prev_f = f;
    }
    let (mut a, mut b) = bracket?;
    let mut fa = irr_npv(a, cf);
    for _ in 0..IRR_BISECT_MAX {
        let m = 0.5 * (a + b);
        let fm = irr_npv(m, cf);
        if fm == 0.0 || (b - a).abs() <= IRR_STEP_TOL {
            return Some(m);
        }
        if fa * fm < 0.0 {
            b = m;
        } else {
            a = m;
            fa = fm;
        }
    }
    Some(0.5 * (a + b))
}
