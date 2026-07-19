// Concern: UNIT-TEST pins for the date/time family built-ins (DATE YEAR MONTH DAY EDATE EOMONTH DATEDIF DAYS DAYS360 WEEKDAY WEEKNUM ISOWEEKNUM WORKDAY WORKDAY.INTL NETWORKDAYS NETWORKDAYS.INTL YEARFRAC HOUR MINUTE SECOND TIME DATEVALUE TIMEVALUE TODAY NOW) exercised through `FUNCS` dispatch — serial construction/normalization with the replicated 1900 leap-year bug, end-of-month clamping, DATEDIF unit folding, day-count (incl. the 360-day DAYS360 US/European rules) / weekday / working-day (incl. `.INTL` custom weekends) / year-fraction / time-of-day arithmetic, the text->serial DATEVALUE/TIMEVALUE readers, the pinned-clock volatiles, and the registry-wide volatility invariant for TODAY/NOW | Non-concern: the date impls (`func/date.rs`) and the shared test fixtures (the parent `tests` module owns `num`/`call`/`arr`) | IO: in-memory `Grid` fixtures (with a pinned clock) + literal `Expr`s -> asserted `Value`s
use super::*;

#[test]
fn date_builds_and_normalizes_a_serial() {
    let g = Grid::new(1, vec![Value::Blank]);
    // A plain in-range date (44927 = 2023-01-01, cross-checked against the TEXT date anchor).
    assert_eq!(
        eval(&call("DATE", vec![num(2023.0), num(1.0), num(1.0)]), &g),
        Value::Number(44927.0)
    );
    // Month roll-over: DATE(2008,14,2) = 2009-02-02 (independently 39846).
    assert_eq!(
        eval(&call("DATE", vec![num(2008.0), num(14.0), num(2.0)]), &g),
        Value::Number(39846.0)
    );
    // Day 0 rolls back to the last day of the previous month: DATE(2023,3,0) = 2023-02-28 (44985).
    assert_eq!(
        eval(&call("DATE", vec![num(2023.0), num(3.0), num(0.0)]), &g),
        Value::Number(44985.0)
    );
    // The two-digit year rule folds 0..=1899 by +1900: DATE(108,1,2) = 2008-01-02 (39449).
    assert_eq!(
        eval(&call("DATE", vec![num(108.0), num(1.0), num(2.0)]), &g),
        Value::Number(39449.0)
    );
    // A year past 9999 is #NUM!.
    assert_eq!(
        eval(&call("DATE", vec![num(10000.0), num(1.0), num(1.0)]), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn year_month_day_read_a_serial_with_the_leap_bug() {
    let g = Grid::new(1, vec![Value::Blank]);
    // 44927 = 2023-01-01.
    assert_eq!(
        eval(&call("YEAR", vec![num(44927.0)]), &g),
        Value::Number(2023.0)
    );
    assert_eq!(
        eval(&call("MONTH", vec![num(44927.0)]), &g),
        Value::Number(1.0)
    );
    assert_eq!(
        eval(&call("DAY", vec![num(44957.0)]), &g),
        Value::Number(31.0) // 2023-01-31
    );
    // The replicated leap-year bug: serial 60 is the fictional 1900-02-29.
    assert_eq!(
        eval(&call("YEAR", vec![num(60.0)]), &g),
        Value::Number(1900.0)
    );
    assert_eq!(
        eval(&call("MONTH", vec![num(60.0)]), &g),
        Value::Number(2.0)
    );
    assert_eq!(eval(&call("DAY", vec![num(60.0)]), &g), Value::Number(29.0));
    // A serial before the epoch (< 1) is out of the supported domain → #NUM!.
    assert_eq!(
        eval(&call("YEAR", vec![num(0.0)]), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn edate_clamps_to_end_of_month() {
    let g = Grid::new(1, vec![Value::Blank]);
    // One month forward from 2023-01-01 (44927) = 2023-02-01 (44958).
    assert_eq!(
        eval(&call("EDATE", vec![num(44927.0), num(1.0)]), &g),
        Value::Number(44958.0)
    );
    // Clamp: one month from 2020-01-31 (43861) lands on 2020-02-29 (43890, a leap February).
    assert_eq!(
        eval(&call("EDATE", vec![num(43861.0), num(1.0)]), &g),
        Value::Number(43890.0)
    );
    // Negative months go back: two months before 2023-01-01 = 2022-11-01 (44866).
    assert_eq!(
        eval(&call("EDATE", vec![num(44927.0), num(-2.0)]), &g),
        Value::Number(44866.0)
    );
    // A non-numeric start is #VALUE!.
    assert_eq!(
        eval(
            &call("EDATE", vec![Expr::Lit(Value::Text("x".into())), num(1.0)]),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn datedif_units() {
    let g = Grid::new(1, vec![Value::Blank]);
    let dd = |a: f64, b: f64, u: &str| {
        call(
            "DATEDIF",
            vec![num(a), num(b), Expr::Lit(Value::Text(u.into()))],
        )
    };
    // Whole days.
    assert_eq!(eval(&dd(44927.0, 44957.0, "D"), &g), Value::Number(30.0));
    // Complete years / months between 2020-01-01 (43831) and 2023-06-01 (45078).
    assert_eq!(eval(&dd(43831.0, 45078.0, "Y"), &g), Value::Number(3.0));
    assert_eq!(eval(&dd(43831.0, 45078.0, "M"), &g), Value::Number(41.0));
    // MD: 2020-01-15 (43845) → 2020-03-20 (43910), day remainder = 5.
    assert_eq!(eval(&dd(43845.0, 43910.0, "MD"), &g), Value::Number(5.0));
    // YM: 2020-01-15 → 2023-06-20 (45097), month remainder = 5.
    assert_eq!(eval(&dd(43845.0, 45097.0, "YM"), &g), Value::Number(5.0));
    // YD: 2020-01-15 → 2023-03-20 (45005), day-of-year remainder = 65.
    assert_eq!(eval(&dd(43845.0, 45005.0, "YD"), &g), Value::Number(65.0));
    // The unit folds case.
    assert_eq!(eval(&dd(44927.0, 44957.0, "d"), &g), Value::Number(30.0));
    // start > end is #NUM!.
    assert_eq!(
        eval(&dd(44957.0, 44927.0, "D"), &g),
        Value::Error(ErrKind::Num)
    );
    // An unknown unit is #NUM!.
    assert_eq!(
        eval(&dd(44927.0, 44957.0, "Q"), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn eomonth_and_days() {
    let g = Grid::new(1, vec![Value::Blank]);
    // Last day of Feb 2020 (leap) = 2020-02-29 (serial 43890).
    assert_eq!(
        eval(
            &call(
                "EOMONTH",
                vec![
                    call("DATE", vec![num(2020.0), num(2.0), num(15.0)]),
                    num(0.0)
                ]
            ),
            &g
        ),
        Value::Number(43890.0)
    );
    // One month back from mid-March 2020 → end of Feb 2020.
    assert_eq!(
        eval(
            &call(
                "EOMONTH",
                vec![
                    call("DATE", vec![num(2020.0), num(3.0), num(15.0)]),
                    num(-1.0)
                ]
            ),
            &g
        ),
        Value::Number(43890.0)
    );
    // DAYS is end-first: 2020-01-31 minus 2020-01-01 = 30.
    assert_eq!(
        eval(&call("DAYS", vec![num(43861.0), num(43831.0)]), &g),
        Value::Number(30.0)
    );
    assert_eq!(
        eval(&call("DAYS", vec![num(43831.0), num(43861.0)]), &g),
        Value::Number(-30.0)
    );
}

#[test]
fn weekday_and_weeknum() {
    let g = Grid::new(1, vec![Value::Blank]);
    // 2020-01-01 (serial 43831) is a Wednesday: default type 1 (Sun=1..Sat=7) → 4.
    assert_eq!(
        eval(&call("WEEKDAY", vec![num(43831.0)]), &g),
        Value::Number(4.0)
    );
    // Type 2 (Mon=1..Sun=7) → 3; type 3 (Mon=0..Sun=6) → 2.
    assert_eq!(
        eval(&call("WEEKDAY", vec![num(43831.0), num(2.0)]), &g),
        Value::Number(3.0)
    );
    assert_eq!(
        eval(&call("WEEKDAY", vec![num(43831.0), num(3.0)]), &g),
        Value::Number(2.0)
    );
    // An unsupported return type is #NUM!.
    assert_eq!(
        eval(&call("WEEKDAY", vec![num(43831.0), num(4.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    // WEEKNUM default (Sunday weeks): 2023-01-01 (Sunday, serial 44927) = week 1; 2023-01-08 = week 2.
    assert_eq!(
        eval(&call("WEEKNUM", vec![num(44927.0)]), &g),
        Value::Number(1.0)
    );
    assert_eq!(
        eval(&call("WEEKNUM", vec![num(44934.0)]), &g),
        Value::Number(2.0)
    );
    // ISO week (type 21): 2023-01-01 belongs to ISO week 52 of 2022.
    assert_eq!(
        eval(&call("WEEKNUM", vec![num(44927.0), num(21.0)]), &g),
        Value::Number(52.0)
    );
}

#[test]
fn workday_and_networkdays() {
    let g = Grid::new(1, vec![Value::Blank]);
    // 2020-01-01 is a Wednesday; +3 workdays skips Sat/Sun → 2020-01-06 (serial 43836).
    assert_eq!(
        eval(&call("WORKDAY", vec![num(43831.0), num(3.0)]), &g),
        Value::Number(43836.0)
    );
    // A holiday on the Thursday pushes the third workday out by one → 2020-01-07 (serial 43837).
    assert_eq!(
        eval(
            &call(
                "WORKDAY",
                vec![num(43831.0), num(3.0), arr(1, 1, vec![n(43832.0)])]
            ),
            &g
        ),
        Value::Number(43837.0)
    );
    // NETWORKDAYS over 2020-01-01..2020-01-07 (Wed..Tue) = 5 working days.
    assert_eq!(
        eval(&call("NETWORKDAYS", vec![num(43831.0), num(43837.0)]), &g),
        Value::Number(5.0)
    );
    // A holiday inside the range drops the count to 4; reversed endpoints negate.
    assert_eq!(
        eval(
            &call(
                "NETWORKDAYS",
                vec![num(43831.0), num(43837.0), arr(1, 1, vec![n(43832.0)])]
            ),
            &g
        ),
        Value::Number(4.0)
    );
    assert_eq!(
        eval(&call("NETWORKDAYS", vec![num(43837.0), num(43831.0)]), &g),
        Value::Number(-5.0)
    );
}

#[test]
fn yearfrac_bases() {
    let g = Grid::new(1, vec![Value::Blank]);
    let yf = |s: f64, e: f64, b: Option<f64>| {
        let mut a = vec![num(s), num(e)];
        if let Some(basis) = b {
            a.push(num(basis));
        }
        call("YEARFRAC", a)
    };
    // 2020-01-01 (43831) → 2021-01-01 (44197): basis 0 (30/360) and basis 1 (act/act) are both 1.0.
    assert_eq!(eval(&yf(43831.0, 44197.0, None), &g), Value::Number(1.0));
    assert_eq!(
        eval(&yf(43831.0, 44197.0, Some(1.0)), &g),
        Value::Number(1.0)
    );
    // Basis 2 (actual/360): 366 actual days / 360.
    assert_eq!(
        eval(&yf(43831.0, 44197.0, Some(2.0)), &g),
        Value::Number(366.0 / 360.0)
    );
    // Basis 3 (actual/365): 366 / 365.
    assert_eq!(
        eval(&yf(43831.0, 44197.0, Some(3.0)), &g),
        Value::Number(366.0 / 365.0)
    );
    // Symmetric in its endpoints.
    assert_eq!(
        eval(&yf(44197.0, 43831.0, Some(0.0)), &g),
        Value::Number(1.0)
    );
    // --- 30/360 day-count clamps over month-end endpoints (Excel-verified) ---
    let d = |y: f64, m: f64, dd: f64| call("DATE", vec![num(y), num(m), num(dd)]);
    let yfd = |a: Expr, b: Expr, basis: f64| call("YEARFRAC", vec![a, b, num(basis)]);
    // Basis 0 (NASD 30/360), 31 -> 31: both endpoints clamp 31 -> 30, so the span is exactly a whole
    // number of 30-day months. Regression pin for the clamp-order bug — before the fix D1 stayed 31,
    // leaving one extra day. Jan-31 -> Jul-31 is 6 months -> 0.5; Jan-31 -> Mar-31 is 2 months -> 1/6.
    assert_eq!(
        eval(&yfd(d(2020.0, 1.0, 31.0), d(2020.0, 7.0, 31.0), 0.0), &g),
        Value::Number(0.5)
    );
    assert_eq!(
        eval(&yfd(d(2020.0, 1.0, 31.0), d(2020.0, 3.0, 31.0), 0.0), &g),
        Value::Number(60.0 / 360.0)
    );
    // Basis 0 end-of-February rule: both endpoints last-of-Feb -> D1=30 and D2=30, an exact year.
    assert_eq!(
        eval(&yfd(d(2020.0, 2.0, 29.0), d(2021.0, 2.0, 28.0), 0.0), &g),
        Value::Number(1.0)
    );
    // Basis 4 (30E/360): only 31 -> 30, never the Feb-EOM rule. 31 -> 31 matches basis 0 at 0.5, but
    // last-Feb -> last-Feb leaves D1=29, D2=28 (no EOM clamp) -> 359/360, distinct from basis 0's 1.0.
    assert_eq!(
        eval(&yfd(d(2020.0, 1.0, 31.0), d(2020.0, 7.0, 31.0), 4.0), &g),
        Value::Number(0.5)
    );
    assert_eq!(
        eval(&yfd(d(2020.0, 2.0, 29.0), d(2021.0, 2.0, 28.0), 4.0), &g),
        Value::Number(359.0 / 360.0)
    );
    // An unsupported basis is #NUM!.
    assert_eq!(
        eval(&yf(43831.0, 44197.0, Some(5.0)), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn time_and_its_components() {
    let g = Grid::new(1, vec![Value::Blank]);
    // TIME(14,30,0) = 0.604166… ; HOUR/MINUTE/SECOND read it back.
    let t = call("TIME", vec![num(14.0), num(30.0), num(15.0)]);
    assert_eq!(
        eval(&call("HOUR", vec![t.clone()]), &g),
        Value::Number(14.0)
    );
    assert_eq!(
        eval(&call("MINUTE", vec![t.clone()]), &g),
        Value::Number(30.0)
    );
    assert_eq!(eval(&call("SECOND", vec![t]), &g), Value::Number(15.0));
    // TIME rolls over a 24-hour day: TIME(25,0,0) = TIME(1,0,0) = 1/24.
    assert_eq!(
        eval(&call("TIME", vec![num(25.0), num(0.0), num(0.0)]), &g),
        Value::Number(1.0 / 24.0)
    );
    // Each component is gated to Excel's 0..=32767 band. The top of the band is accepted (and rolls
    // over the day), a component past it is #NUM!, and a negative component is #NUM!.
    assert_eq!(
        eval(&call("TIME", vec![num(32767.0), num(0.0), num(0.0)]), &g),
        Value::Number((32767 * 3600 % 86_400) as f64 / 86_400.0)
    );
    assert_eq!(
        eval(&call("TIME", vec![num(40000.0), num(0.0), num(0.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("TIME", vec![num(0.0), num(-1.0), num(0.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    // Noon = 0.5 → HOUR 12; a negative time serial is #NUM!.
    assert_eq!(eval(&call("HOUR", vec![num(0.5)]), &g), Value::Number(12.0));
    assert_eq!(
        eval(&call("HOUR", vec![num(-0.5)]), &g),
        Value::Error(ErrKind::Num)
    );
    // A serial's date part is ignored: 44927.5 (2023-01-01T12:00) → HOUR 12.
    assert_eq!(
        eval(&call("HOUR", vec![num(44927.5)]), &g),
        Value::Number(12.0)
    );
    // The upper edge is gated too: a serial at/beyond the day after 9999-12-31 is #NUM! (a date Excel
    // cannot represent), matching the date readers rather than reading a time off an out-of-band serial.
    assert_eq!(
        eval(&call("HOUR", vec![num((MAX_SERIAL + 1) as f64)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("MINUTE", vec![num(1e300)]), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn days360_us_and_european_methods() {
    let g = Grid::new(1, vec![Value::Blank]);
    let d = |y: f64, m: f64, dd: f64| call("DATE", vec![num(y), num(m), num(dd)]);
    let d360 = |a: Expr, b: Expr, method: Option<bool>| {
        let mut args = vec![a, b];
        if let Some(euro) = method {
            args.push(Expr::Lit(Value::Bool(euro)));
        }
        call("DAYS360", args)
    };
    // US (NASD), 31 -> 31: start Jan-31 -> 30, end Mar-31 -> 30 (start is 30). Exactly two 30-day
    // months = 60. (oracle-pinned; formulas lib agrees.)
    assert_eq!(
        eval(&d360(d(2020.0, 1.0, 31.0), d(2020.0, 3.0, 31.0), None), &g),
        Value::Number(60.0)
    );
    // US last-day-of-February start rule: Feb-29-2020 (leap, last day) -> 30, end Feb-28-2021 stays
    // 28 (not the 31st). 360 + 0 + (28 - 30) = 358. Excel applies the last-of-Feb rule to the START
    // only — distinct from YEARFRAC basis 0, which rewrites BOTH Feb month-ends (there = 1.0/360).
    assert_eq!(
        eval(&d360(d(2020.0, 2.0, 29.0), d(2021.0, 2.0, 28.0), None), &g),
        Value::Number(358.0)
    );
    // A mid-month end date is untouched: Jan-15 -> Feb-29 = 30 + (29 - 15) = 44.
    assert_eq!(
        eval(&d360(d(2020.0, 1.0, 15.0), d(2020.0, 2.0, 29.0), None), &g),
        Value::Number(44.0)
    );
    // European (30E/360): only 31 -> 30 on either endpoint, no February rule. Feb-29 stays 29, so
    // Feb-29 -> Mar-31 = 30 + (30 - 29) = 31.
    assert_eq!(
        eval(
            &d360(d(2020.0, 2.0, 29.0), d(2020.0, 3.0, 31.0), Some(true)),
            &g
        ),
        Value::Number(31.0)
    );
    // Reversed endpoints yield a negative count (no reordering).
    assert_eq!(
        eval(&d360(d(2020.0, 3.0, 31.0), d(2020.0, 1.0, 31.0), None), &g),
        Value::Number(-60.0)
    );
}

#[test]
fn isoweeknum_matches_weeknum_type_21() {
    let g = Grid::new(1, vec![Value::Blank]);
    // 2023-01-01 belongs to ISO week 52 of 2022 (oracle-pinned; == WEEKNUM(.,21)).
    assert_eq!(
        eval(&call("ISOWEEKNUM", vec![num(44927.0)]), &g),
        Value::Number(52.0)
    );
    // 2020-01-01 (Wednesday) is ISO week 1.
    assert_eq!(
        eval(&call("ISOWEEKNUM", vec![num(43831.0)]), &g),
        Value::Number(1.0)
    );
    // A serial before the epoch is out of domain -> #NUM!.
    assert_eq!(
        eval(&call("ISOWEEKNUM", vec![num(0.0)]), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn networkdays_intl_custom_weekends() {
    let g = Grid::new(1, vec![Value::Blank]);
    // 2020-01-01 (Wed) .. 2020-01-10 (Fri): default weekend (Sat/Sun) leaves 8 working days.
    assert_eq!(
        eval(
            &call("NETWORKDAYS.INTL", vec![num(43831.0), num(43840.0)]),
            &g
        ),
        Value::Number(8.0)
    );
    // Weekend code 11 (Sunday only) drops just the one Sunday in the span -> 9.
    assert_eq!(
        eval(
            &call(
                "NETWORKDAYS.INTL",
                vec![num(43831.0), num(43840.0), num(11.0)]
            ),
            &g
        ),
        Value::Number(9.0)
    );
    // A 7-char mask "0000011" (Sat+Sun non-working, Mon-first) matches the default -> 8.
    assert_eq!(
        eval(
            &call(
                "NETWORKDAYS.INTL",
                vec![
                    num(43831.0),
                    num(43840.0),
                    Expr::Lit(Value::Text("0000011".into()))
                ]
            ),
            &g
        ),
        Value::Number(8.0)
    );
    // Reversed endpoints negate the count.
    assert_eq!(
        eval(
            &call("NETWORKDAYS.INTL", vec![num(43840.0), num(43831.0)]),
            &g
        ),
        Value::Number(-8.0)
    );
    // An all-`1` mask (no working day) is #VALUE!; a wrong-length mask is #VALUE!; an unsupported
    // numeric code is #NUM!.
    assert_eq!(
        eval(
            &call(
                "NETWORKDAYS.INTL",
                vec![
                    num(43831.0),
                    num(43840.0),
                    Expr::Lit(Value::Text("1111111".into()))
                ]
            ),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(
            &call(
                "NETWORKDAYS.INTL",
                vec![
                    num(43831.0),
                    num(43840.0),
                    Expr::Lit(Value::Text("00011".into()))
                ]
            ),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(
            &call(
                "NETWORKDAYS.INTL",
                vec![num(43831.0), num(43840.0), num(8.0)]
            ),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn workday_intl_custom_weekends_and_holidays() {
    let g = Grid::new(1, vec![Value::Blank]);
    // 2020-01-01 (Wed) + 5 working days, default weekend -> 2020-01-08 (serial 43838).
    assert_eq!(
        eval(&call("WORKDAY.INTL", vec![num(43831.0), num(5.0)]), &g),
        Value::Number(43838.0)
    );
    // Weekend code 11 (Sunday only) makes Saturday a working day, so +5 lands one day earlier,
    // 2020-01-07 (serial 43837).
    assert_eq!(
        eval(
            &call("WORKDAY.INTL", vec![num(43831.0), num(5.0), num(11.0)]),
            &g
        ),
        Value::Number(43837.0)
    );
    // Negative days count backward: 2020-01-01 - 5 working days = 2019-12-25 (serial 43824).
    assert_eq!(
        eval(&call("WORKDAY.INTL", vec![num(43831.0), num(-5.0)]), &g),
        Value::Number(43824.0)
    );
    // A holiday inside the run pushes the result out by one day (mask weekend + holidays arg).
    assert_eq!(
        eval(
            &call(
                "WORKDAY.INTL",
                vec![
                    num(43831.0),
                    num(5.0),
                    Expr::Lit(Value::Text("0000011".into())),
                    arr(1, 1, vec![n(43833.0)])
                ]
            ),
            &g
        ),
        Value::Number(43839.0)
    );
    // An all-`1` mask is #VALUE!; an unsupported code is #NUM!.
    assert_eq!(
        eval(
            &call(
                "WORKDAY.INTL",
                vec![
                    num(43831.0),
                    num(5.0),
                    Expr::Lit(Value::Text("1111111".into()))
                ]
            ),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(
            &call("WORKDAY.INTL", vec![num(43831.0), num(5.0), num(8.0)]),
            &g
        ),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn datevalue_parses_date_text() {
    let g = Grid::new(1, vec![Value::Blank]);
    let dv = |s: &str| call("DATEVALUE", vec![Expr::Lit(Value::Text(s.into()))]);
    // A plain ISO date -> its integer serial (oracle-pinned).
    assert_eq!(eval(&dv("2020-01-01"), &g), Value::Number(43831.0));
    // A trailing clock time is accepted but DROPPED -> the date serial only.
    assert_eq!(eval(&dv("2020-01-01 12:00"), &g), Value::Number(43831.0));
    // A bare clock time has no date part -> #VALUE! (this is Excel's answer; the `formulas` lib
    // MISREADS a bare time as a date, so it is pinned here by hand rather than against the oracle).
    assert_eq!(eval(&dv("12:00"), &g), Value::Error(ErrKind::Value));
    // Unrecognized / empty text is #VALUE!.
    assert_eq!(eval(&dv("not a date"), &g), Value::Error(ErrKind::Value));
    assert_eq!(eval(&dv(""), &g), Value::Error(ErrKind::Value));
    // A numeric (non-text) argument is #VALUE! — DATEVALUE does not coerce a serial back through.
    assert_eq!(
        eval(&call("DATEVALUE", vec![num(40000.0)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn timevalue_parses_time_text() {
    let g = Grid::new(1, vec![Value::Blank]);
    let tv = |s: &str| call("TIMEVALUE", vec![Expr::Lit(Value::Text(s.into()))]);
    // A bare clock -> its day fraction (oracle-pinned).
    assert_eq!(eval(&tv("12:00"), &g), Value::Number(0.5));
    assert_eq!(
        eval(&tv("18:30:30"), &g),
        Value::Number((18 * 3600 + 30 * 60 + 30) as f64 / 86_400.0)
    );
    // A leading date part is DROPPED -> only the clock fraction.
    assert_eq!(eval(&tv("2020-01-01 12:00"), &g), Value::Number(0.5));
    // A bare date has a time-of-day of 0.
    assert_eq!(eval(&tv("2020-01-01"), &g), Value::Number(0.0));
    // Unrecognized text is #VALUE!; a non-text argument is #VALUE!.
    assert_eq!(eval(&tv("nope"), &g), Value::Error(ErrKind::Value));
    assert_eq!(
        eval(&call("TIMEVALUE", vec![num(0.5)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn today_and_now_read_the_pinned_clock() {
    // The test grid pins the clock to PINNED_NOW_SERIAL (44927.5 = 2023-01-01T12:00).
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(eval(&call("TODAY", vec![]), &g), Value::Number(44927.0));
    assert_eq!(eval(&call("NOW", vec![]), &g), Value::Number(44927.5));
    // NOW carries the time-of-day fraction TODAY floors off.
    let frac = Expr::Binary(
        crate::expr::BinOp::Sub,
        Box::new(call("NOW", vec![])),
        Box::new(call("TODAY", vec![])),
    );
    assert_eq!(eval(&frac, &g), Value::Number(0.5));
}

#[test]
fn the_registry_volatiles_are_exactly_the_clock_and_random_functions() {
    // The volatile rows are exactly the two clock functions (TODAY/NOW, reading `now_serial`) and the
    // two random functions (RAND/RANDBETWEEN, reading `rand_unit`); every other row is pure.
    for f in FUNCS {
        let expect = matches!(f.name, "TODAY" | "NOW" | "RAND" | "RANDBETWEEN");
        assert_eq!(f.volatile, expect, "{} volatility", f.name);
    }
}
