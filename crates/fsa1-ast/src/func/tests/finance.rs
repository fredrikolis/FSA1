// Concern: pins the financial built-ins | Non-concern: the impls, the shared fixtures | IO: (Grid, Expr) -> asserted Value
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
    assert_eq!(
        eval(&call("PMT", vec![num(0.0), num(10.0), num(-1000.0)]), &g),
        n(100.0)
    );
    assert_eq!(
        eval(&call("PMT", vec![num(0.5), num(2.0), num(-100.0)]), &g),
        n(90.0)
    );
    assert_eq!(
        eval(
            &call("PMT", vec![num(0.5), num(2.0), num(-100.0), num(-50.0)]),
            &g
        ),
        n(110.0)
    );
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
    assert_eq!(
        eval(&call("PMT", vec![num(0.0), num(0.0), num(100.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
    assert_eq!(
        eval(&call("PMT", vec![num(0.1), num(0.0), num(100.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
    let bad = call(
        "PMT",
        vec![call("SQRT", vec![num(-1.0)]), num(2.0), num(1.0)],
    );
    assert_eq!(eval(&bad, &g), Value::Error(ErrKind::Num));
}

#[test]
fn npv_discounts_from_period_one_over_args_and_ranges() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(
            &call("NPV", vec![num(1.0), num(100.0), num(200.0), num(300.0)]),
            &g
        ),
        n(137.5)
    );
    let grid = Grid::new(1, vec![n(100.0), n(200.0), n(300.0)]);
    assert_eq!(
        eval(&call("NPV", vec![num(1.0), col_range(3)]), &grid),
        n(137.5)
    );
    let bad = call(
        "NPV",
        vec![num(1.0), num(100.0), call("SQRT", vec![num(-1.0)])],
    );
    assert_eq!(eval(&bad, &g), Value::Error(ErrKind::Num));
}

#[test]
fn irr_converges_to_the_root_matching_the_independent_oracle() {
    let g = Grid::new(1, vec![Value::Blank]);
    let cf = arr(1, 4, vec![n(-100.0), n(30.0), n(40.0), n(50.0)]);
    assert_eq!(
        eval(&call("IRR", vec![cf]), &g),
        n(0.088_963_394_693_349_92)
    );
}

#[test]
fn irr_non_convergent_cashflows_are_num_never_a_hang() {
    let g = Grid::new(1, vec![Value::Blank]);
    let allpos = arr(1, 3, vec![n(100.0), n(200.0), n(300.0)]);
    assert_eq!(
        eval(&call("IRR", vec![allpos]), &g),
        Value::Error(ErrKind::Num)
    );
    let one = arr(1, 1, vec![n(-100.0)]);
    assert_eq!(
        eval(&call("IRR", vec![one]), &g),
        Value::Error(ErrKind::Num)
    );
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
    assert_eq!(
        eval(&call("PMT", vec![num(0.1), num(2.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("NPV", vec![num(0.1)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn annuity_family_matches_hand_verified_excel_values() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_close(
        eval(&call("FV", vec![num(0.1), num(10.0), num(-100.0)]), &g),
        1_593.742_460_1,
        1e-4,
    );
    assert_close(
        eval(&call("NPER", vec![num(0.1), num(-100.0), num(500.0)]), &g),
        7.272_540_897_341_713,
        1e-9,
    );
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
    assert_eq!(
        eval(
            &call("IPMT", vec![num(0.1), num(1.0), num(10.0), num(1000.0)]),
            &g
        ),
        n(-100.0)
    );
    assert_close(
        eval(
            &call("IPMT", vec![num(0.1), num(2.0), num(10.0), num(1000.0)]),
            &g,
        ),
        -93.725_460_511_748_85,
        1e-6,
    );
    assert_close(
        eval(
            &call("PPMT", vec![num(0.1), num(1.0), num(10.0), num(1000.0)]),
            &g,
        ),
        -62.745_394_882_511_52,
        1e-6,
    );
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
    assert_eq!(
        eval(&call("NPER", vec![num(0.1), num(-50.0), num(1000.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("NPER", vec![num(0.0), num(0.0), num(-100.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn ipmt_ppmt_period_out_of_range_is_num_and_zero_denom_is_div0() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(
            &call("IPMT", vec![num(0.1), num(0.0), num(10.0), num(1000.0)]),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(
            &call("PPMT", vec![num(0.1), num(11.0), num(10.0), num(1000.0)]),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
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
    let v2 = arr(2, 1, vec![n(-1000.0), n(1200.0)]);
    let d3 = arr(3, 1, vec![n(43831.0), n(43997.0), n(44275.0)]);
    assert_eq!(
        eval(&call("XNPV", vec![num(0.1), v2, d3]), &g),
        Value::Error(ErrKind::Num)
    );
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
    let vals = arr(3, 1, vec![n(-1000.0), n(400.0), n(700.0)]);
    let dates = arr(3, 1, vec![n(43831.0), n(43997.0), n(44275.0)]);
    assert_close(eval(&call("XIRR", vec![vals, dates]), &g), 0.107_67, 1e-3);
}

#[test]
fn xnpv_of_the_benchmark_cashflows_at_ten_percent() {
    let g = Grid::new(1, vec![Value::Blank]);
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
    let allpos = arr(3, 1, vec![n(100.0), n(200.0), n(300.0)]);
    let dates = arr(3, 1, vec![n(43831.0), n(43997.0), n(44275.0)]);
    assert_eq!(
        eval(&call("XIRR", vec![allpos, dates]), &g),
        Value::Error(ErrKind::Num)
    );
    let v2 = arr(2, 1, vec![n(-100.0), n(200.0)]);
    let d3 = arr(3, 1, vec![n(43831.0), n(43997.0), n(44275.0)]);
    assert_eq!(
        eval(&call("XIRR", vec![v2, d3]), &g),
        Value::Error(ErrKind::Num)
    );
    let vals = arr(2, 1, vec![n(-1000.0), n(1200.0)]);
    let bad_dates = arr(2, 1, vec![n(44275.0), n(43831.0)]);
    assert_eq!(
        eval(&call("XIRR", vec![vals, bad_dates]), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn pv_inverts_the_annuity_and_the_rate_zero_branch_is_linear() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("PV", vec![num(0.0), num(60.0), num(-200.0)]), &g),
        n(12000.0)
    );
    assert_eq!(
        eval(&call("PV", vec![num(0.5), num(2.0), num(90.0)]), &g),
        n(-100.0)
    );
    assert_eq!(
        eval(
            &call("PV", vec![num(0.0), num(10.0), num(-100.0), num(200.0)]),
            &g
        ),
        n(800.0)
    );
    let bad = call(
        "PV",
        vec![call("SQRT", vec![num(-1.0)]), num(2.0), num(1.0)],
    );
    assert_eq!(eval(&bad, &g), Value::Error(ErrKind::Num));
}

#[test]
fn mirr_compounds_forward_and_discounts_back_matching_the_oracle() {
    let g = Grid::new(1, vec![Value::Blank]);
    let cf = arr(4, 1, vec![n(-1000.0), n(300.0), n(400.0), n(500.0)]);
    assert_close(
        eval(&call("MIRR", vec![cf, num(0.1), num(0.12)]), &g),
        0.098_156_692_446_315_53,
        1e-9,
    );
}

#[test]
fn mirr_needs_both_signs_and_at_least_two_flows_else_div0() {
    let g = Grid::new(1, vec![Value::Blank]);
    let allpos = arr(3, 1, vec![n(100.0), n(200.0), n(300.0)]);
    assert_eq!(
        eval(&call("MIRR", vec![allpos, num(0.1), num(0.12)]), &g),
        Value::Error(ErrKind::Div0)
    );
    let one = arr(1, 1, vec![n(-100.0)]);
    assert_eq!(
        eval(&call("MIRR", vec![one, num(0.1), num(0.12)]), &g),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn cumipmt_and_cumprinc_sum_the_payment_window_and_refuse_bad_domains() {
    let g = Grid::new(1, vec![Value::Blank]);
    let rate = 0.05_f64 / 12.0;
    assert_close(
        eval(
            &call(
                "CUMIPMT",
                vec![
                    num(rate),
                    num(60.0),
                    num(20000.0),
                    num(1.0),
                    num(12.0),
                    num(0.0),
                ],
            ),
            &g,
        ),
        -917.991_014_930_655_4,
        1e-6,
    );
    assert_close(
        eval(
            &call(
                "CUMPRINC",
                vec![
                    num(rate),
                    num(60.0),
                    num(20000.0),
                    num(1.0),
                    num(12.0),
                    num(0.0),
                ],
            ),
            &g,
        ),
        -3_611.105_059_631_982_4,
        1e-6,
    );
    assert_eq!(
        eval(
            &call(
                "CUMIPMT",
                vec![
                    num(rate),
                    num(60.0),
                    num(20000.0),
                    num(12.0),
                    num(1.0),
                    num(0.0),
                ],
            ),
            &g,
        ),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(
            &call(
                "CUMPRINC",
                vec![
                    num(rate),
                    num(60.0),
                    num(20000.0),
                    num(1.0),
                    num(12.0),
                    num(2.0),
                ],
            ),
            &g,
        ),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn sln_and_syd_are_exact_rational_forms_with_located_refusals() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("SLN", vec![num(10000.0), num(1000.0), num(5.0)]), &g),
        n(1800.0)
    );
    assert_eq!(
        eval(&call("SLN", vec![num(10000.0), num(1000.0), num(0.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
    assert_eq!(
        eval(
            &call("SYD", vec![num(10000.0), num(1000.0), num(5.0), num(2.0)]),
            &g
        ),
        n(2400.0)
    );
    assert_eq!(
        eval(
            &call("SYD", vec![num(10000.0), num(1000.0), num(5.0), num(6.0)]),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(
            &call("SYD", vec![num(10000.0), num(1000.0), num(5.0), num(0.0)]),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn db_declining_balance_matches_excel_and_handles_the_partial_period() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_close(
        eval(
            &call("DB", vec![num(10000.0), num(1000.0), num(5.0), num(2.0)]),
            &g,
        ),
        2328.39,
        1e-6,
    );
    assert_close(
        eval(
            &call(
                "DB",
                vec![num(10000.0), num(1000.0), num(5.0), num(6.0), num(6.0)],
            ),
            &g,
        ),
        238.527_124_587_881_87,
        1e-6,
    );
    assert_eq!(
        eval(
            &call("DB", vec![num(10000.0), num(1000.0), num(5.0), num(6.0)]),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(
            &call(
                "DB",
                vec![num(10000.0), num(1000.0), num(5.0), num(2.0), num(13.0)]
            ),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(
            &call("DB", vec![num(0.0), num(1000.0), num(5.0), num(2.0)]),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn ddb_double_declining_balance_matches_excel_with_located_refusals() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_close(
        eval(
            &call("DDB", vec![num(10000.0), num(1000.0), num(5.0), num(2.0)]),
            &g,
        ),
        2400.0,
        1e-6,
    );
    assert_close(
        eval(
            &call("DDB", vec![num(10000.0), num(1000.0), num(5.0), num(2.5)]),
            &g,
        ),
        1_859.032_006_179_56,
        1e-6,
    );
    assert_eq!(
        eval(
            &call("DDB", vec![num(10000.0), num(1000.0), num(5.0), num(6.0)]),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(
            &call(
                "DDB",
                vec![num(10000.0), num(1000.0), num(5.0), num(3.0), num(0.0)]
            ),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn effect_and_nominal_are_inverses_with_located_refusals() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_close(
        eval(&call("EFFECT", vec![num(0.0525), num(4.0)]), &g),
        0.053_542_667_370_758_19,
        1e-9,
    );
    assert_close(
        eval(&call("NOMINAL", vec![num(0.053543), num(4.0)]), &g),
        0.052_500_319_868_356_016,
        1e-9,
    );
    assert_eq!(
        eval(&call("EFFECT", vec![num(0.05), num(0.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("EFFECT", vec![num(0.0), num(4.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("NOMINAL", vec![num(0.05), num(0.0)]), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn pduration_and_rri_invert_compound_growth_with_located_refusals() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_close(
        eval(
            &call("PDURATION", vec![num(0.05), num(1000.0), num(2000.0)]),
            &g,
        ),
        14.206_699_082_890_472,
        1e-9,
    );
    assert_close(
        eval(
            &call("RRI", vec![num(96.0), num(10000.0), num(11000.0)]),
            &g,
        ),
        0.000_993_307_376_291_330_3,
        1e-12,
    );
    assert_eq!(
        eval(
            &call("PDURATION", vec![num(0.0), num(1000.0), num(2000.0)]),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("RRI", vec![num(0.0), num(10000.0), num(11000.0)]), &g),
        Value::Error(ErrKind::Num)
    );
}
