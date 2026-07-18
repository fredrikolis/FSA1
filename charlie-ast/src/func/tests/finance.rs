// Concern: UNIT-TEST pins for the financial family built-ins (PMT NPV IRR) exercised through `FUNCS` dispatch — PMT's linear-vs-annuity branches with the #DIV/0! denominators, NPV period-one discounting over args and ranges, IRR's oracle-matching convergence and its guaranteed #NUM! (never a hang) on unbracketed cashflows, and dispatch's arity gate | Non-concern: the finance impls (`func/finance.rs`) and the shared test fixtures (the parent `tests` module owns `num`/`call`/`col_range`/`arr`/`n`) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
use super::*;

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
