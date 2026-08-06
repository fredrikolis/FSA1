// Concern: grades every rendered oracle cell against the W1 value oracles | Non-concern: how a value is computed, how the oracles were derived | IO: (corpus + oracles) -> a zero-divergence verdict
//! The hard gate: every graded cell must Match its W1 oracle.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fsa1_ast::Value;
use fsa1_ast::a1::parse_a1;
use fsa1_model::{Workbook, display_value};

/// The W1 encoding corpus root (the workbook sheet files, reused verbatim).
fn encoding_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/encoding")
}

/// The migrated rendered-VALUE oracle root (`conformance/render/`). ORACLE-INPUT PURITY: every file
/// here was python/hand-computed in W1, never by FSA1 (see `conformance/render/PROVENANCE.md`).
fn render_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/render")
}

/// One category workbook: `(label, corpus dir relative to encoding_root, oracle file relative to
/// render_root)`. The six W1 valid category workbooks that carry a rendered-VALUE oracle.
const CATEGORIES: &[(&str, &str, &str)] = &[
    (
        "aggregation",
        "artifacts/aggregation/sales_report",
        "oracle/aggregation/expected_values.json",
    ),
    (
        "conditional",
        "artifacts/conditional/customer_tiers",
        "oracle/conditional/expected_values.json",
    ),
    (
        "text",
        "artifacts/text/contacts-clean",
        "oracle/text/contacts-clean.oracle.json",
    ),
    (
        "model",
        "artifacts/model/loan_amortization",
        "oracle/model/oracle_values.json",
    ),
    (
        "dates",
        "artifacts/dates/invoice-aging",
        "oracle/dates/invoice-aging.oracle.csv",
    ),
    (
        "lookup-join",
        "artifacts/lookup-join",
        "oracle/lookup-join/oracle.json",
    ),
];

/// A fixed evaluation clock (an Excel serial) so any `TODAY()`/`NOW()` is deterministic. The corpus
/// formulas pin their own reference dates (`dates` uses `=DATE(2026,3,31)`), so this only guards
/// against a stray volatile — it is never load-bearing for the oracle values.
const PINNED_NOW_SERIAL: f64 = 46112.0;

/// One oracle cell's expected value: a text label/string, or a number (rendered value).
#[derive(Debug, Clone)]
enum Expected {
    Text(String),
    Num(f64),
}

/// Minimal flat-JSON object reader for `{ "Sheet!A1": "text" | number, ... }`. The W1 value oracles
/// are flat maps with no escapes, no nesting, and no bool/null (verified against the frozen files),
/// so a small quote-aware scanner is sufficient and dependency-free. It is deliberately strict:
/// a shape it does not expect panics (a corrupt oracle must fail LOUD, never grade silently wrong).
fn parse_flat_json(s: &str) -> Vec<(String, Expected)> {
    let b: Vec<char> = s.chars().collect();
    let n = b.len();
    let mut i = 0usize;
    let mut out = Vec::new();
    while i < n && b[i] != '{' {
        i += 1;
    }
    i += 1;
    loop {
        while i < n && (b[i].is_whitespace() || b[i] == ',') {
            i += 1;
        }
        if i >= n || b[i] == '}' {
            break;
        }
        assert_eq!(b[i], '"', "oracle JSON: expected a key string at char {i}");
        i += 1;
        let mut key = String::new();
        while i < n && b[i] != '"' {
            key.push(b[i]);
            i += 1;
        }
        i += 1;
        while i < n && b[i].is_whitespace() {
            i += 1;
        }
        assert_eq!(b[i], ':', "oracle JSON: expected ':' after key {key:?}");
        i += 1;
        while i < n && b[i].is_whitespace() {
            i += 1;
        }
        if b[i] == '"' {
            i += 1;
            let mut v = String::new();
            while i < n && b[i] != '"' {
                v.push(b[i]);
                i += 1;
            }
            i += 1;
            out.push((key, Expected::Text(v)));
        } else {
            let mut tok = String::new();
            while i < n && b[i] != ',' && b[i] != '}' && !b[i].is_whitespace() {
                tok.push(b[i]);
                i += 1;
            }
            let x: f64 = tok
                .parse()
                .unwrap_or_else(|_| panic!("oracle JSON: bad number {tok:?} for key {key:?}"));
            out.push((key, Expected::Num(x)));
        }
    }
    out
}

/// The `dates` oracle CSV (`cell,rendered_value,kind,note`). `note` may contain commas (formula
/// text like `=DATE(2026,3,31)`), so the row is split into at most four fields and only the first
/// three (all comma-free) are read. `kind` selects the value type; other kinds are skipped.
fn parse_dates_csv(s: &str) -> Vec<(String, Expected)> {
    let mut out = Vec::new();
    for line in s.lines().skip(1) {
        if line.trim().is_empty() {
            continue;
        }
        let mut it = line.splitn(4, ',');
        let cell = it.next().unwrap_or("").trim();
        let rendered = it.next().unwrap_or("");
        let kind = it.next().unwrap_or("").trim();
        if cell.is_empty() {
            continue;
        }
        // Loud, so a corrupt oracle can never grade fewer cells than it carries and still pass.
        match kind {
            "number" => {
                let x = rendered.parse::<f64>().unwrap_or_else(|_| {
                    panic!(
                        "dates oracle: cell {cell:?} kind=number has non-numeric value {rendered:?}"
                    )
                });
                out.push((cell.to_string(), Expected::Num(x)));
            }
            "text" => out.push((cell.to_string(), Expected::Text(rendered.to_string()))),
            other => panic!(
                "dates oracle: cell {cell:?} has unexpected kind {other:?} (expected `number` or `text`)"
            ),
        }
    }
    out
}

/// Load a category's oracle into a sorted `address -> Expected` map (`.json` vs the `dates` `.csv`).
fn load_oracle(rel: &str) -> BTreeMap<String, Expected> {
    let path = render_root().join(rel);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read oracle {rel}: {e}"));
    let pairs = if rel.ends_with(".json") {
        parse_flat_json(&text)
    } else {
        parse_dates_csv(&text)
    };
    pairs.into_iter().collect()
}

/// The value the ORACLE AUTHORS pin, not one FSA1 chose: IEEE-754 accumulation over the 12-period
/// amortization, plus the oracle's own 10-dp display rounding, puts those cells off bit-exact.
const NUM_ABS_TOL: f64 = 1e-6;
const NUM_REL_TOL: f64 = 1e-6;

/// How one cell graded.
enum Grade {
    /// Bit-exact (floats by bit pattern; text/error by exact spelling).
    Exact,
    /// Numeric, not bit-exact but within the documented tolerance (`|Δ|` carried for the report).
    Tol(f64),
    /// Diverges beyond tolerance — a surfaced FACT (and, systematically, a B3 signal).
    Diverge(String),
}

/// Grade one oracle cell: resolve its sheet+address, pull the value through the model, and compare
/// to the expected value. Text compares by exact spelling (an error token like `#N/A` compares via
/// its rendered spelling, so an oracle `"#N/A"` matches a `Value::Error(Na)`). Numbers compare
/// bit-exact first, then under the documented tolerance.
fn grade(wb: &Workbook, key: &str, expected: &Expected) -> Grade {
    let (sheet_name, a1) = match key.split_once('!') {
        Some((s, a)) => (Some(s), a),
        None => (None, key),
    };
    let sheet = match sheet_name {
        Some(name) => match wb.tab_index(name) {
            Some(i) => i,
            None => return Grade::Diverge(format!("no tab named {name:?}")),
        },
        // A bare address (single-sheet workbook) resolves against tab 0.
        None => 0,
    };
    let addr = match parse_a1(a1) {
        Ok(a) => a,
        Err(e) => return Grade::Diverge(format!("bad address {a1:?}: {e:?}")),
    };
    let got = wb.value_at(sheet, addr.col, addr.row);
    match expected {
        Expected::Text(s) => match &got {
            Value::Text(t) if t == s => Grade::Exact,
            // An oracle error token (e.g. "#N/A") matches FSA1's error value by its spelling.
            Value::Error(_) if &display_value(&got) == s => Grade::Exact,
            _ => Grade::Diverge(format!("want text {s:?}, got {}", short(&got))),
        },
        Expected::Num(x) => match &got {
            Value::Number(y) => {
                if x.to_bits() == y.to_bits() {
                    Grade::Exact
                } else {
                    let diff = (x - y).abs();
                    let tol = NUM_ABS_TOL.max(NUM_REL_TOL * x.abs());
                    if diff <= tol {
                        Grade::Tol(diff)
                    } else {
                        Grade::Diverge(format!("want {x}, got {y} (|d|={diff:e})"))
                    }
                }
            }
            _ => Grade::Diverge(format!("want number {x}, got {}", short(&got))),
        },
    }
}

/// A compact one-token spelling of a value for a diagnostic detail (kept tab/newline-free).
fn short(v: &Value) -> String {
    match v {
        Value::Number(n) => format!("num:{n}"),
        Value::Text(t) => format!("text:{t:?}"),
        Value::Bool(b) => format!("bool:{b}"),
        Value::Error(_) => format!("err:{}", display_value(v)),
        Value::Blank => "blank".to_string(),
        Value::Array(shape, _) => format!("array:{}x{}", shape.rows, shape.cols),
    }
}

/// One graded cell: its stable key (`<category>/<oracle-address>`) and whether it Matched.
struct Verdict {
    key: String,
    matched: bool,
    detail: String,
}

/// The running tally the report prints.
#[derive(Default)]
struct Tally {
    exact: usize,
    tol: usize,
    diverge: usize,
}

/// Capture the current render facts: load + grade every category workbook. Returns the sorted
/// verdict map and the per-category + grand tally. Fail-LOUD on a corpus that will not load (that is
/// a broken migration, not a render fact).
fn capture() -> (BTreeMap<String, Verdict>, Vec<(String, Tally)>, Tally) {
    let mut verdicts: BTreeMap<String, Verdict> = BTreeMap::new();
    let mut per_cat: Vec<(String, Tally)> = Vec::new();
    let mut grand = Tally::default();

    for (cat, wbrel, orel) in CATEGORIES {
        let dir = encoding_root().join(wbrel);
        let wb = Workbook::load_dir(&dir)
            .unwrap_or_else(|e| panic!("fs read for `{cat}` ({}): {e}", dir.display()))
            .unwrap_or_else(|d| panic!("`{cat}` must load clean, but reported: {d:?}"))
            .with_now(PINNED_NOW_SERIAL);

        let oracle = load_oracle(orel);
        assert!(!oracle.is_empty(), "`{cat}` oracle {orel} is empty");
        let mut t = Tally::default();
        for (addr, exp) in &oracle {
            let key = format!("{cat}/{addr}");
            let (matched, detail) = match grade(&wb, addr, exp) {
                Grade::Exact => {
                    t.exact += 1;
                    (true, "exact".to_string())
                }
                Grade::Tol(d) => {
                    t.tol += 1;
                    (true, format!("tol|d|={d:e}"))
                }
                Grade::Diverge(why) => {
                    t.diverge += 1;
                    (false, why)
                }
            };
            verdicts.insert(
                key.clone(),
                Verdict {
                    key,
                    matched,
                    detail: detail.replace(['\t', '\n', '\r'], " "),
                },
            );
        }
        grand.exact += t.exact;
        grand.tol += t.tol;
        grand.diverge += t.diverge;
        per_cat.push(((*cat).to_string(), t));
    }
    (verdicts, per_cat, grand)
}

/// A broken corpus path or a non-conforming sheet would make the grade below vacuous.
#[test]
fn every_category_workbook_loads_clean() {
    for (cat, wbrel, _) in CATEGORIES {
        let dir = encoding_root().join(wbrel);
        Workbook::load_dir(&dir)
            .unwrap_or_else(|e| panic!("fs read for `{cat}`: {e}"))
            .unwrap_or_else(|d| panic!("`{cat}` must load clean: {d:?}"));
    }
}

/// The hard gate: every corpus cell must Match. Every non-model category
/// is BIT-EXACT; only the loan-amortization schedule stays within [`NUM_ABS_TOL`], its oracle storing
/// 10-dp-rounded values that the whole schedule flows from — so a `Tol` match is a Match.
#[test]
fn render_conformance_is_a_zero_divergence_gate() {
    let (verdicts, per_cat, grand) = capture();
    let matched: usize = grand.exact + grand.tol;
    let total = matched + grand.diverge;

    println!("=== RENDERED-VALUE CONFORMANCE (bet B3) ===");
    for (cat, t) in &per_cat {
        println!(
            "  {cat:<12} cells={:<4} match={:<4} (exact={}, tol={}) diverge={}",
            t.exact + t.tol + t.diverge,
            t.exact + t.tol,
            t.exact,
            t.tol,
            t.diverge
        );
    }
    println!(
        "  GRAND        cells={total:<4} match={matched:<4} (exact={}, tol={}) diverge={}",
        grand.exact, grand.tol, grand.diverge
    );

    let diverged: Vec<String> = verdicts
        .values()
        .filter(|v| !v.matched)
        .map(|v| format!("{} :: {}", v.key, v.detail))
        .collect();
    assert!(
        diverged.is_empty(),
        "RENDER DIVERGENCE — {} cell(s) do not Match the W1 oracle (the gate requires ZERO):\n  {}",
        diverged.len(),
        diverged.join("\n  ")
    );
    // A NON-model cell that ever needed tolerance would be a new fact to vet, not a pass.
    for (cat, t) in &per_cat {
        if cat != "model" {
            assert_eq!(
                t.tol, 0,
                "category `{cat}` has {} tolerance-only match(es); only model-amortization cells are \
                 documented as tol (everything else must be bit-exact)",
                t.tol
            );
        }
    }
}

/// A bounded viewport must evaluate ONLY the visible cells' transitive cone. Proven with a POISON
/// cell — an off-cone circular reference that records a diagnostic IF it is ever evaluated.
#[test]
fn viewport_evaluates_only_the_dependency_cone() {
    // Rows 5-6 are the POISON: a two-cell cycle that refuses the moment anything touches it.
    let f = |b: &str| b.to_string();
    let a1 = f("10");
    let b1 = f("=A1+5");
    let c1 = f("=B1*2");
    let a5 = f("=A6");
    let a6 = f("=A5");
    let wb = Workbook::from_tabs(&[(
        "Sheet1",
        &[
            ("A1", a1.as_str()),
            ("B1", b1.as_str()),
            ("C1", c1.as_str()),
            ("A5", a5.as_str()),
            ("A6", a6.as_str()),
        ],
    )])
    .expect("poison workbook loads clean");

    let vp = fsa1_model::parse_viewport("A1:C1").expect("valid viewport");
    let grid = fsa1_model::render(&wb, 0, vp, fsa1_model::RenderMode::Values);
    assert_eq!(
        grid.rows[0].cells,
        vec!["10", "15", "30"],
        "the on-cone viewport must render its computed cone"
    );
    assert!(
        wb.eval_diagnostics().is_empty(),
        "a bounded viewport must NOT evaluate off-cone cells, but the off-cone poison was touched: \
         {:?}  (B3 signal: the viewport over-evaluated)",
        wb.eval_diagnostics()
    );

    // The control: without it, the silence above could be an inert cell rather than non-evaluation.
    let poison_vp = fsa1_model::parse_viewport("A5:A6").expect("valid viewport");
    let _ = fsa1_model::render(&wb, 0, poison_vp, fsa1_model::RenderMode::Values);
    assert!(
        !wb.eval_diagnostics().is_empty(),
        "the poison cell must refuse (a cycle) when actually inside the viewport — else it proves \
         nothing about the cone"
    );
}
