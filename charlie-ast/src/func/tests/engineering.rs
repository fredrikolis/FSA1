// Concern: UNIT-TEST pins for the engineering family built-in CONVERT exercised through `FUNCS` dispatch — pure-ratio conversions within a system (mass/distance/time/volume/force/power), SI decimal prefixes (including the area/volume square/cube exponent and both deka spellings), the IEC binary prefixes on the information units (Excel-exact powers of two, where the `formulas` reference lib is defective — see conformance KNOWN-LIB-GAPS.md), the affine temperature conversions through Kelvin (C/F/K/Rank/Reau), the multi-system `pc` (parsec vs pico-calorie) disambiguated by the from/to intersection, and the error semantics (cross-system/unknown-unit `#N/A`, boolean-number `#VALUE!`, blank-number-as-0, error propagation) — never a panic | Non-concern: the CONVERT impl (`func/engineering.rs`) and the shared test fixtures (the parent `tests` module owns `num`/`txt`/`call`) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
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
    // Mass: 1 lbm = 0.45359237 kg (exact).
    assert_eq!(conv(1.0, "lbm", "kg"), Value::Number(0.45359237));
    // Distance: 2.5 ft = 0.762 m; 1 mi = 1.609344 km.
    assert_close(conv(2.5, "ft", "m"), 0.762);
    assert_close(conv(1.0, "mi", "km"), 1.609344);
    assert_close(conv(1.0, "km", "mi"), 0.621371192237334);
    // Time: 1 day = 24 hr; 1 yr = 365.25 day (the Julian year Excel uses).
    assert_eq!(conv(1.0, "day", "hr"), Value::Number(24.0));
    assert_close(conv(1.0, "yr", "day"), 365.25);
    // Volume: 1 gal = 3.785411784 l.
    assert_eq!(conv(1.0, "gal", "l"), Value::Number(3.785411784));
    // Force / power.
    assert_eq!(conv(1.0, "lbf", "N"), Value::Number(4.4482216152605));
    assert_eq!(conv(1.0, "HP", "W"), Value::Number(745.69987158227));
    // Pressure.
    assert_close(conv(1.0, "atm", "mmHg"), 760.0021001785152);
}

#[test]
fn si_prefixes_scale_the_metric_base() {
    // Simple linear prefixes.
    assert_eq!(conv(1.0, "kg", "g"), Value::Number(1000.0));
    assert_eq!(conv(100.0, "m", "cm"), Value::Number(10000.0));
    assert_eq!(conv(1.0, "m", "mm"), Value::Number(1000.0));
    // Angstrom base (metric).
    assert_close(conv(1.0, "ang", "m"), 1e-10);
    // Both deka spellings (`da` and `e`) are accepted and equal.
    assert_eq!(conv(1.0, "dag", "g"), conv(1.0, "eg", "g"));
    assert_eq!(conv(1.0, "dag", "g"), Value::Number(10.0));
}

#[test]
fn area_and_volume_prefixes_take_the_dimensional_exponent() {
    // A `k` on m2 scales by 10^6 (squared), on m3 by 10^9 (cubed) — not 10^3.
    assert_close(conv(1.0, "km2", "m2"), 1_000_000.0);
    assert_close(conv(1.0, "km^2", "m^2"), 1_000_000.0);
    assert_close(conv(1.0, "km3", "m3"), 1_000_000_000.0);
    // Liter is NOT a cubed-length unit: its prefix is linear. 1 l = 0.001 m3.
    assert_close(conv(1.0, "l", "m3"), 0.001);
    assert_close(conv(1.0, "tsp", "ml"), 4.92892159375);
}

#[test]
fn binary_prefixes_are_excel_exact_powers_of_two() {
    // Excel: the IEC prefix `ki`=2^10, `Mi`=2^20, `Gi`=2^30. (The `formulas` reference lib is
    // defective here — it yields 8/64/… — so these are hand-verified against Excel; the oracle
    // corpus declares them lib-gaps.)
    assert_eq!(conv(1.0, "kibyte", "byte"), Value::Number(1024.0));
    assert_eq!(conv(1.0, "Mibyte", "byte"), Value::Number(1_048_576.0));
    assert_eq!(conv(1.0, "kibit", "bit"), Value::Number(1024.0));
    assert_eq!(conv(1.0, "Gibit", "bit"), Value::Number(1_073_741_824.0));
    // The SI (decimal) information prefix stays base-10: 1 kbit = 1000 bit, 1 byte = 8 bit.
    assert_eq!(conv(1.0, "kbit", "bit"), Value::Number(1000.0));
    assert_eq!(conv(1.0, "byte", "bit"), Value::Number(8.0));
}

#[test]
fn temperature_is_an_affine_conversion_through_kelvin() {
    // Water freeze/boil landmarks.
    assert_close(conv(32.0, "F", "C"), 0.0);
    assert_close(conv(100.0, "C", "F"), 212.0);
    assert_close(conv(0.0, "C", "K"), 273.15);
    assert_close(conv(2.0, "kel", "C"), -271.15);
    // Réaumur and Rankine.
    assert_close(conv(1.0, "C", "Reau"), 0.8);
    assert_close(conv(1.0, "Rank", "K"), 0.555_555_555_555_555_6);
    // Lowercase aliases behave identically.
    assert_close(conv(32.0, "fah", "cel"), 0.0);
}

#[test]
fn pc_is_disambiguated_by_the_shared_system() {
    // `pc` = parsec (distance) OR pico-calorie (energy); the to-unit selects the system.
    assert_close(conv(1.0, "pc", "ly"), 3.26156377694566);
    // 1e12 pc (pico-calorie) = 1 c (thermodynamic calorie) = 4.184 J.
    assert_close(conv(1e12, "pc", "J"), 4.184);
}

#[test]
fn bad_units_and_pairs_are_na_never_a_panic() {
    // Unknown unit.
    assert_eq!(conv(5.0, "foo", "m"), Value::Error(ErrKind::Na));
    // Cross-system pair (distance vs mass).
    assert_eq!(conv(1.0, "m", "kg"), Value::Error(ErrKind::Na));
    // Units are case- and whitespace-sensitive: a leading space is not the km unit.
    assert_eq!(conv(1.0, " km", "mi"), Value::Error(ErrKind::Na));
    // A bare prefix is not a unit.
    assert_eq!(conv(1.0, "k", "m"), Value::Error(ErrKind::Na));
}

#[test]
fn number_coercions_and_error_propagation() {
    let g = Grid::new(1, vec![Value::Blank]);
    // A boolean number is refused (#VALUE!), not coerced to 1.
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
    // Blank number is 0 (0 C = 32 F).
    assert_close(
        eval(
            &call("CONVERT", vec![Expr::Lit(Value::Blank), txt("C"), txt("F")]),
            &g,
        ),
        32.0,
    );
    // Numeric text coerces.
    assert_eq!(conv_text("1000", "g", "kg"), Value::Number(1.0));
    // An error in any argument propagates.
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
