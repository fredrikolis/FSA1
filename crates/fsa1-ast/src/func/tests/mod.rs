// Concern: builds the shared Grid/Expr fixtures and pins the registry itself | Non-concern: the impls, cross-crate conformance | IO: (Grid, Expr) -> asserted Value
use super::*;
use crate::eval::eval;
use crate::refs::RangeNode;
use crate::test_support::Grid;

mod aggregation;
mod array;
mod combinatorics;
mod database;
mod date;
mod engineering;
mod finance;
mod info;
mod logical;
mod lookup;
mod math;
mod random;
mod spill;
mod stats;
mod stats_desc;
mod stats_dist;
mod stats_rank;
mod stats_reg;
mod subtotal;
mod text;
mod text_format;
mod trig;

fn num(n: f64) -> Expr {
    Expr::Lit(Value::Number(n))
}
fn call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::Call(lookup(name).expect("known function"), args)
}
/// A full-column range A1:A{rows} over a 1-wide grid (contiguous in the stub).
fn col_range(rows: u32) -> Expr {
    Expr::Range(RangeNode {
        start_col: 0,
        start_row: 0,
        end_col: 0,
        end_row: rows - 1,
        start_col_abs: false,
        start_row_abs: false,
        end_col_abs: false,
        end_row_abs: false,
        sheet: None,
    })
}

fn txt(s: &str) -> Expr {
    Expr::Lit(Value::Text(s.into()))
}
fn text(v: Value) -> String {
    match v {
        Value::Text(t) => t,
        other => panic!("expected Text, got {other:?}"),
    }
}

/// A literal array argument of a given shape (row-major), sidestepping the whole-row test-Grid
/// stub so a single column/row can be presented cleanly.
fn arr(rows: u32, cols: u32, cells: Vec<Value>) -> Expr {
    Expr::Lit(arr_val(rows, cols, cells))
}

/// The same array as a VALUE, for asserting what a lifted call answered rather than feeding it one.
fn arr_val(rows: u32, cols: u32, cells: Vec<Value>) -> Value {
    Value::Array(crate::value::Shape { rows, cols }, cells)
}
fn n(x: f64) -> Value {
    Value::Number(x)
}
fn t(s: &str) -> Value {
    Value::Text(s.into())
}

#[test]
fn registry_is_self_consistent() {
    for (i, f) in FUNCS.iter().enumerate() {
        assert_eq!(
            lookup(f.name),
            Some(FuncId(i as u32)),
            "name maps to its index"
        );
        assert_eq!(def(FuncId(i as u32)).unwrap().name, f.name);
        if let Some(max) = f.max_args {
            assert!(max >= f.min_args, "{}: max >= min", f.name);
        }
        assert!(
            f.name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '.'),
            "UPPERCASE name (a `.` is allowed for the dotted Excel spellings, e.g. STDEV.S; a \
             digit for names like LOG10/ATAN2)"
        );
    }
    let mut names: Vec<String> = FUNCS.iter().map(|f| f.name.to_ascii_uppercase()).collect();
    let before = names.len();
    names.sort();
    names.dedup();
    assert_eq!(before, names.len(), "function names are unique");
    assert_eq!(lookup("sum"), lookup("SUM"));
    assert_eq!(lookup("NoSuchFn"), None);
}

#[test]
fn dispatch_guards_synthesized_off_arity_and_bad_id_without_panicking() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(eval(&call("IF", vec![]), &g), Value::Error(ErrKind::Value));
    assert_eq!(
        eval(&call("IFERROR", vec![num(1.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("ROUND", vec![num(1.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("ABS", vec![num(1.0), num(2.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&Expr::Call(FuncId(9999), vec![]), &g),
        Value::Error(ErrKind::Name)
    );
}

#[test]
fn arity_bounds() {
    let sum = def(lookup("SUM").unwrap()).unwrap();
    assert!(!sum.arity_ok(0));
    assert!(sum.arity_ok(1) && sum.arity_ok(99));
    let iff = def(lookup("IF").unwrap()).unwrap();
    assert!(!iff.arity_ok(1) && iff.arity_ok(2) && iff.arity_ok(3) && !iff.arity_ok(4));
}

#[test]
fn parse_iso_serial_recovers_the_datetime_whitespace_split_bit_exactly() {
    assert_eq!(super::parse_iso_serial("2021-05-15"), Some(44331.0));
    assert_eq!(super::parse_iso_serial("13:30:00"), Some(0.5625));
    assert_eq!(
        super::parse_iso_serial("2021-05-15 13:30:00"),
        Some(44331.5625)
    );
    assert_eq!(super::parse_iso_serial("5/15/2021"), None);
    assert_eq!(super::parse_iso_serial("hello"), None);
}

#[test]
fn format_value_facade_renders_through_the_one_numfmt_engine() {
    assert_eq!(
        super::format_value(&Value::Number(1234.5), "$#,##0.00"),
        Value::Text("$1,234.50".to_string())
    );
    assert_eq!(
        super::format_value(&Value::Number(44331.0), "m/d/yyyy"),
        Value::Text("5/15/2021".to_string())
    );
    assert_eq!(
        super::format_value(&Value::Error(ErrKind::Div0), "0.00"),
        Value::Error(ErrKind::Div0)
    );
}
