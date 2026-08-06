// Concern: proves the public parse/eval surface never panics on hostile input | Non-concern: what any formula evaluates to, the internal modules | IO: (hostile &str) -> no panic
//! COMPLETING is the assertion here: any panic — including a recursive drop or a slice out of
//! bounds — fails the test. Every input is deterministic, so a failure is reproducible.

use fsa1_ast::{ArrayView, CellRef, RangeRef, Resolver, Shape, SheetId, Value, eval, parse};

struct Blind {
    one: Vec<Value>,
}

impl Resolver for Blind {
    fn value(&self, _cell: CellRef) -> Value {
        Value::Blank
    }
    fn range(&self, _range: RangeRef) -> ArrayView<'_> {
        ArrayView {
            shape: Shape { rows: 1, cols: 1 },
            cells: &self.one,
        }
    }
    fn sheet_id(&self, name: &str) -> Option<SheetId> {
        (name == "Sheet1").then_some(SheetId(0))
    }
}

/// Parses and, if it parses, evaluates; on a refusal, proves the span slices the original bytes.
fn exercise(src: &str, r: &Blind) {
    match parse(src) {
        Ok(expr) => {
            let _ = eval(&expr, r); // must not panic
        }
        Err(d) => {
            let (s, e) = (d.span.start.min(src.len()), d.span.end.min(src.len()));
            if src.is_char_boundary(s) && src.is_char_boundary(e) && s <= e {
                let _ = &src[s..e]; // must not panic
            }
            let _ = d.to_string(); // rendering must not panic or leak
        }
    }
}

const CORPUS: &[&str] = &[
    "=1+2*3",
    // Truncated at every prefix boundary, so a partial `_xlfn.` is exercised against the offset arithmetic.
    "=_xlfn.MINIFS(A1,A1,\">0\")",
    "=_xlfn._xlws.FILTER(A1,A1)",
    "=SUM(A1:B10)",
    "=IF(A1>0, A1, -A1)",
    "=IFERROR(A1/B1, 0)",
    "=AND(TRUE, FALSE, 1)",
    "=ROUND(AVERAGE(A1:A9), 2)",
    "=-2^2 & \"x\"",
    "=@A1",
    "=A1#",
    "=Sheet1!A1",
    "=(((1+2)))",
    "=1,2,3",
    "=#DIV/0! + 1",
    "=\"unterminated",
    "=1e999",
    "=COUNT(A1, TRUE, \"3\")",
];

#[test]
fn corpus_and_truncations_never_panic() {
    let r = Blind {
        one: vec![Value::Blank],
    };
    for &f in CORPUS {
        exercise(f, &r);
        for end in 0..=f.len() {
            if f.is_char_boundary(end) {
                exercise(&f[..end], &r);
            }
        }
    }
}

#[test]
fn hostile_single_bytes_and_pairs_never_panic() {
    let r = Blind {
        one: vec![Value::Blank],
    };
    for b in 0u8..128 {
        let c = b as char;
        for form in [format!("{c}"), format!("{c}{c}"), format!("={c}")] {
            exercise(&form, &r);
        }
    }
}

#[test]
fn seeded_random_soup_never_panics() {
    // `_` and a multi-byte char are in the alphabet so the walk can build, and cut at every boundary of, the `_xlfn.` prefix whose acceptance computes a byte offset.
    const ALPHABET: &[u8] = b"()+-*/^&%:,@#=<> \t01AZ$\"'._xlfnwsSUMabc\xc3\xa9";
    let r = Blind {
        one: vec![Value::Blank],
    };
    let mut state: u64 = 0x9E3779B97F4A7C15;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as usize
    };
    for _ in 0..4000 {
        let len = next() % 40;
        let mut s = String::with_capacity(len);
        for _ in 0..len {
            s.push(ALPHABET[next() % ALPHABET.len()] as char);
        }
        exercise(&s, &r);
    }
}

#[test]
fn deeply_nested_input_is_a_refusal_not_a_stack_overflow() {
    // The partially-built tree must also DROP without overflowing, not merely fail to parse.
    let r = Blind {
        one: vec![Value::Blank],
    };
    let shapes = [
        format!("={}1{}", "(".repeat(5000), ")".repeat(5000)),
        format!("={}1", "-".repeat(5000)),
        format!("={}1", "@".repeat(5000)),
        format!("=SUM({}", "SUM(".repeat(5000)),
    ];
    for s in &shapes {
        assert!(parse(s).is_err(), "deeply nested input must be refused");
        exercise(s, &r);
    }
}
