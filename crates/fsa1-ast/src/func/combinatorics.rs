// Concern: GCD LCM FACT COMBIN | Non-concern: general arithmetic, statistics | IO: (&mut EvalCtx, &[Expr]) -> Value
use super::*;

/// The largest integer an `f64` represents exactly (`2^53`). GCD/LCM arguments and results, and the
/// FACT/COMBIN results, must stay `< 2^53` to be exact — Excel errors (`#NUM!`) rather than return a
/// silently-rounded integer past this, and so do we.
const MAX_EXACT_INT: f64 = 9_007_199_254_740_992.0;

/// The Euclidean GCD of two non-negative integers (`gcd(a, 0) = a`).
fn gcd2(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// Gather GCD/LCM's variadic integer arguments: each datum is coerced (SUM's direct-vs-in-range
/// asymmetry, via [`collect_numbers`]), truncated toward zero, and required to be a non-negative
/// integer magnitude below `2^53` — a negative argument is `#NUM!`, matching Excel. Returns the
/// truncated `u64` magnitudes.
fn gather_nonneg_ints(ctx: &mut EvalCtx, args: &[Expr]) -> Result<Vec<u64>, ErrKind> {
    let nums = collect_numbers(ctx, args)?;
    let mut ints = Vec::with_capacity(nums.len());
    for n in nums {
        let t = n.trunc();
        if !(0.0..MAX_EXACT_INT).contains(&t) {
            return Err(ErrKind::Num);
        }
        ints.push(t as u64);
    }
    Ok(ints)
}

pub(crate) fn gcd_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match gather_nonneg_ints(ctx, args) {
        Err(k) => Value::Error(k),
        Ok(ints) => Value::Number(ints.into_iter().fold(0u64, gcd2) as f64),
    }
}

pub(crate) fn lcm_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let ints = match gather_nonneg_ints(ctx, args) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let mut acc: u128 = 1;
    for x in ints {
        if x == 0 {
            return Value::Number(0.0);
        }
        // `acc / gcd · x` in u128 so the intermediate cannot wrap; `acc mod x` is < x, so the Euclid step stays in u64.
        let g = gcd2((acc % (x as u128)) as u64, x) as u128;
        acc = acc / g * (x as u128);
        if acc as f64 >= MAX_EXACT_INT {
            return Value::Error(ErrKind::Num);
        }
    }
    Value::Number(acc as f64)
}

pub(crate) fn fact_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let n = match one_num(ctx, &args[0]) {
        Ok(x) => x.trunc(),
        Err(k) => return Value::Error(k),
    };
    if !(0.0..=170.0).contains(&n) {
        return Value::Error(ErrKind::Num);
    }
    let mut r = 1.0f64;
    for i in 2..=(n as u64) {
        r *= i as f64;
    }
    finite_or_num(r)
}

/// The exact binomial coefficient `C(n, k_small)` as a `u128`, or `None` if the exact integer product
/// overflows `u128` (a result far past the `f64`-exact range, which the caller then approximates in
/// `f64` to match Excel). `k_small` is the already-reduced `min(k, n−k)`. The running accumulator is
fn combin_exact(n: u64, k_small: u64) -> Option<u128> {
    let mut acc: u128 = 1;
    for i in 0..k_small {
        acc = acc.checked_mul((n - i) as u128)? / (i as u128 + 1);
    }
    Some(acc)
}

pub(crate) fn combin_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let n = match one_num(ctx, &args[0]) {
        Ok(x) => x.trunc(),
        Err(k) => return Value::Error(k),
    };
    let k = match one_num(ctx, &args[1]) {
        Ok(x) => x.trunc(),
        Err(k) => return Value::Error(k),
    };
    if n < 0.0 || k < 0.0 || k > n {
        return Value::Error(ErrKind::Num);
    }
    let n_u = n as u64;
    let k_small = (k as u64).min(n_u - k as u64);
    match combin_exact(n_u, k_small) {
        Some(exact) => finite_or_num(exact as f64),
        // DIVIDE before multiplying: each running value is then the binomial `C(n, i+1)`, bounded by the final peak, so a near-f64-max result never overflows mid-build; only a genuinely out-of-range final reaches infinity.
        None => {
            let mut r = 1.0f64;
            for i in 0..k_small {
                r = r / ((i + 1) as f64) * ((n_u - i) as f64);
            }
            finite_or_num(r)
        }
    }
}
