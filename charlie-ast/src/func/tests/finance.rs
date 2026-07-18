// Concern: UNIT-TEST pins for the financial family built-ins (PMT FV NPER RATE IPMT PPMT NPV IRR XNPV XIRR) exercised through `FUNCS` dispatch — PMT's linear-vs-annuity branches with the located #DIV/0! denominators (shared with IPMT/PPMT's zero-denom and NPER's zero-pmt), the annuity family (FV/NPER/RATE/IPMT/PPMT) against hand-verified Excel values, RATE's scaled-residual convergence on a large-magnitude annuity and its no-real-root/NPER-no-solution/period-out-of-range #NUM! refusals, NPV period-one discounting over args and ranges, IRR's oracle-matching convergence and its guaranteed #NUM! (never a hang) on unbracketed cashflows, the Actual/365 XNPV/XIRR on irregularly-dated cashflows (the benchmark ~10.76% case) and their mismatched-length/out-of-order-date #NUM! refusals, and dispatch's arity gate | Non-concern: the finance impls (`func/finance.rs`) and the shared test fixtures (the parent `tests` module owns `num`/`call`/`col_range`/`arr`/`n`) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
use super::*;

/// Assert an eval yields a number within `tol` of `want` (the closeness bar for the iterative /
/// fractional-power finance functions, whose f64 result is not authorable bit-exact by hand).
fn assert_close(got: Value, want: f64, tol: f64) {
    match got {
        Value::Number(x) => assert!((x - want).abs() < tol, "got {x}, want {want} (tol {tol})"),
        other => panic!("expected a number near {want}, got {other:?}"),
    }
}

#[test]
fn pmt_rate_zero_is_linear_and_nonzero_is_the_annuity_form() {
    let g = Grid::new(1, vec![Value::Blank]);
    // rate == 0 -> linear: -(pv+fv)/nper = -(-1000+0)/10 = 100.
    assert_eq!(
        eval(&call("PMT", vec![num(0.0), num(10.0), num(-1000.0)]), &g),
        n(100.0)
    );
    // rate != 0 annuity: PMT(0.5, 2, -100) = 90 exactly (1.5^2 = 2.25 is dyadic-exact).
    assert_eq!(
        eval(&call("PMT", vec![num(0.5), num(2.0), num(-100.0)]), &g),
        n(90.0)
    );
    // With a future value: PMT(0.5, 2, -100, -50) = 110.
    assert_eq!(
        eval(
            &call("PMT", vec![num(0.5), num(2.0), num(-100.0), num(-50.0)]),
            &g
        ),
        n(110.0)
    );
    // type = 1 (annuity due): PMT(0.5, 2, -100, 0, 1) = 60.
    assert_eq!(
        eval(
            &call(
                "PMT",
                vec![num(0.5), num(2.0), num(-100.0), num(0.0), num(1.0)]
            ),
            &g
        ),
        n(60.0)
    );
}

#[test]
fn pmt_zero_denominator_is_div0_and_errors_propagate() {
    let g = Grid::new(1, vec![Value::Blank]);
    // nper == 0 with rate == 0 -> #DIV/0! (linear branch divides by nper).
    assert_eq!(
        eval(&call("PMT", vec![num(0.0), num(0.0), num(100.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
    // nper == 0 with rate != 0 -> denom (temp-1) is 0 -> #DIV/0!.
    assert_eq!(
        eval(&call("PMT", vec![num(0.1), num(0.0), num(100.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
    // A propagated error argument (1/0) short-circuits.
    let bad = call(
        "PMT",
        vec![call("SQRT", vec![num(-1.0)]), num(2.0), num(1.0)],
    );
    assert_eq!(eval(&bad, &g), Value::Error(ErrKind::Num));
}

#[test]
fn npv_discounts_from_period_one_over_args_and_ranges() {
    let g = Grid::new(1, vec![Value::Blank]);
    // NPV(1, 100, 200, 300) = 100/2 + 200/4 + 300/8 = 50 + 50 + 37.5 = 137.5 (all dyadic-exact).
    assert_eq!(
        eval(
            &call("NPV", vec![num(1.0), num(100.0), num(200.0), num(300.0)]),
            &g
        ),
        n(137.5)
    );
    // Same values inside a range flatten row-major and discount identically.
    let grid = Grid::new(1, vec![n(100.0), n(200.0), n(300.0)]);
    assert_eq!(
        eval(&call("NPV", vec![num(1.0), col_range(3)]), &grid),
        n(137.5)
    );
    // An error value propagates.
    let bad = call(
        "NPV",
        vec![num(1.0), num(100.0), call("SQRT", vec![num(-1.0)])],
    );
    assert_eq!(eval(&bad, &g), Value::Error(ErrKind::Num));
}

#[test]
fn irr_converges_to_the_root_matching_the_independent_oracle() {
    let g = Grid::new(1, vec![Value::Blank]);
    // IRR([-100, 30, 40, 50]) — the hand-Newton oracle's exact f64 (finance_oracle.py; NPV at
    // this rate is ~7e-15, cross-checked vs numpy np.roots = 0.088963394693350… to ~1e-12).
    let cf = arr(1, 4, vec![n(-100.0), n(30.0), n(40.0), n(50.0)]);
    assert_eq!(
        eval(&call("IRR", vec![cf]), &g),
        n(0.088_963_394_693_349_92)
    );
}

#[test]
fn irr_non_convergent_cashflows_are_num_never_a_hang() {
    let g = Grid::new(1, vec![Value::Blank]);
    // All-positive flows have no sign change -> no real IRR. Newton diverges, bisection finds no
    // bracket, and the HARD iteration caps guarantee a prompt #NUM! rather than an infinite loop.
    let allpos = arr(1, 3, vec![n(100.0), n(200.0), n(300.0)]);
    assert_eq!(
        eval(&call("IRR", vec![allpos]), &g),
        Value::Error(ErrKind::Num)
    );
    // A single flow can never bracket a root either -> #NUM! (guarded before iterating).
    let one = arr(1, 1, vec![n(-100.0)]);
    assert_eq!(
        eval(&call("IRR", vec![one]), &g),
        Value::Error(ErrKind::Num)
    );
    // A supplied guess still converges to the SAME root (to within tolerance — a different start
    // trajectory can settle on a neighbouring ULP, so this is a closeness check, not bit-exact).
    let cf = arr(1, 4, vec![n(-100.0), n(30.0), n(40.0), n(50.0)]);
    match eval(&call("IRR", vec![cf, num(0.3)]), &g) {
        Value::Number(x) => assert!(
            (x - 0.088_963_394_693_349_92).abs() < 1e-9,
            "converged to {x} from guess 0.3"
        ),
        other => panic!("expected a converged rate, got {other:?}"),
    }
}

#[test]
fn finance_arity_is_gated_by_dispatch() {
    let g = Grid::new(1, vec![Value::Blank]);
    // PMT needs 3..=5 args — 2 is under-arity -> #VALUE! from the dispatch arity gate.
    assert_eq!(
        eval(&call("PMT", vec![num(0.1), num(2.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    // NPV needs >= 2 — a lone rate is under-arity.
    assert_eq!(
        eval(&call("NPV", vec![num(0.1)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn annuity_family_matches_hand_verified_excel_values() {
    let g = Grid::new(1, vec![Value::Blank]);
    // FV(0.1, 10, -100) = 1000·(1.1^10 − 1) = 1593.7424601 (Excel).
    assert_close(
        eval(&call("FV", vec![num(0.1), num(10.0), num(-100.0)]), &g),
        1_593.742_460_1,
        1e-4,
    );
    // NPER(0.1, -100, 500): w = -1000, ratio = (-1000)/(-500) = 2, so ln(2)/ln(1.1) ≈ 7.272540897.
    assert_close(
        eval(&call("NPER", vec![num(0.1), num(-100.0), num(500.0)]), &g),
        7.272_540_897_341_713,
        1e-9,
    );
    // RATE(10, PMT(0.1,10,1000), 1000) recovers ~0.1 (Excel PMT(0.1,10,1000) = -162.74539488251152).
    assert_close(
        eval(
            &call(
                "RATE",
                vec![num(10.0), num(-162.745_394_882_511_52), num(1000.0)],
            ),
            &g,
        ),
        0.1,
        1e-6,
    );
    // IPMT(0.1, 1, 10, 1000) = -pv·rate = -100 exactly (interest on the initial 1000 balance).
    assert_eq!(
        eval(
            &call("IPMT", vec![num(0.1), num(1.0), num(10.0), num(1000.0)]),
            &g
        ),
        n(-100.0)
    );
    // IPMT(0.1, 2, 10, 1000) = -93.7254605117 (Excel) — interest on the period-2 opening balance.
    assert_close(
        eval(
            &call("IPMT", vec![num(0.1), num(2.0), num(10.0), num(1000.0)]),
            &g,
        ),
        -93.725_460_511_748_85,
        1e-6,
    );
    // PPMT(0.1, 1, 10, 1000) = PMT − IPMT = -162.7453948825 − (-100) = -62.7453948825 (Excel).
    assert_close(
        eval(
            &call("PPMT", vec![num(0.1), num(1.0), num(10.0), num(1000.0)]),
            &g,
        ),
        -62.745_394_882_511_52,
        1e-6,
    );
    // The split is exact for any period: IPMT(per) + PPMT(per) == PMT.
    let pmt = eval(&call("PMT", vec![num(0.1), num(10.0), num(1000.0)]), &g);
    let ip = eval(
        &call("IPMT", vec![num(0.1), num(3.0), num(10.0), num(1000.0)]),
        &g,
    );
    let pp = eval(
        &call("PPMT", vec![num(0.1), num(3.0), num(10.0), num(1000.0)]),
        &g,
    );
    match (pmt, ip, pp) {
        (Value::Number(pmt), Value::Number(ip), Value::Number(pp)) => {
            assert!(
                (ip + pp - pmt).abs() < 1e-9,
                "IPMT + PPMT == PMT for the period"
            );
        }
        other => panic!("expected three numbers, got {other:?}"),
    }
}

#[test]
fn rate_large_magnitude_converges_via_scaled_residual() {
    let g = Grid::new(1, vec![Value::Blank]);
    // A mortgage-scale annuity: borrow 1e9, pay 5e6/period for 360 periods. Newton converges (relative
    // step below 1e-12) at r≈0.0036559279523627, but the balance DERIVATIVE is ~1e9-scale, so the
    // residual there is ~1e-4 — a FIXED-absolute 1e-6 residual bar would reject this good root as
    // #NUM!. RATE scales the bar by the cashflow magnitude (as XIRR does), so it converges. (The true
    // root is cross-checked by an independent bisection of the annuity balance — both int-power and
    // f64::powf land on 0.00365592795236270.)
    assert_close(
        eval(
            &call(
                "RATE",
                vec![num(360.0), num(-5_000_000.0), num(1_000_000_000.0)],
            ),
            &g,
        ),
        0.003_655_927_952_362_7,
        1e-9,
    );
}

#[test]
fn rate_with_no_real_root_is_num_never_a_hang() {
    let g = Grid::new(1, vec![Value::Blank]);
    // pv and fv both +100, no payments, nper=2 -> (1+r)^2 = -1 has no real solution. Newton cannot
    // settle inside RATE_NEWTON_MAX and there is no root to accept -> a prompt #NUM! (the hard cap
    // guarantees termination, never a spin).
    assert_eq!(
        eval(
            &call("RATE", vec![num(2.0), num(0.0), num(100.0), num(100.0)]),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn nper_no_real_solution_is_num_and_zero_pmt_is_div0() {
    let g = Grid::new(1, vec![Value::Blank]);
    // Borrow 1000 at 10%/period paying only 50 (< the 100 interest) — the balance never amortizes, so
    // the log argument (w - fv)/(w + pv) = -1 is non-positive: no real period count -> #NUM!.
    assert_eq!(
        eval(&call("NPER", vec![num(0.1), num(-50.0), num(1000.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    // rate == 0 with pmt == 0: the linear -(pv+fv)/pmt divides by zero -> a located #DIV/0! (Excel),
    // NOT the #NUM! an unguarded infinity would demote to.
    assert_eq!(
        eval(&call("NPER", vec![num(0.0), num(0.0), num(-100.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn ipmt_ppmt_period_out_of_range_is_num_and_zero_denom_is_div0() {
    let g = Grid::new(1, vec![Value::Blank]);
    // per == 0 is below the 1..=nper window -> #NUM! (no such period), for both IPMT and PPMT.
    assert_eq!(
        eval(
            &call("IPMT", vec![num(0.1), num(0.0), num(10.0), num(1000.0)]),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
    // per > nper is above the window -> #NUM!.
    assert_eq!(
        eval(
            &call("PPMT", vec![num(0.1), num(11.0), num(10.0), num(1000.0)]),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
    // A zero annuity denominator (rate == -1 with type == 1 vanishes (1 + rate·type)) is a located
    // #DIV/0! — the SAME error PMT emits for a zero denominator, not #NUM! (Excel returns #DIV/0!).
    assert_eq!(
        eval(
            &call(
                "IPMT",
                vec![
                    num(-1.0),
                    num(1.0),
                    num(5.0),
                    num(100.0),
                    num(0.0),
                    num(1.0)
                ]
            ),
            &g
        ),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn xnpv_refuses_mismatched_lengths_and_out_of_order_dates() {
    let g = Grid::new(1, vec![Value::Blank]);
    // Two values but three dates -> the paired stream lengths disagree -> #NUM! (XNPV's own error
    // demotion, distinct from the XIRR path even though both share x_cashflows/day_tenors).
    let v2 = arr(2, 1, vec![n(-1000.0), n(1200.0)]);
    let d3 = arr(3, 1, vec![n(43831.0), n(43997.0), n(44275.0)]);
    assert_eq!(
        eval(&call("XNPV", vec![num(0.1), v2, d3]), &g),
        Value::Error(ErrKind::Num)
    );
    // A later cashflow dated BEFORE the schedule start -> an out-of-order schedule -> #NUM!.
    let vals = arr(2, 1, vec![n(-1000.0), n(1200.0)]);
    let bad_dates = arr(2, 1, vec![n(44275.0), n(43831.0)]);
    assert_eq!(
        eval(&call("XNPV", vec![num(0.1), vals, bad_dates]), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn xirr_matches_the_benchmark_irregularly_dated_case() {
    let g = Grid::new(1, vec![Value::Blank]);
    // The benchmark: values (-1000, 400, 700) on 2020-01-01 / 2020-06-15 / 2021-03-20 (charlie 1900
    // serials; XIRR depends only on the day GAPS 0/166/444, so the absolute base cancels) -> ~10.767%
    // (hand-checked: the Actual/365 XNPV at that rate is ~0).
    let vals = arr(3, 1, vec![n(-1000.0), n(400.0), n(700.0)]);
    let dates = arr(3, 1, vec![n(43831.0), n(43997.0), n(44275.0)]);
    assert_close(eval(&call("XIRR", vec![vals, dates]), &g), 0.107_67, 1e-3);
}

#[test]
fn xnpv_of_the_benchmark_cashflows_at_ten_percent() {
    let g = Grid::new(1, vec![Value::Blank]);
    // XNPV(0.10, (-1000,400,700), same dates) = -1000 + 400/1.1^(166/365) + 700/1.1^(444/365) ≈ 6.40.
    let vals = arr(3, 1, vec![n(-1000.0), n(400.0), n(700.0)]);
    let dates = arr(3, 1, vec![n(43831.0), n(43997.0), n(44275.0)]);
    assert_close(
        eval(&call("XNPV", vec![num(0.10), vals, dates]), &g),
        6.40,
        0.1,
    );
}

#[test]
fn xirr_bad_input_is_num_never_a_hang() {
    let g = Grid::new(1, vec![Value::Blank]);
    // All-positive flows -> no sign change -> no real rate -> #NUM! (the hard caps guarantee no hang).
    let allpos = arr(3, 1, vec![n(100.0), n(200.0), n(300.0)]);
    let dates = arr(3, 1, vec![n(43831.0), n(43997.0), n(44275.0)]);
    assert_eq!(
        eval(&call("XIRR", vec![allpos, dates]), &g),
        Value::Error(ErrKind::Num)
    );
    // Mismatched value/date lengths -> #NUM!.
    let v2 = arr(2, 1, vec![n(-100.0), n(200.0)]);
    let d3 = arr(3, 1, vec![n(43831.0), n(43997.0), n(44275.0)]);
    assert_eq!(
        eval(&call("XIRR", vec![v2, d3]), &g),
        Value::Error(ErrKind::Num)
    );
    // An out-of-order date (a later cashflow dated before the schedule start) -> #NUM!.
    let vals = arr(2, 1, vec![n(-1000.0), n(1200.0)]);
    let bad_dates = arr(2, 1, vec![n(44275.0), n(43831.0)]);
    assert_eq!(
        eval(&call("XIRR", vec![vals, bad_dates]), &g),
        Value::Error(ErrKind::Num)
    );
}
