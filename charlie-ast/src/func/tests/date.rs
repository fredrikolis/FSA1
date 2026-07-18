// Concern: UNIT-TEST pins for the date/time family built-ins (DATE YEAR MONTH DAY EDATE DATEDIF TODAY NOW) exercised through `FUNCS` dispatch — serial construction/normalization with the replicated 1900 leap-year bug, end-of-month clamping, DATEDIF unit folding, the pinned-clock volatiles, and the registry-wide volatility invariant for TODAY/NOW | Non-concern: the date impls (`func/date.rs`) and the shared test fixtures (the parent `tests` module owns `num`/`call`) | IO: in-memory `Grid` fixtures (with a pinned clock) + literal `Expr`s -> asserted `Value`s
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
fn today_and_now_are_the_registry_volatiles() {
    // Exactly TODAY and NOW carry `volatile: true`; every other row is pure.
    for f in FUNCS {
        let expect = matches!(f.name, "TODAY" | "NOW");
        assert_eq!(f.volatile, expect, "{} volatility", f.name);
    }
}
