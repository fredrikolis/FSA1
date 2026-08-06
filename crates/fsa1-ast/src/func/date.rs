// Concern: the date/time built-ins and the 1900 serial<->date maps | Non-concern: rendering a date as text, the clock seam | IO: (&mut EvalCtx, &[Expr]) -> Value
use super::*;
use std::collections::HashSet;

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
        // Every caller normalizes the month first, so an impossible one must be a located panic rather than a plausible-but-wrong 30.
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

/// The ONE home of the 1900 leap-bug policy: a date on/after 1900-03-01 shifts `+1`, an earlier one
/// passes straight through. The inverse of `serial_to_ymd` for every real civil date — the phantom
/// serial 60 has no civil pre-image. The result may fall outside the valid band for a pre-epoch
/// input, so callers gate the range.
pub fn serial_from_ymd(y: i64, m: u32, d: u32) -> i64 {
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
    // Day roll-over is just arithmetic in the contiguous serial space, which includes the phantom serial 60.
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

pub(crate) fn year_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match date_serial_arg(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok(s) => Value::Number(serial_to_ymd(s).0 as f64),
    }
}

pub(crate) fn month_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match date_serial_arg(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok(s) => Value::Number(serial_to_ymd(s).1 as f64),
    }
}

pub(crate) fn day_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match date_serial_arg(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok(s) => Value::Number(serial_to_ymd(s).2 as f64),
    }
}

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
        // The borrow is computed in i64 so the known `MD` corner cannot underflow-panic.
        "MD" => {
            if d2 >= d1 {
                (d2 - d1) as i64
            } else {
                let (py, pm) = if m2 == 1 { (y2 - 1, 12) } else { (y2, m2 - 1) };
                days_in_month(py, pm) as i64 - d1 as i64 + d2 as i64
            }
        }
        "YM" => {
            let mut mo = m2 as i64 - m1 as i64;
            if d2 < d1 {
                mo -= 1;
            }
            mo.rem_euclid(12)
        }
        // The end's month/day is re-homed into the start's year (or the next) before differencing.
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

pub(crate) fn isoweeknum_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match date_serial_arg(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok(s) => Value::Number(iso_weeknum(s) as f64),
    }
}

pub(crate) fn days360_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let s = match date_serial_arg(ctx, &args[0]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let e = match date_serial_arg(ctx, &args[1]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let european = match opt_bool(ctx, args, 2, false) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let (y1, m1, d1r) = serial_to_ymd(s);
    let (y2, m2, d2r) = serial_to_ymd(e);
    let (mut d1, mut d2) = (d1r as i64, d2r as i64);
    if european {
        // 30E/360: a day of 31 clamps to 30 on BOTH endpoints, with no February special case.
        if d1 == 31 {
            d1 = 30;
        }
        if d2 == 31 {
            d2 = 30;
        }
    } else {
        // The last-day-of-February rule applies to the START only here — a deliberate divergence from `yf_30_360`, which rewrites both February month-ends.
        if d1 == 31 || (m1 == 2 && d1r == days_in_month(y1, m1)) {
            d1 = 30;
        }
        if d2 == 31 && d1 == 30 {
            d2 = 30;
        }
    }
    let days = 360 * (y2 - y1) + 30 * (m2 as i64 - m1 as i64) + (d2 - d1);
    Value::Number(days as f64)
}

/// The weekday index Mon=0..Sun=6 of a date serial — the indexing the `.INTL` weekend mask/code use
/// (a 7-slot `[bool; 7]` keyed Monday-first, matching Excel's string mask and numeric-code layout).
fn weekday_mon0(serial: i64) -> usize {
    (weekday_sun1(serial) + 5).rem_euclid(7) as usize
}

/// The non-working weekdays, indexed Mon=0..Sun=6. A TEXT argument is a 7-character `0`/`1` mask
/// (Mon first, `1` = non-working); a numeric one is a weekend CODE.
fn opt_weekend(
    ctx: &mut EvalCtx,
    args: &[Expr],
    idx: usize,
    default_code: i64,
) -> Result<[bool; 7], ErrKind> {
    match args.get(idx) {
        None => weekend_from_code(default_code),
        Some(e) => match scalarize(ctx.eval(e)) {
            Value::Text(s) => weekend_from_mask(&s),
            Value::Error(k) => Err(k),
            other => weekend_from_code(coerce_num(&other)?.trunc() as i64),
        },
    }
}

/// A `.INTL` numeric weekend code → its non-working-day set (Mon=0..Sun=6). Codes 1–7 mark a
/// consecutive two-day weekend (1 = Sat+Sun, 2 = Sun+Mon, … 7 = Fri+Sat); codes 11–17 mark a single
/// weekend day (11 = Sun, 12 = Mon, … 17 = Sat). Any other code is #NUM!.
fn weekend_from_code(code: i64) -> Result<[bool; 7], ErrKind> {
    let mut w = [false; 7];
    match code {
        1..=7 => {
            w[(code + 4).rem_euclid(7) as usize] = true;
            w[(code + 5).rem_euclid(7) as usize] = true;
        }
        11..=17 => {
            w[(code - 12).rem_euclid(7) as usize] = true;
        }
        _ => return Err(ErrKind::Num),
    }
    Ok(w)
}

/// A `.INTL` weekend string mask → its non-working-day set (Mon=0..Sun=6). The mask must be exactly
/// seven characters, each `0` (working) or `1` (non-working), and cannot be all `1`s (a week with no
/// working day is rejected). Any other shape/character is #VALUE!.
fn weekend_from_mask(s: &str) -> Result<[bool; 7], ErrKind> {
    let bytes = s.as_bytes();
    if bytes.len() != 7 {
        return Err(ErrKind::Value);
    }
    let mut w = [false; 7];
    let mut any_working = false;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'0' => any_working = true,
            b'1' => w[i] = true,
            _ => return Err(ErrKind::Value),
        }
    }
    if any_working {
        Ok(w)
    } else {
        Err(ErrKind::Value)
    }
}

pub(crate) fn networkdays_intl_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let s = match date_serial_arg(ctx, &args[0]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let e = match date_serial_arg(ctx, &args[1]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let weekend = match opt_weekend(ctx, args, 2, 1) {
        Ok(w) => w,
        Err(k) => return Value::Error(k),
    };
    let holidays = match opt_holidays(ctx, args, 3) {
        Ok(h) => h,
        Err(k) => return Value::Error(k),
    };
    let (lo, hi, sign) = if s <= e { (s, e, 1) } else { (e, s, -1) };
    let mut count = 0i64;
    for cur in lo..=hi {
        if weekend[weekday_mon0(cur)] || holidays.contains(&cur) {
            continue;
        }
        count += 1;
    }
    Value::Number((sign * count) as f64)
}

pub(crate) fn workday_intl_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let start = match date_serial_arg(ctx, &args[0]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let days = match date_int_arg(ctx, &args[1]) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let weekend = match opt_weekend(ctx, args, 2, 1) {
        Ok(w) => w,
        Err(k) => return Value::Error(k),
    };
    let holidays = match opt_holidays(ctx, args, 3) {
        Ok(h) => h,
        Err(k) => return Value::Error(k),
    };
    let step = if days >= 0 { 1 } else { -1 };
    let mut remaining = days.abs();
    let mut cur = start;
    while remaining > 0 {
        cur += step;
        if !(1..=MAX_SERIAL).contains(&cur) {
            return Value::Error(ErrKind::Num);
        }
        if weekend[weekday_mon0(cur)] || holidays.contains(&cur) {
            continue;
        }
        remaining -= 1;
    }
    Value::Number(cur as f64)
}

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
        // The start day must clamp BEFORE the end day is tested, or a 31-to-31 span reduces only one endpoint.
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
        // Every step moves one serial toward a band edge, so this exit bounds the loop for ANY `days`.
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

/// Whole seconds in `[0, 86400)`, rounded to the nearest second so a value a hair under the next day
/// rolls to 0. Gated to the same datetime band the date readers use, rather than reading a time off
/// an out-of-band serial.
fn time_of_day_seconds(ctx: &mut EvalCtx, e: &Expr) -> Result<i64, ErrKind> {
    let n = one_num(ctx, e)?;
    if !(0.0..(MAX_SERIAL + 1) as f64).contains(&n) {
        return Err(ErrKind::Num);
    }
    let frac = n - n.floor();
    Ok(((frac * 86_400.0).round() as i64).rem_euclid(86_400))
}

pub(crate) fn hour_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match time_of_day_seconds(ctx, &args[0]) {
        Ok(secs) => Value::Number((secs / 3600) as f64),
        Err(k) => Value::Error(k),
    }
}

pub(crate) fn minute_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match time_of_day_seconds(ctx, &args[0]) {
        Ok(secs) => Value::Number((secs / 60 % 60) as f64),
        Err(k) => Value::Error(k),
    }
}

pub(crate) fn second_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match time_of_day_seconds(ctx, &args[0]) {
        Ok(secs) => Value::Number((secs % 60) as f64),
        Err(k) => Value::Error(k),
    }
}

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

pub(crate) fn datevalue_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match scalarize(ctx.eval(&args[0])) {
        Value::Text(t) => match datevalue_serial(t.trim()) {
            Some(serial) => Value::Number(serial as f64),
            None => Value::Error(ErrKind::Value),
        },
        Value::Error(k) => Value::Error(k),
        _ => Value::Error(ErrKind::Value),
    }
}

/// Parse DATEVALUE's accepted text to an integer date serial: a bare `yyyy-mm-dd`, or a `yyyy-mm-dd`
/// followed by an `hh:mm[:ss]` clock (the clock is validated but discarded). `None` for a bare clock
/// time (no date) or any other shape.
fn datevalue_serial(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    match parts.as_slice() {
        [date] => parse_iso_date(date),
        [date, time] => {
            let day = parse_iso_date(date)?;
            parse_clock(time)?;
            Some(day)
        }
        _ => None,
    }
}

pub(crate) fn timevalue_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match scalarize(ctx.eval(&args[0])) {
        Value::Text(t) => match timevalue_frac(t.trim()) {
            Some(frac) => Value::Number(frac),
            None => Value::Error(ErrKind::Value),
        },
        Value::Error(k) => Value::Error(k),
        _ => Value::Error(ErrKind::Value),
    }
}

/// Parse TIMEVALUE's accepted text to a day fraction: a bare `hh:mm[:ss]` clock, a bare `yyyy-mm-dd`
/// date (whose time-of-day is `0`), or a `yyyy-mm-dd hh:mm[:ss]` pair (the date is validated but its
/// serial discarded — only the clock fraction is returned). `None` for any other shape.
fn timevalue_frac(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    match parts.as_slice() {
        [one] => parse_clock(one).or_else(|| parse_iso_date(one).map(|_| 0.0)),
        [date, time] => {
            parse_iso_date(date)?;
            parse_clock(time)
        }
        _ => None,
    }
}

pub(crate) fn today_fn(ctx: &mut EvalCtx, _args: &[Expr]) -> Value {
    finite_or_num(ctx.now_serial().floor())
}

pub(crate) fn now_fn(ctx: &mut EvalCtx, _args: &[Expr]) -> Value {
    finite_or_num(ctx.now_serial())
}
