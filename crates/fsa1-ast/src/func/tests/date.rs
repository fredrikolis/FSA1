// Concern: pins the date/time built-ins | Non-concern: the impls, the shared fixtures | IO: (Grid, Expr) -> asserted Value
use super::*;

#[test]
fn date_builds_and_normalizes_a_serial() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("DATE", vec![num(2023.0), num(1.0), num(1.0)]), &g),
        Value::Number(44927.0)
    );
    assert_eq!(
        eval(&call("DATE", vec![num(2008.0), num(14.0), num(2.0)]), &g),
        Value::Number(39846.0)
    );
    assert_eq!(
        eval(&call("DATE", vec![num(2023.0), num(3.0), num(0.0)]), &g),
        Value::Number(44985.0)
    );
    assert_eq!(
        eval(&call("DATE", vec![num(108.0), num(1.0), num(2.0)]), &g),
        Value::Number(39449.0)
    );
    assert_eq!(
        eval(&call("DATE", vec![num(10000.0), num(1.0), num(1.0)]), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn year_month_day_read_a_serial_with_the_leap_bug() {
    let g = Grid::new(1, vec![Value::Blank]);
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
    assert_eq!(
        eval(&call("YEAR", vec![num(60.0)]), &g),
        Value::Number(1900.0)
    );
    assert_eq!(
        eval(&call("MONTH", vec![num(60.0)]), &g),
        Value::Number(2.0)
    );
    assert_eq!(eval(&call("DAY", vec![num(60.0)]), &g), Value::Number(29.0));
    assert_eq!(
        eval(&call("YEAR", vec![num(0.0)]), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn edate_clamps_to_end_of_month() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("EDATE", vec![num(44927.0), num(1.0)]), &g),
        Value::Number(44958.0)
    );
    assert_eq!(
        eval(&call("EDATE", vec![num(43861.0), num(1.0)]), &g),
        Value::Number(43890.0)
    );
    assert_eq!(
        eval(&call("EDATE", vec![num(44927.0), num(-2.0)]), &g),
        Value::Number(44866.0)
    );
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
    assert_eq!(eval(&dd(44927.0, 44957.0, "D"), &g), Value::Number(30.0));
    assert_eq!(eval(&dd(43831.0, 45078.0, "Y"), &g), Value::Number(3.0));
    assert_eq!(eval(&dd(43831.0, 45078.0, "M"), &g), Value::Number(41.0));
    assert_eq!(eval(&dd(43845.0, 43910.0, "MD"), &g), Value::Number(5.0));
    assert_eq!(eval(&dd(43845.0, 45097.0, "YM"), &g), Value::Number(5.0));
    assert_eq!(eval(&dd(43845.0, 45005.0, "YD"), &g), Value::Number(65.0));
    assert_eq!(eval(&dd(44927.0, 44957.0, "d"), &g), Value::Number(30.0));
    assert_eq!(
        eval(&dd(44957.0, 44927.0, "D"), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&dd(44927.0, 44957.0, "Q"), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn eomonth_and_days() {
    let g = Grid::new(1, vec![Value::Blank]);
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
    assert_eq!(
        eval(&call("WEEKDAY", vec![num(43831.0)]), &g),
        Value::Number(4.0)
    );
    assert_eq!(
        eval(&call("WEEKDAY", vec![num(43831.0), num(2.0)]), &g),
        Value::Number(3.0)
    );
    assert_eq!(
        eval(&call("WEEKDAY", vec![num(43831.0), num(3.0)]), &g),
        Value::Number(2.0)
    );
    assert_eq!(
        eval(&call("WEEKDAY", vec![num(43831.0), num(4.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("WEEKNUM", vec![num(44927.0)]), &g),
        Value::Number(1.0)
    );
    assert_eq!(
        eval(&call("WEEKNUM", vec![num(44934.0)]), &g),
        Value::Number(2.0)
    );
    assert_eq!(
        eval(&call("WEEKNUM", vec![num(44927.0), num(21.0)]), &g),
        Value::Number(52.0)
    );
}

#[test]
fn workday_and_networkdays() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("WORKDAY", vec![num(43831.0), num(3.0)]), &g),
        Value::Number(43836.0)
    );
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
    assert_eq!(
        eval(&call("NETWORKDAYS", vec![num(43831.0), num(43837.0)]), &g),
        Value::Number(5.0)
    );
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
    assert_eq!(eval(&yf(43831.0, 44197.0, None), &g), Value::Number(1.0));
    assert_eq!(
        eval(&yf(43831.0, 44197.0, Some(1.0)), &g),
        Value::Number(1.0)
    );
    assert_eq!(
        eval(&yf(43831.0, 44197.0, Some(2.0)), &g),
        Value::Number(366.0 / 360.0)
    );
    assert_eq!(
        eval(&yf(43831.0, 44197.0, Some(3.0)), &g),
        Value::Number(366.0 / 365.0)
    );
    assert_eq!(
        eval(&yf(44197.0, 43831.0, Some(0.0)), &g),
        Value::Number(1.0)
    );
    let d = |y: f64, m: f64, dd: f64| call("DATE", vec![num(y), num(m), num(dd)]);
    let yfd = |a: Expr, b: Expr, basis: f64| call("YEARFRAC", vec![a, b, num(basis)]);
    assert_eq!(
        eval(&yfd(d(2020.0, 1.0, 31.0), d(2020.0, 7.0, 31.0), 0.0), &g),
        Value::Number(0.5)
    );
    assert_eq!(
        eval(&yfd(d(2020.0, 1.0, 31.0), d(2020.0, 3.0, 31.0), 0.0), &g),
        Value::Number(60.0 / 360.0)
    );
    assert_eq!(
        eval(&yfd(d(2020.0, 2.0, 29.0), d(2021.0, 2.0, 28.0), 0.0), &g),
        Value::Number(1.0)
    );
    assert_eq!(
        eval(&yfd(d(2020.0, 1.0, 31.0), d(2020.0, 7.0, 31.0), 4.0), &g),
        Value::Number(0.5)
    );
    assert_eq!(
        eval(&yfd(d(2020.0, 2.0, 29.0), d(2021.0, 2.0, 28.0), 4.0), &g),
        Value::Number(359.0 / 360.0)
    );
    assert_eq!(
        eval(&yf(43831.0, 44197.0, Some(5.0)), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn time_and_its_components() {
    let g = Grid::new(1, vec![Value::Blank]);
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
    assert_eq!(
        eval(&call("TIME", vec![num(25.0), num(0.0), num(0.0)]), &g),
        Value::Number(1.0 / 24.0)
    );
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
    assert_eq!(eval(&call("HOUR", vec![num(0.5)]), &g), Value::Number(12.0));
    assert_eq!(
        eval(&call("HOUR", vec![num(-0.5)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("HOUR", vec![num(44927.5)]), &g),
        Value::Number(12.0)
    );
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
    assert_eq!(
        eval(&d360(d(2020.0, 1.0, 31.0), d(2020.0, 3.0, 31.0), None), &g),
        Value::Number(60.0)
    );
    assert_eq!(
        eval(&d360(d(2020.0, 2.0, 29.0), d(2021.0, 2.0, 28.0), None), &g),
        Value::Number(358.0)
    );
    assert_eq!(
        eval(&d360(d(2020.0, 1.0, 15.0), d(2020.0, 2.0, 29.0), None), &g),
        Value::Number(44.0)
    );
    assert_eq!(
        eval(
            &d360(d(2020.0, 2.0, 29.0), d(2020.0, 3.0, 31.0), Some(true)),
            &g
        ),
        Value::Number(31.0)
    );
    assert_eq!(
        eval(&d360(d(2020.0, 3.0, 31.0), d(2020.0, 1.0, 31.0), None), &g),
        Value::Number(-60.0)
    );
}

#[test]
fn isoweeknum_matches_weeknum_type_21() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("ISOWEEKNUM", vec![num(44927.0)]), &g),
        Value::Number(52.0)
    );
    assert_eq!(
        eval(&call("ISOWEEKNUM", vec![num(43831.0)]), &g),
        Value::Number(1.0)
    );
    assert_eq!(
        eval(&call("ISOWEEKNUM", vec![num(0.0)]), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn networkdays_intl_custom_weekends() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(
            &call("NETWORKDAYS.INTL", vec![num(43831.0), num(43840.0)]),
            &g
        ),
        Value::Number(8.0)
    );
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
    assert_eq!(
        eval(
            &call("NETWORKDAYS.INTL", vec![num(43840.0), num(43831.0)]),
            &g
        ),
        Value::Number(-8.0)
    );
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
    assert_eq!(
        eval(&call("WORKDAY.INTL", vec![num(43831.0), num(5.0)]), &g),
        Value::Number(43838.0)
    );
    assert_eq!(
        eval(
            &call("WORKDAY.INTL", vec![num(43831.0), num(5.0), num(11.0)]),
            &g
        ),
        Value::Number(43837.0)
    );
    assert_eq!(
        eval(&call("WORKDAY.INTL", vec![num(43831.0), num(-5.0)]), &g),
        Value::Number(43824.0)
    );
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
    assert_eq!(eval(&dv("2020-01-01"), &g), Value::Number(43831.0));
    assert_eq!(eval(&dv("2020-01-01 12:00"), &g), Value::Number(43831.0));
    assert_eq!(eval(&dv("12:00"), &g), Value::Error(ErrKind::Value));
    assert_eq!(eval(&dv("not a date"), &g), Value::Error(ErrKind::Value));
    assert_eq!(eval(&dv(""), &g), Value::Error(ErrKind::Value));
    assert_eq!(
        eval(&call("DATEVALUE", vec![num(40000.0)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn timevalue_parses_time_text() {
    let g = Grid::new(1, vec![Value::Blank]);
    let tv = |s: &str| call("TIMEVALUE", vec![Expr::Lit(Value::Text(s.into()))]);
    assert_eq!(eval(&tv("12:00"), &g), Value::Number(0.5));
    assert_eq!(
        eval(&tv("18:30:30"), &g),
        Value::Number((18 * 3600 + 30 * 60 + 30) as f64 / 86_400.0)
    );
    assert_eq!(eval(&tv("2020-01-01 12:00"), &g), Value::Number(0.5));
    assert_eq!(eval(&tv("2020-01-01"), &g), Value::Number(0.0));
    assert_eq!(eval(&tv("nope"), &g), Value::Error(ErrKind::Value));
    assert_eq!(
        eval(&call("TIMEVALUE", vec![num(0.5)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn today_and_now_read_the_pinned_clock() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(eval(&call("TODAY", vec![]), &g), Value::Number(44927.0));
    assert_eq!(eval(&call("NOW", vec![]), &g), Value::Number(44927.5));
    let frac = Expr::Binary(
        crate::expr::BinOp::Sub,
        Box::new(call("NOW", vec![])),
        Box::new(call("TODAY", vec![])),
    );
    assert_eq!(eval(&frac, &g), Value::Number(0.5));
}

#[test]
fn the_registry_volatiles_are_exactly_the_clock_and_random_functions() {
    for f in FUNCS {
        let expect = matches!(f.name, "TODAY" | "NOW" | "RAND" | "RANDBETWEEN");
        assert_eq!(f.volatile, expect, "{} volatility", f.name);
    }
}
