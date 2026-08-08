// Concern: binds unpack/pack to every frozen presentation expectation | Non-concern: the Python leg's third-party reopen, authoring the corpus | IO: (presentation/fixtures + expected) -> pass/fail

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fsa1_model::{Overlay, Workbook};

/// Repeated verbatim in every assertion message, because an agent reading one failure must not have
/// to find this file to learn which side is allowed to move.
const CORRECTION_RULE: &str = "a frozen expectation is corrected ONLY when the reading of the \
     openpyxl-authored fixture was wrong -- never edited to chase an FSA1 regression";

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("presentation")
}

fn base(path: &Path) -> String {
    path.file_name()
        .expect("a corpus path always names its last segment")
        .to_string_lossy()
        .into_owned()
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .expect("a corpus path always names its last segment")
        .to_string_lossy()
        .into_owned()
}

/// Every entry under a workbook dir — range files and presentation sidecars alike — keyed
/// `"<tab>/<name>"`. `BTreeMap` so the comparison and the diff are both in one order.
fn read_tree(root: &Path) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    let mut tabs: Vec<PathBuf> = std::fs::read_dir(root)
        .expect("the unpacked workbook is readable")
        .map(|e| e.expect("a readable entry").path())
        .filter(|p| p.is_dir())
        .collect();
    tabs.sort();
    for tab in tabs {
        let tab_name = base(&tab);
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&tab)
            .expect("the tab is readable")
            .map(|e| e.expect("a readable entry").path())
            .filter(|p| p.is_file())
            .collect();
        entries.sort();
        for entry in entries {
            // A frozen `file:` line names a REGION, which has one canonical spelling.
            let raw = base(&entry);
            let name = fsa1_model::canonical_range_name(&raw);
            let content = std::fs::read_to_string(&entry).expect("a UTF-8 range file");
            files.insert(format!("{tab_name}/{name}"), content);
        }
    }
    files
}

/// The corpus's canonical text form, which IS the on-disk expectation format: a `warning:` line per
/// SER3 item, then a `file:` line per range file with its contents `|`-prefixed. The prefix is what
/// makes an empty grid line distinguishable from nothing at all.
fn render(warnings: &[String], files: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    for warning in warnings {
        out.push_str(&format!("warning: {warning}\n"));
    }
    for (name, content) in files {
        out.push_str(&format!("file: {name}\n"));
        for line in content.split('\n') {
            out.push_str(&format!("|{line}\n"));
        }
    }
    out
}

/// The inverse of [`render`] over a frozen file, ignoring `#` comments and blank separators.
fn parse_expectation(name: &str, text: &str) -> (Vec<String>, BTreeMap<String, String>) {
    let mut warnings = Vec::new();
    let mut files: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut current = String::new();
    for line in text.split('\n') {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("warning: ") {
            warnings.push(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("file: ") {
            current = rest.to_string();
            files.insert(current.clone(), Vec::new());
        } else if let Some(rest) = line.strip_prefix('|') {
            files
                .get_mut(&current)
                .unwrap_or_else(|| panic!("{name}: a `|` content line before any `file:` line"))
                .push(rest.to_string());
        } else {
            panic!("{name}: not a directive, a comment or a `|` content line: {line:?}");
        }
    }
    (
        warnings,
        files.into_iter().map(|(k, v)| (k, v.join("\n"))).collect(),
    )
}

/// A per-fixture scratch root, wiped first so a previous run's tree can never be read as this one's.
/// The caller removes it on EVERY exit from a grading, so a fixture that ends early leaves none.
fn workdir(stem: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fsa1-presentation-{}-{stem}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch dir");
    dir
}

/// The two renderings differ, as the text a failure carries. `None` where they agree.
fn diff(what: &str, want: &str, got: &str) -> Option<String> {
    (want != got).then(|| format!("{what}\n--- frozen ---\n{want}--- got ---\n{got}"))
}

fn fixtures() -> Vec<PathBuf> {
    let dir = corpus().join("fixtures");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{} must be readable: {e}", dir.display()))
        .map(|e| e.expect("a readable entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "xlsx"))
        .collect();
    found.sort();
    found
}

fn expectation_of(fixture: &Path) -> PathBuf {
    corpus()
        .join("expected")
        .join(format!("{}.expected", stem(fixture)))
}

/// A fixture with no reading is graded by nothing, and the grader below skips it silently. Adding one
/// without its expectation is therefore the one change that makes this corpus quietly stop covering
/// what it claims to, so it is caught here rather than at the next regression.
#[test]
fn every_fixture_carries_a_frozen_expectation() {
    let missing: Vec<String> = fixtures()
        .iter()
        .filter(|f| !expectation_of(f).exists())
        .map(|f| base(f))
        .collect();
    assert!(
        missing.is_empty(),
        "no expectation under conformance/presentation/expected/ for {missing:?}. Freeze each by \
         reading the openpyxl-authored fixture (conformance/presentation/PROVENANCE.md says how); \
         {CORRECTION_RULE}"
    );
}

/// One `unpack` -> frozen-diff -> `check` -> `pack` -> `unpack` cycle over ONE fixture, its verdict
/// returned rather than raised. The second unpack is what proves the styling SURVIVED the pack: an
/// export that came out visually blank re-unpacks to a tree with no sidecar at all, whatever the
/// first leg said.
fn grade(fixture: &Path, work: &Path) -> Result<(), String> {
    let stem = stem(fixture);
    let text = std::fs::read_to_string(expectation_of(fixture)).expect("a readable expectation");
    let (want_warnings, want_files) = parse_expectation(&stem, &text);

    let unpacked = work.join("wb");
    let report = fsa1_ingest::import_file(fixture, &unpacked, false)
        .map_err(|e| format!("unpack failed: {e}"))?;

    let got_files = read_tree(&unpacked);
    if let Some(diff) = diff(
        &format!("the range files unpack wrote are not the frozen ones; {CORRECTION_RULE}"),
        &render(&[], &want_files),
        &render(&[], &got_files),
    ) {
        return Err(diff);
    }

    let got_warnings: Vec<String> = report.warnings.iter().map(|w| w.to_string()).collect();
    if got_warnings != want_warnings {
        return Err(format!(
            "the SER3 warnings unpack reported are not the frozen ones; {CORRECTION_RULE}\n\
             --- frozen ---\n{want_warnings:#?}\n--- got ---\n{got_warnings:#?}"
        ));
    }

    let workbook = Workbook::load_dir(&unpacked)
        .expect("the unpacked tree is readable")
        .map_err(|diags| format!("`check` refuses what `unpack` wrote: {diags:?}"))?;
    // What `check` does: both loads are graded, and the pack below opens its own overlay anyway.
    Overlay::load_dir(&unpacked)
        .expect("the unpacked tree is readable")
        .map_err(|diags| format!("`check` refuses the sidecars `unpack` wrote: {diags:?}"))?;
    let lint = workbook.lint();
    if !lint.is_empty() {
        return Err(format!(
            "`check` reports {} diagnostic(s) on what `unpack` wrote: {lint:?}",
            lint.len()
        ));
    }

    if got_files.is_empty() {
        return Ok(());
    }
    let packed = work.join("packed.xlsx");
    // The VERB, not the writer: grading a pack that skipped the chart leg grades nothing a caller runs.
    let packed_report = fsa1_verbs::ops::pack(&unpacked, Some(&packed), "xlsx", false)
        .map_err(|e| format!("pack failed: {}", e.message))?;
    if !packed_report.not_drawn.is_empty() {
        return Err(format!(
            "pack drew no chart for {:?}",
            packed_report
                .not_drawn
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<String>>()
        ));
    }

    let reopened = work.join("reopened");
    fsa1_ingest::import_file(&packed, &reopened, false)
        .map_err(|e| format!("re-unpack of the packed export failed: {e}"))?;
    match diff(
        "the tree did not survive the pack -- unpack(pack(unpack(x))) differs from unpack(x)",
        &render(&[], &got_files),
        &render(&[], &read_tree(&reopened)),
    ) {
        Some(diff) => Err(diff),
        None => Ok(()),
    }
}

/// Every fixture graded in ONE run, so a change that breaks four of them names four rather than the
/// alphabetically first. Each fixture's scratch tree is removed whatever its verdict.
#[test]
fn every_fixture_unpacks_to_its_frozen_scope_and_survives_a_pack() {
    let mut failures: Vec<String> = Vec::new();
    for fixture in fixtures() {
        let stem = stem(&fixture);
        if !expectation_of(&fixture).exists() {
            continue;
        }
        let work = workdir(&stem);
        let verdict = grade(&fixture, &work);
        let _ = std::fs::remove_dir_all(&work);
        if let Err(why) = verdict {
            failures.push(format!("=== {stem} ===\n{why}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} fixture(s) failed:\n\n{}",
        failures.len(),
        fixtures().len(),
        failures.join("\n\n")
    );
}
