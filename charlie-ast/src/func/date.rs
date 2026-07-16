// Concern: the DATE/TIME worksheet functions (DATE YEAR MONTH DAY EDATE DATEDIF TODAY NOW) — the Excel 1900 date-serial built-ins WITH the 1900 leap-year bug replicated (`serial_from_ymd`/`serial_to_ymd` are the one place it lives), the valid serial band, and the TODAY/NOW volatiles that read the resolver's injectable clock (never `std::time` from here) | Non-concern: the registry table + dispatch (func/mod.rs), the injectable clock seam (eval.rs/`Resolver` own `now_serial`), and the shared `one_num`/`arg_text`/`finite_or_num` helpers (func/helpers.rs) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
use super::*;

// Date/time batch v1: DATE YEAR MONTH DAY EDATE DATEDIF TODAY NOW.
//
// EPOCH (the load-bearing call, worth a reviewer's eye — docs/format.md §14, and §13.2 for the
// shared serial↔date mapping): the Excel 1900 date-serial system, WITH Excel's 1900 leap-year bug
// REPLICATED (serial 60 = the fictional 1900-02-29; serials ≥ 61 shift back one day), so a serial an
// xlsx round-trip authored in Excel maps to the same civil date later. `serial_to_ymd` (the forward
// map, already used by TEXT's date render) and `serial_from_ymd` (its inverse) are the one place that
// bug lives; DATE/EDATE build a serial by day-offset arithmetic in the CONTIGUOUS serial space, so
// DATE(1900,2,29) reproduces the phantom serial 60 with no special case.
//
// VOLATILITY: TODAY/NOW read the resolver's INJECTABLE clock (`EvalCtx::now_serial` →
// `Resolver::now_serial`), never `std::time` from here — so conformance/tests PIN "now" to a fixed
// instant (`Resolver`'s `PINNED_NOW_SERIAL` = 2023-01-01T12:00, serial 44927.5) and every fixture is
// reproducible, while production's default impl returns real system time. Both rows are `volatile:
// true` in the registry.
//
// COERCION/DOMAIN: DATE truncates its year/month/day toward zero (Excel) and NORMALIZES an
// out-of-range month/day (DATE(2008,14,2) = 2009-02-02; DATE(2023,3,0) = 2023-02-28), folding a year
// in 0..=1899 by +1900. The serial readers (YEAR/MONTH/DAY/EDATE/DATEDIF) `floor` their serial
// argument (matching TEXT's date render) and accept the valid band [1, 2958465] (1900-01-01 …
// 9999-12-31); a serial or a DATE/EDATE result outside it is #NUM!. DATEDIF with start > end is
// #NUM! and an unknown unit is #NUM!. A non-coercible argument is #VALUE!; an error propagates.
/// The largest valid Excel date serial: 9999-12-31.
pub(crate) const MAX_SERIAL: i64 = 2_958_465;

/// Whether `y` is a leap year in the proleptic-Gregorian calendar. (The 1900 leap-year *bug* is NOT
/// applied here — it lives only in the serial↔date mapping; normalization arithmetic uses the real
/// calendar, and v1's date fixtures stay clear of Jan/Feb 1900 where the two would disagree.)
fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// The number of days in month `m` (1-based) of year `y`.
fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        // Every caller passes a month already normalized into 1..=12 (EDATE's `rem_euclid(12) + 1`,
        // DATEDIF's `serial_to_ymd`-derived month), so no other arm is reachable; assert it so an
        // impossible month is a located panic rather than a plausible-but-wrong 30.
        _ => unreachable!("days_in_month expects a normalized 1..=12 month, got {m}"),
    }
}

/// Howard Hinnant's `days_from_civil`: a proleptic-Gregorian `(year, month, day)` → days since the
/// Unix epoch (1970-01-01). Exact integer arithmetic; the inverse of [`civil_from_days`].
const fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 }; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// Convert a proleptic-Gregorian `(year, month, day)` to an Excel 1900-system date serial, replicating
/// Excel's leap-year bug: a date on/after 1900-03-01 is shifted `+1` (Excel counts the phantom
/// 1900-02-29 in the run-up, so serial 61 = 1900-03-01), while an earlier date passes straight through
/// (serial 1 = 1900-01-01). The inverse of [`serial_to_ymd`] for every real civil date (the phantom
/// serial 60 has no civil pre-image — it is produced only by the forward map / by day-offset
/// arithmetic). The returned serial may fall outside the valid band for a pre-epoch input; callers
/// gate the range.
fn serial_from_ymd(y: i64, m: u32, d: u32) -> i64 {
    // On/after 1900-03-01 the phantom leap day sits in the count → shift +1. The threshold is a
    // compile-time constant (const-folded, not recomputed per call).
    const LEAP_BUG_SHIFT_THRESHOLD: i64 = days_from_civil(1900, 3, 1);
    let unix_days = days_from_civil(y, m as i64, d as i64);
    let serial = unix_days - EPOCH_1899_12_31;
    if unix_days >= LEAP_BUG_SHIFT_THRESHOLD {
        serial + 1
    } else {
        serial
    }
}

/// Evaluate a DATE year/month/day argument to an integer, TRUNCATING toward zero (Excel) and rejecting
/// a value outside a safe band (`|x| ≤ 1e15`) as #NUM! so the later `as i64` never saturates. A
/// non-coercible value is #VALUE!; an error propagates.
fn date_int_arg(ctx: &mut EvalCtx, e: &Expr) -> Result<i64, ErrKind> {
    let n = one_num(ctx, e)?.trunc();
    if n.abs() > 1e15 {
        return Err(ErrKind::Num);
    }
    Ok(n as i64)
}

/// Evaluate a serial-valued argument (YEAR/MONTH/DAY/EDATE/DATEDIF): coerce, `floor` to the integer day
/// (matching TEXT's date render), and gate the valid serial band [1, MAX_SERIAL] — a serial before
/// 1900-01-01 or after 9999-12-31 is #NUM!. A non-coercible value is #VALUE!; an error propagates.
fn date_serial_arg(ctx: &mut EvalCtx, e: &Expr) -> Result<i64, ErrKind> {
    let n = one_num(ctx, e)?.floor();
    if !(1.0..=MAX_SERIAL as f64).contains(&n) {
        return Err(ErrKind::Num);
    }
    Ok(n as i64)
}

/// `DATE(year, month, day)` — build a date serial, NORMALIZING an out-of-range month/day (they roll
/// over into adjacent years/months) and folding a year in 0..=1899 by +1900 (Excel). A year outside
/// 0..=9999, or a normalized serial outside [1, MAX_SERIAL], is #NUM!.
pub(crate) fn date_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let y = match date_int_arg(ctx, &args[0]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let m = match date_int_arg(ctx, &args[1]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let d = match date_int_arg(ctx, &args[2]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    // Excel year rule: 0..=1899 folds into the 1900s; a year outside 0..=9999 is #NUM!.
    let year = if (0..=1899).contains(&y) { y + 1900 } else { y };
    if !(0..=9999).contains(&year) {
        return Value::Error(ErrKind::Num);
    }
    // Normalize the (possibly out-of-range) month into a (year, month) with month in 1..=12, then
    // build the serial of that month's day 1 and add (day − 1) days — day roll-over is just serial
    // arithmetic in the contiguous serial space (which includes the phantom serial 60).
    let total = year * 12 + (m - 1);
    let ny = total.div_euclid(12);
    let nm = (total.rem_euclid(12) + 1) as u32;
    if !(0..=9999).contains(&ny) {
        return Value::Error(ErrKind::Num);
    }
    let serial = serial_from_ymd(ny, nm, 1) + (d - 1);
    if !(1..=MAX_SERIAL).contains(&serial) {
        return Value::Error(ErrKind::Num);
    }
    Value::Number(serial as f64)
}

/// `YEAR(serial)` — the Gregorian year of a date serial (1900 system, leap-bug faithful:
/// `YEAR(60) = 1900`).
pub(crate) fn year_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match date_serial_arg(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok(s) => Value::Number(serial_to_ymd(s).0 as f64),
    }
}

/// `MONTH(serial)` — the 1-based month of a date serial (`MONTH(60) = 2`, the phantom 1900-02-29).
pub(crate) fn month_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match date_serial_arg(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok(s) => Value::Number(serial_to_ymd(s).1 as f64),
    }
}

/// `DAY(serial)` — the 1-based day-of-month of a date serial (`DAY(60) = 29`, the phantom day).
pub(crate) fn day_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match date_serial_arg(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok(s) => Value::Number(serial_to_ymd(s).2 as f64),
    }
}

/// `EDATE(start_date, months)` — the date `months` months from `start_date` (a serial), CLAMPING the
/// day to the target month's last day (`EDATE(2020-01-31, 1) = 2020-02-29`). `months` truncates toward
/// zero; a result outside [1, MAX_SERIAL] is #NUM!.
pub(crate) fn edate_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let start = match date_serial_arg(ctx, &args[0]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let months = match date_int_arg(ctx, &args[1]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (y, m, d) = serial_to_ymd(start);
    let total = y * 12 + (m as i64 - 1) + months;
    let ny = total.div_euclid(12);
    let nm = (total.rem_euclid(12) + 1) as u32;
    if !(0..=9999).contains(&ny) {
        return Value::Error(ErrKind::Num);
    }
    let nd = d.min(days_in_month(ny, nm));
    let serial = serial_from_ymd(ny, nm, nd);
    if !(1..=MAX_SERIAL).contains(&serial) {
        return Value::Error(ErrKind::Num);
    }
    Value::Number(serial as f64)
}

/// `DATEDIF(start_date, end_date, unit)` — the elapsed time between two serials in `unit`:
/// `"Y"`/`"M"`/`"D"` (complete years / months / days) plus `"MD"`/`"YM"`/`"YD"` (day/month/day
/// remainders ignoring the larger units). The unit folds case (Excel accepts either). `start > end`
/// is #NUM!; an unknown unit is #NUM!.
pub(crate) fn datedif_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let s = match date_serial_arg(ctx, &args[0]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let e = match date_serial_arg(ctx, &args[1]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let unit = match arg_text(ctx, &args[2]) {
        Ok(u) => u,
        Err(k) => return Value::Error(k),
    };
    if s > e {
        return Value::Error(ErrKind::Num);
    }
    let (y1, m1, d1) = serial_to_ymd(s);
    let (y2, m2, d2) = serial_to_ymd(e);
    let result: i64 = match unit.to_ascii_uppercase().as_str() {
        "D" => e - s,
        "Y" => {
            let mut yr = y2 - y1;
            if (m2, d2) < (m1, d1) {
                yr -= 1;
            }
            yr
        }
        "M" => {
            let mut mo = (y2 - y1) * 12 + (m2 as i64 - m1 as i64);
            if d2 < d1 {
                mo -= 1;
            }
            mo
        }
        // Days ignoring months and years. When the end day is on/after the start day it is a plain
        // difference; otherwise borrow the previous month's length (computed in i64 so the known
        // Excel `MD` borrow corner cannot underflow-panic — v1 fixtures use the clean branch).
        "MD" => {
            if d2 >= d1 {
                (d2 - d1) as i64
            } else {
                let (py, pm) = if m2 == 1 { (y2 - 1, 12) } else { (y2, m2 - 1) };
                days_in_month(py, pm) as i64 - d1 as i64 + d2 as i64
            }
        }
        // Months ignoring years (and the day remainder): the month gap, less one if the end day has
        // not yet reached the start day, folded into 0..=11.
        "YM" => {
            let mut mo = m2 as i64 - m1 as i64;
            if d2 < d1 {
                mo -= 1;
            }
            mo.rem_euclid(12)
        }
        // Days ignoring years: re-home the end's month/day into the start's year (or the next year if
        // it falls before the start's month/day) and take the serial difference.
        "YD" => {
            let (ey, em, ed) = if (m2, d2) >= (m1, d1) {
                (y1, m2, d2)
            } else {
                (y1 + 1, m2, d2)
            };
            serial_from_ymd(ey, em, ed) - serial_from_ymd(y1, m1, d1)
        }
        _ => return Value::Error(ErrKind::Num),
    };
    Value::Number(result as f64)
}

/// `TODAY()` — the current date as an integer serial (the time-of-day fraction FLOORed off). VOLATILE:
/// reads the resolver's injectable clock (pinned in tests/conformance, system time in production), and
/// returns a `Value::Number` usable in arithmetic (`TODAY()+7`).
pub(crate) fn today_fn(ctx: &mut EvalCtx, _args: &[Expr]) -> Value {
    finite_or_num(ctx.now_serial().floor())
}

/// `NOW()` — the current date AND time as a serial: the integer date plus a fractional time-of-day
/// (noon = 0.5). VOLATILE, same injectable clock as [`today_fn`].
pub(crate) fn now_fn(ctx: &mut EvalCtx, _args: &[Expr]) -> Value {
    finite_or_num(ctx.now_serial())
}
