// Concern: UNIT-TEST pins for the ranking/positional extensions (RANK.EQ RANK.AVG PERCENTILE.EXC QUARTILE.EXC PERCENTRANK[.INC/.EXC] MODE.MULT FREQUENCY) exercised through `FUNCS` dispatch — the average-rank tie rule, the exclusive-percentile domain, the significant-digit truncation of PERCENTRANK, and the array shapes of MODE.MULT/FREQUENCY | Non-concern: the impls (func/stats_rank.rs) and the shared test fixtures (`num`/`call`/`arr`/`n`/`t`) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
use super::*;

/// Assert a `Value::Number` is within `tol` of `expected`.
fn close(v: Value, expected: f64, tol: f64) {
    match v {
        Value::Number(got) => assert!(
            (got - expected).abs() <= tol,
            "expected ~{expected}, got {got}"
        ),
        other => panic!("expected a Number, got {other:?}"),
    }
}

#[test]
fn rank_eq_and_avg_share_and_average_ties() {
    let g = Grid::new(1, vec![Value::Blank]);
    let data = || arr(1, 4, vec![n(10.0), n(8.0), n(8.0), n(5.0)]);
    // RANK.EQ ties share the best rank (2); RANK.AVG averages the 2..3 span → 2.5.
    assert_eq!(eval(&call("RANK.EQ", vec![num(8.0), data()]), &g), n(2.0));
    close(
        eval(&call("RANK.AVG", vec![num(8.0), data()]), &g),
        2.5,
        1e-12,
    );
    // Ascending (non-zero order): RANK.AVG(5, …, 1) → best rank 1, no tie → 1.
    close(
        eval(&call("RANK.AVG", vec![num(5.0), data(), num(1.0)]), &g),
        1.0,
        1e-12,
    );
    // A number absent from ref is #N/A.
    assert_eq!(
        eval(&call("RANK.AVG", vec![num(7.0), data()]), &g),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn percentile_and_quartile_exclusive() {
    let g = Grid::new(1, vec![Value::Blank]);
    let data = || arr(1, 4, vec![n(1.0), n(2.0), n(3.0), n(4.0)]);
    // PERCENTILE.EXC at 0.5: rank = 0.5*(4+1) = 2.5 → interpolate 2..3 → 2.5.
    close(
        eval(&call("PERCENTILE.EXC", vec![data(), num(0.5)]), &g),
        2.5,
        1e-12,
    );
    // k below 1/(n+1)=0.2 is out of the exclusive domain → #NUM!.
    assert_eq!(
        eval(&call("PERCENTILE.EXC", vec![data(), num(0.1)]), &g),
        Value::Error(ErrKind::Num)
    );
    // QUARTILE.EXC on {1..8}: quart 2 → PERCENTILE.EXC(0.5) → rank 4.5 → 4.5.
    let d8 = || {
        arr(
            1,
            8,
            vec![
                n(1.0),
                n(2.0),
                n(3.0),
                n(4.0),
                n(5.0),
                n(6.0),
                n(7.0),
                n(8.0),
            ],
        )
    };
    close(
        eval(&call("QUARTILE.EXC", vec![d8(), num(2.0)]), &g),
        4.5,
        1e-12,
    );
    // quart 0 and 4 are the endpoints the exclusive form cannot reach → #NUM!.
    assert_eq!(
        eval(&call("QUARTILE.EXC", vec![d8(), num(0.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("QUARTILE.EXC", vec![d8(), num(4.0)]), &g),
        Value::Error(ErrKind::Num)
    );
}

#[test]
fn percentrank_inclusive_exclusive_and_significance() {
    let g = Grid::new(1, vec![Value::Blank]);
    let data = || arr(1, 5, vec![n(10.0), n(20.0), n(30.0), n(40.0), n(50.0)]);
    // Exact match at index 2 of 5 → 2/4 = 0.5 (inclusive).
    close(
        eval(&call("PERCENTRANK", vec![data(), num(30.0)]), &g),
        0.5,
        1e-12,
    );
    close(
        eval(&call("PERCENTRANK.INC", vec![data(), num(30.0)]), &g),
        0.5,
        1e-12,
    );
    // Between 20 and 30: (1 + 0.5)/4 = 0.375.
    close(
        eval(&call("PERCENTRANK", vec![data(), num(25.0)]), &g),
        0.375,
        1e-12,
    );
    // Exclusive: (2+1)/(5+1) = 0.5.
    close(
        eval(&call("PERCENTRANK.EXC", vec![data(), num(30.0)]), &g),
        0.5,
        1e-12,
    );
    // Significant-digit truncation: 1/6 = 0.16666… → 0.166 (default 3), 0.16666 (5 digits).
    let d7 = || {
        arr(
            1,
            7,
            vec![n(1.0), n(2.0), n(3.0), n(4.0), n(5.0), n(6.0), n(7.0)],
        )
    };
    close(
        eval(&call("PERCENTRANK", vec![d7(), num(2.0)]), &g),
        0.166,
        1e-12,
    );
    close(
        eval(&call("PERCENTRANK", vec![d7(), num(2.0), num(5.0)]), &g),
        0.16666,
        1e-12,
    );
    // x outside [min, max] is #N/A.
    assert_eq!(
        eval(&call("PERCENTRANK", vec![data(), num(99.0)]), &g),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn mode_mult_returns_all_modes_as_a_column() {
    let g = Grid::new(1, vec![Value::Blank]);
    // {1,2,2,3,3}: both 2 and 3 tie at count 2 → vertical array [2;3].
    assert_eq!(
        eval(
            &call(
                "MODE.MULT",
                vec![arr(1, 5, vec![n(1.0), n(2.0), n(2.0), n(3.0), n(3.0)])]
            ),
            &g
        ),
        Value::Array(Shape { rows: 2, cols: 1 }, vec![n(2.0), n(3.0)])
    );
    // No repeat → #N/A.
    assert_eq!(
        eval(
            &call("MODE.MULT", vec![arr(1, 3, vec![n(1.0), n(2.0), n(3.0)])]),
            &g
        ),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn frequency_bins_into_a_column() {
    let g = Grid::new(1, vec![Value::Blank]);
    // data {1,2,3,4,5}, bins {2,4} → [≤2 : 2][>2..≤4 : 2][>4 : 1].
    assert_eq!(
        eval(
            &call(
                "FREQUENCY",
                vec![
                    arr(1, 5, vec![n(1.0), n(2.0), n(3.0), n(4.0), n(5.0)]),
                    arr(1, 2, vec![n(2.0), n(4.0)])
                ]
            ),
            &g
        ),
        Value::Array(Shape { rows: 3, cols: 1 }, vec![n(2.0), n(2.0), n(1.0)])
    );
}
