// Concern: the INTEGER / COMBINATORIAL worksheet functions (GCD LCM FACT COMBIN) — number-theory and counting built-ins over non-negative integers, each truncating its arguments toward zero, rejecting a negative or out-of-domain argument with `#NUM!`; GCD/LCM/FACT stay within the exact-integer f64 range (`#NUM!` past it, matching Excel — FACT's cap is a true f64 overflow), while COMBIN follows Excel in returning the (lossy) f64 for a large-but-valid result and refuses only at genuine f64 overflow — so a too-large answer is `#NUM!` (or a lossy float, per Excel) rather than an infinite loop or a panic | Non-concern: the arithmetic/rounding functions (func/math.rs), the transcendental functions (func/trig.rs), the volatile random functions (func/random.rs), the registry table + dispatch (func/mod.rs), and the shared `one_num`/`collect_numbers`/`finite_or_num` helpers (func/helpers.rs) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
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

/// `GCD(a, b, …)` — the greatest common divisor of the (truncated) integer arguments; a negative
/// argument is `#NUM!`. `GCD` of all-zero (or no numeric) data is `0` (`gcd(0, 0) = 0`).
pub(crate) fn gcd_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match gather_nonneg_ints(ctx, args) {
        Err(k) => Value::Error(k),
        Ok(ints) => Value::Number(ints.into_iter().fold(0u64, gcd2) as f64),
    }
}

/// `LCM(a, b, …)` — the least common multiple of the (truncated) integer arguments; a negative
/// argument is `#NUM!`, and a result past the exact-integer range (`2^53`) is `#NUM!` (Excel).
/// `LCM` is `0` if any argument is `0`.
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
        // lcm(acc, x) = acc / gcd(acc, x) · x, in u128 so the intermediate cannot wrap. gcd(acc, x) =
        // gcd(x, acc mod x); `acc mod x` fits in u64 (it is < x), so the Euclid step stays in u64.
        let g = gcd2((acc % (x as u128)) as u64, x) as u128;
        acc = acc / g * (x as u128);
        if acc as f64 >= MAX_EXACT_INT {
            return Value::Error(ErrKind::Num);
        }
    }
    Value::Number(acc as f64)
}

/// `FACT(number)` — the factorial of `number` truncated toward zero. A negative `number` is `#NUM!`;
/// `FACT(0) = 1`; above `170` the result overflows `f64`, so it is `#NUM!` (Excel's cap), which also
/// bounds the product loop.
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
/// `C(n, i+1)` after step `i`; `acc·(n−i)` is divisible by `i+1` (that quotient IS the next binomial),
/// so every division is exact and `acc` is never rounded — only the final `as f64` demotion is lossy.
fn combin_exact(n: u64, k_small: u64) -> Option<u128> {
    let mut acc: u128 = 1;
    for i in 0..k_small {
        acc = acc.checked_mul((n - i) as u128)? / (i as u128 + 1);
    }
    Some(acc)
}

/// `COMBIN(number, number_chosen)` — the count of unordered `number_chosen`-combinations of `number`
/// items (the binomial coefficient), both arguments truncated toward zero. Requires
/// `0 ≤ number_chosen ≤ number`; otherwise `#NUM!`. Computed EXACTLY in `u128` over the smaller of
/// `k`/`n−k`, then demoted to `f64` (round-to-nearest). Unlike FACT — whose 170-cap is a true `f64`
/// overflow — COMBIN is NOT capped at the exact-integer range: Excel returns the (lossy) `f64` for a
/// large-but-valid result (`COMBIN(60, 30) ≈ 1.18e17`), so charlie does too, refusing with `#NUM!`
/// only at genuine `f64` overflow (`finite_or_num`, at `n ≈ 1030`). When the exact `u128` product
/// itself overflows (`n ≳ 137`), the value is far past `f64`-exact anyway and is built by the
/// multiplicative `f64` formula, matching Excel's own float in that range.
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
        // Exact integer (correctly-rounded on demotion) — covers every result within `u128`.
        Some(exact) => finite_or_num(exact as f64),
        // The exact product overflowed `u128`; Excel is lossy here too, so match its `f64` build.
        // DIVIDE before multiplying (`r/(i+1)·(n−i)`, not `r·(n−i)/(i+1)`): each running value is the
        // monotonically-growing binomial `C(n, i+1)` bounded by the final peak, so a near-`f64`-max
        // result like `COMBIN(1029, 514) ≈ 1.43e308` never overflows mid-build — only a genuinely
        // out-of-range final (`COMBIN(1030, 515)`) reaches `∞`, which `finite_or_num` maps to `#NUM!`.
        None => {
            let mut r = 1.0f64;
            for i in 0..k_small {
                r = r / ((i + 1) as f64) * ((n_u - i) as f64);
            }
            finite_or_num(r)
        }
    }
}
