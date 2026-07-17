// Concern: the RENDERED-VALUE conformance harness (bet B3) — load each of the 6 W1 category workbooks via charlie-model, evaluate EVERY oracle cell demand-driven, and grade the computed `Value` against the externally-authored W1 value oracle (bit-exact for exact rationals/strings, within the oracle authors' documented float tolerance for accumulated numerics); the per-cell Match/Diverge verdicts are ratcheted through a committed facts-snapshot so a render regression (a cell that Matched losing its Match) backslides LOUDLY, and a bounded `--range` viewport is proven to evaluate ONLY the transitive dependency cone (an off-cone poison cell is never touched) | Non-concern: HOW values are computed (charlie-ast owns eval; charlie-model owns the demand-driven pull) and HOW the oracle values were derived (python/hand, W1 — see conformance/render/PROVENANCE.md; never charlie) | IO: (the frozen corpus tree + the frozen value oracles) -> per-cell verdicts + the ratchet gate + the pass tally
//! Rendered-value conformance harness (bet **B3**). This grades `charlie-model`'s demand-driven
//! evaluation against the frozen W1 value oracles migrated into `conformance/render/`. Each of the
//! six category workbooks is loaded, every oracle cell is pulled through the model, and the computed
//! `Value` is compared to the oracle's expected value.
//!
//! A per-cell verdict is `Match` (bit-exact, or within the oracle authors' documented float
//! tolerance for accumulated numerics — see `conformance/render/oracle/model/PROVENANCE.md`) or
//! `Diverge`. Verdicts are ratcheted through a committed facts-snapshot
//! (`conformance/render/facts-snapshot.tsv`): the hard gate is that no cell that Matched in the
//! committed anchor may Diverge now (a render regression), mirroring the formula-conformance ratchet.
//! A `Diverge` is a surfaced FACT, not itself a gate failure — the report prints the standing tally.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use charlie_ast::Value;
use charlie_ast::a1::parse_a1;
use charlie_model::{Workbook, display_value};

/// The frozen W1 encoding corpus root (the workbook sheet files — reused verbatim, fingerprinted by
/// `conformance/encoding/MANIFEST.sha256`).
fn encoding_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance/encoding")
}

/// The migrated rendered-VALUE oracle root (`conformance/render/`, fingerprinted by its own
/// `MANIFEST.sha256`). ORACLE-INPUT PURITY: every file here was python/hand-computed in W1, never by
/// charlie (see `conformance/render/PROVENANCE.md`).
fn render_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance/render")
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

// ------------------------------------------------------------------------------------------------
// Oracle formats (parsed from the frozen W1 files; charlie never authored these).
// ------------------------------------------------------------------------------------------------

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
        // Fail LOUD on an unexpected shape so the oracle can never be silently under-populated
        // (fewer cells graded than the oracle carries): a corrupt oracle must never grade clean.
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

// ------------------------------------------------------------------------------------------------
// Grading.
// ------------------------------------------------------------------------------------------------

/// The oracle authors' documented numeric tolerance (`oracle/model/PROVENANCE.md`): numbers that are
/// exact rationals compare bit-exact, but IEEE-754 accumulation over the 12-period amortization (and
/// the oracle's own 10-dp display rounding) means the accumulated cells must be compared under a
/// small absolute/relative tolerance, NOT bit-exact. `1e-6` is the value the oracle authors pin.
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
            // An oracle error token (e.g. "#N/A") matches charlie's error value by its spelling.
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

// ------------------------------------------------------------------------------------------------
// The facts snapshot (the render ratchet's memory) — a self-contained, dependency-free TSV mirroring
// the formula-conformance anchor shape. Kept in charlie-model (never in the `conformance` crate,
// which is firewalled to charlie-ast) because grading rendered values must load the filesystem model.
// ------------------------------------------------------------------------------------------------

const SNAPSHOT_SCHEMA: &str = "charlie-render-conformance/v1";

/// The committed anchor path: a TRACKED file traveling with the commit, so "what rendered correctly
/// at HEAD" is versioned alongside the corpus and oracles it grades.
fn anchor_path() -> PathBuf {
    render_root().join("facts-snapshot.tsv")
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

/// Serialize verdicts to the anchor's TSV wire form: a schema line, a deterministic summary comment,
/// then one row per cell (`MATCH<TAB>key` or `DIVERGE<TAB>key<TAB>detail`). No timestamp/git meta —
/// the anchor is deterministic so re-capturing an unchanged tree is a no-op diff.
fn to_tsv(verdicts: &BTreeMap<String, Verdict>) -> String {
    let matched = verdicts.values().filter(|v| v.matched).count();
    let total = verdicts.len();
    let mut s = String::new();
    s.push_str(SNAPSHOT_SCHEMA);
    s.push('\n');
    s.push_str(&format!(
        "# render-conformance ratchet: {matched}/{total} cells Match \
         (the rest are surfaced Diverge FACTS — see conformance/render/PROVENANCE.md)\n"
    ));
    s.push_str("# columns: VERDICT<TAB>key[<TAB>detail]\n");
    for v in verdicts.values() {
        if v.matched {
            s.push_str(&format!("MATCH\t{}\n", v.key));
        } else {
            s.push_str(&format!("DIVERGE\t{}\t{}\n", v.key, v.detail));
        }
    }
    s
}

/// Parse the committed anchor's `key -> matched?` map. Fail-LOUD on a foreign schema (a broken
/// anchor must read as "can't verify", never as a silent clean).
fn parse_anchor(text: &str) -> BTreeMap<String, bool> {
    let mut lines = text.lines();
    let schema = lines.next().unwrap_or("").trim();
    assert_eq!(
        schema, SNAPSHOT_SCHEMA,
        "render anchor schema mismatch (foreign/stale anchor)"
    );
    let mut out = BTreeMap::new();
    for raw in lines {
        let line = raw.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let mut cols = line.splitn(3, '\t');
        let matched = match cols.next() {
            Some("MATCH") => true,
            Some("DIVERGE") => false,
            other => panic!("bad verdict token {other:?} in render anchor"),
        };
        let key = cols
            .next()
            .expect("a verdict row must carry a key")
            .to_string();
        out.insert(key, matched);
    }
    out
}

// ------------------------------------------------------------------------------------------------
// Tests.
// ------------------------------------------------------------------------------------------------

/// Sanity: every one of the six category workbooks loads through the filesystem model with no
/// load-time refusal (a broken corpus path or a non-conforming sheet would make the grade below
/// vacuous).
#[test]
fn every_category_workbook_loads_clean() {
    for (cat, wbrel, _) in CATEGORIES {
        let dir = encoding_root().join(wbrel);
        Workbook::load_dir(&dir)
            .unwrap_or_else(|e| panic!("fs read for `{cat}`: {e}"))
            .unwrap_or_else(|d| panic!("`{cat}` must load clean: {d:?}"));
    }
}

/// The whole-sheet grade + the ZERO-DIVERGENCE gate. Grades every oracle cell in all six workbooks,
/// prints the per-category and grand pass tally, and enforces — as the primary hard gate — that
/// **every one of the ~542 corpus cells Matches** the independent W1 oracle: zero `Diverge`. Every
/// non-model category is BIT-EXACT; the loan-amortization schedule cells stay within the documented
/// `1e-6` tolerance ([`NUM_ABS_TOL`], `oracle/model/PROVENANCE.md`: the oracle stores 10-dp-rounded
/// values and the whole schedule flows from a rounded `PMT`), so a `Tol` match is a Match, never a
/// Diverge. The committed-anchor backslide check is retained as a second, belt-and-suspenders guard
/// (no cell that Matched may regress), but the ratchet is no longer the ceiling — divergence must be
/// zero outright.
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

    // The PRIMARY gate: ZERO divergence. Every corpus cell must Match the independent W1 oracle
    // (bit-exact, or — for the documented model-amortization cells only — within the 1e-6 tolerance).
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
    // Belt-and-suspenders: every non-exact Match is a model-amortization tol cell (all other
    // categories are bit-exact). If a NON-model cell ever needs tolerance, that is a new fact to vet.
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

    // The ratchet: compare to the committed anchor. A missing/unreadable anchor fails LOUD (fail
    // SAFE — "can't verify" is never "clean").
    let anchor_text = std::fs::read_to_string(anchor_path()).unwrap_or_else(|e| {
        panic!(
            "cannot read the committed render anchor {} ({e}). Generate it once with \
             RENDER_RESNAPSHOT=1 (see the resnapshot test).",
            anchor_path().display()
        )
    });
    let anchor = parse_anchor(&anchor_text);

    let mut backslid: Vec<String> = Vec::new();
    for (key, was_match) in &anchor {
        if !was_match {
            continue; // only a lost Match is a regression
        }
        match verdicts.get(key) {
            Some(v) if !v.matched => backslid.push(format!("{key} :: now {}", v.detail)),
            _ => {} // still matches, or removed (a conscious corpus edit) — both exempt
        }
    }
    assert!(
        backslid.is_empty(),
        "RENDER BACKSLIDE — {} cell(s) that Matched the committed anchor now Diverge:\n  {}",
        backslid.len(),
        backslid.join("\n  ")
    );

    // Cross-check: the anchor's committed Match count equals the current Match count (a purely
    // informational belt-and-suspenders that the snapshot is in sync with the tree; growth/removal
    // are the only sanctioned reasons for a difference, and there are none in a frozen corpus).
    let anchor_matches = anchor.values().filter(|m| **m).count();
    assert_eq!(
        anchor_matches, matched,
        "committed anchor records {anchor_matches} matches but the tree now shows {matched}; \
         re-bless consciously with RENDER_RESNAPSHOT=1 and record WHY"
    );
}

/// Conscious (re)snapshot of the render anchor — the render analogue of
/// `conformance resnapshot`. A NO-OP unless `RENDER_RESNAPSHOT=1` is set, so a normal
/// `--include-ignored` run never writes into the source tree; re-blessing the baseline is a
/// deliberate act whose reason must be recorded in the carrying commit.
#[test]
#[ignore = "writes the committed anchor; run consciously with RENDER_RESNAPSHOT=1"]
fn resnapshot_render_anchor() {
    if std::env::var("RENDER_RESNAPSHOT").as_deref() != Ok("1") {
        eprintln!(
            "resnapshot_render_anchor: skipped (set RENDER_RESNAPSHOT=1 to write the anchor)"
        );
        return;
    }
    let (verdicts, _, _) = capture();
    let path = anchor_path();
    std::fs::write(&path, to_tsv(&verdicts))
        .unwrap_or_else(|e| panic!("write render anchor {}: {e}", path.display()));
    eprintln!("resnapshot_render_anchor: wrote {}", path.display());
}

/// **Dependency-cone instrumentation.** A bounded viewport (`--range`) must evaluate ONLY the
/// transitive dependency cone of the visible cells — never an off-cone cell. Proven with a POISON
/// cell: an off-cone circular reference that, IF evaluated, records a `#REF!`-class cycle diagnostic.
/// Rendering the on-cone viewport must produce the correct values AND leave the poison untouched
/// (zero eval diagnostics). The final step renders a viewport that DOES include the poison, proving
/// the poison is genuinely poisonous — so the earlier silence was non-evaluation, not an inert cell.
#[test]
fn viewport_evaluates_only_the_dependency_cone() {
    // A file's content is exactly its grid (GRID1) — no annotation line.
    let f = |b: &str| b.to_string();
    // On-cone chain in row 1: A1=10 (literal), B1==A1+5 (formula), C1==B1*2 (formula).
    // Off-cone POISON in rows 5-6: A5==A6, A6==A5 — a two-cell cycle that refuses if touched.
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

    // Render ONLY the on-cone viewport A1:C1 (the `--range` path).
    let vp = charlie_model::parse_viewport("A1:C1").expect("valid viewport");
    let grid = charlie_model::render(&wb, 0, vp, charlie_model::RenderMode::Values);
    assert_eq!(
        grid.rows[0].cells,
        vec!["10", "15", "30"],
        "the on-cone viewport must render its computed cone"
    );
    // The proof: the off-cone poison was never evaluated, so no cycle diagnostic was recorded.
    assert!(
        wb.eval_diagnostics().is_empty(),
        "a bounded viewport must NOT evaluate off-cone cells, but the off-cone poison was touched: \
         {:?}  (B3 signal: the viewport over-evaluated)",
        wb.eval_diagnostics()
    );

    // Control: a viewport that DOES cover the poison surfaces the cycle — the poison is real, so the
    // silence above was genuine non-evaluation of an off-cone cell, not a dud.
    let poison_vp = charlie_model::parse_viewport("A5:A6").expect("valid viewport");
    let _ = charlie_model::render(&wb, 0, poison_vp, charlie_model::RenderMode::Values);
    assert!(
        !wb.eval_diagnostics().is_empty(),
        "the poison cell must refuse (a cycle) when actually inside the viewport — else it proves \
         nothing about the cone"
    );
}

// ------------------------------------------------------------------------------------------------
// Oracle-input purity: the migrated value oracles must be byte-identical to the frozen W1 contract.
// ------------------------------------------------------------------------------------------------

/// The pinned `sha256(MANIFEST.sha256)` for `conformance/render/` — a single value fingerprinting the
/// whole migrated oracle set. Pinning it here in CODE (not only in PROVENANCE.md) closes the
/// "regenerate BOTH the oracle files AND the manifest" hole: a co-regeneration changes this digest
/// and this assertion fails. Keep in sync with `conformance/render/PROVENANCE.md` if the oracle set
/// is ever deliberately grown.
const PINNED_RENDER_MANIFEST_DIGEST: &str =
    "3b400d10e747cb117175960849e7ef746956e48035bb7be37c089de9f6318d37";

/// Run `sha256sum <args>` in `conformance/render/`, returning `(exit_ok, stdout)`. Fails LOUD if the
/// system tool is absent — the purity guarantee depends on it (charlie-model is crypto-free by
/// design, so it shells out rather than pulling a hashing dependency).
fn render_sha256sum(args: &[&str]) -> (bool, String) {
    let out = Command::new("sha256sum")
        .args(args)
        .current_dir(render_root())
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "system `sha256sum` is required to verify the render oracle fingerprint \
                 (conformance/render/PROVENANCE.md), but could not run: {e}"
            )
        });
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

/// ORACLE-INPUT PURITY at the grading site: (1) every migrated oracle file is byte-identical to the
/// frozen contract (`sha256sum -c`), and (2) the manifest itself is the pinned one. Both together
/// prove charlie has not silently rewritten its own oracle inputs.
#[test]
fn render_oracles_match_the_frozen_fingerprint() {
    let (ok, _) = render_sha256sum(&["-c", "MANIFEST.sha256", "--quiet"]);
    assert!(
        ok,
        "render oracle fingerprint FAILED: `sha256sum -c MANIFEST.sha256` reported a mismatch — a \
         value oracle was edited (oracle-input purity, conformance/render/PROVENANCE.md)"
    );

    let (ran, stdout) = render_sha256sum(&["MANIFEST.sha256"]);
    assert!(ran, "`sha256sum MANIFEST.sha256` did not run cleanly");
    let digest = stdout
        .split_whitespace()
        .next()
        .expect("sha256sum output starts with the digest");
    assert_eq!(
        digest, PINNED_RENDER_MANIFEST_DIGEST,
        "MANIFEST.sha256 does not match its pinned digest (PROVENANCE.md) — the manifest itself was \
         rewritten (a fixture+manifest co-regeneration, the silent oracle-swap this pin guards)"
    );
}
