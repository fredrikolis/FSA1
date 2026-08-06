// Concern: pins the normal-distribution built-ins and STANDARDIZE | Non-concern: the impls, the shared fixtures | IO: (Grid, Expr) -> asserted Value
use super::*;

/// Assert a `Value::Number` is within `tol` of `expected`.
fn close(v: Value, expected: f64, tol: f64) {
    match v {
        Value::Number(got) => assert!(
            (got - expected).abs() <= tol,
            "expected ~{expected}, got {got}"
        ),
        other => panic!("expected a Number, got {other:?}"),
    }
}

#[test]
fn standardize_z_score_and_domain() {
    let g = Grid::new(1, vec![Value::Blank]);
    close(
        eval(
            &call("STANDARDIZE", vec![num(42.0), num(40.0), num(1.5)]),
            &g,
        ),
        4.0 / 3.0,
        1e-12,
    );
    assert_eq!(
        eval(
            &call("STANDARDIZE", vec![num(42.0), num(40.0), num(0.0)]),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn norm_dist_cdf_pdf_and_legacy() {
    let g = Grid::new(1, vec![Value::Blank]);
    let bt = || Expr::Lit(Value::Bool(true));
    let bf = || Expr::Lit(Value::Bool(false));
    close(
        eval(
            &call("NORM.DIST", vec![num(42.0), num(40.0), num(1.5), bt()]),
            &g,
        ),
        0.908_788_780_274_132_1,
        1e-11,
    );
    close(
        eval(
            &call("NORMDIST", vec![num(42.0), num(40.0), num(1.5), bt()]),
            &g,
        ),
        0.908_788_780_274_132_1,
        1e-11,
    );
    close(
        eval(
            &call("NORM.DIST", vec![num(42.0), num(40.0), num(1.5), bf()]),
            &g,
        ),
        0.109_340_049_783_995_75,
        1e-11,
    );
    assert_eq!(
        eval(
            &call("NORM.DIST", vec![num(42.0), num(40.0), num(0.0), bt()]),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn norm_inv_and_legacy_and_domain() {
    let g = Grid::new(1, vec![Value::Blank]);
    close(
        eval(&call("NORM.INV", vec![num(0.9), num(40.0), num(1.5)]), &g),
        41.922_327_348_316_9,
        1e-9,
    );
    close(
        eval(&call("NORMINV", vec![num(0.9), num(40.0), num(1.5)]), &g),
        41.922_327_348_316_9,
        1e-9,
    );
    assert_eq!(
        eval(&call("NORM.INV", vec![num(0.0), num(40.0), num(1.5)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("NORM.INV", vec![num(1.0), num(40.0), num(1.5)]), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn standard_normal_dist_and_inverse() {
    let g = Grid::new(1, vec![Value::Blank]);
    let bt = || Expr::Lit(Value::Bool(true));
    let bf = || Expr::Lit(Value::Bool(false));
    close(
        eval(&call("NORM.S.DIST", vec![num(1.333), bt()]), &g),
        0.908_734_098_099_558_4,
        1e-11,
    );
    close(
        eval(&call("NORMSDIST", vec![num(1.333)]), &g),
        0.908_734_098_099_558_4,
        1e-11,
    );
    close(
        eval(&call("NORM.S.DIST", vec![num(0.0), bf()]), &g),
        0.398_942_280_401_432_7,
        1e-12,
    );
    close(
        eval(&call("NORM.S.INV", vec![num(0.9)]), &g),
        1.281_551_565_544_600_8,
        1e-9,
    );
    close(
        eval(&call("NORMSINV", vec![num(0.975)]), &g),
        1.959_963_984_540_053_6,
        1e-9,
    );
    assert_eq!(
        eval(&call("NORM.S.INV", vec![num(0.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("NORM.S.INV", vec![num(1.0)]), &g),
        Value::Error(ErrKind::Num)
    );
}
