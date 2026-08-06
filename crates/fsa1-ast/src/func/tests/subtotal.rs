// Concern: pins SUBTOTAL and AGGREGATE | Non-concern: the impls, the reducers | IO: (Grid, Expr) -> asserted Value
use super::*;

/// The shared 5-value column fixture {3,1,4,1,5}: sum 14, product 60, min 1, max 5, median 3.
fn data() -> Grid {
    Grid::new(
        1,
        vec![
            Value::Number(3.0),
            Value::Number(1.0),
            Value::Number(4.0),
            Value::Number(1.0),
            Value::Number(5.0),
        ],
    )
}

#[test]
fn subtotal_maps_function_numbers_and_the_100_series() {
    let g = data();
    assert_eq!(
        eval(&call("SUBTOTAL", vec![num(9.0), col_range(5)]), &g),
        Value::Number(14.0)
    ); // SUM
    assert_eq!(
        eval(&call("SUBTOTAL", vec![num(1.0), col_range(5)]), &g),
        Value::Number(2.8)
    ); // AVERAGE
    assert_eq!(
        eval(&call("SUBTOTAL", vec![num(2.0), col_range(5)]), &g),
        Value::Number(5.0)
    ); // COUNT
    assert_eq!(
        eval(&call("SUBTOTAL", vec![num(4.0), col_range(5)]), &g),
        Value::Number(5.0)
    ); // MAX
    assert_eq!(
        eval(&call("SUBTOTAL", vec![num(5.0), col_range(5)]), &g),
        Value::Number(1.0)
    ); // MIN
    assert_eq!(
        eval(&call("SUBTOTAL", vec![num(6.0), col_range(5)]), &g),
        Value::Number(60.0)
    ); // PRODUCT
    assert_eq!(
        eval(&call("SUBTOTAL", vec![num(109.0), col_range(5)]), &g),
        Value::Number(14.0)
    );
    assert_eq!(
        eval(&call("SUBTOTAL", vec![num(99.0), col_range(5)]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("SUBTOTAL", vec![num(112.0), col_range(5)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn subtotal_propagates_an_error_in_its_data() {
    let g = Grid::new(
        1,
        vec![
            Value::Number(2.0),
            Value::Error(ErrKind::Div0),
            Value::Number(4.0),
        ],
    );
    assert_eq!(
        eval(&call("SUBTOTAL", vec![num(9.0), col_range(3)]), &g),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn aggregate_reference_form_and_error_options() {
    let g = data();
    assert_eq!(
        eval(
            &call("AGGREGATE", vec![num(9.0), num(4.0), col_range(5)]),
            &g
        ),
        Value::Number(14.0)
    ); // SUM
    assert_eq!(
        eval(
            &call("AGGREGATE", vec![num(1.0), num(4.0), col_range(5)]),
            &g
        ),
        Value::Number(2.8)
    ); // AVERAGE
    assert_eq!(
        eval(
            &call("AGGREGATE", vec![num(12.0), num(4.0), col_range(5)]),
            &g
        ),
        Value::Number(3.0)
    ); // MEDIAN

    let with_err = Grid::new(
        1,
        vec![
            Value::Number(2.0),
            Value::Error(ErrKind::Div0),
            Value::Number(4.0),
        ],
    );
    assert_eq!(
        eval(
            &call("AGGREGATE", vec![num(9.0), num(6.0), col_range(3)]),
            &with_err
        ),
        Value::Number(6.0)
    );
    assert_eq!(
        eval(
            &call("AGGREGATE", vec![num(9.0), num(4.0), col_range(3)]),
            &with_err
        ),
        Value::Error(ErrKind::Div0)
    );
    assert_eq!(
        eval(
            &call("AGGREGATE", vec![num(2.0), num(6.0), col_range(3)]),
            &with_err
        ),
        Value::Number(2.0)
    );
}

#[test]
fn aggregate_array_form_order_statistics() {
    let g = data(); // sorted {1,1,3,4,5}
    assert_eq!(
        eval(
            &call(
                "AGGREGATE",
                vec![num(14.0), num(4.0), col_range(5), num(2.0)]
            ),
            &g
        ),
        Value::Number(4.0)
    );
    assert_eq!(
        eval(
            &call(
                "AGGREGATE",
                vec![num(15.0), num(4.0), col_range(5), num(1.0)]
            ),
            &g
        ),
        Value::Number(1.0)
    );
    assert_eq!(
        eval(
            &call(
                "AGGREGATE",
                vec![num(16.0), num(4.0), col_range(5), num(0.5)]
            ),
            &g
        ),
        Value::Number(3.0)
    );
    assert_eq!(
        eval(
            &call(
                "AGGREGATE",
                vec![num(18.0), num(4.0), col_range(5), num(0.25)]
            ),
            &g
        ),
        Value::Number(1.0)
    );
    assert_eq!(
        eval(
            &call(
                "AGGREGATE",
                vec![num(19.0), num(4.0), col_range(5), num(1.0)]
            ),
            &g
        ),
        Value::Number(1.0)
    );
    assert_eq!(
        eval(
            &call("AGGREGATE", vec![num(14.0), num(4.0), col_range(5)]),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn aggregate_rejects_out_of_range_selectors() {
    let g = data();
    assert_eq!(
        eval(
            &call("AGGREGATE", vec![num(20.0), num(4.0), col_range(5)]),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(
            &call("AGGREGATE", vec![num(9.0), num(8.0), col_range(5)]),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
}
