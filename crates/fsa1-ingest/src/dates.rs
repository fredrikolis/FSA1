// Concern: parses ISO-8601 date, date-time and duration text into an Excel serial | Non-concern: the calendar map (fsa1-ast), timezone offsets | IO: (&str) -> Option<f64>

use fsa1_ast::serial_from_ymd;

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

fn iso_date_serial(s: &str) -> Option<i64> {
    let mut parts = s.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(serial_from_ymd(y, m, d))
}

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

pub fn iso_duration_to_serial(s: &str) -> Option<f64> {
    let body = s.strip_prefix('P')?;
    let (date_desig, time_desig) = match body.split_once('T') {
        Some((d, t)) => (d, t),
        None => (body, ""),
    };
    let mut total_days = 0.0;
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
    if num.is_empty() { Some(out) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_date_maps_to_the_excel_serial() {
        assert_eq!(iso_datetime_to_serial("2024-01-15"), Some(45306.0));
        assert_eq!(iso_datetime_to_serial("1900-03-01"), Some(61.0));
        assert_eq!(iso_datetime_to_serial("1970-01-01"), Some(25569.0));
    }

    #[test]
    fn iso_datetime_adds_the_time_fraction() {
        assert_eq!(iso_datetime_to_serial("2024-01-15T12:00:00"), Some(45306.5));
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
        assert_eq!(iso_datetime_to_serial("2024-13-01"), None);
        assert_eq!(iso_datetime_to_serial("2024-01-32"), None);
        assert_eq!(iso_datetime_to_serial("2024-01-15T10:xx"), None);
        assert_eq!(iso_duration_to_serial("P1Y"), None);
        assert_eq!(iso_duration_to_serial("PT10X"), None);
    }
}
