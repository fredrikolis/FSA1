// Concern: the ENGINEERING worksheet function CONVERT — the measurement-unit converter that scales a number between two units of the SAME physical system (mass/distance/time/pressure/force/energy/power/magnetism/temperature/volume/area/information/speed), applying the SI decimal prefixes (Y..y, plus deka's `da`/`e`) to a metric base unit and the binary IEC prefixes (ki..Yi = 2^10..2^80) to the information units, with temperature handled as an AFFINE conversion via Kelvin (Excel's C/F/K/Rank/Reau offsets) rather than a pure ratio; a bad/unknown unit or a cross-system pair is `#N/A` and a boolean `number` is `#VALUE!`, never a panic (CORE2) | Non-concern: the arithmetic/rounding functions (func/math.rs), the transcendental family (func/trig.rs), the registry table + dispatch (func/mod.rs), the coercion machinery (eval.rs owns `coerce_num`/`scalarize`/`to_text`), and the shared `arg_text`/`finite_or_num` helpers (func/helpers.rs) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
use super::*;

// The physical-system tags. CONVERT only scales BETWEEN units that share a system; a from/to pair
// whose systems do not intersect is `#N/A`. `TEMPERATURE` and `INFORMATION` are named because they
// take special paths (affine offsets / binary prefixes); the rest are pure ratio conversions that
// need only equality of the tag.
const AREA: u8 = 0;
const DISTANCE: u8 = 1;
const ENERGY: u8 = 2;
const FORCE: u8 = 3;
const INFORMATION: u8 = 4;
const MAGNETISM: u8 = 5;
const MASS: u8 = 6;
const POWER: u8 = 7;
const PRESSURE: u8 = 8;
const SPEED: u8 = 9;
const TEMPERATURE: u8 = 10;
const TIME: u8 = 11;
const VOLUME: u8 = 12;

/// One BASE (unprefixed) unit: its exact `name` (units are case- AND spelling-sensitive in Excel),
/// its physical `sys`, the `factor` giving its value in the system's canonical base (so a conversion
/// is `number · from_factor / to_factor`), whether it accepts SI metric prefixes (`si`), and the
/// dimensional `pow` a prefix is raised to before scaling — `2` for the area units (a `k` on `m2` is
/// `10^6`, not `10^3`), `3` for the cubic-length volume units, `1` for everything else. The full
/// prefixed name space (~1200 names) is generated on demand by [`resolve`] stripping a prefix and
/// looking the remainder up here, rather than materialized as a table.
struct Unit {
    name: &'static str,
    sys: u8,
    factor: f64,
    si: bool,
    pow: u8,
}

/// Terse constructor so the [`UNITS`] table reads as one row per line.
const fn unit(name: &'static str, sys: u8, factor: f64, si: bool, pow: u8) -> Unit {
    Unit {
        name,
        sys,
        factor,
        si,
        pow,
    }
}

/// The base-unit table (the exact Excel factors — the same constants the `formulas` reference lib
/// tabulates). Grouped by system for readability only; lookup is by exact name.
static UNITS: &[Unit] = &[
    // area
    unit("Morgen", AREA, 2500.0, false, 1),
    unit("Nmi2", AREA, 3429904.0, false, 2),
    unit("Nmi^2", AREA, 3429904.0, false, 2),
    unit("Pica2", AREA, 1.24452160493827e-07, false, 2),
    unit("Pica^2", AREA, 1.24452160493827e-07, false, 2),
    unit("Picapt2", AREA, 1.24452160493827e-07, false, 2),
    unit("Picapt^2", AREA, 1.24452160493827e-07, false, 2),
    unit("ang2", AREA, 1e-20, true, 2),
    unit("ang^2", AREA, 1e-20, true, 2),
    unit("ar", AREA, 100.0, true, 1),
    unit("ft2", AREA, 0.09290304, false, 2),
    unit("ft^2", AREA, 0.09290304, false, 2),
    unit("ha", AREA, 10000.0, false, 1),
    unit("in2", AREA, 0.00064516, false, 2),
    unit("in^2", AREA, 0.00064516, false, 2),
    unit("ly2", AREA, 8.95054210748189e+31, false, 2),
    unit("ly^2", AREA, 8.95054210748189e+31, false, 2),
    unit("m2", AREA, 1.0, true, 2),
    unit("m^2", AREA, 1.0, true, 2),
    unit("mi2", AREA, 2589988.110336, false, 2),
    unit("mi^2", AREA, 2589988.110336, false, 2),
    unit("uk_acre", AREA, 4046.8564224, false, 1),
    unit("us_acre", AREA, 4046.87260987425, false, 1),
    unit("yd2", AREA, 0.83612736, false, 2),
    unit("yd^2", AREA, 0.83612736, false, 2),
    // distance
    unit("Nmi", DISTANCE, 1852.0, false, 1),
    unit("Pica", DISTANCE, 0.000352777777777778, false, 1),
    unit("Picapt", DISTANCE, 0.000352777777777778, false, 1),
    unit("ang", DISTANCE, 1e-10, true, 1),
    unit("ell", DISTANCE, 1.143, false, 1),
    unit("ft", DISTANCE, 0.3048, false, 1),
    unit("in", DISTANCE, 0.0254, false, 1),
    unit("ly", DISTANCE, 9460730472580800.0, true, 1),
    unit("m", DISTANCE, 1.0, true, 1),
    unit("mi", DISTANCE, 1609.344, false, 1),
    unit("parsec", DISTANCE, 3.0856775812815532e+16, true, 1),
    unit("pc", DISTANCE, 3.0856775812815532e+16, true, 1),
    unit("pica", DISTANCE, 0.00423333333333333, false, 1),
    unit("survey_mi", DISTANCE, 1609.34721869444, false, 1),
    unit("yd", DISTANCE, 0.9144, false, 1),
    // energy
    unit("BTU", ENERGY, 1055.05585262, false, 1),
    unit("HPh", ENERGY, 2684519.53769617, false, 1),
    unit("J", ENERGY, 1.0, true, 1),
    unit("Wh", ENERGY, 3600.0, true, 1),
    unit("btu", ENERGY, 1055.05585262, false, 1),
    unit("c", ENERGY, 4.184, true, 1),
    unit("cal", ENERGY, 4.1868, true, 1),
    unit("e", ENERGY, 1e-07, true, 1),
    unit("eV", ENERGY, 1.602176487e-19, true, 1),
    unit("ev", ENERGY, 1.602176487e-19, true, 1),
    unit("flb", ENERGY, 1.3558179483314, false, 1),
    unit("hh", ENERGY, 2684519.53769617, false, 1),
    unit("wh", ENERGY, 3600.0, true, 1),
    // force
    unit("N", FORCE, 1.0, true, 1),
    unit("dy", FORCE, 1e-05, true, 1),
    unit("dyn", FORCE, 1e-05, true, 1),
    unit("lbf", FORCE, 4.4482216152605, false, 1),
    // information
    unit("bit", INFORMATION, 1.0, true, 1),
    unit("byte", INFORMATION, 8.0, true, 1),
    // magnetism
    unit("T", MAGNETISM, 1.0, true, 1),
    unit("ga", MAGNETISM, 0.0001, true, 1),
    // mass
    unit("LTON", MASS, 1016046.9088, false, 1),
    unit("brton", MASS, 1016046.9088, false, 1),
    unit("cwt", MASS, 45359.237, false, 1),
    unit("g", MASS, 1.0, true, 1),
    unit("grain", MASS, 0.06479891, false, 1),
    unit("hweight", MASS, 50802.34544, false, 1),
    unit("lbm", MASS, 453.59237, false, 1),
    unit("lcwt", MASS, 50802.34544, false, 1),
    unit("ozm", MASS, 28.349523125, false, 1),
    unit("sg", MASS, 14593.9029372064, false, 1),
    unit("shweight", MASS, 45359.237, false, 1),
    unit("stone", MASS, 6350.29318, false, 1),
    unit("ton", MASS, 907184.74, false, 1),
    unit("u", MASS, 1.660538782e-24, true, 1),
    unit("uk_cwt", MASS, 50802.34544, false, 1),
    unit("uk_ton", MASS, 1016046.9088, false, 1),
    // power
    unit("HP", POWER, 745.69987158227, false, 1),
    unit("PS", POWER, 735.49875, false, 1),
    unit("W", POWER, 1.0, true, 1),
    unit("h", POWER, 745.69987158227, false, 1),
    unit("w", POWER, 1.0, true, 1),
    // pressure
    unit("Pa", PRESSURE, 1.0, true, 1),
    unit("Torr", PRESSURE, 133.322368421053, false, 1),
    unit("at", PRESSURE, 101325.0, true, 1),
    unit("atm", PRESSURE, 101325.0, true, 1),
    unit("mmHg", PRESSURE, 133.322, true, 1),
    unit("p", PRESSURE, 1.0, true, 1),
    unit("psi", PRESSURE, 6894.75729316836, false, 1),
    // speed
    unit("admkn", SPEED, 1853.184, false, 1),
    unit("kn", SPEED, 1852.0, false, 1),
    unit("m/h", SPEED, 1.0, true, 1),
    unit("m/hr", SPEED, 1.0, true, 1),
    unit("m/s", SPEED, 3600.0, true, 1),
    unit("m/sec", SPEED, 3600.0, true, 1),
    unit("mph", SPEED, 1609.344, true, 1),
    // temperature
    unit("C", TEMPERATURE, 1.0, false, 1),
    unit("F", TEMPERATURE, 1.0, false, 1),
    unit("K", TEMPERATURE, 1.0, true, 1),
    unit("Rank", TEMPERATURE, 1.0, false, 1),
    unit("Reau", TEMPERATURE, 1.0, false, 1),
    unit("cel", TEMPERATURE, 1.0, false, 1),
    unit("fah", TEMPERATURE, 1.0, false, 1),
    unit("kel", TEMPERATURE, 1.0, true, 1),
    // time
    unit("d", TIME, 86400.0, false, 1),
    unit("day", TIME, 86400.0, false, 1),
    unit("hr", TIME, 3600.0, false, 1),
    unit("min", TIME, 60.0, false, 1),
    unit("mn", TIME, 60.0, false, 1),
    unit("s", TIME, 1.0, true, 1),
    unit("sec", TIME, 1.0, true, 1),
    unit("yr", TIME, 31557600.0, false, 1),
    // volume
    unit("L", VOLUME, 1.0, true, 1),
    unit("MTON", VOLUME, 1132.67386368, false, 1),
    unit("Nmi3", VOLUME, 6352182208000.0, false, 3),
    unit("Nmi^3", VOLUME, 6352182208000.0, false, 3),
    unit("Pica3", VOLUME, 4.39039566186557e-08, false, 3),
    unit("Pica^3", VOLUME, 4.39039566186557e-08, false, 3),
    unit("Picapt3", VOLUME, 4.39039566186557e-08, false, 3),
    unit("Picapt^3", VOLUME, 4.39039566186557e-08, false, 3),
    unit("ang3", VOLUME, 1e-27, true, 3),
    unit("ang^3", VOLUME, 1e-27, true, 3),
    unit("barrel", VOLUME, 158.987294928, false, 1),
    unit("bushel", VOLUME, 35.23907016688, false, 1),
    unit("cup", VOLUME, 0.2365882365, false, 1),
    unit("ft3", VOLUME, 28.316846592, false, 3),
    unit("ft^3", VOLUME, 28.316846592, false, 3),
    unit("gal", VOLUME, 3.785411784, false, 1),
    unit("in3", VOLUME, 0.016387064, false, 3),
    unit("in^3", VOLUME, 0.016387064, false, 3),
    unit("l", VOLUME, 1.0, true, 1),
    unit("lt", VOLUME, 1.0, true, 1),
    unit("ly3", VOLUME, 8.467866646237152e+50, false, 3),
    unit("ly^3", VOLUME, 8.467866646237152e+50, false, 3),
    unit("m3", VOLUME, 1000.0, true, 3),
    unit("m^3", VOLUME, 1000.0, true, 3),
    unit("mi3", VOLUME, 4168181825440.5796, false, 3),
    unit("mi^3", VOLUME, 4168181825440.5796, false, 3),
    unit("oz", VOLUME, 0.0295735295625, false, 1),
    unit("pt", VOLUME, 0.473176473, false, 1),
    unit("qt", VOLUME, 0.946352946, false, 1),
    unit("regton", VOLUME, 2831.6846592, false, 1),
    unit("tbs", VOLUME, 0.01478676478125, false, 1),
    unit("tsp", VOLUME, 0.00492892159375, false, 1),
    unit("tspm", VOLUME, 0.005, false, 1),
    unit("uk_gal", VOLUME, 4.54609, false, 1),
    unit("uk_pt", VOLUME, 0.56826125, true, 1),
    unit("uk_qt", VOLUME, 1.1365225, false, 1),
    unit("us_pt", VOLUME, 0.473176473, false, 1),
    unit("yd3", VOLUME, 764.554857984, false, 3),
    unit("yd^3", VOLUME, 764.554857984, false, 3),
];

/// The SI decimal prefixes `(abbreviation, multiplier)`. Excel accepts BOTH `da` and `e` for deka
/// (×10). Applied only to a base marked `si`; the multiplier is raised to the base's `pow` (so an
/// area/volume prefix scales by the square/cube).
static SI_PREFIXES: &[(&str, f64)] = &[
    ("Y", 1e24),
    ("Z", 1e21),
    ("E", 1e18),
    ("P", 1e15),
    ("T", 1e12),
    ("G", 1e9),
    ("M", 1e6),
    ("k", 1e3),
    ("h", 1e2),
    ("da", 1e1),
    ("e", 1e1),
    ("d", 1e-1),
    ("c", 1e-2),
    ("m", 1e-3),
    ("u", 1e-6),
    ("n", 1e-9),
    ("p", 1e-12),
    ("f", 1e-15),
    ("a", 1e-18),
    ("z", 1e-21),
    ("y", 1e-24),
];

/// The IEC binary prefixes `(abbreviation, multiplier = 2^(10·n))`, valid only on the information
/// units (`bit`/`byte`). The multipliers are exact powers of two (representable in `f64`).
static BIN_PREFIXES: &[(&str, f64)] = &[
    ("ki", 1024.0),
    ("Mi", 1048576.0),
    ("Gi", 1073741824.0),
    ("Ti", 1099511627776.0),
    ("Pi", 1125899906842624.0),
    ("Ei", 1152921504606846976.0),
    ("Zi", 1180591620717411303424.0),
    ("Yi", 1208925819614629174706176.0),
];

/// Resolve a unit name to every `(system, factor)` it denotes. A name is looked up directly as a
/// base, then — for each prefix it starts with — the remainder is looked up as a prefixable base.
/// Almost every valid name yields exactly one interpretation; the deliberate exception is `pc`,
/// which is both the parsec (distance) AND pico-calorie (energy), so a name can legitimately live in
/// two systems (Excel intersects the from/to systems to disambiguate). An unknown name yields none.
fn resolve(name: &str) -> Vec<(u8, f64)> {
    let mut out: Vec<(u8, f64)> = Vec::new();
    for u in UNITS {
        if u.name == name {
            out.push((u.sys, u.factor));
        }
    }
    for &(pf, mult) in SI_PREFIXES {
        if let Some(rem) = name.strip_prefix(pf) {
            if rem.is_empty() {
                continue;
            }
            for u in UNITS {
                if u.name == rem && u.si {
                    out.push((u.sys, mult.powi(i32::from(u.pow)) * u.factor));
                }
            }
        }
    }
    for &(bp, mult) in BIN_PREFIXES {
        if let Some(rem) = name.strip_prefix(bp) {
            for u in UNITS {
                if u.name == rem && u.sys == INFORMATION {
                    out.push((INFORMATION, mult * u.factor));
                }
            }
        }
    }
    out
}

/// The affine temperature conversion (Excel's model): scale the input by the from-unit's prefix
/// factor, convert to Kelvin using the unit's offset, convert Kelvin to the to-unit, then divide by
/// the to-unit's prefix factor. The offsets key on the exact unit STRING (an unprefixed C/F/Rank/Reau
/// or the already-Kelvin K/kel and any prefixed kelvin, which fall through as pure scaling), mirroring
/// Excel/the `formulas` reference so `CONVERT(0,"C","F")` is `32`, not a ratio.
fn convert_temp(mut n: f64, from: &str, to: &str, from_factor: f64, to_factor: f64) -> f64 {
    n *= from_factor;
    match from {
        "C" | "cel" => n += 273.15,
        "F" | "fah" => n = (n + 459.67) * 5.0 / 9.0,
        "Rank" => n *= 5.0 / 9.0,
        "Reau" => n = n * 5.0 / 4.0 + 273.15,
        _ => {}
    }
    match to {
        "C" | "cel" => n -= 273.15,
        "F" | "fah" => n = n * 9.0 / 5.0 - 459.67,
        "Rank" => n /= 5.0 / 9.0,
        "Reau" => n = (n - 273.15) / 5.0 * 4.0,
        _ => {}
    }
    n / to_factor
}

/// `CONVERT(number, from_unit, to_unit)` — convert `number` from `from_unit` to `to_unit`. The units
/// must belong to the same measurement system (else `#N/A`); an unrecognized unit is `#N/A`; a boolean
/// `number` is `#VALUE!` (matching Excel — it refuses a logical rather than coercing it); a blank
/// `number` is `0` and numeric text coerces. SI prefixes apply to metric base units and IEC binary
/// prefixes to the information units. Temperature is an affine (through-Kelvin) conversion.
pub(crate) fn convert_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    // `number`: a boolean is refused up front (Excel `#VALUE!`); everything else coerces as usual
    // (blank → 0, numeric text → its value, non-numeric text → `#VALUE!`, error propagates).
    let number = match scalarize(ctx.eval(&args[0])) {
        Value::Error(k) => return Value::Error(k),
        Value::Bool(_) => return Value::Error(ErrKind::Value),
        other => match coerce_num(&other) {
            Ok(n) => n,
            Err(k) => return Value::Error(k),
        },
    };
    let from_unit = match arg_text(ctx, &args[1]) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let to_unit = match arg_text(ctx, &args[2]) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };

    let froms = resolve(&from_unit);
    let tos = resolve(&to_unit);
    // The shared system disambiguates a multi-system name (`pc`): take the first system present in
    // both operands. No common system — including any unknown unit, whose `resolve` is empty — is
    // `#N/A`, exactly as Excel reports an unconvertible pair.
    let mut chosen = None;
    for &(fs, ff) in &froms {
        if let Some(&(_, tf)) = tos.iter().find(|&&(ts, _)| ts == fs) {
            chosen = Some((fs, ff, tf));
            break;
        }
    }
    let (sys, from_factor, to_factor) = match chosen {
        Some(triple) => triple,
        None => return Value::Error(ErrKind::Na),
    };

    let result = if sys == TEMPERATURE {
        convert_temp(number, &from_unit, &to_unit, from_factor, to_factor)
    } else {
        number * from_factor / to_factor
    };
    finite_or_num(result)
}
