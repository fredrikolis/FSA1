// Concern: the func UNIT-TEST tree ROOT — the shared fixtures every family test reuses (the `num`/`call`/`col_range`/`txt`/`text`/`arr`/`n`/`t` literal-`Expr` builders over an in-memory `Grid`) plus the REGISTRY-level pins that belong to no single family (name<->id/arity/UPPERCASE self-consistency, the arity-bounds accessor, and dispatch's off-arity/bad-id no-panic guards); the per-family behavioral pins live in the sibling submodules (aggregation/logical/math/stats/text/date/lookup/info/finance), each holding the tests for the functions its `func::*` twin implements | Non-concern: the function implementations under test (the `func/*.rs` submodules own them) and cross-crate conformance (the conformance crate owns the fixture corpus) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
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
    Expr::Lit(Value::Array(crate::value::Shape { rows, cols }, cells))
}
fn n(x: f64) -> Value {
    Value::Number(x)
}
fn t(s: &str) -> Value {
    Value::Text(s.into())
}

#[test]
fn registry_is_self_consistent() {
    // Names unique (case-insensitively), index == FuncId, arity bounds well-formed.
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
    // case-insensitive lookup
    assert_eq!(lookup("sum"), lookup("SUM"));
    assert_eq!(lookup("NoSuchFn"), None);
}

#[test]
fn dispatch_guards_synthesized_off_arity_and_bad_id_without_panicking() {
    // A synthesized off-arity Call (the parser would refuse these via BadArity) must NOT panic
    // the positional built-ins — dispatch's arity gate turns each into #VALUE!.
    let g = Grid::new(1, vec![Value::Blank]);
    // IF/IFERROR/ROUND handed too few args; ABS handed too many.
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
    // An out-of-range (synthesized) FuncId stays #NAME? — the sibling guard.
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
