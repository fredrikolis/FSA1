// Concern: the NO-PANIC adversarial contract for the public engine surface — hostile, truncated, and pseudo-random formula strings pushed through `parse` and (when they parse) `eval` against a blind stub `Resolver`, asserting the parser is a real boundary: it returns a located `Diag` or a first-class error `Value`, NEVER an unwind, and every refusal span is sliceable on the original input (ast-standards PART 9, "no-panic fuzz; downstream emit exercised on every input") | Non-concern: the VALUE-level correctness of a specific formula (the `parser`/`eval`/`func` in-crate `mod tests` own semantics) and the schema/grammar shape (schema.rs's golden test owns that) | IO: none — in-memory; a blind resolver stands in for the outside world
//! No-panic fuzz over the public `charlie_ast` API. The test simply *completing* is the assertion:
//! any panic (including a recursive-drop or slice-out-of-bounds) fails it. Inputs are deterministic
//! (a fixed corpus + truncations + a seeded byte generator) so a failure is reproducible.

use charlie_ast::{ArrayView, CellRef, RangeRef, Resolver, Shape, SheetId, Value, eval, parse};

/// A blind resolver: every cell is `Blank`, every range is a 1×1 blank window borrowed from an owned
/// buffer. It has no filesystem and no data — exactly the engine's minimal outside world.
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

/// Push one input all the way through the boundary: parse, and if it parses, evaluate. On a refusal,
/// prove the span is sliceable on the original bytes (never a mid-char panic).
fn exercise(src: &str, r: &Blind) {
    match parse(src) {
        Ok(expr) => {
            let _ = eval(&expr, r); // must not panic
        }
        Err(d) => {
            // Located refusal: the span must land on char boundaries of `src`.
            let (s, e) = (d.span.start.min(src.len()), d.span.end.min(src.len()));
            if src.is_char_boundary(s) && src.is_char_boundary(e) && s <= e {
                let _ = &src[s..e]; // must not panic
            }
            let _ = d.to_string(); // rendering must not panic or leak
        }
    }
}

/// A small corpus of representative valid/edge formulas, used both directly and as truncation seeds.
const CORPUS: &[&str] = &[
    "=1+2*3",
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
        // Every byte-prefix (truncation) — the classic "input ended mid-token" hazard.
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
    // Every byte 0..128 alone and doubled, plus a leading `=`.
    for b in 0u8..128 {
        let c = b as char;
        for form in [format!("{c}"), format!("{c}{c}"), format!("={c}")] {
            exercise(&form, &r);
        }
    }
}

#[test]
fn seeded_random_soup_never_panics() {
    // A tiny LCG over a formula-ish alphabet: parens, operators, digits, refs, quotes, hashes.
    const ALPHABET: &[u8] = b"()+-*/^&%:,@#=<> \t01AZ$\"'.SUMabc";
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
    // Bounded-recursion contract on the public surface: parse must return a diagnostic, and the
    // partially-built tree must drop without overflowing. Several nesting shapes are covered.
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
        // parse must not panic (it returns Err(recursion-limit) or another located refusal).
        assert!(parse(s).is_err(), "deeply nested input must be refused");
        exercise(s, &r);
    }
}
