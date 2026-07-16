// Concern: the func registry + families UNIT TESTS — behavioral pins for the built-ins exercised through the `FUNCS` dispatch (SUM…IRR: aggregation, laziness, criteria, math, stats, text, date, lookup, info, finance), plus the registry self-consistency invariant (name<->id, arity, UPPERCASE) | Non-concern: the function implementations under test (the func/*.rs submodules) and cross-crate conformance (the conformance crate owns the fixture corpus) | IO: in-memory `Grid` fixtures + literal `Expr`s -> asserted `Value`s
use super::*;
use crate::eval::eval;
use crate::refs::RangeNode;
use crate::test_support::Grid;

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
        sheet: None,
    })
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
            f.name.chars().all(|c| c.is_ascii_uppercase()),
            "UPPERCASE name"
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
fn sum_average_count_over_a_range_with_mixed_cells() {
    // A1..A5 = 1, "x"(text), TRUE(bool), <blank>, 4  -> numbers are {1, 4}
    let g = Grid::new(
        1,
        vec![
            Value::Number(1.0),
            Value::Text("x".into()),
            Value::Bool(true),
            Value::Blank,
            Value::Number(4.0),
        ],
    );
    assert_eq!(
        eval(&call("SUM", vec![col_range(5)]), &g),
        Value::Number(5.0)
    );
    assert_eq!(
        eval(&call("AVERAGE", vec![col_range(5)]), &g),
        Value::Number(2.5)
    );
    // COUNT counts only the two numbers (in-range bool/text ignored).
    assert_eq!(
        eval(&call("COUNT", vec![col_range(5)]), &g),
        Value::Number(2.0)
    );
}

#[test]
fn direct_vs_in_range_coercion_asymmetry() {
    let g = Grid::new(1, vec![Value::Blank]);
    // Direct booleans/numeric-text coerce and count.
    assert_eq!(
        eval(
            &call(
                "SUM",
                vec![
                    num(1.0),
                    Expr::Lit(Value::Bool(true)),
                    Expr::Lit(Value::Text("2".into()))
                ]
            ),
            &g
        ),
        Value::Number(4.0)
    );
    assert_eq!(
        eval(
            &call(
                "COUNT",
                vec![
                    Expr::Lit(Value::Bool(true)),
                    Expr::Lit(Value::Text("3".into()))
                ]
            ),
            &g
        ),
        Value::Number(2.0)
    );
    // A direct non-numeric text is #VALUE! for SUM.
    assert_eq!(
        eval(&call("SUM", vec![Expr::Lit(Value::Text("x".into()))]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn sum_propagates_but_count_ignores_errors() {
    let g = Grid::new(
        1,
        vec![
            Value::Number(1.0),
            Value::Error(ErrKind::Div0),
            Value::Number(2.0),
        ],
    );
    assert_eq!(
        eval(&call("SUM", vec![col_range(3)]), &g),
        Value::Error(ErrKind::Div0)
    );
    // COUNT never returns an error from its data.
    assert_eq!(
        eval(&call("COUNT", vec![col_range(3)]), &g),
        Value::Number(2.0)
    );
}

#[test]
fn if_is_lazy_and_iferror_catches() {
    let g = Grid::new(1, vec![Value::Blank]);
    // IF(TRUE, 1, 1/0) -> 1 (else branch not evaluated).
    let div0 = Expr::Binary(
        crate::expr::BinOp::Div,
        Box::new(num(1.0)),
        Box::new(num(0.0)),
    );
    let e = call(
        "IF",
        vec![Expr::Lit(Value::Bool(true)), num(1.0), div0.clone()],
    );
    assert_eq!(eval(&e, &g), Value::Number(1.0));
    // Two-arg false -> FALSE.
    let e = call("IF", vec![Expr::Lit(Value::Bool(false)), num(1.0)]);
    assert_eq!(eval(&e, &g), Value::Bool(false));
    // IFERROR(1/0, 99) -> 99.
    let e = call("IFERROR", vec![div0, num(99.0)]);
    assert_eq!(eval(&e, &g), Value::Number(99.0));
    // IFERROR passes a non-error through.
    let e = call("IFERROR", vec![num(7.0), num(99.0)]);
    assert_eq!(eval(&e, &g), Value::Number(7.0));
}

#[test]
fn and_or_semantics() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(
            &call("AND", vec![Expr::Lit(Value::Bool(true)), num(1.0)]),
            &g
        ),
        Value::Bool(true)
    );
    assert_eq!(
        eval(
            &call("AND", vec![Expr::Lit(Value::Bool(true)), num(0.0)]),
            &g
        ),
        Value::Bool(false)
    );
    assert_eq!(
        eval(
            &call(
                "OR",
                vec![num(0.0), Expr::Lit(Value::Bool(false)), num(1.0)]
            ),
            &g
        ),
        Value::Bool(true)
    );
    // error propagates
    assert_eq!(
        eval(
            &call(
                "AND",
                vec![
                    Expr::Lit(Value::Error(ErrKind::Ref)),
                    Expr::Lit(Value::Bool(true))
                ]
            ),
            &g
        ),
        Value::Error(ErrKind::Ref)
    );
}

#[test]
fn abs_and_round() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(eval(&call("ABS", vec![num(-5.0)]), &g), Value::Number(5.0));
    assert_eq!(
        eval(&call("ROUND", vec![num(1.2345), num(2.0)]), &g),
        Value::Number(1.23)
    );
    // ties away from zero
    assert_eq!(
        eval(&call("ROUND", vec![num(2.5), num(0.0)]), &g),
        Value::Number(3.0)
    );
    assert_eq!(
        eval(&call("ROUND", vec![num(-2.5), num(0.0)]), &g),
        Value::Number(-3.0)
    );
    // negative digits round left of the point
    assert_eq!(
        eval(&call("ROUND", vec![num(1234.0), num(-2.0)]), &g),
        Value::Number(1200.0)
    );
}

#[test]
fn math_batch_scalar_semantics() {
    let g = Grid::new(1, vec![Value::Blank]);
    // MOD sign follows the divisor.
    assert_eq!(
        eval(&call("MOD", vec![num(7.0), num(3.0)]), &g),
        Value::Number(1.0)
    );
    assert_eq!(
        eval(&call("MOD", vec![num(7.0), num(-3.0)]), &g),
        Value::Number(-2.0)
    );
    assert_eq!(
        eval(&call("MOD", vec![num(-7.0), num(3.0)]), &g),
        Value::Number(2.0)
    );
    assert_eq!(
        eval(&call("MOD", vec![num(5.0), num(0.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
    // INT floors toward −∞ (not toward zero).
    assert_eq!(eval(&call("INT", vec![num(-2.5)]), &g), Value::Number(-3.0));
    assert_eq!(eval(&call("INT", vec![num(2.9)]), &g), Value::Number(2.0));
    // SQRT of a negative is #NUM!.
    assert_eq!(eval(&call("SQRT", vec![num(16.0)]), &g), Value::Number(4.0));
    assert_eq!(
        eval(&call("SQRT", vec![num(-4.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    // POWER shares the operator's error mapping.
    assert_eq!(
        eval(&call("POWER", vec![num(2.0), num(10.0)]), &g),
        Value::Number(1024.0)
    );
    assert_eq!(
        eval(&call("POWER", vec![num(0.0), num(-1.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
    assert_eq!(
        eval(&call("POWER", vec![num(-8.0), num(0.5)]), &g),
        Value::Error(ErrKind::Num)
    );
    // ROUNDUP away from zero; ROUNDDOWN toward zero; negative digits shift left.
    assert_eq!(
        eval(&call("ROUNDUP", vec![num(1.234), num(2.0)]), &g),
        Value::Number(1.24)
    );
    assert_eq!(
        eval(&call("ROUNDUP", vec![num(-1.234), num(2.0)]), &g),
        Value::Number(-1.24)
    );
    assert_eq!(
        eval(&call("ROUNDDOWN", vec![num(1.789), num(2.0)]), &g),
        Value::Number(1.78)
    );
    assert_eq!(
        eval(&call("ROUNDDOWN", vec![num(3.99999), num(0.0)]), &g),
        Value::Number(3.0)
    );
}

#[test]
fn ceiling_floor_sign_and_zero_asymmetry() {
    let g = Grid::new(1, vec![Value::Blank]);
    // Away-from-zero / toward-zero to a multiple.
    assert_eq!(
        eval(&call("CEILING", vec![num(2.5), num(1.0)]), &g),
        Value::Number(3.0)
    );
    assert_eq!(
        eval(&call("FLOOR", vec![num(2.5), num(1.0)]), &g),
        Value::Number(2.0)
    );
    assert_eq!(
        eval(&call("CEILING", vec![num(-2.5), num(-2.0)]), &g),
        Value::Number(-4.0)
    );
    assert_eq!(
        eval(&call("FLOOR", vec![num(-2.5), num(-2.0)]), &g),
        Value::Number(-2.0)
    );
    // Different-signed args are #NUM! for both.
    assert_eq!(
        eval(&call("CEILING", vec![num(2.5), num(-1.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    assert_eq!(
        eval(&call("FLOOR", vec![num(2.5), num(-1.0)]), &g),
        Value::Error(ErrKind::Num)
    );
    // Zero significance: CEILING → 0, FLOOR → #DIV/0! (the legacy asymmetry).
    assert_eq!(
        eval(&call("CEILING", vec![num(5.0), num(0.0)]), &g),
        Value::Number(0.0)
    );
    assert_eq!(
        eval(&call("FLOOR", vec![num(5.0), num(0.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn product_and_sumproduct_semantics() {
    // PRODUCT over a range multiplies the numbers; a range with no numbers is 0.
    let g = Grid::new(
        1,
        vec![Value::Number(2.0), Value::Number(3.0), Value::Number(4.0)],
    );
    assert_eq!(
        eval(&call("PRODUCT", vec![col_range(3)]), &g),
        Value::Number(24.0)
    );
    // Direct-arg coercion mirrors SUM (bool → 1/0, numeric-text parses).
    assert_eq!(
        eval(
            &call(
                "PRODUCT",
                vec![
                    num(2.0),
                    Expr::Lit(Value::Bool(true)),
                    Expr::Lit(Value::Text("3".into()))
                ]
            ),
            &g
        ),
        Value::Number(6.0)
    );
    // Empty product (no numeric datum) is 0, not the 1 identity.
    let blank = Grid::new(1, vec![Value::Text("x".into()), Value::Blank]);
    assert_eq!(
        eval(&call("PRODUCT", vec![col_range(2)]), &blank),
        Value::Number(0.0)
    );
    // SUMPRODUCT multiplies aligned arrays then sums (array literals sidestep the whole-row
    // stub, which cannot window a single column of a multi-column grid).
    let col3 = |a: f64, b: f64, c: f64| {
        Expr::Lit(Value::Array(
            crate::value::Shape { rows: 3, cols: 1 },
            vec![Value::Number(a), Value::Number(b), Value::Number(c)],
        ))
    };
    assert_eq!(
        eval(
            &call("SUMPRODUCT", vec![col3(1.0, 2.0, 3.0), col3(4.0, 5.0, 6.0)]),
            &g
        ),
        Value::Number(32.0)
    );
    // A shape mismatch (3×1 vs 2×1) is a static #VALUE!.
    let col2 = Expr::Lit(Value::Array(
        crate::value::Shape { rows: 2, cols: 1 },
        vec![Value::Number(4.0), Value::Number(5.0)],
    ));
    assert_eq!(
        eval(&call("SUMPRODUCT", vec![col3(1.0, 2.0, 3.0), col2]), &g),
        Value::Error(ErrKind::Value)
    );
    // A non-numeric cell counts as 0 (so an unfiltered text zeroes its product term).
    let with_text = Expr::Lit(Value::Array(
        crate::value::Shape { rows: 3, cols: 1 },
        vec![
            Value::Number(2.0),
            Value::Text("x".into()),
            Value::Number(4.0),
        ],
    ));
    assert_eq!(
        eval(
            &call("SUMPRODUCT", vec![with_text, col3(5.0, 5.0, 5.0)]),
            &g
        ),
        Value::Number(30.0)
    );
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
fn min_max_range_vs_direct_arg_asymmetry() {
    // In a RANGE, text/blank/logical are ignored (only numbers) — so TRUE does NOT count as 1.
    let g = Grid::new(
        1,
        vec![
            Value::Number(-5.0),
            Value::Bool(true),
            Value::Blank,
            Value::Number(-2.0),
        ],
    );
    assert_eq!(
        eval(&call("MIN", vec![col_range(4)]), &g),
        Value::Number(-5.0)
    );
    assert_eq!(
        eval(&call("MAX", vec![col_range(4)]), &g),
        Value::Number(-2.0)
    );
    // DIRECT booleans/numeric-text coerce (TRUE -> 1), the asymmetry's other half.
    let b = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(
            &call(
                "MAX",
                vec![num(-5.0), Expr::Lit(Value::Bool(true)), num(-2.0)]
            ),
            &b
        ),
        Value::Number(1.0)
    );
    assert_eq!(
        eval(
            &call("MIN", vec![num(3.0), Expr::Lit(Value::Text("2".into()))]),
            &b
        ),
        Value::Number(2.0)
    );
    // No numeric datum -> 0 (Excel), and an in-range error propagates.
    let empty = Grid::new(1, vec![Value::Text("x".into()), Value::Blank]);
    assert_eq!(
        eval(&call("MIN", vec![col_range(2)]), &empty),
        Value::Number(0.0)
    );
    let with_err = Grid::new(1, vec![Value::Number(5.0), Value::Error(ErrKind::Div0)]);
    assert_eq!(
        eval(&call("MAX", vec![col_range(2)]), &with_err),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn median_even_averages_two_middles_and_empty_is_num() {
    // Even count {1,2,3,4} -> (2+3)/2 = 2.5 (in-range text ignored).
    let g = Grid::new(
        1,
        vec![
            Value::Number(1.0),
            Value::Number(2.0),
            Value::Text("x".into()),
            Value::Number(3.0),
            Value::Number(4.0),
        ],
    );
    assert_eq!(
        eval(&call("MEDIAN", vec![col_range(5)]), &g),
        Value::Number(2.5)
    );
    // Odd count -> the exact middle.
    let odd = Grid::new(
        1,
        vec![
            Value::Number(5.0),
            Value::Number(3.0),
            Value::Number(1.0),
            Value::Number(4.0),
            Value::Number(2.0),
        ],
    );
    assert_eq!(
        eval(&call("MEDIAN", vec![col_range(5)]), &odd),
        Value::Number(3.0)
    );
    // No numeric datum -> #NUM! (distinct from MIN/MAX's 0); an error propagates.
    let empty = Grid::new(1, vec![Value::Text("x".into()), Value::Blank]);
    assert_eq!(
        eval(&call("MEDIAN", vec![col_range(2)]), &empty),
        Value::Error(ErrKind::Num)
    );
    let with_err = Grid::new(1, vec![Value::Number(1.0), Value::Error(ErrKind::Ref)]);
    assert_eq!(
        eval(&call("MEDIAN", vec![col_range(2)]), &with_err),
        Value::Error(ErrKind::Ref)
    );
}

#[test]
fn rank_descending_default_ties_share_lowest_and_missing_is_na() {
    // {10,8,8,5}: RANK(8) descending -> 2 (one value strictly greater); both 8s share rank 2.
    let g = Grid::new(
        1,
        vec![
            Value::Number(10.0),
            Value::Number(8.0),
            Value::Number(8.0),
            Value::Number(5.0),
        ],
    );
    assert_eq!(
        eval(&call("RANK", vec![num(8.0), col_range(4)]), &g),
        Value::Number(2.0)
    );
    // Ascending (non-zero order): RANK(10, …, 1) -> 4 (three strictly less).
    assert_eq!(
        eval(&call("RANK", vec![num(10.0), col_range(4), num(1.0)]), &g),
        Value::Number(4.0)
    );
    // A number not present in ref is #N/A.
    assert_eq!(
        eval(&call("RANK", vec![num(7.0), col_range(4)]), &g),
        Value::Error(ErrKind::Na)
    );
    // A non-numeric `number` argument is #VALUE!.
    assert_eq!(
        eval(
            &call(
                "RANK",
                vec![Expr::Lit(Value::Text("x".into())), col_range(4)]
            ),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn counta_and_countblank_over_a_range() {
    // A1..A5 = 1, "", "x", <blank>, #N/A. COUNTA counts non-empty: 1, "", "x", #N/A = 4
    // (the empty-string "" is non-empty; error counts; only the blank does not).
    let g = Grid::new(
        1,
        vec![
            Value::Number(1.0),
            Value::Text(String::new()),
            Value::Text("x".into()),
            Value::Blank,
            Value::Error(ErrKind::Na),
        ],
    );
    assert_eq!(
        eval(&call("COUNTA", vec![col_range(5)]), &g),
        Value::Number(4.0)
    );
    // COUNTBLANK counts the empty: the "" AND the <blank> = 2 (error/number/text not blank).
    assert_eq!(
        eval(&call("COUNTBLANK", vec![col_range(5)]), &g),
        Value::Number(2.0)
    );
    // COUNTA of a direct blank does not count it; a direct value does.
    let b = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("COUNTA", vec![Expr::Lit(Value::Blank), num(1.0)]), &b),
        Value::Number(1.0)
    );
}

#[test]
fn ifs_first_true_wins_lazily_and_none_is_na() {
    let g = Grid::new(1, vec![Value::Blank]);
    let t = || Expr::Lit(Value::Bool(true));
    let f = || Expr::Lit(Value::Bool(false));
    let div0 = || {
        Expr::Binary(
            crate::expr::BinOp::Div,
            Box::new(num(1.0)),
            Box::new(num(0.0)),
        )
    };
    // First TRUE test's value wins; the earlier FALSE pair's value is skipped.
    assert_eq!(
        eval(
            &call("IFS", vec![f(), num(1.0), t(), num(2.0), t(), num(3.0)]),
            &g
        ),
        Value::Number(2.0)
    );
    // Lazy: the unreached value (1/0) after the first match is never evaluated.
    assert_eq!(
        eval(&call("IFS", vec![t(), num(1.0), t(), div0()]), &g),
        Value::Number(1.0)
    );
    // No TRUE test -> #N/A.
    assert_eq!(
        eval(&call("IFS", vec![f(), num(1.0), f(), num(2.0)]), &g),
        Value::Error(ErrKind::Na)
    );
    // A test that errors propagates (evaluated before any match).
    assert_eq!(
        eval(&call("IFS", vec![div0(), num(1.0), t(), num(2.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
    // An odd argument count (dangling test) is a structural #VALUE!.
    assert_eq!(
        eval(&call("IFS", vec![f(), num(1.0), t()]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn not_coerces_and_propagates() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(&call("NOT", vec![Expr::Lit(Value::Bool(true))]), &g),
        Value::Bool(false)
    );
    // A non-zero number coerces to TRUE -> NOT is FALSE; zero -> TRUE.
    assert_eq!(eval(&call("NOT", vec![num(5.0)]), &g), Value::Bool(false));
    assert_eq!(eval(&call("NOT", vec![num(0.0)]), &g), Value::Bool(true));
    // A non-logical text is #VALUE!; an error propagates.
    assert_eq!(
        eval(&call("NOT", vec![Expr::Lit(Value::Text("x".into()))]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("NOT", vec![Expr::Lit(Value::Error(ErrKind::Na))]), &g),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn ifna_catches_only_na() {
    let g = Grid::new(1, vec![Value::Blank]);
    // Catches #N/A.
    assert_eq!(
        eval(
            &call(
                "IFNA",
                vec![Expr::Lit(Value::Error(ErrKind::Na)), num(99.0)]
            ),
            &g
        ),
        Value::Number(99.0)
    );
    // Passes a normal value through.
    assert_eq!(
        eval(&call("IFNA", vec![num(42.0), num(99.0)]), &g),
        Value::Number(42.0)
    );
    // Does NOT catch a different error (the distinction from IFERROR).
    assert_eq!(
        eval(
            &call(
                "IFNA",
                vec![Expr::Lit(Value::Error(ErrKind::Div0)), num(99.0)]
            ),
            &g
        ),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn switch_matches_first_with_optional_default() {
    let g = Grid::new(1, vec![Value::Blank]);
    let txt = |s: &str| Expr::Lit(Value::Text(s.into()));
    // Matches the second value.
    assert_eq!(
        eval(
            &call(
                "SWITCH",
                vec![
                    num(2.0),
                    num(1.0),
                    txt("one"),
                    num(2.0),
                    txt("two"),
                    num(3.0),
                    txt("three")
                ]
            ),
            &g
        ),
        Value::Text("two".into())
    );
    // No match + trailing default -> the default; no match + no default -> #N/A.
    assert_eq!(
        eval(
            &call("SWITCH", vec![num(9.0), num(1.0), txt("one"), txt("none")]),
            &g
        ),
        Value::Text("none".into())
    );
    assert_eq!(
        eval(&call("SWITCH", vec![num(9.0), num(1.0), txt("one")]), &g),
        Value::Error(ErrKind::Na)
    );
    // Text matching is case-insensitive (Excel `=`).
    assert_eq!(
        eval(
            &call("SWITCH", vec![txt("hello"), txt("HELLO"), num(1.0)]),
            &g
        ),
        Value::Number(1.0)
    );
    // The expression's error propagates.
    let div0 = Expr::Binary(
        crate::expr::BinOp::Div,
        Box::new(num(1.0)),
        Box::new(num(0.0)),
    );
    assert_eq!(
        eval(
            &call("SWITCH", vec![div0, num(1.0), txt("one"), txt("def")]),
            &g
        ),
        Value::Error(ErrKind::Div0)
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

// ---- Text batch v1 ------------------------------------------------------------------------

fn txt(s: &str) -> Expr {
    Expr::Lit(Value::Text(s.into()))
}
fn text(v: Value) -> String {
    match v {
        Value::Text(t) => t,
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn concat_and_textjoin() {
    let g = Grid::new(1, vec![Value::Blank]);
    // CONCAT stringifies each arg (number → general text, bool → TRUE/FALSE).
    assert_eq!(
        eval(
            &call(
                "CONCAT",
                vec![txt("a"), num(1.0), Expr::Lit(Value::Bool(true))]
            ),
            &g
        ),
        Value::Text("a1TRUE".into())
    );
    // CONCAT flattens a range (in-range blank → "").
    let r = Grid::new(
        1,
        vec![Value::Text("x".into()), Value::Blank, Value::Number(2.0)],
    );
    assert_eq!(
        eval(&call("CONCAT", vec![col_range(3)]), &r),
        Value::Text("x2".into())
    );
    // TEXTJOIN with ignore_empty=TRUE drops the blank; delimiter between kept pieces.
    assert_eq!(
        eval(
            &call(
                "TEXTJOIN",
                vec![txt("-"), Expr::Lit(Value::Bool(true)), col_range(3)]
            ),
            &r
        ),
        Value::Text("x-2".into())
    );
    // ignore_empty=FALSE keeps the empty slot (doubled delimiter).
    assert_eq!(
        eval(
            &call(
                "TEXTJOIN",
                vec![txt("-"), Expr::Lit(Value::Bool(false)), col_range(3)]
            ),
            &r
        ),
        Value::Text("x--2".into())
    );
    // An error anywhere propagates.
    let e = Grid::new(1, vec![Value::Number(1.0), Value::Error(ErrKind::Div0)]);
    assert_eq!(
        eval(&call("CONCAT", vec![col_range(2)]), &e),
        Value::Error(ErrKind::Div0)
    );
}

#[test]
fn left_right_mid_len_clamp_and_coerce() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        text(eval(&call("LEFT", vec![txt("hello"), num(2.0)]), &g)),
        "he"
    );
    assert_eq!(text(eval(&call("LEFT", vec![txt("hi")]), &g)), "h"); // default 1
    assert_eq!(
        text(eval(&call("RIGHT", vec![txt("hello"), num(3.0)]), &g)),
        "llo"
    );
    // Out-of-range count clamps to the whole string.
    assert_eq!(
        text(eval(&call("LEFT", vec![txt("hi"), num(99.0)]), &g)),
        "hi"
    );
    // Negative count is #VALUE!.
    assert_eq!(
        eval(&call("LEFT", vec![txt("hi"), num(-1.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    // MID from 1-based start, clamped take; start past end → "".
    assert_eq!(
        text(eval(
            &call("MID", vec![txt("hello"), num(2.0), num(3.0)]),
            &g
        )),
        "ell"
    );
    assert_eq!(
        text(eval(
            &call("MID", vec![txt("hello"), num(10.0), num(3.0)]),
            &g
        )),
        ""
    );
    assert_eq!(
        eval(&call("MID", vec![txt("hi"), num(0.0), num(1.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    // LEN over the general text of non-text values.
    assert_eq!(
        eval(&call("LEN", vec![txt("hello")]), &g),
        Value::Number(5.0)
    );
    assert_eq!(
        eval(&call("LEN", vec![Expr::Lit(Value::Bool(true))]), &g),
        Value::Number(4.0)
    );
}

#[test]
fn find_is_case_sensitive_search_is_wildcard() {
    let g = Grid::new(1, vec![Value::Blank]);
    // FIND: 1-based, case-SENSITIVE.
    assert_eq!(
        eval(&call("FIND", vec![txt("l"), txt("hello")]), &g),
        Value::Number(3.0)
    );
    assert_eq!(
        eval(&call("FIND", vec![txt("l"), txt("hello"), num(4.0)]), &g),
        Value::Number(4.0)
    );
    // Case mismatch → not found → #VALUE!.
    assert_eq!(
        eval(&call("FIND", vec![txt("H"), txt("hello")]), &g),
        Value::Error(ErrKind::Value)
    );
    // Empty needle returns start_num.
    assert_eq!(
        eval(&call("FIND", vec![txt(""), txt("abc")]), &g),
        Value::Number(1.0)
    );
    // SEARCH: case-INSENSITIVE and wildcards.
    assert_eq!(
        eval(&call("SEARCH", vec![txt("H"), txt("hello")]), &g),
        Value::Number(1.0)
    );
    assert_eq!(
        eval(&call("SEARCH", vec![txt("l?o"), txt("hello")]), &g),
        Value::Number(3.0)
    );
    assert_eq!(
        eval(&call("SEARCH", vec![txt("e*o"), txt("hello")]), &g),
        Value::Number(2.0)
    );
    // A literal `?` via `~?`.
    assert_eq!(
        eval(&call("SEARCH", vec![txt("~?"), txt("a?b")]), &g),
        Value::Number(2.0)
    );
    // start_num past len+1 is #VALUE!.
    assert_eq!(
        eval(&call("FIND", vec![txt("a"), txt("abc"), num(5.0)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn search_multi_star_matches_and_terminates_fast() {
    // Regression: the wildcard matcher is iterative (single-star backtrack), so a MULTI-star
    // pattern is O(text·pattern), not the old exponential recursive backtracking (a ReDoS). Both
    // arms complete instantly; the no-match arm is the one that used to blow up.
    let g = Grid::new(1, vec![Value::Blank]);
    // Multi-star, leftmost anchored match at position 1.
    assert_eq!(
        eval(&call("SEARCH", vec![txt("h*o*d"), txt("hello world")]), &g),
        Value::Number(1.0)
    );
    // The pathological shape: many stars over a long run with NO final match → #VALUE!, fast.
    let hay = "a".repeat(64);
    assert_eq!(
        eval(&call("SEARCH", vec![txt("*a*a*a*a*a*z"), txt(&hay)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn substitute_and_replace() {
    let g = Grid::new(1, vec![Value::Blank]);
    // SUBSTITUTE all occurrences.
    assert_eq!(
        text(eval(
            &call("SUBSTITUTE", vec![txt("a-b-c"), txt("-"), txt("+")]),
            &g
        )),
        "a+b+c"
    );
    // SUBSTITUTE the Nth only.
    assert_eq!(
        text(eval(
            &call(
                "SUBSTITUTE",
                vec![txt("a-b-c"), txt("-"), txt("+"), num(2.0)]
            ),
            &g
        )),
        "a-b+c"
    );
    // Empty old_text returns text unchanged.
    assert_eq!(
        text(eval(
            &call("SUBSTITUTE", vec![txt("abc"), txt(""), txt("X")]),
            &g
        )),
        "abc"
    );
    // instance_num < 1 is #VALUE!.
    assert_eq!(
        eval(
            &call("SUBSTITUTE", vec![txt("a-b"), txt("-"), txt("+"), num(0.0)]),
            &g
        ),
        Value::Error(ErrKind::Value)
    );
    // REPLACE positional splice.
    assert_eq!(
        text(eval(
            &call("REPLACE", vec![txt("abcdef"), num(2.0), num(3.0), txt("X")]),
            &g
        )),
        "aXef"
    );
    // start past end appends.
    assert_eq!(
        text(eval(
            &call("REPLACE", vec![txt("ab"), num(9.0), num(0.0), txt("!")]),
            &g
        )),
        "ab!"
    );
}

#[test]
fn trim_upper_lower() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(text(eval(&call("TRIM", vec![txt("  a   b  ")]), &g)), "a b");
    assert_eq!(text(eval(&call("UPPER", vec![txt("aBc")]), &g)), "ABC");
    assert_eq!(text(eval(&call("LOWER", vec![txt("aBc")]), &g)), "abc");
}

#[test]
fn text_format_subset_and_error_paths() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        text(eval(&call("TEXT", vec![num(12.5), txt("0.00")]), &g)),
        "12.50"
    );
    assert_eq!(
        text(eval(&call("TEXT", vec![num(-7.0), txt("0.00")]), &g)),
        "-7.00"
    );
    assert_eq!(
        text(eval(&call("TEXT", vec![num(1234567.0), txt("#,##0")]), &g)),
        "1,234,567"
    );
    assert_eq!(
        text(eval(&call("TEXT", vec![num(0.5), txt("0%")]), &g)),
        "50%"
    );
    assert_eq!(
        text(eval(&call("TEXT", vec![num(0.1234), txt("0.00%")]), &g)),
        "12.34%"
    );
    assert_eq!(
        text(eval(&call("TEXT", vec![num(5.0), txt("General")]), &g)),
        "5"
    );
    // The 1900 date system with the leap-year bug: serial 60 is the phantom 1900-02-29,
    // serial 61 is 1900-03-01, serial 44927 is 2023-01-01.
    assert_eq!(
        text(eval(
            &call("TEXT", vec![num(44927.0), txt("yyyy-mm-dd")]),
            &g
        )),
        "2023-01-01"
    );
    assert_eq!(
        text(eval(&call("TEXT", vec![num(60.0), txt("yyyy-mm-dd")]), &g)),
        "1900-02-29"
    );
    assert_eq!(
        text(eval(&call("TEXT", vec![num(61.0), txt("yyyy-mm-dd")]), &g)),
        "1900-03-01"
    );
    // Serial-band gate (regression: a large serial used to overflow `civil_from_days` — a panic
    // under overflow-checks, a wrapped nonsense date in release — instead of a located refusal).
    // The band `[1, MAX_SERIAL]` is refused as `#VALUE!` on BOTH edges plus `NaN`, never rendered.
    assert_eq!(
        eval(&call("TEXT", vec![num(1e300), txt("yyyy-mm-dd")]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("TEXT", vec![num(2_958_466.0), txt("yyyy-mm-dd")]), &g),
        Value::Error(ErrKind::Value)
    );
    assert_eq!(
        eval(&call("TEXT", vec![num(0.0), txt("yyyy-mm-dd")]), &g),
        Value::Error(ErrKind::Value)
    );
    // The exact upper edge (9999-12-31 = serial 2958465) still renders.
    assert_eq!(
        text(eval(
            &call("TEXT", vec![num(2_958_465.0), txt("yyyy-mm-dd")]),
            &g
        )),
        "9999-12-31"
    );
    // Error propagation and a non-numeric value into a numeric format.
    assert_eq!(
        eval(
            &call(
                "TEXT",
                vec![Expr::Lit(Value::Error(ErrKind::Div0)), txt("0.00")]
            ),
            &g
        ),
        Value::Error(ErrKind::Div0)
    );
    assert_eq!(
        eval(&call("TEXT", vec![txt("abc"), txt("0.00")]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn text_unsupported_literal_format_is_a_parse_refusal() {
    use crate::parse;
    // A supported literal format parses.
    assert!(parse("=TEXT(1,\"0.00\")").is_ok());
    // An unsupported literal format is a located `unsupported-format` refusal, not a wrong guess.
    let d = parse("=TEXT(1,\"$#,##0.00\")").unwrap_err();
    assert_eq!(d.code, crate::DiagCode::UnsupportedFormat);
}

#[test]
fn text_nonliteral_format_is_accepted_and_deferred_to_eval() {
    use crate::parse;
    // Accept-under-uncertainty (ast-standards PART 6): a computed format v1 cannot vet statically
    // is NOT refused at parse — a false-reject is the cardinal sin, since it RESOLVES-to-supported
    // at runtime (real Excel accepts and computes `=TEXT(A1, B1)`).
    let expr = parse("=TEXT(1,A1)").expect("a non-literal format parses (deferred to eval)");
    // A1 resolves to a SUPPORTED format → computes (the false-reject the old blanket refusal made).
    let supported = Grid::new(1, vec![Value::Text("0.00".to_string())]);
    assert_eq!(eval(&expr, &supported), Value::Text("1.00".to_string()));
    // A1 resolves to an UNSUPPORTED format → the deferred `#VALUE!` (a false-NEGATIVE, allowed).
    let unsupported = Grid::new(1, vec![Value::Text("$#,##0.00".to_string())]);
    assert_eq!(eval(&expr, &unsupported), Value::Error(ErrKind::Value));
}

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

// --- Lookup batch ------------------------------------------------------------------------

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
fn index_scalar_whole_row_whole_col_and_bounds() {
    let g = Grid::new(1, vec![Value::Blank]);
    // A 2×3 block {1,2,3;4,5,6}.
    let block = || arr(2, 3, vec![n(1.0), n(2.0), n(3.0), n(4.0), n(5.0), n(6.0)]);
    // Single cell (row 2, col 3) = 6.
    assert_eq!(
        eval(&call("INDEX", vec![block(), num(2.0), num(3.0)]), &g),
        n(6.0)
    );
    // Whole row 1 → 1×3 array.
    assert_eq!(
        eval(&call("INDEX", vec![block(), num(1.0), num(0.0)]), &g),
        Value::Array(
            crate::value::Shape { rows: 1, cols: 3 },
            vec![n(1.0), n(2.0), n(3.0)]
        )
    );
    // Whole col 2 → 2×1 array.
    assert_eq!(
        eval(&call("INDEX", vec![block(), num(0.0), num(2.0)]), &g),
        Value::Array(
            crate::value::Shape { rows: 2, cols: 1 },
            vec![n(2.0), n(5.0)]
        )
    );
    // Out-of-bounds row → #REF!.
    assert_eq!(
        eval(&call("INDEX", vec![block(), num(3.0), num(1.0)]), &g),
        Value::Error(ErrKind::Ref)
    );
    // Negative index → #VALUE!.
    assert_eq!(
        eval(&call("INDEX", vec![block(), num(-1.0), num(1.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    // 2-arg over a single column indexes the row; a blank cell reads as 0.
    let colv = arr(3, 1, vec![n(10.0), Value::Blank, n(30.0)]);
    assert_eq!(eval(&call("INDEX", vec![colv, num(2.0)]), &g), n(0.0));
}

#[test]
fn match_exact_and_approximate_both_directions() {
    let g = Grid::new(1, vec![Value::Blank]);
    let asc = || arr(4, 1, vec![n(10.0), n(20.0), n(30.0), n(40.0)]);
    // Approx ascending (default match_type 1): largest <= 25 is 20 → position 2.
    assert_eq!(eval(&call("MATCH", vec![num(25.0), asc()]), &g), n(2.0));
    // Needle below every key → #N/A.
    assert_eq!(
        eval(&call("MATCH", vec![num(5.0), asc(), num(1.0)]), &g),
        Value::Error(ErrKind::Na)
    );
    // Exact (match_type 0).
    assert_eq!(
        eval(&call("MATCH", vec![num(30.0), asc(), num(0.0)]), &g),
        n(3.0)
    );
    assert_eq!(
        eval(&call("MATCH", vec![num(31.0), asc(), num(0.0)]), &g),
        Value::Error(ErrKind::Na)
    );
    // Approx descending (match_type -1): smallest >= 25 is 30 → position 2.
    let desc = arr(4, 1, vec![n(40.0), n(30.0), n(20.0), n(10.0)]);
    assert_eq!(
        eval(&call("MATCH", vec![num(25.0), desc, num(-1.0)]), &g),
        n(2.0)
    );
    // Exact text needle honors wildcards, case-insensitively, first hit.
    let words = arr(2, 1, vec![t("apple"), t("banana")]);
    assert_eq!(
        eval(
            &call("MATCH", vec![Expr::Lit(t("BAN*")), words, num(0.0)]),
            &g
        ),
        n(2.0)
    );
    // A 2-D array is #N/A.
    let two_d = arr(2, 2, vec![n(1.0), n(2.0), n(3.0), n(4.0)]);
    assert_eq!(
        eval(&call("MATCH", vec![num(1.0), two_d, num(0.0)]), &g),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn vlookup_approximate_default_and_exact() {
    let g = Grid::new(1, vec![Value::Blank]);
    // {1,"one";2,"two";3,"three"} — first column sorted ascending.
    let table = || {
        arr(
            3,
            2,
            vec![n(1.0), t("one"), n(2.0), t("two"), n(3.0), t("three")],
        )
    };
    // Approx default: exact key present.
    assert_eq!(
        eval(&call("VLOOKUP", vec![num(2.0), table(), num(2.0)]), &g),
        t("two")
    );
    // Approx default: 2.5 falls to the largest key <= 2.5 → row of key 2.
    assert_eq!(
        eval(&call("VLOOKUP", vec![num(2.5), table(), num(2.0)]), &g),
        t("two")
    );
    // Exact (FALSE): 2.5 is not present → #N/A.
    assert_eq!(
        eval(
            &call(
                "VLOOKUP",
                vec![num(2.5), table(), num(2.0), Expr::Lit(Value::Bool(false))]
            ),
            &g
        ),
        Value::Error(ErrKind::Na)
    );
    // col_index past the table width → #REF!.
    assert_eq!(
        eval(&call("VLOOKUP", vec![num(2.0), table(), num(3.0)]), &g),
        Value::Error(ErrKind::Ref)
    );
    // A needle below every key → #N/A (approximate).
    assert_eq!(
        eval(&call("VLOOKUP", vec![num(0.0), table(), num(2.0)]), &g),
        Value::Error(ErrKind::Na)
    );
}

#[test]
fn xlookup_exact_default_if_not_found_and_approximate_modes() {
    let g = Grid::new(1, vec![Value::Blank]);
    let keys = || arr(3, 1, vec![n(1.0), n(2.0), n(3.0)]);
    let vals = || arr(3, 1, vec![t("a"), t("b"), t("c")]);
    // Exact by default.
    assert_eq!(
        eval(&call("XLOOKUP", vec![num(2.0), keys(), vals()]), &g),
        t("b")
    );
    // Miss returns the if_not_found argument (evaluated only on the miss).
    assert_eq!(
        eval(
            &call(
                "XLOOKUP",
                vec![num(9.0), keys(), vals(), Expr::Lit(t("none"))]
            ),
            &g
        ),
        t("none")
    );
    // Miss with no if_not_found → #N/A.
    assert_eq!(
        eval(&call("XLOOKUP", vec![num(9.0), keys(), vals()]), &g),
        Value::Error(ErrKind::Na)
    );
    // match_mode 1 = exact-or-next-LARGER: 2.5 → key 3 → "c".
    assert_eq!(
        eval(
            &call(
                "XLOOKUP",
                vec![num(2.5), keys(), vals(), Expr::Lit(t("x")), num(1.0)]
            ),
            &g
        ),
        t("c")
    );
    // match_mode -1 = exact-or-next-SMALLER: 2.5 → key 2 → "b".
    assert_eq!(
        eval(
            &call(
                "XLOOKUP",
                vec![num(2.5), keys(), vals(), Expr::Lit(t("x")), num(-1.0)]
            ),
            &g
        ),
        t("b")
    );
    // Reverse search returns the LAST exact hit on duplicate keys.
    let dupk = arr(3, 1, vec![n(5.0), n(5.0), n(5.0)]);
    let dupv = arr(3, 1, vec![t("first"), t("mid"), t("last")]);
    assert_eq!(
        eval(
            &call(
                "XLOOKUP",
                vec![num(5.0), dupk, dupv, Expr::Lit(t("x")), num(0.0), num(-1.0)]
            ),
            &g
        ),
        t("last")
    );
    // Mismatched array lengths → #VALUE!.
    let short = arr(2, 1, vec![t("a"), t("b")]);
    assert_eq!(
        eval(&call("XLOOKUP", vec![num(1.0), keys(), short]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn choose_selects_lazily_and_bounds_check() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(
        eval(
            &call(
                "CHOOSE",
                vec![num(2.0), Expr::Lit(t("a")), Expr::Lit(t("b"))]
            ),
            &g
        ),
        t("b")
    );
    // Only the selected branch is evaluated: an error in an unpicked branch never surfaces.
    let bad = Expr::Binary(
        crate::expr::BinOp::Div,
        Box::new(num(1.0)),
        Box::new(num(0.0)),
    );
    assert_eq!(
        eval(&call("CHOOSE", vec![num(1.0), num(7.0), bad]), &g),
        n(7.0)
    );
    // Out-of-range index → #VALUE!.
    assert_eq!(
        eval(&call("CHOOSE", vec![num(3.0), num(7.0), num(8.0)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn row_and_column_read_the_reference_node() {
    let g = Grid::new(1, vec![Value::Blank]);
    let b3 = Expr::Ref(crate::refs::RefNode {
        col: 1,
        row: 2,
        col_abs: false,
        row_abs: false,
        sheet: None,
    });
    assert_eq!(eval(&call("ROW", vec![b3.clone()]), &g), n(3.0));
    assert_eq!(eval(&call("COLUMN", vec![b3]), &g), n(2.0));
    // A range reads its top-left.
    let rng = Expr::Range(RangeNode {
        start_col: 2,
        start_row: 4,
        end_col: 5,
        end_row: 9,
        sheet: None,
    });
    assert_eq!(eval(&call("ROW", vec![rng.clone()]), &g), n(5.0));
    assert_eq!(eval(&call("COLUMN", vec![rng]), &g), n(3.0));
    // A non-reference argument is #VALUE!.
    assert_eq!(
        eval(&call("ROW", vec![num(5.0)]), &g),
        Value::Error(ErrKind::Value)
    );
}

#[test]
fn indirect_and_offset_are_reserved_ref_function_refusals() {
    use crate::diag::DiagCode;
    // Both refuse at PARSE with the located reserved-ref-function code — never unknown-function,
    // never a value. The span points at the call name (offset 1, right after `=`).
    for (formula, name_len) in [("=INDIRECT(\"A1\")", 8), ("=OFFSET(A1,1,1)", 6)] {
        let err = crate::parse(formula).expect_err("reserved ref function refuses");
        assert_eq!(err.code, DiagCode::ReservedRefFunction, "{formula}");
        assert_eq!(err.span.start, 1, "located on the name: {formula}");
        assert_eq!(err.span.end, 1 + name_len, "spans the name: {formula}");
    }
    // The refusal fires REGARDLESS of argument count (arity never gates first).
    assert_eq!(
        crate::parse("=INDIRECT()").expect_err("still refuses").code,
        DiagCode::ReservedRefFunction
    );
}

#[test]
fn information_predicates_report_operand_kind() {
    let g = Grid::new(1, vec![Value::Blank]);
    // ISBLANK: a blank literal is TRUE; a number/text/empty-string is FALSE.
    assert_eq!(
        eval(&call("ISBLANK", vec![Expr::Lit(Value::Blank)]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISBLANK", vec![num(0.0)]), &g),
        Value::Bool(false)
    );
    assert_eq!(
        eval(&call("ISBLANK", vec![Expr::Lit(t(""))]), &g),
        Value::Bool(false)
    );
    // ISNUMBER / ISTEXT discriminate the scalar kinds (a bool is NEITHER a number nor text).
    assert_eq!(
        eval(&call("ISNUMBER", vec![num(3.5)]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISNUMBER", vec![Expr::Lit(t("3"))]), &g),
        Value::Bool(false)
    );
    assert_eq!(
        eval(&call("ISTEXT", vec![Expr::Lit(t("hi"))]), &g),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&call("ISTEXT", vec![num(3.0)]), &g),
        Value::Bool(false)
    );
    assert_eq!(
        eval(&call("ISNUMBER", vec![Expr::Lit(Value::Bool(true))]), &g),
        Value::Bool(false)
    );
    assert_eq!(
        eval(&call("ISTEXT", vec![Expr::Lit(Value::Bool(true))]), &g),
        Value::Bool(false)
    );
}

#[test]
fn iserror_is_error_transparent_not_propagating() {
    let g = Grid::new(1, vec![Value::Blank]);
    // The defining case: an operand that EVALUATES to an error is CAUGHT (TRUE), never returned.
    let div0 = Expr::Binary(
        crate::expr::BinOp::Div,
        Box::new(num(1.0)),
        Box::new(num(0.0)),
    );
    assert_eq!(eval(&call("ISERROR", vec![div0]), &g), Value::Bool(true));
    // A literal #N/A is an error too (ISERROR catches #N/A, unlike the deferred ISERR).
    assert_eq!(
        eval(
            &call("ISERROR", vec![Expr::Lit(Value::Error(ErrKind::Na))]),
            &g
        ),
        Value::Bool(true)
    );
    // NA() itself is an error operand -> ISERROR(NA()) is TRUE.
    assert_eq!(
        eval(&call("ISERROR", vec![call("NA", vec![])]), &g),
        Value::Bool(true)
    );
    // A non-error operand is FALSE — the error path is not spuriously taken.
    assert_eq!(
        eval(&call("ISERROR", vec![num(5.0)]), &g),
        Value::Bool(false)
    );
    // NA() with no argument mints #N/A directly (the sole error-PRODUCING member).
    assert_eq!(eval(&call("NA", vec![]), &g), Value::Error(ErrKind::Na));
}

#[test]
fn type_reports_the_code_and_is_error_and_array_transparent() {
    let g = Grid::new(1, vec![Value::Blank]);
    assert_eq!(eval(&call("TYPE", vec![num(1.0)]), &g), n(1.0));
    assert_eq!(eval(&call("TYPE", vec![Expr::Lit(t("x"))]), &g), n(2.0));
    assert_eq!(
        eval(&call("TYPE", vec![Expr::Lit(Value::Bool(true))]), &g),
        n(4.0)
    );
    // An error operand reports 16 — TRANSPARENT, not propagated as the error itself.
    assert_eq!(eval(&call("TYPE", vec![call("NA", vec![])]), &g), n(16.0));
    // An empty cell reports as a number (1), Excel-faithful.
    assert_eq!(
        eval(&call("TYPE", vec![Expr::Lit(Value::Blank)]), &g),
        n(1.0)
    );
    // A multi-cell array operand reports 64 — inspected AS an array, not scalarized to #VALUE!.
    assert_eq!(
        eval(&call("TYPE", vec![arr(1, 2, vec![n(1.0), n(2.0)])]), &g),
        n(64.0)
    );
    // And the IS-predicates see an array as "not a number/text/blank/error" -> FALSE (transparent).
    assert_eq!(
        eval(&call("ISERROR", vec![arr(1, 2, vec![n(1.0), n(2.0)])]), &g),
        Value::Bool(false)
    );
    assert_eq!(
        eval(&call("ISNUMBER", vec![arr(1, 2, vec![n(1.0), n(2.0)])]), &g),
        Value::Bool(false)
    );
}

// --- Financial batch (PMT / NPV / IRR) ---------------------------------------------------

#[test]
fn pmt_rate_zero_is_linear_and_nonzero_is_the_annuity_form() {
    let g = Grid::new(1, vec![Value::Blank]);
    // rate == 0 -> linear: -(pv+fv)/nper = -(-1000+0)/10 = 100.
    assert_eq!(
        eval(&call("PMT", vec![num(0.0), num(10.0), num(-1000.0)]), &g),
        n(100.0)
    );
    // rate != 0 annuity: PMT(0.5, 2, -100) = 90 exactly (1.5^2 = 2.25 is dyadic-exact).
    assert_eq!(
        eval(&call("PMT", vec![num(0.5), num(2.0), num(-100.0)]), &g),
        n(90.0)
    );
    // With a future value: PMT(0.5, 2, -100, -50) = 110.
    assert_eq!(
        eval(
            &call("PMT", vec![num(0.5), num(2.0), num(-100.0), num(-50.0)]),
            &g
        ),
        n(110.0)
    );
    // type = 1 (annuity due): PMT(0.5, 2, -100, 0, 1) = 60.
    assert_eq!(
        eval(
            &call(
                "PMT",
                vec![num(0.5), num(2.0), num(-100.0), num(0.0), num(1.0)]
            ),
            &g
        ),
        n(60.0)
    );
}

#[test]
fn pmt_zero_denominator_is_div0_and_errors_propagate() {
    let g = Grid::new(1, vec![Value::Blank]);
    // nper == 0 with rate == 0 -> #DIV/0! (linear branch divides by nper).
    assert_eq!(
        eval(&call("PMT", vec![num(0.0), num(0.0), num(100.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
    // nper == 0 with rate != 0 -> denom (temp-1) is 0 -> #DIV/0!.
    assert_eq!(
        eval(&call("PMT", vec![num(0.1), num(0.0), num(100.0)]), &g),
        Value::Error(ErrKind::Div0)
    );
    // A propagated error argument (1/0) short-circuits.
    let bad = call(
        "PMT",
        vec![call("SQRT", vec![num(-1.0)]), num(2.0), num(1.0)],
    );
    assert_eq!(eval(&bad, &g), Value::Error(ErrKind::Num));
}

#[test]
fn npv_discounts_from_period_one_over_args_and_ranges() {
    let g = Grid::new(1, vec![Value::Blank]);
    // NPV(1, 100, 200, 300) = 100/2 + 200/4 + 300/8 = 50 + 50 + 37.5 = 137.5 (all dyadic-exact).
    assert_eq!(
        eval(
            &call("NPV", vec![num(1.0), num(100.0), num(200.0), num(300.0)]),
            &g
        ),
        n(137.5)
    );
    // Same values inside a range flatten row-major and discount identically.
    let grid = Grid::new(1, vec![n(100.0), n(200.0), n(300.0)]);
    assert_eq!(
        eval(&call("NPV", vec![num(1.0), col_range(3)]), &grid),
        n(137.5)
    );
    // An error value propagates.
    let bad = call(
        "NPV",
        vec![num(1.0), num(100.0), call("SQRT", vec![num(-1.0)])],
    );
    assert_eq!(eval(&bad, &g), Value::Error(ErrKind::Num));
}

#[test]
fn irr_converges_to_the_root_matching_the_independent_oracle() {
    let g = Grid::new(1, vec![Value::Blank]);
    // IRR([-100, 30, 40, 50]) — the hand-Newton oracle's exact f64 (finance_oracle.py; NPV at
    // this rate is ~7e-15, cross-checked vs numpy np.roots = 0.088963394693350… to ~1e-12).
    let cf = arr(1, 4, vec![n(-100.0), n(30.0), n(40.0), n(50.0)]);
    assert_eq!(
        eval(&call("IRR", vec![cf]), &g),
        n(0.088_963_394_693_349_92)
    );
}

#[test]
fn irr_non_convergent_cashflows_are_num_never_a_hang() {
    let g = Grid::new(1, vec![Value::Blank]);
    // All-positive flows have no sign change -> no real IRR. Newton diverges, bisection finds no
    // bracket, and the HARD iteration caps guarantee a prompt #NUM! rather than an infinite loop.
    let allpos = arr(1, 3, vec![n(100.0), n(200.0), n(300.0)]);
    assert_eq!(
        eval(&call("IRR", vec![allpos]), &g),
        Value::Error(ErrKind::Num)
    );
    // A single flow can never bracket a root either -> #NUM! (guarded before iterating).
    let one = arr(1, 1, vec![n(-100.0)]);
    assert_eq!(
        eval(&call("IRR", vec![one]), &g),
        Value::Error(ErrKind::Num)
    );
    // A supplied guess still converges to the SAME root (to within tolerance — a different start
    // trajectory can settle on a neighbouring ULP, so this is a closeness check, not bit-exact).
    let cf = arr(1, 4, vec![n(-100.0), n(30.0), n(40.0), n(50.0)]);
    match eval(&call("IRR", vec![cf, num(0.3)]), &g) {
        Value::Number(x) => assert!(
            (x - 0.088_963_394_693_349_92).abs() < 1e-9,
            "converged to {x} from guess 0.3"
        ),
        other => panic!("expected a converged rate, got {other:?}"),
    }
}

#[test]
fn finance_arity_is_gated_by_dispatch() {
    let g = Grid::new(1, vec![Value::Blank]);
    // PMT needs 3..=5 args — 2 is under-arity -> #VALUE! from the dispatch arity gate.
    assert_eq!(
        eval(&call("PMT", vec![num(0.1), num(2.0)]), &g),
        Value::Error(ErrKind::Value)
    );
    // NPV needs >= 2 — a lone rate is under-arity.
    assert_eq!(
        eval(&call("NPV", vec![num(0.1)]), &g),
        Value::Error(ErrKind::Value)
    );
}
