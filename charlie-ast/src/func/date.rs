// Concern: the DATE/TIME worksheet functions (DATE YEAR MONTH DAY EDATE EOMONTH DATEDIF DAYS WEEKDAY WEEKNUM WORKDAY NETWORKDAYS YEARFRAC HOUR MINUTE SECOND TIME TODAY NOW) — the Excel 1900 date-serial built-ins WITH the 1900 leap-year bug replicated (the ymd->serial map `serial_from_ymd` lives here; its inverse `serial_to_ymd` + the shared epoch live in `func::text`, which TEXT's date render also needs), the valid serial band, the day-count / weekday / working-day / time-of-day arithmetic, and the TODAY/NOW volatiles that read the resolver's injectable clock (never `std::time` from here) | Non-concern: the registry table + dispatch (func/mod.rs), the injectable clock seam (eval.rs/`Resolver` own `now_serial`), and the shared `one_num`/`arg_text`/`finite_or_num` helpers (func/helpers.rs) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
use super::*;
use std::collections::HashSet;

// Date/time batch v1: DATE YEAR MONTH DAY EDATE DATEDIF TODAY NOW.
//
// EPOCH (the load-bearing call, worth a reviewer's eye — the shared serial↔date mapping is single-
// homed across two files: the forward `serial_to_ymd` in `func::text` and its inverse `serial_from_ymd`
// below): the Excel 1900 date-serial system, WITH Excel's 1900 leap-year bug
// REPLICATED (serial 60 = the fictional 1900-02-29; serials ≥ 61 shift back one day), so a serial an
// xlsx round-trip authored in Excel maps to the same civil date later. `serial_to_ymd` (the forward
// map in `func::text`, already used by TEXT's date render) and `serial_from_ymd` (its inverse, below)
// each carry that same bug; DATE/EDATE build a serial by day-offset arithmetic in the CONTIGUOUS
// serial space, so DATE(1900,2,29) reproduces the phantom serial 60 with no special case.
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

/// The number of days in month `m` (1-based) of year `y`. Shared with `func::text` (VALUE's date-text
/// parser validates a day against its month here) alongside [`serial_from_ymd`].
pub(crate) fn days_in_month(y: i64, m: u32) -> u32 {
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
/// gate the range. Shared with `func::text` (VALUE's date-text parser turns a `yyyy-mm-dd` string into
/// a serial through this map).
pub(crate) fn serial_from_ymd(y: i64, m: u32, d: u32) -> i64 {
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

/// `EOMONTH(start_date, months)` — the serial of the LAST day of the month `months` months from
/// `start_date` (`months` truncates toward zero; negative goes back). A result outside [1, MAX_SERIAL]
/// is #NUM!.
pub(crate) fn eomonth_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let start = match date_serial_arg(ctx, &args[0]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let months = match date_int_arg(ctx, &args[1]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (y, m, _) = serial_to_ymd(start);
    let total = y * 12 + (m as i64 - 1) + months;
    let ny = total.div_euclid(12);
    let nm = (total.rem_euclid(12) + 1) as u32;
    if !(0..=9999).contains(&ny) {
        return Value::Error(ErrKind::Num);
    }
    let serial = serial_from_ymd(ny, nm, days_in_month(ny, nm));
    if !(1..=MAX_SERIAL).contains(&serial) {
        return Value::Error(ErrKind::Num);
    }
    Value::Number(serial as f64)
}

/// `DAYS(end_date, start_date)` — the number of days from `start_date` to `end_date` (both serials);
/// note the END-first argument order. Negative when `end_date` precedes `start_date`.
pub(crate) fn days_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let end = match date_serial_arg(ctx, &args[0]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let start = match date_serial_arg(ctx, &args[1]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    Value::Number((end - start) as f64)
}

/// The day-of-week of a date serial in Excel's default `WEEKDAY` numbering (1 = Sunday … 7 = Saturday),
/// computed straight from the serial modulo 7 (serial 1 = Sunday, matching Excel — the count runs in
/// the contiguous serial space, so the 1900 leap bug rides along automatically). The shared weekday
/// primitive behind WEEKDAY / WEEKNUM / WORKDAY / NETWORKDAYS.
fn weekday_sun1(serial: i64) -> i64 {
    (serial - 1).rem_euclid(7) + 1
}

/// `WEEKDAY(serial, [return_type])` — the day of week of a date serial. `return_type` picks the
/// numbering: 1 (default) Sun=1..Sat=7; 2 or 11 Mon=1..Sun=7; 3 Mon=0..Sun=6; 12..17 weeks starting
/// Tue..Sun returning 1..7. An unsupported type is #NUM!.
pub(crate) fn weekday_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let serial = match date_serial_arg(ctx, &args[0]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let rtype = match opt_int_arg(ctx, args, 1, 1) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let wd = weekday_sun1(serial); // 1=Sun..7=Sat
    let result = match rtype {
        1 => wd,                         // Sun=1..Sat=7
        2 => (wd - 2).rem_euclid(7) + 1, // Mon=1..Sun=7
        3 => (wd - 2).rem_euclid(7),     // Mon=0..Sun=6
        // 11→Mon(Sun-code 2) … 16→Sat(7) … 17→Sun(1); return 1..7 from the week's first day.
        11..=17 => {
            let start = if rtype == 17 { 1 } else { rtype - 9 };
            (wd - start).rem_euclid(7) + 1
        }
        _ => return Value::Error(ErrKind::Num),
    };
    Value::Number(result as f64)
}

/// `WEEKNUM(serial, [return_type])` — the week of the year for a date serial. For the day-of-week
/// types (1 default = weeks start Sunday; 2 / 11..17 = weeks start Mon..Sun) week 1 is the week
/// CONTAINING January 1. Type 21 is the ISO-8601 week (weeks start Monday; week 1 holds the year's
/// first Thursday). An unsupported type is #NUM!.
pub(crate) fn weeknum_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let serial = match date_serial_arg(ctx, &args[0]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let rtype = match opt_int_arg(ctx, args, 1, 1) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    if rtype == 21 {
        return Value::Number(iso_weeknum(serial) as f64);
    }
    // The Sun=1..Sat=7 code of each simple numbering's week-start day.
    let start = match rtype {
        1 | 17 => 1,          // Sunday
        2 | 11 => 2,          // Monday
        12..=16 => rtype - 9, // Tue..Sat
        _ => return Value::Error(ErrKind::Num),
    };
    let (y, _, _) = serial_to_ymd(serial);
    let jan1 = serial_from_ymd(y, 1, 1);
    // Days from Jan 1 (0-based) plus Jan 1's offset within its week gives the completed-week count.
    let offset = (weekday_sun1(jan1) - start).rem_euclid(7);
    let week = (serial - jan1 + offset) / 7 + 1;
    Value::Number(week as f64)
}

/// The ISO-8601 week number of a date serial (weeks start Monday; week 1 holds the year's first
/// Thursday), resolving the year-boundary weeks that belong to the adjacent ISO year.
fn iso_weeknum(serial: i64) -> i64 {
    let (y, _, _) = serial_to_ymd(serial);
    let jan1 = serial_from_ymd(y, 1, 1);
    let ordinal = serial - jan1 + 1; // 1-based day of year
    let weekday_iso = (weekday_sun1(serial) + 5).rem_euclid(7) + 1; // Mon=1..Sun=7
    let week = (ordinal - weekday_iso + 10).div_euclid(7);
    if week < 1 {
        iso_weeks_in_year(y - 1)
    } else if week > iso_weeks_in_year(y) {
        1
    } else {
        week
    }
}

/// The number of ISO-8601 weeks in year `y` — 53 when the year starts on a Thursday (or a leap year
/// starts on a Wednesday), else 52.
fn iso_weeks_in_year(y: i64) -> i64 {
    let p = |y: i64| (y + y.div_euclid(4) - y.div_euclid(100) + y.div_euclid(400)).rem_euclid(7);
    if p(y) == 4 || p(y - 1) == 3 { 53 } else { 52 }
}

/// `YEARFRAC(start_date, end_date, [basis])` — the fraction of a year between two date serials under
/// the day-count `basis`: 0 (default) 30/360 US (NASD), 1 actual/actual, 2 actual/360, 3 actual/365,
/// 4 30E/360 (European). The endpoints are used order-independently (`YEARFRAC` is symmetric). An
/// unsupported basis is #NUM!.
pub(crate) fn yearfrac_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let a = match date_serial_arg(ctx, &args[0]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let b = match date_serial_arg(ctx, &args[1]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let basis = match opt_int_arg(ctx, args, 2, 0) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    // Symmetric: order the endpoints so start <= end.
    let (s, e) = if a <= b { (a, b) } else { (b, a) };
    let frac = match basis {
        0 => yf_30_360(s, e, false),
        1 => yf_actual_actual(s, e),
        2 => (e - s) as f64 / 360.0,
        3 => (e - s) as f64 / 365.0,
        4 => yf_30_360(s, e, true),
        _ => return Value::Error(ErrKind::Num),
    };
    finite_or_num(frac)
}

/// The 30/360 year fraction between two serials. `european` selects 30E/360 (basis 4): a day of 31 is
/// clamped to 30 on both endpoints. The US (NASD, basis 0) form additionally applies the end-of-Feb
/// rule and only clamps the end day to 30 when the start day is already 30 — Excel's documented order.
fn yf_30_360(s: i64, e: i64, european: bool) -> f64 {
    let (y1, m1, d1r) = serial_to_ymd(s);
    let (y2, m2, d2r) = serial_to_ymd(e);
    let (mut d1, mut d2) = (d1r as i64, d2r as i64);
    if european {
        if d1 == 31 {
            d1 = 30;
        }
        if d2 == 31 {
            d2 = 30;
        }
    } else {
        let last1 = d1r == days_in_month(y1, m1);
        let last2 = d2r == days_in_month(y2, m2);
        if m1 == 2 && last1 && m2 == 2 && last2 {
            d2 = 30;
        }
        if m1 == 2 && last1 {
            d1 = 30;
        }
        // Clamp the start day (31 -> 30) BEFORE testing the end day, so a 31-to-31 span reduces both
        // endpoints. Reversing these — the earlier bug — left d1 == 31 when the d2 test ran, so a
        // month-end-to-month-end pair (e.g. 2020-01-31 -> 2020-07-31) was one day too long.
        if d1 == 31 {
            d1 = 30;
        }
        if d2 == 31 && d1 == 30 {
            d2 = 30;
        }
    }
    let days = 360 * (y2 - y1) + 30 * (m2 as i64 - m1 as i64) + (d2 - d1);
    days as f64 / 360.0
}

/// The actual/actual year fraction (basis 1): actual elapsed days over a year length that is the
/// year's own length when both endpoints share a year, 365/366 for a sub-year span crossing one year
/// boundary (366 iff a February 29 lies in the interval), and the average calendar-year length over
/// the spanned years otherwise — Excel's documented method.
fn yf_actual_actual(s: i64, e: i64) -> f64 {
    if s == e {
        return 0.0;
    }
    let days = (e - s) as f64;
    let (y1, m1, d1) = serial_to_ymd(s);
    let (y2, m2, d2) = serial_to_ymd(e);
    let denom = if y1 == y2 {
        days_in_year(y1) as f64
    } else if y2 - y1 == 1 && (m1, d1) >= (m2, d2) {
        // A span of at most one year that crosses a single year boundary.
        if leap_day_in_range(s, e) {
            366.0
        } else {
            365.0
        }
    } else {
        let total: i64 = (y1..=y2).map(days_in_year).sum();
        total as f64 / (y2 - y1 + 1) as f64
    };
    days / denom
}

/// The number of days in year `y` (366 in a leap year).
fn days_in_year(y: i64) -> i64 {
    if is_leap(y) { 366 } else { 365 }
}

/// Whether a February 29 falls within the inclusive serial range `[s, e]` (used by actual/actual to
/// pick a 366-day denominator for a sub-year span). Only the two endpoints' years can contribute for
/// such a span, so both are checked.
fn leap_day_in_range(s: i64, e: i64) -> bool {
    let (y1, _, _) = serial_to_ymd(s);
    let (y2, _, _) = serial_to_ymd(e);
    (y1..=y2).any(|y| {
        is_leap(y) && {
            let feb29 = serial_from_ymd(y, 2, 29);
            s <= feb29 && feb29 <= e
        }
    })
}

/// Gather the optional `holidays` argument (a scalar, range, or array) into a set of integer date
/// serials for WORKDAY / NETWORKDAYS. A blank cell is skipped; a non-coercible value is #VALUE!; an
/// error propagates.
fn gather_holidays(ctx: &mut EvalCtx, e: &Expr) -> Result<HashSet<i64>, ErrKind> {
    let mut set = HashSet::new();
    let push = |v: &Value, set: &mut HashSet<i64>| -> Result<(), ErrKind> {
        match v {
            Value::Blank => {}
            Value::Error(k) => return Err(*k),
            other => {
                set.insert(coerce_num(other)?.floor() as i64);
            }
        }
        Ok(())
    };
    match ctx.eval(e) {
        Value::Array(_, cells) => {
            for c in &cells {
                push(c, &mut set)?;
            }
        }
        other => push(&other, &mut set)?,
    }
    Ok(set)
}

/// `WORKDAY(start_date, days, [holidays])` — the serial `days` working days (Mon–Fri, excluding any
/// `holidays`) after `start_date` (`days` truncates toward zero; negative counts backward). A result
/// outside [1, MAX_SERIAL] is #NUM! — the band exit also BOUNDS the step loop against a huge `days`.
pub(crate) fn workday_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let start = match date_serial_arg(ctx, &args[0]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let days = match date_int_arg(ctx, &args[1]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let holidays = match opt_holidays(ctx, args, 2) {
        Ok(h) => h,
        Err(k) => return Value::Error(k),
    };
    let step = if days >= 0 { 1 } else { -1 };
    let mut remaining = days.abs();
    let mut cur = start;
    while remaining > 0 {
        cur += step;
        // Every step moves one serial toward a band edge, so this exit bounds the loop even for a
        // `days` far larger than the ~2.9M-wide valid band.
        if !(1..=MAX_SERIAL).contains(&cur) {
            return Value::Error(ErrKind::Num);
        }
        let wd = weekday_sun1(cur);
        if wd == 1 || wd == 7 || holidays.contains(&cur) {
            continue;
        }
        remaining -= 1;
    }
    Value::Number(cur as f64)
}

/// `NETWORKDAYS(start_date, end_date, [holidays])` — the count of working days (Mon–Fri, excluding any
/// `holidays`) in the inclusive interval between the two serials. Negative when `end_date` precedes
/// `start_date` (Excel's sign convention).
pub(crate) fn networkdays_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let s = match date_serial_arg(ctx, &args[0]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let e = match date_serial_arg(ctx, &args[1]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let holidays = match opt_holidays(ctx, args, 2) {
        Ok(h) => h,
        Err(k) => return Value::Error(k),
    };
    let (lo, hi, sign) = if s <= e { (s, e, 1) } else { (e, s, -1) };
    let mut count = 0i64;
    for cur in lo..=hi {
        let wd = weekday_sun1(cur);
        if wd == 1 || wd == 7 || holidays.contains(&cur) {
            continue;
        }
        count += 1;
    }
    Value::Number((sign * count) as f64)
}

/// Resolve an optional integer argument at `idx` (truncated toward zero), or `default` when the call
/// omitted it. Shared by WEEKDAY/WEEKNUM/YEARFRAC's trailing type/basis selector.
fn opt_int_arg(ctx: &mut EvalCtx, args: &[Expr], idx: usize, default: i64) -> Result<i64, ErrKind> {
    match args.get(idx) {
        Some(e) => Ok(one_num(ctx, e)?.trunc() as i64),
        None => Ok(default),
    }
}

/// Resolve the optional `holidays` argument at `idx` into a serial set, or an empty set when omitted.
fn opt_holidays(ctx: &mut EvalCtx, args: &[Expr], idx: usize) -> Result<HashSet<i64>, ErrKind> {
    match args.get(idx) {
        Some(e) => gather_holidays(ctx, e),
        None => Ok(HashSet::new()),
    }
}

/// Evaluate a serial argument to its time-of-day in whole seconds, `[0, 86400)`. The fractional part
/// of the serial is the time of day, rounded to the nearest second (so a value a hair under the next
/// day rolls to 0). The serial is gated to Excel's valid datetime band `[0, MAX_SERIAL + 1)` — a
/// negative serial OR one at/beyond the day after 9999-12-31 (a date Excel cannot represent) is #NUM!,
/// so HOUR/MINUTE/SECOND agree with the date readers' upper edge instead of silently reading a time
/// off an out-of-band serial.
fn time_of_day_seconds(ctx: &mut EvalCtx, e: &Expr) -> Result<i64, ErrKind> {
    let n = one_num(ctx, e)?;
    if !(0.0..(MAX_SERIAL + 1) as f64).contains(&n) {
        return Err(ErrKind::Num);
    }
    let frac = n - n.floor();
    Ok(((frac * 86_400.0).round() as i64).rem_euclid(86_400))
}

/// `HOUR(serial)` — the hour (0–23) of a serial's time-of-day fraction.
pub(crate) fn hour_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match time_of_day_seconds(ctx, &args[0]) {
        Ok(secs) => Value::Number((secs / 3600) as f64),
        Err(k) => Value::Error(k),
    }
}

/// `MINUTE(serial)` — the minute (0–59) of a serial's time-of-day fraction.
pub(crate) fn minute_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match time_of_day_seconds(ctx, &args[0]) {
        Ok(secs) => Value::Number((secs / 60 % 60) as f64),
        Err(k) => Value::Error(k),
    }
}

/// `SECOND(serial)` — the second (0–59) of a serial's time-of-day fraction.
pub(crate) fn second_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match time_of_day_seconds(ctx, &args[0]) {
        Ok(secs) => Value::Number((secs % 60) as f64),
        Err(k) => Value::Error(k),
    }
}

/// `TIME(hour, minute, second)` — the day fraction for a time of day. Each component truncates toward
/// zero and must lie in Excel's accepted `0..=32767` band (a value outside it is #NUM!); the total then
/// rolls over a 24-hour day (`TIME(25,0,0)` = `TIME(1,0,0)`).
pub(crate) fn time_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let h = match time_component_arg(ctx, &args[0]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let m = match time_component_arg(ctx, &args[1]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let s = match time_component_arg(ctx, &args[2]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    // Each component is in 0..=32767, so this sum cannot overflow i64.
    let total = h * 3600 + m * 60 + s;
    Value::Number(total.rem_euclid(86_400) as f64 / 86_400.0)
}

/// Evaluate a TIME hour/minute/second argument: truncate toward zero (Excel) and gate the accepted
/// `0..=32767` band — a component outside it (negative, or larger than Excel's per-field cap) is #NUM!.
/// A non-coercible value is #VALUE!; an error propagates.
fn time_component_arg(ctx: &mut EvalCtx, e: &Expr) -> Result<i64, ErrKind> {
    let n = one_num(ctx, e)?.trunc();
    if !(0.0..=32767.0).contains(&n) {
        return Err(ErrKind::Num);
    }
    Ok(n as i64)
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
