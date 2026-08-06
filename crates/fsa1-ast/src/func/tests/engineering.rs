// Concern: pins CONVERT | Non-concern: the impl, the shared fixtures | IO: (Grid, Expr) -> asserted Value
use super::*;

/// Assert an eval yields a number within a RELATIVE tolerance of `want` (unit factors are f64
/// products, not authorable bit-exact by hand for the irrational ratios).
fn assert_close(got: Value, want: f64) {
    match got {
        Value::Number(x) => {
            let tol = want.abs() * 1e-9 + 1e-12;
            assert!((x - want).abs() <= tol, "got {x}, want {want}");
        }
        other => panic!("expected a number near {want}, got {other:?}"),
    }
}

fn conv(number: f64, from: &str, to: &str) -> Value {
    let g = Grid::new(1, vec![Value::Blank]);
    eval(&call("CONVERT", vec![num(number), txt(from), txt(to)]), &g)
}

#[test]
fn ratio_conversions_within_a_system() {
    assert_eq!(conv(1.0, "lbm", "kg"), Value::Number(0.45359237));
    assert_close(conv(2.5, "ft", "m"), 0.762);
    assert_close(conv(1.0, "mi", "km"), 1.609344);
    assert_close(conv(1.0, "km", "mi"), 0.621371192237334);
    assert_eq!(conv(1.0, "day", "hr"), Value::Number(24.0));
    assert_close(conv(1.0, "yr", "day"), 365.25);
    assert_eq!(conv(1.0, "gal", "l"), Value::Number(3.785411784));
    assert_eq!(conv(1.0, "lbf", "N"), Value::Number(4.4482216152605));
    assert_eq!(conv(1.0, "HP", "W"), Value::Number(745.69987158227));
    assert_close(conv(1.0, "atm", "mmHg"), 760.0021001785152);
}

#[test]
fn si_prefixes_scale_the_metric_base() {
    assert_eq!(conv(1.0, "kg", "g"), Value::Number(1000.0));
    assert_eq!(conv(100.0, "m", "cm"), Value::Number(10000.0));
    assert_eq!(conv(1.0, "m", "mm"), Value::Number(1000.0));
    assert_close(conv(1.0, "ang", "m"), 1e-10);
    assert_eq!(conv(1.0, "dag", "g"), conv(1.0, "eg", "g"));
    assert_eq!(conv(1.0, "dag", "g"), Value::Number(10.0));
}

#[test]
fn area_and_volume_prefixes_take_the_dimensional_exponent() {
    assert_close(conv(1.0, "km2", "m2"), 1_000_000.0);
    assert_close(conv(1.0, "km^2", "m^2"), 1_000_000.0);
    assert_close(conv(1.0, "km3", "m3"), 1_000_000_000.0);
    assert_close(conv(1.0, "l", "m3"), 0.001);
    assert_close(conv(1.0, "tsp", "ml"), 4.92892159375);
}

#[test]
fn binary_prefixes_are_excel_exact_powers_of_two() {
    assert_eq!(conv(1.0, "kibyte", "byte"), Value::Number(1024.0));
    assert_eq!(conv(1.0, "Mibyte", "byte"), Value::Number(1_048_576.0));
    assert_eq!(conv(1.0, "kibit", "bit"), Value::Number(1024.0));
    assert_eq!(conv(1.0, "Gibit", "bit"), Value::Number(1_073_741_824.0));
    assert_eq!(conv(1.0, "kbit", "bit"), Value::Number(1000.0));
    assert_eq!(conv(1.0, "byte", "bit"), Value::Number(8.0));
}

#[test]
fn temperature_is_an_affine_conversion_through_kelvin() {
    assert_close(conv(32.0, "F", "C"), 0.0);
    assert_close(conv(100.0, "C", "F"), 212.0);
    assert_close(conv(0.0, "C", "K"), 273.15);
    assert_close(conv(2.0, "kel", "C"), -271.15);
    assert_close(conv(1.0, "C", "Reau"), 0.8);
    assert_close(conv(1.0, "Rank", "K"), 0.555_555_555_555_555_6);
    assert_close(conv(32.0, "fah", "cel"), 0.0);
}

#[test]
fn pc_is_disambiguated_by_the_shared_system() {
    assert_close(conv(1.0, "pc", "ly"), 3.26156377694566);
    assert_close(conv(1e12, "pc", "J"), 4.184);
}

#[test]
fn bad_units_and_pairs_are_na_never_a_panic() {
    assert_eq!(conv(5.0, "foo", "m"), Value::Error(ErrKind::Na));
    assert_eq!(conv(1.0, "m", "kg"), Value::Error(ErrKind::Na));
    assert_eq!(conv(1.0, " km", "mi"), Value::Error(ErrKind::Na));
    assert_eq!(conv(1.0, "k", "m"), Value::Error(ErrKind::Na));
}

#[test]
fn number_coercions_and_error_propagation() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(
            &call(
                "CONVERT",
                vec![Expr::Lit(Value::Bool(true)), txt("m"), txt("ft")]
            ),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
    assert_close(
        eval(
            &call("CONVERT", vec![Expr::Lit(Value::Blank), txt("C"), txt("F")]),
            &g,
        ),
        32.0,
    );
    assert_eq!(conv_text("1000", "g", "kg"), Value::Number(1.0));
    assert_eq!(
        eval(
            &call(
                "CONVERT",
                vec![Expr::Lit(Value::Error(ErrKind::Div0)), txt("m"), txt("ft")]
            ),
            &g
        ),
        Value::Error(ErrKind::Div0)
    );
}

fn conv_text(number: &str, from: &str, to: &str) -> Value {
    let g = Grid::new(1, vec![Value::Blank]);
    eval(&call("CONVERT", vec![txt(number), txt(from), txt(to)]), &g)
}
