// Concern: UNIT-TEST pins for the math family built-ins (ABS ROUND MOD INT SQRT POWER ROUNDUP ROUNDDOWN CEILING FLOOR PRODUCT SUMPRODUCT, plus the parity-batch extensions TRUNC SIGN MROUND CEILING.MATH FLOOR.MATH EVEN ODD SUMSQ QUOTIENT) exercised through `FUNCS` dispatch — rounding tie/sign rules, the CEILING/FLOOR zero-significance asymmetry, TRUNC-toward-zero vs INT-toward-−∞, MROUND ties/opposite-sign refusal, the .MATH mode flag, EVEN/ODD away-from-zero rounding, and PRODUCT/SUMPRODUCT array semantics | Non-concern: the math impls (`func/math.rs`) and the shared test fixtures (the parent `tests` module owns `num`/`call`/`col_range`) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
use super::*;

#[test]
fn abs_and_round() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(eval(&call("ABS", vec![num(-5.0)]), &g), Value::Number(5.0));
    assert_eq!(
        eval(&call("ROUND", vec![num(1.2345), num(2.0)]), &g),
        Value::Number(1.23)
    );
    // ties away from zero
    assert_eq!(
        eval(&call("ROUND", vec![num(2.5), num(0.0)]), &g),
        Value::Number(3.0)
    );
    assert_eq!(
        eval(&call("ROUND", vec![num(-2.5), num(0.0)]), &g),
        Value::Number(-3.0)
    );
    // negative digits round left of the point
    assert_eq!(
        eval(&call("ROUND", vec![num(1234.0), num(-2.0)]), &g),
        Value::Number(1200.0)
    );
}

#[test]
fn math_batch_scalar_semantics() {
    let g = Grid::new(1, vec![Value::Blank]);
    // MOD sign follows the divisor.
    assert_eq!(
        eval(&call("MOD", vec![num(7.0), num(3.0)]), &g),
        Value::Number(1.0)
    );
    assert_eq!(
        eval(&call("MOD", vec![num(7.0), num(-3.0)]), &g),
        Value::Number(-2.0)
    );
    assert_eq!(
        eval(&call("MOD", vec![num(-7.0), num(3.0)]), &g),
        Value::Number(2.0)
    );
    assert_eq!(
        eval(&call("MOD", vec![num(5.0), num(0.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
    // INT floors toward −∞ (not toward zero).
    assert_eq!(eval(&call("INT", vec![num(-2.5)]), &g), Value::Number(-3.0));
    assert_eq!(eval(&call("INT", vec![num(2.9)]), &g), Value::Number(2.0));
    // SQRT of a negative is #NUM!.
    assert_eq!(eval(&call("SQRT", vec![num(16.0)]), &g), Value::Number(4.0));
    assert_eq!(
        eval(&call("SQRT", vec![num(-4.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    // POWER shares the operator's error mapping.
    assert_eq!(
        eval(&call("POWER", vec![num(2.0), num(10.0)]), &g),
        Value::Number(1024.0)
    );
    assert_eq!(
        eval(&call("POWER", vec![num(0.0), num(-1.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
    assert_eq!(
        eval(&call("POWER", vec![num(-8.0), num(0.5)]), &g),
        Value::Error(ErrKind::Num)
    );
    // ROUNDUP away from zero; ROUNDDOWN toward zero; negative digits shift left.
    assert_eq!(
        eval(&call("ROUNDUP", vec![num(1.234), num(2.0)]), &g),
        Value::Number(1.24)
    );
    assert_eq!(
        eval(&call("ROUNDUP", vec![num(-1.234), num(2.0)]), &g),
        Value::Number(-1.24)
    );
    assert_eq!(
        eval(&call("ROUNDDOWN", vec![num(1.789), num(2.0)]), &g),
        Value::Number(1.78)
    );
    assert_eq!(
        eval(&call("ROUNDDOWN", vec![num(3.99999), num(0.0)]), &g),
        Value::Number(3.0)
    );
}

#[test]
fn ceiling_floor_sign_and_zero_asymmetry() {
    let g = Grid::new(1, vec![Value::Blank]);
    // Away-from-zero / toward-zero to a multiple.
    assert_eq!(
        eval(&call("CEILING", vec![num(2.5), num(1.0)]), &g),
        Value::Number(3.0)
    );
    assert_eq!(
        eval(&call("FLOOR", vec![num(2.5), num(1.0)]), &g),
        Value::Number(2.0)
    );
    assert_eq!(
        eval(&call("CEILING", vec![num(-2.5), num(-2.0)]), &g),
        Value::Number(-4.0)
    );
    assert_eq!(
        eval(&call("FLOOR", vec![num(-2.5), num(-2.0)]), &g),
        Value::Number(-2.0)
    );
    // Different-signed args are #NUM! for both.
    assert_eq!(
        eval(&call("CEILING", vec![num(2.5), num(-1.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("FLOOR", vec![num(2.5), num(-1.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    // Zero significance: CEILING → 0, FLOOR → #DIV/0! (the legacy asymmetry).
    assert_eq!(
        eval(&call("CEILING", vec![num(5.0), num(0.0)]), &g),
        Value::Number(0.0)
    );
    assert_eq!(
        eval(&call("FLOOR", vec![num(5.0), num(0.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn product_and_sumproduct_semantics() {
    // PRODUCT over a range multiplies the numbers; a range with no numbers is 0.
    let g = Grid::new(
        1,
        vec![Value::Number(2.0), Value::Number(3.0), Value::Number(4.0)],
    );
    assert_eq!(
        eval(&call("PRODUCT", vec![col_range(3)]), &g),
        Value::Number(24.0)
    );
    // Direct-arg coercion mirrors SUM (bool → 1/0, numeric-text parses).
    assert_eq!(
        eval(
            &call(
                "PRODUCT",
                vec![
                    num(2.0),
                    Expr::Lit(Value::Bool(true)),
                    Expr::Lit(Value::Text("3".into()))
                ]
            ),
            &g
        ),
        Value::Number(6.0)
    );
    // Empty product (no numeric datum) is 0, not the 1 identity.
    let blank = Grid::new(1, vec![Value::Text("x".into()), Value::Blank]);
    assert_eq!(
        eval(&call("PRODUCT", vec![col_range(2)]), &blank),
        Value::Number(0.0)
    );
    // SUMPRODUCT multiplies aligned arrays then sums (array literals sidestep the whole-row
    // stub, which cannot window a single column of a multi-column grid).
    let col3 = |a: f64, b: f64, c: f64| {
        Expr::Lit(Value::Array(
            crate::value::Shape { rows: 3, cols: 1 },
            vec![Value::Number(a), Value::Number(b), Value::Number(c)],
        ))
    };
    assert_eq!(
        eval(
            &call("SUMPRODUCT", vec![col3(1.0, 2.0, 3.0), col3(4.0, 5.0, 6.0)]),
            &g
        ),
        Value::Number(32.0)
    );
    // A shape mismatch (3×1 vs 2×1) is a static #VALUE!.
    let col2 = Expr::Lit(Value::Array(
        crate::value::Shape { rows: 2, cols: 1 },
        vec![Value::Number(4.0), Value::Number(5.0)],
    ));
    assert_eq!(
        eval(&call("SUMPRODUCT", vec![col3(1.0, 2.0, 3.0), col2]), &g),
        Value::Error(ErrKind::Value)
    );
    // A non-numeric cell counts as 0 (so an unfiltered text zeroes its product term).
    let with_text = Expr::Lit(Value::Array(
        crate::value::Shape { rows: 3, cols: 1 },
        vec![
            Value::Number(2.0),
            Value::Text("x".into()),
            Value::Number(4.0),
        ],
    ));
    assert_eq!(
        eval(
            &call("SUMPRODUCT", vec![with_text, col3(5.0, 5.0, 5.0)]),
            &g
        ),
        Value::Number(30.0)
    );
}

#[test]
fn trunc_sign_quotient_semantics() {
    let g = Grid::new(1, vec![Value::Blank]);
    // TRUNC truncates toward zero (unlike INT toward −∞); default 0 digits, and negative digits shift
    // left of the point.
    assert_eq!(eval(&call("TRUNC", vec![num(8.9)]), &g), Value::Number(8.0));
    assert_eq!(
        eval(&call("TRUNC", vec![num(-8.9)]), &g),
        Value::Number(-8.0)
    );
    assert_eq!(
        eval(&call("TRUNC", vec![num(5.678), num(2.0)]), &g),
        Value::Number(5.67)
    );
    assert_eq!(
        eval(&call("TRUNC", vec![num(123.45), num(-1.0)]), &g),
        Value::Number(120.0)
    );
    // SIGN → 1 / 0 / -1.
    assert_eq!(eval(&call("SIGN", vec![num(10.0)]), &g), Value::Number(1.0));
    assert_eq!(eval(&call("SIGN", vec![num(0.0)]), &g), Value::Number(0.0));
    assert_eq!(
        eval(&call("SIGN", vec![num(-0.5)]), &g),
        Value::Number(-1.0)
    );
    // QUOTIENT truncates the quotient toward zero; a zero divisor is #DIV/0!.
    assert_eq!(
        eval(&call("QUOTIENT", vec![num(5.0), num(2.0)]), &g),
        Value::Number(2.0)
    );
    assert_eq!(
        eval(&call("QUOTIENT", vec![num(-5.0), num(2.0)]), &g),
        Value::Number(-2.0)
    );
    assert_eq!(
        eval(&call("QUOTIENT", vec![num(5.0), num(0.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn mround_ties_away_and_rejects_opposite_signs() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("MROUND", vec![num(10.0), num(3.0)]), &g),
        Value::Number(9.0)
    );
    assert_eq!(
        eval(&call("MROUND", vec![num(-10.0), num(-3.0)]), &g),
        Value::Number(-9.0)
    );
    // Half rounds AWAY from zero.
    assert_eq!(
        eval(&call("MROUND", vec![num(1.5), num(1.0)]), &g),
        Value::Number(2.0)
    );
    // Opposite signs → #NUM!; a zero multiple → 0.
    assert_eq!(
        eval(&call("MROUND", vec![num(5.0), num(-2.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("MROUND", vec![num(7.0), num(0.0)]), &g),
        Value::Number(0.0)
    );
}

#[test]
fn ceiling_floor_math_mode_and_zero_significance() {
    let g = Grid::new(1, vec![Value::Blank]);
    // Default significance 1, positive numbers.
    assert_eq!(
        eval(&call("CEILING.MATH", vec![num(6.7)]), &g),
        Value::Number(7.0)
    );
    assert_eq!(
        eval(&call("FLOOR.MATH", vec![num(6.7)]), &g),
        Value::Number(6.0)
    );
    // Significance multiple.
    assert_eq!(
        eval(&call("CEILING.MATH", vec![num(24.3), num(5.0)]), &g),
        Value::Number(25.0)
    );
    assert_eq!(
        eval(&call("FLOOR.MATH", vec![num(24.3), num(5.0)]), &g),
        Value::Number(20.0)
    );
    // Negatives with default mode: CEILING.MATH toward +∞, FLOOR.MATH toward −∞.
    assert_eq!(
        eval(&call("CEILING.MATH", vec![num(-8.1), num(2.0)]), &g),
        Value::Number(-8.0)
    );
    assert_eq!(
        eval(&call("FLOOR.MATH", vec![num(-8.1), num(2.0)]), &g),
        Value::Number(-10.0)
    );
    // A nonzero mode flips the negative side (away from / toward zero).
    assert_eq!(
        eval(
            &call("CEILING.MATH", vec![num(-5.5), num(2.0), num(-1.0)]),
            &g
        ),
        Value::Number(-6.0)
    );
    assert_eq!(
        eval(
            &call("FLOOR.MATH", vec![num(-5.5), num(2.0), num(-1.0)]),
            &g
        ),
        Value::Number(-4.0)
    );
    // Zero significance → 0 for BOTH (no legacy CEILING/FLOOR asymmetry).
    assert_eq!(
        eval(&call("CEILING.MATH", vec![num(5.0), num(0.0)]), &g),
        Value::Number(0.0)
    );
    assert_eq!(
        eval(&call("FLOOR.MATH", vec![num(5.0), num(0.0)]), &g),
        Value::Number(0.0)
    );
}

#[test]
fn even_odd_round_away_from_zero_and_sumsq() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(eval(&call("EVEN", vec![num(1.5)]), &g), Value::Number(2.0));
    assert_eq!(eval(&call("EVEN", vec![num(3.0)]), &g), Value::Number(4.0));
    assert_eq!(eval(&call("EVEN", vec![num(2.0)]), &g), Value::Number(2.0));
    assert_eq!(
        eval(&call("EVEN", vec![num(-1.0)]), &g),
        Value::Number(-2.0)
    );
    assert_eq!(eval(&call("EVEN", vec![num(0.0)]), &g), Value::Number(0.0));
    assert_eq!(eval(&call("ODD", vec![num(1.5)]), &g), Value::Number(3.0));
    assert_eq!(eval(&call("ODD", vec![num(2.0)]), &g), Value::Number(3.0));
    assert_eq!(eval(&call("ODD", vec![num(3.0)]), &g), Value::Number(3.0));
    assert_eq!(eval(&call("ODD", vec![num(1.0)]), &g), Value::Number(1.0));
    assert_eq!(eval(&call("ODD", vec![num(0.0)]), &g), Value::Number(1.0));
    assert_eq!(eval(&call("ODD", vec![num(-2.0)]), &g), Value::Number(-3.0));
    // SUMSQ sums squares (direct coercion + in-range gathering).
    assert_eq!(
        eval(&call("SUMSQ", vec![num(3.0), num(4.0)]), &g),
        Value::Number(25.0)
    );
    let data = Grid::new(1, vec![Value::Number(-2.0), Value::Number(2.0)]);
    assert_eq!(
        eval(&call("SUMSQ", vec![col_range(2)]), &data),
        Value::Number(8.0)
    );
}
