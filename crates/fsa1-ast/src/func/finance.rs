// Concern: the financial built-ins and their iterative rate finds | Non-concern: general arithmetic, the date serial maps | IO: (&mut EvalCtx, &[Expr]) -> Value
use super::*;

/// `base` raised to a NON-NEGATIVE integer power via a deterministic left-to-right `f64` multiply
/// loop. Deliberately NOT [`f64::powi`]: a plain multiply chain is one fixed IEEE-754 round-to-nearest
/// sequence that a from-scratch oracle can reproduce EXACTLY (so a bit-exact conformance literal is
/// authorable), whereas `powi`/`llvm.powi` may pick a different (equally-valid) rounding per toolchain.
fn pow_int(base: f64, exp: u32) -> f64 {
    let mut acc = 1.0;
    for i in 0..exp {
        let next = acc * base;
        if !next.is_finite() || next.abs() == acc.abs() {
            acc = next;
            if base < 0.0 && (exp - i - 1) % 2 == 1 {
                acc = -acc;
            }
            return acc;
        }
        acc = next;
    }
    acc
}

/// The integer period count `pow_int` consumes: `nper` truncated toward zero, magnitude-capped to
/// `u32`. Shared by every annuity built-in so the "v1 pins the integer-period annuity" rule (see the
/// module comment) lives in ONE place.
fn int_periods(nper: f64) -> u32 {
    nper.trunc().abs().min(u32::MAX as f64) as u32
}

/// Evaluate a call's arguments to scalar numbers in order — the pure-scalar annuity family (PMT/FV/
/// NPER/RATE/IPMT/PPMT) takes only scalars — propagating the leftmost error. Arity is already gated by
/// dispatch, so the required leading positions are always present; optionals are read via `Vec::get`.
fn scalars(ctx: &mut EvalCtx, args: &[Expr]) -> Result<Vec<f64>, ErrKind> {
    args.iter().map(|e| one_num(ctx, e)).collect()
}

/// The annuity BALANCE residual shared by the whole family: with `t = (1+rate)^nper`,
/// `pv·t + pmt·(1 + rate·type)·(t − 1)/rate + fv` (the linear `pv + pmt·nper + fv` when `rate == 0`).
/// Its root in a chosen unknown is what each annuity function returns; RATE roots it in `rate`.
fn annuity_balance(rate: f64, nper: f64, pmt: f64, pv: f64, fv: f64, typ: f64) -> f64 {
    if rate == 0.0 {
        return pv + pmt * nper + fv;
    }
    let t = pow_int(1.0 + rate, int_periods(nper));
    pv * t + pmt * (1.0 + rate * typ) * (t - 1.0) / rate + fv
}

/// The ONE condition the payment-splitting family turns into a located `#DIV/0!`, kept distinct from
/// the `#NUM!` overflow demotion [`finite_or_num`] applies.
fn annuity_denom_is_zero(rate: f64, nper: f64, typ: f64) -> bool {
    if rate == 0.0 {
        nper == 0.0
    } else {
        (1.0 + rate * typ) * (pow_int(1.0 + rate, int_periods(nper)) - 1.0) == 0.0
    }
}

/// The closed-form annuity payment (PMT's core): [`annuity_balance`] solved for `pmt`. A zero
/// denominator ([`annuity_denom_is_zero`]) yields a non-finite value every caller (PMT/IPMT/PPMT)
/// screens FIRST as a located `#DIV/0!`. `rate == 0` degenerates to the linear `−(pv+fv)/nper`.
fn annuity_pmt(rate: f64, nper: f64, pv: f64, fv: f64, typ: f64) -> f64 {
    if rate == 0.0 {
        return -(pv + fv) / nper;
    }
    let t = pow_int(1.0 + rate, int_periods(nper));
    let denom = (1.0 + rate * typ) * (t - 1.0);
    -(fv + pv * t) * rate / denom
}

/// The closed-form future value (FV's core): `−annuity_balance(…, fv = 0)` — the balance the payments
/// and present value grow to. Also the running loan balance IPMT/PPMT read at a period boundary.
fn fv_core(rate: f64, nper: f64, pmt: f64, pv: f64, typ: f64) -> f64 {
    -annuity_balance(rate, nper, pmt, pv, 0.0, typ)
}

/// The interest portion of period `per`'s payment: the balance at the START of the period ([`fv_core`]
/// over `per − 1` elapsed periods) times `rate`, with Excel's begin-period (`type == 1`) adjustment —
/// the balance is discounted one period and period 1 carries no interest.
fn ipmt_core(rate: f64, per: f64, nper: f64, pv: f64, fv: f64, typ: f64) -> f64 {
    let pmt = annuity_pmt(rate, nper, pv, fv, typ);
    let mut ip = fv_core(rate, per - 1.0, pmt, pv, typ) * rate;
    if typ != 0.0 {
        ip /= 1.0 + rate;
        if per == 1.0 {
            ip = 0.0;
        }
    }
    ip
}

/// The principal portion of period `per`'s payment: the whole payment minus its interest
/// ([`ipmt_core`]), so `PPMT + IPMT = PMT` exactly.
fn ppmt_core(rate: f64, per: f64, nper: f64, pv: f64, fv: f64, typ: f64) -> f64 {
    annuity_pmt(rate, nper, pv, fv, typ) - ipmt_core(rate, per, nper, pv, fv, typ)
}

pub(crate) fn pmt_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let v = match scalars(ctx, args) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (rate, nper, pv) = (v[0], v[1], v[2]);
    let fv = *v.get(3).unwrap_or(&0.0);
    let typ = *v.get(4).unwrap_or(&0.0);
    // The #DIV/0! guard, kept LOCATED (distinct from #NUM! overflow) and shared with IPMT/PPMT.
    if annuity_denom_is_zero(rate, nper, typ) {
        return Value::Error(ErrKind::Div0);
    }
    finite_or_num(annuity_pmt(rate, nper, pv, fv, typ))
}

pub(crate) fn fv_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let v = match scalars(ctx, args) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (rate, nper, pmt) = (v[0], v[1], v[2]);
    let pv = *v.get(3).unwrap_or(&0.0);
    let typ = *v.get(4).unwrap_or(&0.0);
    finite_or_num(fv_core(rate, nper, pmt, pv, typ))
}

pub(crate) fn nper_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let v = match scalars(ctx, args) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (rate, pmt, pv) = (v[0], v[1], v[2]);
    let fv = *v.get(3).unwrap_or(&0.0);
    let typ = *v.get(4).unwrap_or(&0.0);
    if rate == 0.0 {
        // A zero `pmt` must be a located #DIV/0!, not the #NUM! an unguarded division would demote to.
        if pmt == 0.0 {
            return Value::Error(ErrKind::Div0);
        }
        return finite_or_num(-(pv + fv) / pmt);
    }
    let w = pmt * (1.0 + rate * typ) / rate;
    let ratio = (w - fv) / (w + pv);
    // A non-positive OR NaN (a 0/0 degenerate) ratio has no real logarithm — no period count.
    if ratio.is_nan() || ratio <= 0.0 {
        return Value::Error(ErrKind::Num);
    }
    finite_or_num(ratio.ln() / (1.0 + rate).ln())
}

/// The HARD cap on RATE's Newton iterations — the guarantee RATE can never spin forever.
const RATE_NEWTON_MAX: usize = 100;
/// The per-unit-of-cashflow residual a converged RATE must satisfy: the `|annuity_balance|` bar is
/// this times the problem's cashflow magnitude. FIXED-absolute (1e-6) would reject a perfectly good
/// large-magnitude annuity — a big balance derivative makes Newton's relative step converge while the
/// absolute residual stays far above 1e-6 — so RATE scales it exactly as XIRR scales [`IRR_RESID_TOL`].
const RATE_RESID_TOL: f64 = 1e-6;

pub(crate) fn rate_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let v = match scalars(ctx, args) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (nper, pmt, pv) = (v[0], v[1], v[2]);
    let fv = *v.get(3).unwrap_or(&0.0);
    let typ = *v.get(4).unwrap_or(&0.0);
    let guess = *v.get(5).unwrap_or(&0.1);
    let start = if guess.is_finite() { guess } else { 0.1 };
    let f = |r: f64| annuity_balance(r, nper, pmt, pv, fv, typ);
    // Symmetric finite-difference derivative; the step scales with `|r|` to stay meaningful at any magnitude.
    let d = |r: f64| {
        let h = 1e-6 * r.abs().max(1e-3);
        (annuity_balance(r + h, nper, pmt, pv, fv, typ)
            - annuity_balance(r - h, nper, pmt, pv, fv, typ))
            / (2.0 * h)
    };
    // Convergence is judged RELATIVE to the cashflow magnitude, not against an absolute tolerance.
    let scale = (pv.abs() + (pmt * nper).abs() + fv.abs()).max(1.0);
    match newton_rate(
        f,
        d,
        start,
        RATE_RESID_TOL * scale,
        RATE_NEWTON_MAX,
        f64::NEG_INFINITY,
    ) {
        Some(r) => finite_or_num(r),
        None => Value::Error(ErrKind::Num),
    }
}

pub(crate) fn ipmt_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let v = match scalars(ctx, args) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (rate, per, nper, pv) = (v[0], v[1], v[2], v[3]);
    let fv = *v.get(4).unwrap_or(&0.0);
    let typ = *v.get(5).unwrap_or(&0.0);
    if per < 1.0 || per > nper {
        return Value::Error(ErrKind::Num);
    }
    if annuity_denom_is_zero(rate, nper, typ) {
        return Value::Error(ErrKind::Div0);
    }
    finite_or_num(ipmt_core(rate, per, nper, pv, fv, typ))
}

pub(crate) fn ppmt_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let v = match scalars(ctx, args) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (rate, per, nper, pv) = (v[0], v[1], v[2], v[3]);
    let fv = *v.get(4).unwrap_or(&0.0);
    let typ = *v.get(5).unwrap_or(&0.0);
    if per < 1.0 || per > nper {
        return Value::Error(ErrKind::Num);
    }
    if annuity_denom_is_zero(rate, nper, typ) {
        return Value::Error(ErrKind::Div0);
    }
    finite_or_num(ppmt_core(rate, per, nper, pv, fv, typ))
}

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

/// The HARD cap on Newton iterations — the primary guarantee IRR/XIRR can never spin forever.
const IRR_NEWTON_MAX: usize = 50;

/// The HARD cap on bisection halvings in the fallback bracket search.
const IRR_BISECT_MAX: usize = 200;

/// The relative step size below which Newton is declared converged.
const IRR_STEP_TOL: f64 = 1e-12;

/// The residual `|NPV|` a converged rate must satisfy — guards a spurious tiny Newton step at a
/// non-root (huge derivative) from being mistaken for a solution. XIRR scales this by its cashflow
/// magnitude (its flows can dwarf IRR's unit-scale ones), so convergence is judged relative to scale.
const IRR_RESID_TOL: f64 = 1e-6;

/// The ONE bounded Newton loop behind every iterative rate find; only `f`/`d` and `domain_floor`
/// differ across them. `domain_floor` is `-1.0` where `(1+r)^t` must stay defined and
/// `f64::NEG_INFINITY` where a negative return is legitimate. Cannot loop forever: `max_iter` is a
/// hard integer cap.
fn newton_rate(
    f: impl Fn(f64) -> f64,
    d: impl Fn(f64) -> f64,
    start: f64,
    resid_tol: f64,
    max_iter: usize,
    domain_floor: f64,
) -> Option<f64> {
    let mut rate = start;
    for _ in 0..max_iter {
        let y = f(rate);
        let dy = d(rate);
        if dy == 0.0 || !y.is_finite() || !dy.is_finite() {
            return None;
        }
        let step = y / dy;
        let next = rate - step;
        if !next.is_finite() || next <= domain_floor {
            return None;
        }
        if step.abs() <= IRR_STEP_TOL * next.abs().max(1.0) {
            return (f(next).abs() <= resid_tol).then_some(next);
        }
        rate = next;
    }
    None
}

/// The bracketing fallback when Newton fails. `None` when the grid holds no sign change — the honest
/// "no real rate" the caller turns into `#NUM!`. Cannot loop forever: both loops have hard caps.
fn bisect_rate(f: impl Fn(f64) -> f64) -> Option<f64> {
    // A fixed grid strictly above -1; 0.005 steps resolve any economically-meaningful root.
    const GRID: usize = 1000;
    const LO: f64 = -0.999;
    const STEP: f64 = 0.005;
    let mut prev_r = LO;
    let mut prev_f = f(prev_r);
    let mut bracket: Option<(f64, f64)> = None;
    for k in 1..=GRID {
        let r = LO + STEP * k as f64;
        let fr = f(r);
        if fr.is_finite() && prev_f.is_finite() && fr * prev_f < 0.0 {
            bracket = Some((prev_r, r));
            break;
        }
        prev_r = r;
        prev_f = fr;
    }
    let (mut a, mut b) = bracket?;
    let mut fa = f(a);
    for _ in 0..IRR_BISECT_MAX {
        let m = 0.5 * (a + b);
        let fm = f(m);
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
    if let Some(r) = newton_rate(
        |r| irr_npv(r, &cf),
        |r| irr_npv_deriv(r, &cf),
        start,
        IRR_RESID_TOL,
        IRR_NEWTON_MAX,
        -1.0,
    ) {
        return finite_or_num(r);
    }
    match bisect_rate(|r| irr_npv(r, &cf)) {
        Some(r) => finite_or_num(r),
        None => Value::Error(ErrKind::Num),
    }
}

/// The Actual/365 day-count tenor for each cashflow: `(date_i − date_0)/365`, `date_0` the FIRST date
/// (Excel's schedule start). `None` if any later date precedes the start (Excel refuses an out-of-order
/// schedule) — the caller turns it into `#NUM!`.
fn day_tenors(dates: &[f64]) -> Option<Vec<f64>> {
    let d0 = dates[0];
    let mut ts = Vec::with_capacity(dates.len());
    for &d in dates {
        if d < d0 {
            return None;
        }
        ts.push((d - d0) / 365.0);
    }
    Some(ts)
}

/// Pair an XNPV/XIRR `values` range with its `dates` range: materialize both to blocks ([`block`]),
/// require equal NON-EMPTY cell counts, and coerce each cell to a number (a date to a whole-day serial
/// via `floor`). Mismatched lengths or an empty stream is `#NUM!`; a non-numeric cell or a propagated
/// error surfaces its error.
fn x_cashflows(
    ctx: &mut EvalCtx,
    values: &Expr,
    dates: &Expr,
) -> Result<(Vec<f64>, Vec<f64>), ErrKind> {
    let (_, _, vcells) = block(ctx, values)?;
    let (_, _, dcells) = block(ctx, dates)?;
    if vcells.is_empty() || vcells.len() != dcells.len() {
        return Err(ErrKind::Num);
    }
    let mut cf = Vec::with_capacity(vcells.len());
    let mut ds = Vec::with_capacity(dcells.len());
    for (v, d) in vcells.iter().zip(&dcells) {
        cf.push(coerce_num(v)?);
        ds.push(coerce_num(d)?.floor());
    }
    Ok((cf, ds))
}

/// The XNPV sum `Σ cf_i/(1+rate)^t_i` over the Actual/365 tenors `t_i` — the irregular-date analogue
/// of NPV, anchored at the first date (`t_0 = 0`, so `cf_0` is undiscounted). Uses `f64::powf` (a
/// FRACTIONAL exponent), so XNPV/XIRR are closeness-graded, not bit-exact.
fn xnpv_at(rate: f64, cf: &[f64], tenors: &[f64]) -> f64 {
    let base = 1.0 + rate;
    let mut acc = 0.0;
    for (c, t) in cf.iter().zip(tenors) {
        acc += c / base.powf(*t);
    }
    acc
}

/// The derivative of [`xnpv_at`] in `rate`: `Σ −t_i·cf_i/(1+rate)^{t_i+1}`. Drives XIRR's Newton step.
fn xnpv_deriv(rate: f64, cf: &[f64], tenors: &[f64]) -> f64 {
    let base = 1.0 + rate;
    let mut acc = 0.0;
    for (c, t) in cf.iter().zip(tenors) {
        acc += -t * c / base.powf(t + 1.0);
    }
    acc
}

pub(crate) fn xnpv_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let rate = match one_num(ctx, &args[0]) {
        Ok(x) => x,
        Err(k) => return Value::Error(k),
    };
    let (cf, dates) = match x_cashflows(ctx, &args[1], &args[2]) {
        Ok(pair) => pair,
        Err(k) => return Value::Error(k),
    };
    let tenors = match day_tenors(&dates) {
        Some(t) => t,
        None => return Value::Error(ErrKind::Num),
    };
    finite_or_num(xnpv_at(rate, &cf, &tenors))
}

pub(crate) fn xirr_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (cf, dates) = match x_cashflows(ctx, &args[0], &args[1]) {
        Ok(pair) => pair,
        Err(k) => return Value::Error(k),
    };
    // A well-posed XIRR needs at least two flows (one in, one out) to bracket a sign change.
    if cf.len() < 2 {
        return Value::Error(ErrKind::Num);
    }
    let tenors = match day_tenors(&dates) {
        Some(t) => t,
        None => return Value::Error(ErrKind::Num),
    };
    let guess = match args.get(2) {
        Some(e) => match one_num(ctx, e) {
            Ok(x) => x,
            Err(k) => return Value::Error(k),
        },
        None => 0.1,
    };
    let start = if guess > -1.0 && guess.is_finite() {
        guess
    } else {
        0.1
    };
    // Convergence is judged RELATIVE to the cashflow magnitude, which here can dwarf a unit-scale flow.
    let scale = cf.iter().map(|c| c.abs()).sum::<f64>().max(1.0);
    let resid_tol = IRR_RESID_TOL * scale;
    if let Some(r) = newton_rate(
        |r| xnpv_at(r, &cf, &tenors),
        |r| xnpv_deriv(r, &cf, &tenors),
        start,
        resid_tol,
        IRR_NEWTON_MAX,
        -1.0,
    ) {
        return finite_or_num(r);
    }
    match bisect_rate(|r| xnpv_at(r, &cf, &tenors)) {
        Some(r) => finite_or_num(r),
        None => Value::Error(ErrKind::Num),
    }
}

/// The present value ([`annuity_balance`] solved for `pv`): `−(fv + pmt·(1+rate·type)·(t−1)/rate)/t`
/// with `t = (1+rate)^nper` (the linear `−(pmt·nper + fv)` when `rate == 0`). Shares the family's
/// integer-period `pow_int`, so PV is bit-exact-graded like PMT/FV.
fn pv_core(rate: f64, nper: f64, pmt: f64, fv: f64, typ: f64) -> f64 {
    if rate == 0.0 {
        return -(pmt * nper + fv);
    }
    let t = pow_int(1.0 + rate, int_periods(nper));
    -(fv + pmt * (1.0 + rate * typ) * (t - 1.0) / rate) / t
}

pub(crate) fn pv_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let v = match scalars(ctx, args) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (rate, nper, pmt) = (v[0], v[1], v[2]);
    let fv = *v.get(3).unwrap_or(&0.0);
    let typ = *v.get(4).unwrap_or(&0.0);
    finite_or_num(pv_core(rate, nper, pmt, fv, typ))
}

pub(crate) fn mirr_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let cf = match collect_numbers(ctx, &args[0..1]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let frate = match one_num(ctx, &args[1]) {
        Ok(x) => x,
        Err(k) => return Value::Error(k),
    };
    let rrate = match one_num(ctx, &args[2]) {
        Ok(x) => x,
        Err(k) => return Value::Error(k),
    };
    let n = cf.len();
    // A single flow gives a `1/0` exponent and an all-one-sign stream zeroes one leg of the ratio.
    if n < 2 || !cf.iter().any(|&c| c > 0.0) || !cf.iter().any(|&c| c < 0.0) {
        return Value::Error(ErrKind::Div0);
    }
    // NPV discounts value i from period i+1, so a positive flow compounds by `(1+rr)^(n-i-1)` and a negative one discounts by `(1+fr)^i` once the leading period cancels.
    let mut npv_pos = 0.0;
    let mut npv_neg = 0.0;
    for (i, &c) in cf.iter().enumerate() {
        if c > 0.0 {
            npv_pos += c / pow_int(1.0 + rrate, i as u32 + 1);
        } else if c < 0.0 {
            npv_neg += c / pow_int(1.0 + frate, i as u32 + 1);
        }
    }
    let fv_pos = -npv_pos * pow_int(1.0 + rrate, n as u32);
    let pv_neg = npv_neg * (1.0 + frate);
    if pv_neg == 0.0 {
        return Value::Error(ErrKind::Div0);
    }
    finite_or_num((fv_pos / pv_neg).powf(1.0 / (n as f64 - 1.0)) - 1.0)
}

/// The shared #NUM! guard for CUMIPMT/CUMPRINC (Excel refuses the SAME conditions for both): a
/// non-positive `rate`/`nper`/`pv`, a period endpoint below 1, a reversed window (`start > end`), or a
/// `type` that is neither 0 nor 1. Returns `true` when the arguments are OUT of Excel's domain.
fn cum_args_out_of_range(rate: f64, nper: f64, pv: f64, start: f64, end: f64, typ: f64) -> bool {
    rate <= 0.0
        || nper <= 0.0
        || pv <= 0.0
        || start < 1.0
        || end < 1.0
        || start > end
        || (typ != 0.0 && typ != 1.0)
}

/// Sum a per-period payment component over the inclusive window `start..=end` (both truncated toward
/// zero, Excel's period integers), applying `part` — [`ipmt_core`] for CUMIPMT, [`ppmt_core`] for
/// CUMPRINC — to each period with `fv = 0`. The ONE window-summation both cumulative functions share.
fn cum_sum(
    rate: f64,
    nper: f64,
    pv: f64,
    start: f64,
    end: f64,
    typ: f64,
    part: impl Fn(f64, f64, f64, f64, f64, f64) -> f64,
) -> f64 {
    let mut acc = 0.0;
    let (s, e) = (start.trunc() as i64, end.trunc() as i64);
    for per in s..=e {
        acc += part(rate, per as f64, nper, pv, 0.0, typ);
    }
    acc
}

pub(crate) fn cumipmt_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let v = match scalars(ctx, args) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (rate, nper, pv, start, end, typ) = (v[0], v[1], v[2], v[3], v[4], v[5]);
    if cum_args_out_of_range(rate, nper, pv, start, end, typ) {
        return Value::Error(ErrKind::Num);
    }
    finite_or_num(cum_sum(rate, nper, pv, start, end, typ, ipmt_core))
}

pub(crate) fn cumprinc_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let v = match scalars(ctx, args) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (rate, nper, pv, start, end, typ) = (v[0], v[1], v[2], v[3], v[4], v[5]);
    if cum_args_out_of_range(rate, nper, pv, start, end, typ) {
        return Value::Error(ErrKind::Num);
    }
    finite_or_num(cum_sum(rate, nper, pv, start, end, typ, ppmt_core))
}

pub(crate) fn sln_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let v = match scalars(ctx, args) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (cost, salvage, life) = (v[0], v[1], v[2]);
    if life == 0.0 {
        return Value::Error(ErrKind::Div0);
    }
    finite_or_num((cost - salvage) / life)
}

pub(crate) fn syd_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let v = match scalars(ctx, args) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (cost, salvage, life, per) = (v[0], v[1], v[2], v[3]);
    if per < 1.0 || per > life {
        return Value::Error(ErrKind::Num);
    }
    finite_or_num((cost - salvage) * (life - per + 1.0) * 2.0 / (life * (life + 1.0)))
}

pub(crate) fn db_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let v = match scalars(ctx, args) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (cost, salvage, life, period) = (v[0], v[1], v[2], v[3]);
    let month = v.get(4).copied().unwrap_or(12.0).trunc();
    if !(1.0..=12.0).contains(&month) || cost <= 0.0 || salvage < 0.0 || life < 1.0 || period < 1.0
    {
        return Value::Error(ErrKind::Num);
    }
    // With a partial first period (month < 12) the schedule spans one extra period (life+1).
    let max_period = if month == 12.0 { life } else { life + 1.0 };
    if period > max_period {
        return Value::Error(ErrKind::Num);
    }
    let rate = ((1.0 - (salvage / cost).powf(1.0 / life)) * 1000.0).round() / 1000.0;
    let mut accumulated = 0.0;
    let mut dep = 0.0;
    for p in 1..=(period as i64) {
        dep = if p == 1 {
            cost * rate * month / 12.0
        } else if (p as f64) <= life {
            (cost - accumulated) * rate
        } else {
            // The final partial period (p == life+1, reached only when month < 12).
            (cost - accumulated) * rate * (12.0 - month) / 12.0
        };
        accumulated += dep;
    }
    finite_or_num(dep)
}

pub(crate) fn ddb_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let v = match scalars(ctx, args) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (cost, salvage, life, period) = (v[0], v[1], v[2], v[3]);
    let factor = v.get(4).copied().unwrap_or(2.0);
    if cost < 0.0 || salvage < 0.0 || factor <= 0.0 || period < 1.0 || period > life {
        return Value::Error(ErrKind::Num);
    }
    let rate = factor / life;
    // Clamped so the `(1 - rate)` base never goes negative under a power; a rate >= 1 writes the whole cost off in period 1.
    let (old_value, new_value) = if rate >= 1.0 {
        let old = if period == 1.0 { cost } else { 0.0 };
        (old, 0.0)
    } else {
        (
            cost * (1.0 - rate).powf(period - 1.0),
            cost * (1.0 - rate).powf(period),
        )
    };
    let mut dep = if new_value < salvage {
        old_value - salvage
    } else {
        old_value - new_value
    };
    if dep < 0.0 {
        dep = 0.0;
    }
    finite_or_num(dep)
}

/// The truncated compounding frequency EFFECT/NOMINAL share: `npery` truncated toward zero (Excel
/// truncates the periods-per-year), or `None` when it is below 1 (no valid frequency -> the caller's
/// `#NUM!`).
fn npery_periods(npery: f64) -> Option<f64> {
    let n = npery.trunc();
    (n >= 1.0).then_some(n)
}

pub(crate) fn effect_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let v = match scalars(ctx, args) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (nominal, npery) = (v[0], v[1]);
    let n = match npery_periods(npery) {
        Some(n) if nominal > 0.0 => n,
        _ => return Value::Error(ErrKind::Num),
    };
    finite_or_num(pow_int(1.0 + nominal / n, int_periods(n)) - 1.0)
}

pub(crate) fn nominal_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let v = match scalars(ctx, args) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (effect, npery) = (v[0], v[1]);
    let n = match npery_periods(npery) {
        Some(n) if effect > 0.0 => n,
        _ => return Value::Error(ErrKind::Num),
    };
    finite_or_num(n * ((1.0 + effect).powf(1.0 / n) - 1.0))
}

pub(crate) fn pduration_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let v = match scalars(ctx, args) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (rate, pv, fv) = (v[0], v[1], v[2]);
    if rate <= 0.0 || pv <= 0.0 || fv <= 0.0 {
        return Value::Error(ErrKind::Num);
    }
    finite_or_num((fv.ln() - pv.ln()) / (1.0 + rate).ln())
}

pub(crate) fn rri_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let v = match scalars(ctx, args) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (nper, pv, fv) = (v[0], v[1], v[2]);
    if nper <= 0.0 {
        return Value::Error(ErrKind::Num);
    }
    finite_or_num((fv / pv).powf(1.0 / nper) - 1.0)
}
