// Concern: the ENCODING CONFORMANCE HARNESS — run charlie-model over the whole frozen W1 corpus (../conformance/encoding) and assert every verdict matches its EXPECTED ledger: the 6 valid workbooks parse+conform+don't-overlap, conformance-forms yield the stated broadcast placement, invalid-forms yield the stated rejection Code+class, each assertion citing the fixture + FORMAT.md § | Non-concern: the verdict LOGIC itself (charlie-model/src owns parse/conform/overlap; this only grades it) and the W4 rendered-VALUE oracles (not migrated) | IO: (the on-disk corpus tree) -> pass/fail per fixture
//! Encoding conformance harness (bet **B1**). This is the seed of the coverage ratchet: it grades
//! `charlie-model` against the frozen `conformance/encoding/` corpus and its `EXPECTED.md` ledgers.
//! If any corpus sheet cannot be parsed/expressed, or any dimension verdict is genuinely
//! two-defensible, a test here fails LOUDLY — that is the B1 kill signal.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use charlie_ast::ErrKind;
use charlie_model::{Body, Code, Placement, Rect, detect_overlaps, parse_file};

/// Absolute path to the migrated corpus root (`charlie/conformance/encoding`).
fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance/encoding")
}

/// A fixture's on-disk basename (what `parse_file`/`parse_filename` see) plus its full path so a
/// failing assertion can name the exact file.
struct Fixture {
    /// The filename the model parses — e.g. `A1:B3.range`, `$A$1.cell`.
    name: String,
    /// The post-`# `-annotation-and-all raw file contents.
    contents: String,
    /// Corpus-relative path, for diagnostics/citations.
    rel: String,
}

fn load_fixture(rel: &str) -> Fixture {
    let root = corpus_root();
    let path = root.join(rel);
    let contents = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("corpus fixture {rel} must be readable: {e}"));
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_else(|| panic!("fixture {rel} has no filename"))
        .to_string();
    Fixture {
        name,
        contents,
        rel: rel.to_string(),
    }
}

/// Recursively collect every `.cell` / `.range` sheet file under `dir` (skips `EXPECTED.md`,
/// `FORMAT.md`, and any non-sheet file). Paths are corpus-relative.
fn collect_sheet_files(rel_dir: &str) -> Vec<String> {
    let root = corpus_root();
    let mut out = Vec::new();
    walk(&root, &root.join(rel_dir), &mut out);
    out.sort();
    out
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .map(|e| e.expect("dir entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(root, &path, out);
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && (name.ends_with(".cell") || name.ends_with(".range"))
        {
            let rel = path
                .strip_prefix(root)
                .expect("under root")
                .to_string_lossy()
                .into_owned();
            out.push(rel);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The 6 VALID category workbooks (FORMAT §2/§4/§5 parse, §6 conform, §7 no unintended overlap).
//
// Expectation for every file: `parse_file` returns Ok; a literal body yields a `Placement`
// (conforms under §6); within each tab (folder) `detect_overlaps` is empty. No per-file EXPECTED
// ledger — the uniform contract is "accepts", stated in oracle/*/EXPECTED.md by omission from the
// reject list.
// ---------------------------------------------------------------------------------------------

/// Grade one valid category workbook: every sheet file parses (§2/§4/§5) and, if literal, conforms
/// (§6); every tab is overlap-free (§7). Returns the count of files checked (for the ratchet tally).
fn assert_valid_workbook(category: &str) -> usize {
    let files = collect_sheet_files(&format!("artifacts/{category}"));
    assert!(
        !files.is_empty(),
        "valid workbook `{category}` must contain sheet files (corpus path broke?)"
    );

    // Per-tab overlap accumulator, keyed by the tab's corpus-relative directory.
    let mut tabs: std::collections::BTreeMap<String, Vec<(String, Rect)>> =
        std::collections::BTreeMap::new();

    for rel in &files {
        let fx = load_fixture(rel);

        // (a) PARSE + CONFORM (FORMAT §2/§4/§5/§6): the end-to-end B1 load must succeed.
        let parsed = parse_file(&fx.name, &fx.contents).unwrap_or_else(|d| {
            panic!(
                "valid workbook file {rel} must PARSE+CONFORM (FORMAT §2/§4/§5/§6), \
                 but charlie-model rejected it:\n  {d}"
            )
        });

        // A literal body must have a §6 placement verdict; a formula body is opaque in W2 (§4.1).
        match parsed.body {
            Body::Literal(_) => assert!(
                parsed.placement.is_some(),
                "literal body in {rel} must yield a §6 Placement (FORMAT §6)"
            ),
            Body::Formula(_) => assert!(
                parsed.placement.is_none(),
                "formula body in {rel} is opaque in W2 — no §6 verdict yet (FORMAT §4.1)"
            ),
        }

        // Region for the §7 overlap check, keyed by the containing tab (parent directory). Reuse the
        // region `parse_file` already recovered — no second `parse_filename` of the same name.
        let tab = Path::new(rel)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        tabs.entry(tab)
            .or_default()
            .push((fx.name.clone(), parsed.region));
    }

    // (b) NO UNINTENDED OVERLAP within any tab (FORMAT §7).
    for (tab, claims) in &tabs {
        let diags = detect_overlaps(tab, claims);
        assert!(
            diags.is_empty(),
            "tab `{tab}` in valid workbook `{category}` must be overlap-free (FORMAT §7), \
             but charlie-model reported:\n  {}",
            diags
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join("\n  ")
        );
    }

    files.len()
}

#[test]
fn valid_workbook_aggregation() {
    assert!(assert_valid_workbook("aggregation") > 0);
}

#[test]
fn valid_workbook_conditional() {
    assert!(assert_valid_workbook("conditional") > 0);
}

#[test]
fn valid_workbook_dates() {
    assert!(assert_valid_workbook("dates") > 0);
}

#[test]
fn valid_workbook_lookup_join() {
    assert!(assert_valid_workbook("lookup-join") > 0);
}

#[test]
fn valid_workbook_model() {
    assert!(assert_valid_workbook("model") > 0);
}

#[test]
fn valid_workbook_text() {
    assert!(assert_valid_workbook("text") > 0);
}

// ---------------------------------------------------------------------------------------------
// conformance-forms/ — the stated broadcast verdict per fixture (FORMAT §6 / §6.1).
// Each assertion cites the fixture path + the § its EXPECTED.md pins.
// ---------------------------------------------------------------------------------------------

/// Assert a conformance-forms fixture loads to the expected §6 `Placement`.
fn assert_placement(rel: &str, expected: Placement, section: &str) {
    let fx = load_fixture(rel);
    let parsed = parse_file(&fx.name, &fx.contents)
        .unwrap_or_else(|d| panic!("{rel} must load (its EXPECTED verdict is VALID): {d}"));
    assert_eq!(
        parsed.placement,
        Some(expected),
        "{rel}: EXPECTED.md verdict is {expected:?} ({section})"
    );
}

#[test]
fn conformance_form_broadcast_down() {
    // Declared 2×3 (R>1), body 1×3 row vector -> broadcast DOWN (k==C).
    assert_placement(
        "artifacts/conformance-forms/broadcast-down/Rates/A1:C2.range",
        Placement::BroadcastDown,
        "FORMAT §6 row-vector row + §6.1",
    );
}

#[test]
fn conformance_form_broadcast_across() {
    // Declared 3×2 (C>1), body 3×1 col vector -> broadcast ACROSS (k==R).
    assert_placement(
        "artifacts/conformance-forms/broadcast-across/Rates/A1:B3.range",
        Placement::BroadcastAcross,
        "FORMAT §6 col-vector row + §6.1",
    );
}

#[test]
fn conformance_form_square_disambiguator_is_single_verdict() {
    // The deliberate B1 kill-probe: a SQUARE 3×3 range (R==C) with a 1×3 row-vector body. §6.1 fixes
    // the axis from the body's own shape -> exactly ONE defensible verdict: broadcast DOWN. If this
    // asserted anything OTHER than a single unambiguous placement, THAT would be the B1 kill signal.
    assert_placement(
        "artifacts/conformance-forms/square-disambiguator/Grid/B2:D4.range",
        Placement::BroadcastDown,
        "FORMAT §6.1 disambiguator (R==C, body decides axis) — NOT ambiguous, no B1 kill",
    );
}

#[test]
fn conformance_form_degenerate_1x1_range_is_rejected() {
    // Placed under conformance-forms as an edge case, but its resolved verdict is a REJECTION: a 1×1
    // `.range` is illegal (a single cell is always `.cell`), rejected at the filename grammar.
    let fx = load_fixture("artifacts/conformance-forms/degenerate-1x1-range/Cell/A1:A1.range");
    let d = parse_file(&fx.name, &fx.contents).expect_err(&format!(
        "{}: EXPECTED.md verdict is REJECT (degenerate 1×1 range)",
        fx.rel
    ));
    assert_eq!(
        d.code,
        Code::DegenerateRange,
        "{}: must reject as degenerate-range (FORMAT §1/§2/§11 — rename to A1.cell)",
        fx.rel
    );
}

#[test]
fn single_row_range_degenerate_tie_resolves_to_exact_on_a_frozen_fixture() {
    // The R==1 degenerate tie (conformance.rs `classify_placement` notes): a 1×C body into a 1×C
    // range satisfies BOTH the row-vector rule (k==C) and the exact-array rule. The two labels place
    // the SAME cells, so no independent oracle could distinguish them and no EXPECTED.md pins one —
    // it is a behaviorally-unobservable, deterministic internal choice (strongest match -> Exact).
    // Anchor that choice to a FROZEN, provenance-guarded corpus input (a real column-header row:
    // `A1:E1.range`, a 1×5 range with a one-line 5-field literal body) rather than only a synthetic
    // shape in a src unit test, so the pinned label rides on the manifest-fingerprinted corpus.
    assert_placement(
        "artifacts/aggregation/sales_report/Sales/A1:E1.range",
        Placement::Exact,
        "FORMAT §6 exact-array wins the R==1 degenerate tie (behaviorally == row-vector; strongest match)",
    );
}

// ---------------------------------------------------------------------------------------------
// invalid-forms/ — the stated REJECTION with the right diagnostic category (FORMAT §… / §11).
// Each assertion cites the fixture + the § its EXPECTED.md pins.
// ---------------------------------------------------------------------------------------------

/// Assert a single-file invalid-forms fixture is rejected with the expected `Code` (and, where the
/// ledger names a spreadsheet-error class, that `err_class`).
fn assert_rejected(rel: &str, expected: Code, err_class: Option<ErrKind>, section: &str) {
    let fx = load_fixture(rel);
    let d = match parse_file(&fx.name, &fx.contents) {
        Ok(p) => panic!(
            "{rel}: EXPECTED.md verdict is REJECT ({section}), but charlie-model ACCEPTED it: {p:?}"
        ),
        Err(d) => d,
    };
    assert_eq!(
        d.code, expected,
        "{rel}: must reject with {expected:?} ({section})"
    );
    if let Some(class) = err_class {
        assert_eq!(
            d.code.err_class(),
            Some(class),
            "{rel}: reject must cite the {class:?} spreadsheet-error class ({section})"
        );
    }
}

#[test]
fn invalid_form_shape_mismatch() {
    // 2×3 body into a 3×3 range — neither exact nor broadcastable -> #SPILL!-class static refusal.
    assert_rejected(
        "artifacts/invalid-forms/shape-mismatch/Grid/A1:C3.range",
        Code::NonConforming,
        Some(ErrKind::Spill),
        "FORMAT §6 last row (#SPILL!-class) + §11",
    );
}

#[test]
fn invalid_form_illegal_name() {
    // `G8:A3.range` — non-canonical spelling (bottom-right:top-left) -> filename reject.
    assert_rejected(
        "artifacts/invalid-forms/illegal-forms/illegal-name/Sheet/G8:A3.range",
        Code::NonCanonicalRange,
        None,
        "FORMAT §2 (top-left:bottom-right) + §11",
    );
}

#[test]
fn invalid_form_ragged_block() {
    // Literal block with unequal field counts (3 then 2) -> #VALUE!-class structural refusal.
    assert_rejected(
        "artifacts/invalid-forms/illegal-forms/ragged-block/Sheet/A1:C2.range",
        Code::RaggedBlock,
        Some(ErrKind::Value),
        "FORMAT §5 (equal field counts, #VALUE!-class) + §11",
    );
}

#[test]
fn invalid_form_dual_body() {
    // Body carries BOTH an =formula line and a literal line -> reject (exactly one body form).
    let rel = "artifacts/invalid-forms/illegal-forms/dual-body/Sheet/A1.cell";
    let fx = load_fixture(rel);
    let d = parse_file(&fx.name, &fx.contents).expect_err(&format!(
        "{rel}: EXPECTED.md verdict is REJECT (dual body, FORMAT §4/§11)"
    ));
    assert_eq!(
        d.code,
        Code::DualBody,
        "{rel}: must reject with DualBody (FORMAT §4 body is exactly one form + §11)"
    );
    // The oracle ledger (dual-body/EXPECTED.md) pins the diagnostic shape: it must NAME the two
    // conflicting FILE lines — "line 2 is a formula, line 3 is a literal" — not just carry the Code.
    let msg = d.message.as_str();
    assert!(
        msg.contains("line 2 is the =formula") && msg.contains("line 3 is a literal"),
        "{rel}: §4/§11 dual-body diagnostic must name the conflicting lines \
         (ledger: line 2 formula, line 3 literal); got:\n  {d}"
    );
}

#[test]
fn invalid_form_stray_dollar() {
    // `$A$1.cell` — a `$` absolute marker in a filename -> filename reject ($ lives in bodies only).
    assert_rejected(
        "artifacts/invalid-forms/illegal-forms/stray-dollar/Sheet/$A$1.cell",
        Code::DollarInFilename,
        None,
        "FORMAT §2 (no $ in filenames) + §11",
    );
}

#[test]
fn invalid_form_overlap_names_both_files_and_contested_cells() {
    // The only MULTI-file fixture: two range files in one tab whose declared regions intersect.
    // Both parse fine individually; the §7 overlap is a tab-level check that must REJECT, naming
    // BOTH files and the contested cells (B2,C2,B3,C3 == the block B2:C3), with no precedence.
    let a = load_fixture("artifacts/invalid-forms/overlap/Orders/A1:C3.range");
    let b = load_fixture("artifacts/invalid-forms/overlap/Orders/B2:D4.range");

    // Each file is valid on its own (a 3×3 literal into a 3×3 range -> Exact).
    let pa = parse_file(&a.name, &a.contents).expect("A1:C3.range is valid in isolation");
    let pb = parse_file(&b.name, &b.contents).expect("B2:D4.range is valid in isolation");
    assert_eq!(pa.placement, Some(Placement::Exact));
    assert_eq!(pb.placement, Some(Placement::Exact));

    let claims = vec![(a.name.clone(), pa.region), (b.name.clone(), pb.region)];
    let diags = detect_overlaps("Orders", &claims);

    assert_eq!(
        diags.len(),
        1,
        "overlap fixture must produce exactly one §7 overlap diagnostic"
    );
    let d = &diags[0];
    assert_eq!(
        d.code,
        Code::Overlap,
        "EXPECTED.md verdict is overlap-reject (FORMAT §7 + §11)"
    );
    let msg = d.message.as_str();
    assert!(
        msg.contains("A1:C3.range") && msg.contains("B2:D4.range"),
        "§7 overlap diagnostic must NAME BOTH files; got:\n  {d}"
    );
    // The four contested cells B2,C2,B3,C3 render as the equivalent block label `B2:C3`.
    assert!(
        msg.contains("B2:C3"),
        "§7 overlap diagnostic must name the contested cells (B2:C3 == B2,C2,B3,C3); got:\n  {d}"
    );
    assert!(
        msg.contains("reject"),
        "§7 precedence is REJECT, never a guessed winner; got:\n  {d}"
    );
}

// ---------------------------------------------------------------------------------------------
// Corpus integrity: guards the harness against a silently-empty / relocated corpus (a path break
// would otherwise make every loop above vacuously pass).
// ---------------------------------------------------------------------------------------------

#[test]
fn corpus_is_present_and_non_trivial() {
    let all = collect_sheet_files("artifacts");
    // 6 valid workbooks + 4 conformance-forms + (6 shape/illegal + 2 overlap) invalid sheet files.
    assert!(
        all.len() >= 90,
        "the frozen corpus must be present and substantial; found only {} sheet files \
         (path break or truncated migration?)",
        all.len()
    );
    // The EXPECTED verdict ledgers must have graduated alongside the fixtures.
    for ledger in [
        "oracle/conformance-forms/EXPECTED.md",
        "oracle/invalid-forms/EXPECTED.md",
        "PROVENANCE.md",
        "MANIFEST.sha256",
        "FORMAT.md",
    ] {
        assert!(
            corpus_root().join(ledger).is_file(),
            "frozen contract must include {ledger}"
        );
    }
}

/// The pinned `sha256(MANIFEST.sha256)` from `conformance/encoding/PROVENANCE.md` — a single value
/// fingerprinting the whole 113-file frozen set. Pinning it *here in code* (not only in prose) is
/// what closes the "regenerate BOTH the fixtures and the manifest" hole: a manifest rewrite that
/// re-lists tampered fixtures changes this digest, and this assertion — not just a human reading the
/// prose — fails. Keep in sync with PROVENANCE.md if the corpus is ever deliberately GROWN.
const PINNED_MANIFEST_DIGEST: &str =
    "daee80c604c7244b157197c589664edceb15a6c7d1202d08110d25d6014eb4e0";

/// Run `sha256sum <args>` in the corpus root and return `(exit_ok, stdout)`. Fails LOUDLY if the
/// system `sha256sum` is absent — the frozen contract's purity guarantee depends on it (PROVENANCE.md
/// keeps the hash out of the crypto-free `charlie-model` by shelling to system `sha256sum`), so a
/// missing tool must not silently green-light an unverified corpus.
fn sha256sum(args: &[&str]) -> (bool, String) {
    let out = Command::new("sha256sum")
        .args(args)
        .current_dir(corpus_root())
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "system `sha256sum` is required to verify the frozen corpus fingerprint \
                 (conformance/encoding/PROVENANCE.md), but could not be run: {e}"
            )
        });
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn corpus_matches_frozen_fingerprint_at_the_grading_site() {
    // ORACLE-INPUT PURITY, asserted where verdicts are actually graded (a plain `cargo test`, the
    // commit gate's observed step), not only in .github/workflows/build.yml. Without this, a local
    // grading run scored charlie-model against whatever fixtures were on disk with no purity check.
    //
    // (1) Every migrated file must be byte-identical to the frozen contract: `sha256sum -c` re-hashes
    //     each of the 113 manifest lines and fails on the first mismatch. This catches a rewrite of
    //     any input fixture or EXPECTED ledger (the harness does NOT assert cell VALUES in the 6
    //     valid workbooks, so an edited literal would otherwise flip no verdict).
    let (ok, _) = sha256sum(&["-c", "MANIFEST.sha256", "--quiet"]);
    assert!(
        ok,
        "frozen corpus fingerprint FAILED: `sha256sum -c MANIFEST.sha256` reported a mismatch — a \
         fixture or EXPECTED ledger was edited (oracle-input purity, conformance/encoding/PROVENANCE.md)"
    );

    // (2) The manifest ITSELF must be the pinned one. Step (1) only proves the fixtures match
    //     whatever MANIFEST.sha256 lists; an agent that regenerates BOTH the fixtures AND the manifest
    //     would still pass (1). Pinning `sha256(MANIFEST.sha256)` to the digest recorded in
    //     PROVENANCE.md — and checking it mechanically here — closes that hole.
    let (ran, stdout) = sha256sum(&["MANIFEST.sha256"]);
    assert!(ran, "`sha256sum MANIFEST.sha256` did not run cleanly");
    let digest = stdout
        .split_whitespace()
        .next()
        .expect("sha256sum output must start with the digest");
    assert_eq!(
        digest, PINNED_MANIFEST_DIGEST,
        "MANIFEST.sha256 does not match its pinned digest (PROVENANCE.md); the manifest itself was \
         rewritten — a fixture+manifest co-regeneration, the exact silent-oracle-swap this pin guards"
    );
}
