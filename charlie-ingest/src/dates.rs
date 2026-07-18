// Concern: convert a source format's date/time text into charlie's Excel date SERIAL (ENG6) — parse an ISO-8601 date (`2024-01-15`) or date-time (`2024-01-15T10:30:00`) into a whole-day serial (via charlie-ast's single-homed `serial_from_ymd`, so the 1900 leap-bug policy is never re-derived) plus a time-of-day fraction, and an ISO-8601 duration (`PT10H30M`) into a bare day fraction; a non-date/time/duration string is `None` (the reader turns that into a located refusal, CORE2) | Non-concern: where the string came from (reader.rs owns calamine's `DateTimeIso`/`DurationIso`) and the serial arithmetic itself (charlie-ast owns `serial_from_ymd`) | IO: (an ISO date/time/duration `&str`) -> `Option<f64>` serial
//! ISO date/time/duration → Excel serial: [`iso_datetime_to_serial`], [`iso_duration_to_serial`].

use charlie_ast::serial_from_ymd;

/// Parse an ISO-8601 date or date-time into an Excel date serial. Accepts `YYYY-MM-DD` (a whole-day
/// serial) and `YYYY-MM-DDThh:mm[:ss[.fff]]` (the day serial plus the time-of-day fraction). Returns
/// `None` for anything else — the caller makes that a located refusal rather than a silent wrong value.
pub fn iso_datetime_to_serial(s: &str) -> Option<f64> {
    let (date_part, time_part) = match s.split_once(['T', ' ']) {
        Some((d, t)) => (d, Some(t)),
        None => (s, None),
    };
    let day = iso_date_serial(date_part)?;
    let frac = match time_part {
        None => 0.0,
        Some(t) => time_fraction(t)?,
    };
    Some(day as f64 + frac)
}

/// Parse a `YYYY-MM-DD` date into a whole-day Excel serial via charlie-ast's serial map.
fn iso_date_serial(s: &str) -> Option<i64> {
    let mut parts = s.split('-');
    // A leading '-' would make the first split field empty; a proleptic negative year is out of the
    // Excel band anyway, so reject it here rather than mis-parse.
    let y: i64 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(serial_from_ymd(y, m, d))
}

/// Parse a `hh:mm[:ss[.fff]]` clock time into a fraction of a day in `[0, 1)`. A trailing `Z` or numeric
/// timezone offset is not modelled (a spreadsheet time is wall-clock); such a suffix makes the seconds
/// field non-numeric and yields `None`.
fn time_fraction(t: &str) -> Option<f64> {
    let mut parts = t.split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let s: f64 = match parts.next() {
        Some(sec) => sec.parse().ok()?,
        None => 0.0,
    };
    if parts.next().is_some() {
        return None;
    }
    Some((h * 3600.0 + m * 60.0 + s) / 86_400.0)
}

/// Parse an ISO-8601 duration (`P[nD]T[nH][nM][nS]`, e.g. `PT10H30M`, `P1DT6H`) into a bare Excel serial
/// — a duration has no date, so it is a day count plus the time-of-day fraction (`PT12H` → `0.5`). Only
/// the day/hour/minute/second designators are modelled (a spreadsheet duration is a clock span); a
/// year/month/week designator or any malformed token yields `None`.
pub fn iso_duration_to_serial(s: &str) -> Option<f64> {
    let body = s.strip_prefix('P')?;
    let (date_desig, time_desig) = match body.split_once('T') {
        Some((d, t)) => (d, t),
        None => (body, ""),
    };
    let mut total_days = 0.0;
    // The date side of a spreadsheet duration only carries days (D). Weeks/months/years are not a
    // fixed span of days, so they are unsupported (None), never guessed.
    for (value, unit) in scan_designators(date_desig)? {
        match unit {
            'D' => total_days += value,
            _ => return None,
        }
    }
    for (value, unit) in scan_designators(time_desig)? {
        match unit {
            'H' => total_days += value / 24.0,
            'M' => total_days += value / 1440.0,
            'S' => total_days += value / 86_400.0,
            _ => return None,
        }
    }
    Some(total_days)
}

/// Split an ISO-duration segment (`10H30M`) into `(value, designator)` pairs. `None` if a run of digits
/// is not terminated by a letter, or a value does not parse.
fn scan_designators(seg: &str) -> Option<Vec<(f64, char)>> {
    let mut out = Vec::new();
    let mut num = String::new();
    for ch in seg.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            num.push(ch);
        } else {
            if num.is_empty() {
                return None;
            }
            out.push((num.parse().ok()?, ch));
            num.clear();
        }
    }
    // A trailing run of digits with no designator is malformed.
    if num.is_empty() { Some(out) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_date_maps_to_the_excel_serial() {
        // 2024-01-15 is Excel serial 45306 (1900 system, leap-bug replicated by charlie-ast).
        assert_eq!(iso_datetime_to_serial("2024-01-15"), Some(45306.0));
        // The 1900 leap-bug anchor: 1900-03-01 is serial 61.
        assert_eq!(iso_datetime_to_serial("1900-03-01"), Some(61.0));
        // 1970-01-01 is serial 25569 (the unix epoch serial).
        assert_eq!(iso_datetime_to_serial("1970-01-01"), Some(25569.0));
    }

    #[test]
    fn iso_datetime_adds_the_time_fraction() {
        // Noon is +0.5 of a day.
        assert_eq!(iso_datetime_to_serial("2024-01-15T12:00:00"), Some(45306.5));
        // 06:00 is +0.25.
        assert_eq!(iso_datetime_to_serial("2024-01-15T06:00"), Some(45306.25));
    }

    #[test]
    fn iso_duration_is_a_bare_serial() {
        assert_eq!(iso_duration_to_serial("PT12H"), Some(0.5));
        assert_eq!(iso_duration_to_serial("PT10H30M"), Some(10.5 / 24.0));
        assert_eq!(iso_duration_to_serial("P1DT6H"), Some(1.25));
    }

    #[test]
    fn non_dates_are_none_never_a_wrong_value() {
        assert_eq!(iso_datetime_to_serial("not-a-date"), None);
        assert_eq!(iso_datetime_to_serial("2024-13-01"), None); // month 13
        assert_eq!(iso_datetime_to_serial("2024-01-32"), None); // day 32
        assert_eq!(iso_datetime_to_serial("2024-01-15T10:xx"), None); // non-numeric minutes
        assert_eq!(iso_duration_to_serial("P1Y"), None); // years unsupported
        assert_eq!(iso_duration_to_serial("PT10X"), None); // bad designator
    }
}
