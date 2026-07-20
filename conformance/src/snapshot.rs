// Concern: the FACTS SNAPSHOT + the backslide diff — the per-fixture `Verdict` (Match/Diverge + detail), the `Coverage` ratchet (modeled functions / v1 target, with an honest denominator and a numerator cross-checked against each formula's actual `Call`s), the whole-state `Facts` crumb and its line-based TSV (de)serialization (the committed anchor that travels with the commit), and the REGRESSION-ONLY diff: the fixtures that LEFT Match vs the anchor, growth-exempt by construction, with the exit-code contract (0 clean / 1 ≥1 lost Match) | Non-concern: GRADING a fixture (grade.rs) and CLI dispatch / anchor IO wiring (main.rs) — this owns the model, its wire form, and the backslide rule | IO: (a corpus + its verdicts) -> a `Facts`; (Facts text) -> Facts; (anchor, current) -> the lost-Match set + exit code
//! The facts snapshot is the coverage ratchet's memory. It records, per fixture, whether charlie's
//! evaluated value Matched the external oracle — and the backslide guard reddens ONLY when a fixture
//! that Matched in the committed anchor no longer does. Growth (a new non-Matching fixture) and
//! improvement (a Diverge that became a Match) never block, so the corpus is free to grow. The wire
//! form is a hand-written TSV (no serde dependency) that is greppable and git-diff-friendly.

use std::collections::BTreeMap;

use crate::corpus::Fixture;

/// The snapshot wire-schema tag — the first line of the anchor. A mismatch is fail-fast: the anchor
/// is our own artifact, so a foreign/older shape is a broken invariant, not input to tolerate.
pub const SCHEMA: &str = "charlie-formula-conformance/v1";

/// The v1 function target — the honest denominator for the coverage ratchet. charlie v1 ships a
/// ~70-function core (`docs/architecture.md` §4); a function counts as *modeled* only once it is in
/// the registry AND has a Matching conformance fixture.
pub const V1_FUNCTION_TARGET: usize = 70;

/// One fixture's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictKind {
    /// The evaluated value equalled the external oracle.
    Match,
    /// It did not (a surfaced FACT, never itself a gate failure — see the backslide rule).
    Diverge,
}

impl VerdictKind {
    /// The wire token.
    pub fn as_str(self) -> &'static str {
        match self {
            VerdictKind::Match => "MATCH",
            VerdictKind::Diverge => "DIVERGE",
        }
    }
}

/// A fixture's graded verdict: its key, the kind, and (for a Diverge) the expected-vs-got detail.
#[derive(Debug, Clone)]
pub struct Verdict {
    pub key: String,
    pub kind: VerdictKind,
    pub detail: String,
}

impl Verdict {
    /// A Match verdict (no detail).
    pub fn matched(key: &str) -> Verdict {
        Verdict {
            key: key.to_string(),
            kind: VerdictKind::Match,
            detail: String::new(),
        }
    }

    /// A Diverge verdict carrying its explanatory detail.
    pub fn diverge(key: &str, detail: String) -> Verdict {
        Verdict {
            key: key.to_string(),
            kind: VerdictKind::Diverge,
            // The detail rides in a single TSV column, so a tab/newline in it would corrupt the row.
            detail: detail.replace(['\t', '\n', '\r'], " "),
        }
    }
}

/// The monotonic coverage ratchet: how many functions are conformance-modeled, out of the v1 target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    /// Distinct registry functions with ≥1 Matching fixture — the honest numerator.
    pub modeled: usize,
    /// The v1 target denominator ([`V1_FUNCTION_TARGET`]).
    pub target: usize,
    /// How many functions the live registry defines (context for the numerator; NOT the denominator).
    pub registry: usize,
    /// The sorted names of the modeled functions.
    pub functions: Vec<String>,
}

impl Coverage {
    /// Compute coverage from the graded corpus: a function is modeled iff it is in the live
    /// `charlie-ast` registry AND at least one fixture that names it currently Matches.
    pub fn compute(fixtures: &[Fixture], verdicts: &[Verdict]) -> Coverage {
        let matched: std::collections::HashSet<&str> = verdicts
            .iter()
            .filter(|v| v.kind == VerdictKind::Match)
            .map(|v| v.key.as_str())
            .collect();

        let registry: std::collections::HashSet<String> = charlie_ast::func::FUNCS
            .iter()
            .map(|f| f.name.to_ascii_uppercase())
            .collect();

        // A declared func counts only if it is in the registry AND the formula ACTUALLY calls it —
        // cross-checking the author's `funcs:` label against the parsed formula so a mislabeled
        // fixture (e.g. `funcs: SUM` on `=1+1`) cannot silently inflate the numerator. A matched
        // fixture necessarily parsed, so re-parsing here succeeds; a parse failure confirms no call
        // and the label simply contributes nothing.
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

    /// The one-line ratchet headline.
    pub fn line(&self) -> String {
        format!(
            "coverage: modeled={} / target={} (registry={} functions defined)",
            self.modeled, self.target, self.registry
        )
    }
}

/// The UPPERCASE names of every registry function actually called in `formula` (recursively). The
/// cross-check backing the honest numerator: an empty set (an unparseable formula) confirms no call,
/// so a mislabeled fixture contributes nothing rather than inflating coverage.
fn called_funcs(formula: &str) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    if let Ok(expr) = charlie_ast::parse(formula) {
        collect_calls(&expr, &mut out);
    }
    out
}

/// Walk an `Expr`, inserting the registry name of every `Call` node into `out`.
fn collect_calls(expr: &charlie_ast::Expr, out: &mut std::collections::HashSet<String>) {
    use charlie_ast::Expr;
    match expr {
        Expr::Call(fid, args) => {
            if let Some(d) = charlie_ast::func::def(*fid) {
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
        Expr::Lit(_) | Expr::Ref(_) | Expr::Range(_) | Expr::WholeRange(_) => {}
    }
}

/// Provenance stamped onto a snapshot at capture time (informational — the backslide comparison
/// reads only the verdict rows, never the meta).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    pub captured_unix: u64,
    pub git_commit: String,
    pub git_dirty: bool,
    pub tool: String,
}

/// The whole-state facts snapshot: provenance + the coverage ratchet + every fixture's verdict.
#[derive(Debug, Clone)]
pub struct Facts {
    pub meta: Meta,
    pub coverage: Coverage,
    /// Verdicts, keyed and sorted by fixture key (BTreeMap keeps the wire order stable).
    pub verdicts: BTreeMap<String, Verdict>,
}

impl Facts {
    /// Capture the current facts: grade `fixtures`, compute coverage, stamp fresh provenance.
    pub fn capture(fixtures: &[Fixture], verdicts: Vec<Verdict>, meta: Meta) -> Facts {
        let coverage = Coverage::compute(fixtures, &verdicts);
        let verdicts = verdicts.into_iter().map(|v| (v.key.clone(), v)).collect();
        Facts {
            meta,
            coverage,
            verdicts,
        }
    }

    /// Serialize to the anchor's TSV wire form (schema line, `#` meta/coverage comments, one row per
    /// fixture). Deterministic (BTreeMap ordering) so re-capturing an unchanged tree is a no-op diff.
    pub fn to_tsv(&self) -> String {
        let mut s = String::new();
        s.push_str(SCHEMA);
        s.push('\n');
        s.push_str(&format!(
            "# captured_unix={} git_commit={} git_dirty={} tool={}\n",
            self.meta.captured_unix, self.meta.git_commit, self.meta.git_dirty, self.meta.tool
        ));
        s.push_str(&format!(
            "# coverage modeled={} target={} registry={} functions={}\n",
            self.coverage.modeled,
            self.coverage.target,
            self.coverage.registry,
            self.coverage.functions.join(","),
        ));
        s.push_str("# columns: VERDICT<TAB>key[<TAB>detail]\n");
        for v in self.verdicts.values() {
            if v.detail.is_empty() {
                s.push_str(&format!("{}\t{}\n", v.kind.as_str(), v.key));
            } else {
                s.push_str(&format!("{}\t{}\t{}\n", v.kind.as_str(), v.key, v.detail));
            }
        }
        s
    }

    /// Parse the verdict rows out of an anchor's TSV. Fail-fast (`Err`) on a schema mismatch or a
    /// malformed verdict token — a broken anchor must read as "can't verify" (exit 2), never "clean".
    /// The `#` meta/coverage lines are informational and skipped; only the verdict rows are compared.
    pub fn parse_verdicts(text: &str) -> Result<BTreeMap<String, Verdict>, String> {
        let mut lines = text.lines();
        let schema = lines.next().unwrap_or("").trim();
        if schema != SCHEMA {
            return Err(format!(
                "snapshot schema {schema:?} != expected {SCHEMA:?} (foreign or stale anchor)"
            ));
        }
        let mut out = BTreeMap::new();
        for raw in lines {
            let line = raw.trim_end_matches(['\r', '\n']);
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            let mut cols = line.splitn(3, '\t');
            let kind = match cols.next() {
                Some("MATCH") => VerdictKind::Match,
                Some("DIVERGE") => VerdictKind::Diverge,
                other => return Err(format!("bad verdict token {other:?} in anchor")),
            };
            let key = cols
                .next()
                .filter(|k| !k.is_empty())
                .ok_or("a verdict row is missing its key")?
                .to_string();
            let detail = cols.next().unwrap_or("").to_string();
            out.insert(key.clone(), Verdict { key, kind, detail });
        }
        Ok(out)
    }
}

/// A single backslide: a fixture that was `Match` in the anchor and is now `Diverge`. Carries the
/// now-detail so the guard can name WHAT diverged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backslide {
    pub key: String,
    pub now_detail: String,
}

/// The REGRESSION-ONLY diff — the whole backslide rule in one function. For each fixture that was
/// `Match` in `anchor`, report it iff it is now PRESENT and `Diverge`. Deliberately exempt:
/// - GROWTH — a key only in `current` never held a Match to lose;
/// - IMPROVEMENT — a `Diverge → Match` transition;
/// - REMOVAL — a former Match no longer in `current` (a conscious corpus edit, under the
///   growth/removal exemption; a re-blessed anchor records it).
pub fn backslides(
    anchor: &BTreeMap<String, Verdict>,
    current: &BTreeMap<String, Verdict>,
) -> Vec<Backslide> {
    let mut out = Vec::new();
    for (key, was) in anchor {
        if was.kind != VerdictKind::Match {
            continue;
        }
        if let Some(now) = current.get(key)
            && now.kind == VerdictKind::Diverge
        {
            out.push(Backslide {
                key: key.clone(),
                now_detail: now.detail.clone(),
            });
        }
    }
    out
}

/// The guard exit code: `1` iff any fixture lost a Match, else `0`. (`2` — an unreadable anchor — is
/// decided by the caller at read time, not here.)
pub fn exit_code(backslid: &[Backslide]) -> i32 {
    i32::from(!backslid.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use charlie_ast::Value;

    fn v(key: &str, kind: VerdictKind, detail: &str) -> (String, Verdict) {
        (
            key.to_string(),
            Verdict {
                key: key.to_string(),
                kind,
                detail: detail.to_string(),
            },
        )
    }

    fn map(items: Vec<(String, Verdict)>) -> BTreeMap<String, Verdict> {
        items.into_iter().collect()
    }

    #[test]
    fn backslides_keeps_only_lost_matches_growth_and_removal_exempt() {
        // anchor: a/keep Match, a/lose Match, a/removed Match, b/already Diverge.
        let anchor = map(vec![
            v("a/keep", VerdictKind::Match, ""),
            v("a/lose", VerdictKind::Match, ""),
            v("a/removed", VerdictKind::Match, ""),
            v("b/already", VerdictKind::Diverge, "old"),
        ]);
        // current: a/keep still Match, a/lose regressed, a/removed gone, b/already improved, c/new grown.
        let current = map(vec![
            v("a/keep", VerdictKind::Match, ""),
            v("a/lose", VerdictKind::Diverge, "now wrong"),
            v("b/already", VerdictKind::Match, ""),
            v("c/new", VerdictKind::Diverge, "fresh non-conforming"),
        ]);

        let backslid = backslides(&anchor, &current);
        let keys: Vec<&str> = backslid.iter().map(|b| b.key.as_str()).collect();
        assert_eq!(
            keys,
            vec!["a/lose"],
            "only a present Match→Diverge is a backslide"
        );
        assert_eq!(exit_code(&backslid), 1);

        // Growth-only stays green.
        let grew = backslides(
            &map(vec![v("x", VerdictKind::Match, "")]),
            &map(vec![
                v("x", VerdictKind::Match, ""),
                v("y", VerdictKind::Diverge, "new"),
            ]),
        );
        assert!(grew.is_empty());
        assert_eq!(exit_code(&grew), 0);
    }

    #[test]
    fn tsv_round_trips_verdicts_and_rejects_a_foreign_schema() {
        let facts = Facts {
            meta: Meta {
                captured_unix: 1,
                git_commit: "deadbeef".into(),
                git_dirty: false,
                tool: "conformance 0.1.0".into(),
            },
            coverage: Coverage {
                modeled: 2,
                target: V1_FUNCTION_TARGET,
                registry: 9,
                functions: vec!["ABS".into(), "SUM".into()],
            },
            verdicts: map(vec![
                v("a/m", VerdictKind::Match, ""),
                v("b/d", VerdictKind::Diverge, "expected 5, got 4"),
            ]),
        };
        let text = facts.to_tsv();
        let parsed = Facts::parse_verdicts(&text).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed["a/m"].kind, VerdictKind::Match);
        assert_eq!(parsed["b/d"].kind, VerdictKind::Diverge);
        assert_eq!(parsed["b/d"].detail, "expected 5, got 4");

        // A foreign schema fails fast (→ the caller maps this to exit 2).
        assert!(Facts::parse_verdicts("nope/v9\nMATCH\ta/m\n").is_err());
        // A malformed verdict token fails fast.
        assert!(Facts::parse_verdicts(&format!("{SCHEMA}\nMAYBE\ta/m\n")).is_err());
    }

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
                key: "agg/unimplemented".into(),
                funcs: vec!["LAMBDA".into()], // a deferred fn, not in the registry — never modeled
                formula: "=LAMBDA(A1,A1:A2)".into(),
                expect: Value::Number(3.0),
                cells: vec![],
                at: None,
            },
            Fixture {
                key: "agg/round-bad".into(),
                funcs: vec!["ROUND".into()], // registry fn, but this fixture DIVERGES → not modeled
                formula: "=ROUND(1,0)".into(),
                expect: Value::Number(99.0),
                cells: vec![],
                at: None,
            },
            Fixture {
                key: "agg/mislabeled".into(),
                funcs: vec!["ABS".into()], // registry fn, MATCHES, but the formula never calls ABS
                formula: "=1+1".into(),
                expect: Value::Number(2.0),
                cells: vec![],
                at: None,
            },
        ];
        let verdicts = vec![
            Verdict::matched("agg/sum"),
            Verdict::matched("agg/unimplemented"),
            Verdict::diverge("agg/round-bad", "x".into()),
            Verdict::matched("agg/mislabeled"),
        ];
        let cov = Coverage::compute(&fixtures, &verdicts);
        // Only SUM: LAMBDA is not in the registry, ROUND's only fixture diverges, and the
        // `funcs: ABS` label on `=1+1` is dropped by the call cross-check (the formula never calls
        // ABS) so a mislabeled fixture cannot inflate the numerator.
        assert_eq!(cov.functions, vec!["SUM"]);
        assert_eq!(cov.modeled, 1);
        assert_eq!(cov.target, V1_FUNCTION_TARGET);
    }
}
