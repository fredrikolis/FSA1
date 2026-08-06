// Concern: grades one fixture against its EXPECTED value | Non-concern: reading the corpus, the snapshot machinery | IO: (&Fixture) -> Verdict

use fsa1_ast::{eval, eval_at, parse};

use crate::corpus::Fixture;
use crate::literal::show;
use crate::resolver::StubResolver;
use crate::snapshot::Verdict;

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
            let got = match fx.at {
                Some((row, col)) => eval_at(&expr, &resolver, row, col),
                None => eval(&expr, &resolver),
            };
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

pub fn grade_all(fixtures: &[Fixture]) -> Vec<Verdict> {
    fixtures.iter().map(grade).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::VerdictKind;
    use fsa1_ast::{ErrKind, Value};

    fn fx(
        key: &str,
        formula: &str,
        expect: Value,
        cells: Vec<(Option<String>, u32, u32, Value)>,
    ) -> Fixture {
        Fixture {
            key: key.to_string(),
            funcs: vec![],
            formula: formula.to_string(),
            expect,
            cells,
            at: None,
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
    fn an_at_fixture_grades_no_arg_row_column_against_its_computing_cell() {
        let mut f = fx("t/row-at", "=ROW()", Value::Number(5.0), vec![]);
        f.at = Some((4, 2));
        assert_eq!(grade(&f).kind, VerdictKind::Match, "at C5 -> ROW() is 5");
        let ad_hoc = fx("t/row-adhoc", "=ROW()", Value::Number(1.0), vec![]);
        assert_eq!(
            grade(&ad_hoc).kind,
            VerdictKind::Match,
            "no `at` -> the ad-hoc path anchors to A1"
        );
    }

    #[test]
    fn a_parse_refusal_is_a_diverge_not_a_panic() {
        let d = grade(&fx("t/refuse", "=myname", Value::Number(1.0), vec![]));
        assert_eq!(d.kind, VerdictKind::Diverge);
        assert!(d.detail.contains("refused to parse"), "{}", d.detail);
    }

    #[test]
    fn a_cross_sheet_fixture_grades_a_value() {
        let m = grade(&fx(
            "t/cross",
            "=Data!A1",
            Value::Number(42.0),
            vec![(Some("Data".to_string()), 0, 0, Value::Number(42.0))],
        ));
        assert_eq!(m.kind, VerdictKind::Match);
    }
}
