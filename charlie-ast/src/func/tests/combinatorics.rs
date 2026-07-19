// Concern: UNIT-TEST pins for the integer/combinatorial family (GCD LCM FACT COMBIN) exercised through `FUNCS` dispatch — hand-verified Excel values, the truncate-toward-zero of non-integer arguments, and the domain-error mappings (a negative argument → #NUM!, FACT above 170 → #NUM!, COMBIN with k>n → #NUM!, a zero in LCM → 0) | Non-concern: the combinatorics impls (`func/combinatorics.rs`) and the shared test fixtures (the parent `tests` module owns `num`/`call`/`col_range`) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
use super::*;

#[test]
fn gcd_and_lcm() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("GCD", vec![num(24.0), num(36.0)]), &g),
        Value::Number(12.0)
    );
    assert_eq!(
        eval(&call("GCD", vec![num(5.0), num(0.0)]), &g),
        Value::Number(5.0)
    );
    assert_eq!(
        eval(&call("GCD", vec![num(7.0), num(1.0)]), &g),
        Value::Number(1.0)
    );
    // Truncation toward zero before the GCD.
    assert_eq!(
        eval(&call("GCD", vec![num(24.9), num(36.1)]), &g),
        Value::Number(12.0)
    );
    // A negative argument is #NUM!.
    assert_eq!(
        eval(&call("GCD", vec![num(-5.0), num(10.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    // Over a range.
    let data = Grid::new(1, vec![Value::Number(12.0), Value::Number(18.0)]);
    assert_eq!(
        eval(&call("GCD", vec![col_range(2)]), &data),
        Value::Number(6.0)
    );

    assert_eq!(
        eval(&call("LCM", vec![num(4.0), num(6.0)]), &g),
        Value::Number(12.0)
    );
    assert_eq!(
        eval(&call("LCM", vec![num(3.0), num(4.0), num(5.0)]), &g),
        Value::Number(60.0)
    );
    // A zero in LCM makes the whole result 0.
    assert_eq!(
        eval(&call("LCM", vec![num(0.0), num(5.0)]), &g),
        Value::Number(0.0)
    );
    assert_eq!(
        eval(&call("LCM", vec![num(-4.0), num(6.0)]), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn fact_and_combin() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("FACT", vec![num(5.0)]), &g),
        Value::Number(120.0)
    );
    assert_eq!(eval(&call("FACT", vec![num(0.0)]), &g), Value::Number(1.0));
    // Truncates toward zero (FACT(1.9) = FACT(1) = 1).
    assert_eq!(eval(&call("FACT", vec![num(1.9)]), &g), Value::Number(1.0));
    // Negative → #NUM!; above 170 overflows f64 → #NUM!.
    assert_eq!(
        eval(&call("FACT", vec![num(-1.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("FACT", vec![num(171.0)]), &g),
        Value::Error(ErrKind::Num)
    );

    assert_eq!(
        eval(&call("COMBIN", vec![num(8.0), num(2.0)]), &g),
        Value::Number(28.0)
    );
    assert_eq!(
        eval(&call("COMBIN", vec![num(10.0), num(3.0)]), &g),
        Value::Number(120.0)
    );
    assert_eq!(
        eval(&call("COMBIN", vec![num(5.0), num(5.0)]), &g),
        Value::Number(1.0)
    );
    assert_eq!(
        eval(&call("COMBIN", vec![num(5.0), num(0.0)]), &g),
        Value::Number(1.0)
    );
    // A larger exact case stays an exact integer (multiplicative build + round).
    assert_eq!(
        eval(&call("COMBIN", vec![num(52.0), num(5.0)]), &g),
        Value::Number(2_598_960.0)
    );
    // k > n and negative arguments are #NUM!.
    assert_eq!(
        eval(&call("COMBIN", vec![num(4.0), num(5.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("COMBIN", vec![num(-1.0), num(1.0)]), &g),
        Value::Error(ErrKind::Num)
    );
}

/// Assert an evaluated result is a number within a tight RELATIVE tolerance of `expected` — for the
/// large COMBIN results whose exact integer exceeds `f64`'s 2^53 range, so the low bits are lossy.
fn rel_approx(v: Value, expected: f64) {
    match v {
        Value::Number(n) => assert!(
            (n - expected).abs() <= expected.abs() * 1e-12,
            "got {n}, want {expected} (rel Δ={})",
            (n - expected).abs() / expected.abs()
        ),
        other => panic!("expected a number, got {other:?}"),
    }
}

// COMBIN follows Excel: a large-but-valid result past the exact-integer (2^53) range is NOT #NUM! —
// it is the (lossy) f64, refused only at genuine f64 overflow (n ≈ 1030). This is the class the old
// `>= MAX_EXACT_INT` cap wrongly rejected; the values here are hand-verified against exact math and
// Excel's own float. (The lib oracle can't cover COMBIN — `formulas` returns #NAME?; see
// KNOWN-LIB-GAPS.md — so these Rust pins are the parity guard.)
#[test]
fn combin_large_matches_excel_not_num() {
    let g = Grid::new(1, vec![Value::Blank]);
    // Exact-integer path (u128, then a single correctly-rounded demotion). C(58,29) ≈ 3.01e16 and
    // C(60,30) ≈ 1.18e17 are both past 2^53 yet each equals its nearest f64 exactly here.
    assert_eq!(
        eval(&call("COMBIN", vec![num(58.0), num(29.0)]), &g),
        Value::Number(3.006_726_649_954_104e16)
    );
    assert_eq!(
        eval(&call("COMBIN", vec![num(60.0), num(30.0)]), &g),
        Value::Number(1.182_645_815_648_614_2e17)
    );
    // Float-fallback path (the exact u128 product overflows at n ≳ 137): matches Excel's own float.
    rel_approx(
        eval(&call("COMBIN", vec![num(200.0), num(100.0)]), &g),
        9.054_851_465_610_328e58,
    );
    // Near the very top of the domain (n ≈ 1029) is still finite, hence still a value, not #NUM!.
    rel_approx(
        eval(&call("COMBIN", vec![num(1029.0), num(514.0)]), &g),
        1.429_820_686_498_904e308,
    );
    // One step further overflows f64 → #NUM! (the only cap that matches Excel).
    assert_eq!(
        eval(&call("COMBIN", vec![num(1030.0), num(515.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    // The exact path stays exact right at the old boundary (no off-by-one from a lost intermediate):
    // C(50,25) = 126410606437752 is < 2^53 and must be dead-on.
    assert_eq!(
        eval(&call("COMBIN", vec![num(50.0), num(25.0)]), &g),
        Value::Number(126_410_606_437_752.0)
    );
}
