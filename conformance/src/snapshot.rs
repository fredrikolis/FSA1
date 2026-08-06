// Concern: the graded facts of one corpus run — verdicts and coverage | Non-concern: grading a fixture, CLI dispatch | IO: (verdicts) -> Facts

use std::collections::BTreeMap;

use crate::corpus::Fixture;

/// The v1 core function count (`docs/architecture.md` §4) — the coverage denominator.
pub const V1_FUNCTION_TARGET: usize = 70;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictKind {
    Match,
    Diverge,
}

impl VerdictKind {
    pub fn as_str(self) -> &'static str {
        match self {
            VerdictKind::Match => "MATCH",
            VerdictKind::Diverge => "DIVERGE",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Verdict {
    pub key: String,
    pub kind: VerdictKind,
    pub detail: String,
}

impl Verdict {
    pub fn matched(key: &str) -> Verdict {
        Verdict {
            key: key.to_string(),
            kind: VerdictKind::Match,
            detail: String::new(),
        }
    }

    pub fn diverge(key: &str, detail: String) -> Verdict {
        Verdict {
            key: key.to_string(),
            kind: VerdictKind::Diverge,
            detail: detail.replace(['\t', '\n', '\r'], " "),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    pub modeled: usize,
    pub target: usize,
    /// What the live registry defines — context for the numerator, never the denominator.
    pub registry: usize,
    pub functions: Vec<String>,
}

impl Coverage {
    pub fn compute(fixtures: &[Fixture], verdicts: &[Verdict]) -> Coverage {
        let matched: std::collections::HashSet<&str> = verdicts
            .iter()
            .filter(|v| v.kind == VerdictKind::Match)
            .map(|v| v.key.as_str())
            .collect();

        let registry: std::collections::HashSet<String> = fsa1_ast::func::FUNCS
            .iter()
            .map(|f| f.name.to_ascii_uppercase())
            .collect();

        let mut modeled: Vec<String> = Vec::new();
        for f in fixtures {
            if !matched.contains(f.key.as_str()) {
                continue;
            }
            let called = called_funcs(&f.formula);
            for name in &f.funcs {
                if registry.contains(name.as_str()) && called.contains(name.as_str()) {
                    modeled.push(name.clone());
                }
            }
        }
        modeled.sort();
        modeled.dedup();

        Coverage {
            modeled: modeled.len(),
            target: V1_FUNCTION_TARGET,
            registry: registry.len(),
            functions: modeled,
        }
    }

    pub fn line(&self) -> String {
        format!(
            "coverage: modeled={} / target={} (registry={} functions defined)",
            self.modeled, self.target, self.registry
        )
    }
}

/// The functions `formula` actually calls — the cross-check that a mislabeled `funcs:` cannot inflate.
fn called_funcs(formula: &str) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    if let Ok(expr) = fsa1_ast::parse(formula) {
        collect_calls(&expr, &mut out);
    }
    out
}

fn collect_calls(expr: &fsa1_ast::Expr, out: &mut std::collections::HashSet<String>) {
    use fsa1_ast::Expr;
    match expr {
        Expr::Call(fid, args) => {
            if let Some(d) = fsa1_ast::func::def(*fid) {
                out.insert(d.name.to_ascii_uppercase());
            }
            for a in args {
                collect_calls(a, out);
            }
        }
        Expr::Unary(_, inner) => collect_calls(inner, out),
        Expr::Binary(_, l, r) => {
            collect_calls(l, out);
            collect_calls(r, out);
        }
        Expr::ImplicitIntersect(inner) | Expr::SpillRef(inner) => collect_calls(inner, out),
        Expr::Lit(_) | Expr::Ref(_) | Expr::Range(_) => {}
    }
}

#[derive(Debug, Clone)]
pub struct Facts {
    pub coverage: Coverage,
    pub verdicts: BTreeMap<String, Verdict>,
}

impl Facts {
    pub fn capture(fixtures: &[Fixture], verdicts: Vec<Verdict>) -> Facts {
        let coverage = Coverage::compute(fixtures, &verdicts);
        let verdicts = verdicts.into_iter().map(|v| (v.key.clone(), v)).collect();
        Facts { coverage, verdicts }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fsa1_ast::Value;

    #[test]
    fn coverage_counts_only_registry_functions_with_a_matching_fixture() {
        let fixtures = vec![
            Fixture {
                key: "agg/sum".into(),
                funcs: vec!["SUM".into()],
                formula: "=SUM(A1:A2)".into(),
                expect: Value::Number(3.0),
                cells: vec![],
                at: None,
            },
            Fixture {
                key: "agg/not-in-registry".into(),
                funcs: vec!["LAMBDA".into()],
                formula: "=LAMBDA(A1,A1:A2)".into(),
                expect: Value::Number(3.0),
                cells: vec![],
                at: None,
            },
            Fixture {
                key: "agg/diverges".into(),
                funcs: vec!["ROUND".into()],
                formula: "=ROUND(1,0)".into(),
                expect: Value::Number(99.0),
                cells: vec![],
                at: None,
            },
            Fixture {
                key: "agg/mislabeled".into(),
                funcs: vec!["ABS".into()],
                formula: "=1+1".into(),
                expect: Value::Number(2.0),
                cells: vec![],
                at: None,
            },
        ];
        let verdicts = vec![
            Verdict::matched("agg/sum"),
            Verdict::matched("agg/not-in-registry"),
            Verdict::diverge("agg/diverges", "x".into()),
            Verdict::matched("agg/mislabeled"),
        ];
        let cov = Coverage::compute(&fixtures, &verdicts);
        assert_eq!(
            cov.functions,
            vec!["SUM"],
            "off the registry, diverging, and mislabeled fixtures all contribute nothing"
        );
        assert_eq!(cov.modeled, 1);
        assert_eq!(cov.target, V1_FUNCTION_TARGET);
    }
}
