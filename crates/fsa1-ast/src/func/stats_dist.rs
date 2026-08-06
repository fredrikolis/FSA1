// Concern: the normal-distribution built-ins and STANDARDIZE | Non-concern: descriptive statistics, ranking, regression | IO: (&mut EvalCtx, &[Expr]) -> Value
use super::*;
use std::f64::consts::PI;

/// The standard-normal probability density `φ(z) = e^(−z²/2) / √(2π)`.
fn norm_pdf(z: f64) -> f64 {
    (-0.5 * z * z).exp() / (2.0 * PI).sqrt()
}

/// The standard-normal cumulative distribution `Φ(z) = P(Z ≤ z)`, via Graeme West's rational
/// approximation — accurate to roughly 1e-15 across the real line, well inside the ENG6 numeric
/// tolerance, so it reproduces Excel's NORM.S.DIST/NORM.DIST cumulative values.
fn norm_cdf(z: f64) -> f64 {
    let x = z.abs();
    if x > 37.0 {
        return if z > 0.0 { 1.0 } else { 0.0 };
    }
    let e = (-0.5 * x * x).exp();
    let tail = if x < 7.071_067_811_865_47 {
        let n = ((((((3.526_249_659_989_11e-2 * x + 0.700_383_064_443_688) * x
            + 6.373_962_203_531_65)
            * x
            + 33.912_866_078_383)
            * x
            + 112.079_291_497_871)
            * x
            + 221.213_596_169_931)
            * x
            + 220.206_867_912_376)
            * e;
        let d = ((((((8.838_834_764_831_84e-2 * x + 1.755_667_163_182_64) * x
            + 16.064_177_579_207)
            * x
            + 86.780_732_202_946_1)
            * x
            + 296.564_248_779_674)
            * x
            + 637.333_633_378_831)
            * x
            + 793.826_512_519_948)
            * x
            + 440.413_735_824_752;
        n / d
    } else {
        let mut b = x + 0.65;
        b = x + 4.0 / b;
        b = x + 3.0 / b;
        b = x + 2.0 / b;
        b = x + 1.0 / b;
        e / b / 2.506_628_274_631
    };
    if z > 0.0 { 1.0 - tail } else { tail }
}

/// The inverse standard-normal CDF `Φ⁻¹(p)` for `p ∈ (0, 1)`: Peter Acklam's rational seed refined by
/// one Halley step against [`norm_cdf`], reaching full double precision. `p` outside `(0, 1)` is a
/// caller-level `#NUM!`, so this is only called on an in-domain probability.
fn norm_inv(p: f64) -> f64 {
    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_69e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239e0,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e1,
        1.615_858_368_580_409e2,
        -1.556_989_798_598_866e2,
        6.680_131_188_771_972e1,
        -1.328_068_155_288_572e1,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838e0,
        -2.549_732_539_343_734e0,
        4.374_664_141_464_968e0,
        2.938_163_982_698_783e0,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-3,
        3.224_671_290_700_398e-1,
        2.445_134_137_142_996e0,
        3.754_408_661_907_416e0,
    ];
    const P_LOW: f64 = 0.02425;
    let mut x = if p < P_LOW {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= 1.0 - P_LOW {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };
    // One Halley refinement step drives the ~1e-9 rational seed to machine precision.
    let e = norm_cdf(x) - p;
    let u = e * (2.0 * PI).sqrt() * (0.5 * x * x).exp();
    x -= u / (1.0 + 0.5 * x * u);
    x
}

pub(crate) fn standardize_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let x = match one_num(ctx, &args[0]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let mean = match one_num(ctx, &args[1]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let sd = match one_num(ctx, &args[2]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    if sd <= 0.0 {
        return Value::Error(ErrKind::Num);
    }
    finite_or_num((x - mean) / sd)
}

/// Shared body of `NORM.DIST` / `NORMDIST`: `cumulative` TRUE gives the CDF `Φ((x−mean)/sd)`, FALSE the
/// density `φ((x−mean)/sd)/sd`. A non-positive `sd` is `#NUM!` (Excel).
fn norm_dist(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let x = match one_num(ctx, &args[0]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let mean = match one_num(ctx, &args[1]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let sd = match one_num(ctx, &args[2]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let cumulative = match coerce_bool(&scalarize(ctx.eval(&args[3]))) {
        Ok(b) => b,
        Err(k) => return Value::Error(k),
    };
    if sd <= 0.0 {
        return Value::Error(ErrKind::Num);
    }
    let z = (x - mean) / sd;
    finite_or_num(if cumulative {
        norm_cdf(z)
    } else {
        norm_pdf(z) / sd
    })
}

pub(crate) fn norm_dist_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    norm_dist(ctx, args)
}

pub(crate) fn normdist_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    norm_dist(ctx, args)
}

/// Shared body of `NORM.INV` / `NORMINV`: the inverse normal CDF `mean + standard_dev·Φ⁻¹(p)`. A
/// `p` outside `(0, 1)`, or a non-positive `standard_dev`, is `#NUM!` (Excel).
fn norm_inv_dispatch(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let p = match one_num(ctx, &args[0]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let mean = match one_num(ctx, &args[1]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let sd = match one_num(ctx, &args[2]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    if !(0.0 < p && p < 1.0) || sd <= 0.0 {
        return Value::Error(ErrKind::Num);
    }
    finite_or_num(mean + sd * norm_inv(p))
}

pub(crate) fn norm_inv_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    norm_inv_dispatch(ctx, args)
}

pub(crate) fn norminv_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    norm_inv_dispatch(ctx, args)
}

pub(crate) fn norm_s_dist_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let z = match one_num(ctx, &args[0]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let cumulative = match coerce_bool(&scalarize(ctx.eval(&args[1]))) {
        Ok(b) => b,
        Err(k) => return Value::Error(k),
    };
    finite_or_num(if cumulative { norm_cdf(z) } else { norm_pdf(z) })
}

pub(crate) fn normsdist_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let z = match one_num(ctx, &args[0]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    finite_or_num(norm_cdf(z))
}

pub(crate) fn norm_s_inv_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let p = match one_num(ctx, &args[0]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    if !(0.0 < p && p < 1.0) {
        return Value::Error(ErrKind::Num);
    }
    finite_or_num(norm_inv(p))
}

pub(crate) fn normsinv_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    norm_s_inv_fn(ctx, args)
}
