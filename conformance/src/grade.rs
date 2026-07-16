// Concern: GRADING one fixture — parse its formula through charlie-ast, build the stub Resolver from its context, evaluate, and compare the produced `Value` to the fixture's EXPECTED value BIT-EXACTLY (Value's own `Eq` is exact — `-0.0 ≠ 0.0`, `NaN == NaN`), yielding a `Match` or a `Diverge` carrying the expected-vs-got detail; a parse REFUSAL of a value-corpus formula is itself a `Diverge` (a value was expected, a refusal is not one) | Non-concern: reading the corpus (corpus.rs) and the snapshot/backslide machinery (snapshot.rs) — this is the pure `Fixture -> Verdict` step | IO: (&Fixture) -> Verdict
//! The grader: the one place a fixture becomes a verdict. It runs the SAME public path a real caller
//! would — `charlie_ast::parse` then `charlie_ast::eval` against a `Resolver` — so a Match certifies
//! the shipping engine, not a re-implementation of it. The oracle (the expected value) is authored
//! externally (`formula/PROVENANCE.md`); this never edits it to match a divergence.

use charlie_ast::{eval, parse};

use crate::corpus::Fixture;
use crate::literal::show;
use crate::resolver::StubResolver;
use crate::snapshot::Verdict;

/// Grade one fixture into a [`Verdict`]. Never panics — a parse refusal is a surfaced `Diverge`.
pub fn grade(fx: &Fixture) -> Verdict {
    match parse(&fx.formula) {
        Err(diag) => Verdict::diverge(
            &fx.key,
            format!(
                "formula refused to parse (a value was expected {}): {diag}",
                show(&fx.expect)
            ),
        ),
        Ok(expr) => {
            let resolver = StubResolver::build(&fx.cells, &expr);
            let got = eval(&expr, &resolver);
            if got == fx.expect {
                Verdict::matched(&fx.key)
            } else {
                Verdict::diverge(
                    &fx.key,
                    format!("expected {}, got {}", show(&fx.expect), show(&got)),
                )
            }
        }
    }
}

/// Grade the whole corpus, sorted by key (the corpus loader already sorts). The one entry the
/// snapshot/backslide/report verbs all build on.
pub fn grade_all(fixtures: &[Fixture]) -> Vec<Verdict> {
    fixtures.iter().map(grade).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::VerdictKind;
    use charlie_ast::{ErrKind, Value};

    fn fx(key: &str, formula: &str, expect: Value, cells: Vec<(u32, u32, Value)>) -> Fixture {
        Fixture {
            key: key.to_string(),
            funcs: vec![],
            formula: formula.to_string(),
            expect,
            cells,
        }
    }

    #[test]
    fn a_matching_fixture_is_match_and_a_wrong_one_diverges() {
        let m = grade(&fx("t/ok", "=1+1", Value::Number(2.0), vec![]));
        assert_eq!(m.kind, VerdictKind::Match);

        let d = grade(&fx("t/bad", "=1+1", Value::Number(3.0), vec![]));
        assert_eq!(d.kind, VerdictKind::Diverge);
        assert!(d.detail.contains("expected 3, got 2"), "{}", d.detail);
    }

    #[test]
    fn an_error_valued_fixture_matches_the_error() {
        let m = grade(&fx("t/div0", "=1/0", Value::Error(ErrKind::Div0), vec![]));
        assert_eq!(m.kind, VerdictKind::Match);
    }

    #[test]
    fn a_parse_refusal_is_a_diverge_not_a_panic() {
        // A cross-sheet ref is a parse-time refusal in charlie-ast — grading it as a value diverges.
        let d = grade(&fx("t/refuse", "=Sheet1!A1", Value::Number(1.0), vec![]));
        assert_eq!(d.kind, VerdictKind::Diverge);
        assert!(d.detail.contains("refused to parse"), "{}", d.detail);
    }
}
