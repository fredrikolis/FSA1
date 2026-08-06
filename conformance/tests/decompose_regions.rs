// Concern: binds the default cut and `--decompose occupancy` to every frozen decompose expectation | Non-concern: authoring the corpus | IO: (decompose/fixtures + expected) -> pass/fail

use std::path::{Path, PathBuf};

use fsa1_ingest::Decomposition;
use fsa1_model::{Code, Workbook};

/// Repeated verbatim in every assertion message, because an agent reading one failure must not have
/// to find PROVENANCE.md to learn which side is allowed to move.
const CORRECTION_RULE: &str = "a frozen expectation is corrected ONLY when the reading of the \
     third-party-authored fixture was wrong -- never edited to chase an FSA1 regression";

/// The two `check` codes the acceptance criteria name. An empty lint is the bar; these are the two
/// failures it exists to catch, so a failure names them rather than leaving them to be looked up.
const NAMED_REFUSALS: [Code; 2] = [Code::NonCanonicalPresentation, Code::DegenerateRange];

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("decompose")
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

/// One labelled row-block: the reading of the fixture's authored structure, written before `unpack`
/// was ever run on it. Its START is what both recall checks are against.
struct Region {
    start: u32,
    label: String,
}

struct Expectation {
    sheet: String,
    regions: Vec<Region>,
    blocks: Vec<String>,
    policy: Decomposition,
    misses: usize,
}

/// One directive per line, `#` comments and blank lines ignored. A malformed frozen file is a corpus
/// defect rather than a verdict, so it panics here instead of failing one fixture's grading.
fn parse_expectation(name: &str, text: &str) -> Expectation {
    let mut sheet: Option<String> = None;
    let mut regions: Vec<Region> = Vec::new();
    let mut blocks: Vec<String> = Vec::new();
    let mut policy: Option<Decomposition> = None;
    let mut misses: Option<usize> = None;
    for line in text.lines().map(str::trim_end) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .unwrap_or_else(|| panic!("{name}: not a directive, a comment or blank: {line:?}"));
        let value = value.trim();
        match key.trim() {
            "sheet" => sheet = Some(value.to_string()),
            "region" => regions.push(parse_region(name, value)),
            "block" => blocks.push(value.to_string()),
            "policy" => {
                policy =
                    Some(value.parse().unwrap_or_else(|()| {
                        panic!("{name}: {value:?} names no decomposition policy")
                    }))
            }
            "misses" => {
                misses = Some(
                    value
                        .parse()
                        .unwrap_or_else(|e| panic!("{name}: {value:?} is not a count: {e}")),
                )
            }
            other => panic!("{name}: unknown directive {other:?}"),
        }
    }
    assert!(!regions.is_empty(), "{name}: no `region:` line");
    assert!(!blocks.is_empty(), "{name}: no `block:` line");
    Expectation {
        sheet: sheet.unwrap_or_else(|| panic!("{name}: no `sheet:` line")),
        regions,
        blocks,
        policy: policy.unwrap_or_else(|| panic!("{name}: no `policy:` line")),
        misses: misses.unwrap_or_else(|| panic!("{name}: no `misses:` line")),
    }
}

fn parse_region(name: &str, value: &str) -> Region {
    let (span, label) = value
        .split_once(' ')
        .unwrap_or_else(|| panic!("{name}: a `region:` is `<first>-<last> <the name>`: {value:?}"));
    let (first, last) = span
        .split_once('-')
        .unwrap_or_else(|| panic!("{name}: a region's rows are `<first>-<last>`: {span:?}"));
    let read = |what: &str, text: &str| -> u32 {
        text.parse()
            .unwrap_or_else(|e| panic!("{name}: {what} row {text:?} is not a row number: {e}"))
    };
    let (start, end) = (read("first", first), read("last", last));
    assert!(
        start <= end,
        "{name}: region {span:?} ends before it starts"
    );
    Region {
        start,
        label: label.to_string(),
    }
}

/// The `(row, col)` a closed A1 range anchors at. A one-cell block is named `A1`, never `A1:A1`, so
/// the head of the split is the anchor either way.
fn anchor(what: &str, range: &str) -> (u32, u32) {
    let head = range
        .split(':')
        .next()
        .expect("a split always yields a first part");
    let digits = head
        .find(|c: char| c.is_ascii_digit())
        .unwrap_or_else(|| panic!("{what}: {range:?} is not an A1 range"));
    let (letters, row) = head.split_at(digits);
    assert!(
        !letters.is_empty() && letters.bytes().all(|b| b.is_ascii_uppercase()),
        "{what}: {range:?} does not open with an uppercase column"
    );
    let col = letters
        .bytes()
        .fold(0u32, |n, b| n * 26 + u32::from(b - b'A' + 1));
    let row = row
        .parse()
        .unwrap_or_else(|e| panic!("{what}: {range:?} carries no row number: {e}"));
    (row, col)
}

/// Every range file the run wrote for `sheet`, in ascending anchor order -- the order the policy
/// itself emits blocks in, and the order the frozen `block:` lines are held to.
fn read_blocks(what: &str, root: &Path, sheet: &str) -> Result<Vec<String>, String> {
    let mut tabs: Vec<String> = std::fs::read_dir(root)
        .map_err(|e| format!("the unpacked workbook is unreadable: {e}"))?
        .map(|e| e.expect("a readable entry").path())
        .filter(|p| p.is_dir())
        .map(|p| base(&p))
        .collect();
    tabs.sort();
    if tabs != [sheet.to_string()] {
        return Err(format!(
            "the run wrote tabs {tabs:?}, and the frozen reading covers {sheet:?} alone"
        ));
    }
    let mut names: Vec<String> = std::fs::read_dir(root.join(sheet))
        .map_err(|e| format!("the tab {sheet:?} is unreadable: {e}"))?
        .map(|e| e.expect("a readable entry").path())
        .filter(|p| p.is_file())
        .map(|p| base(&p))
        .collect();
    names.sort_by_key(|name| anchor(what, name));
    Ok(names)
}

/// The labelled starts no block starts at. Empty is recall 1.000 -- the property has no margin, no
/// mean and no bar: it holds or it does not.
fn missed<'a>(what: &str, regions: &'a [Region], blocks: &[String]) -> Vec<&'a Region> {
    let starts: Vec<u32> = blocks.iter().map(|b| anchor(what, b).0).collect();
    regions
        .iter()
        .filter(|r| !starts.contains(&r.start))
        .collect()
}

fn spell(regions: &[&Region]) -> String {
    regions
        .iter()
        .map(|r| format!("row {} ({})", r.start, r.label))
        .collect::<Vec<String>>()
        .join(", ")
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

fn frozen(fixture: &Path) -> Expectation {
    let text = std::fs::read_to_string(expectation_of(fixture)).expect("a readable expectation");
    parse_expectation(&stem(fixture), &text)
}

/// A per-fixture scratch root, wiped first so a previous run's tree can never be read as this one's.
/// The caller removes it on EVERY exit from a grading, so a fixture that ends early leaves none.
fn workdir(stem: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fsa1-decompose-{}-{stem}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch dir");
    dir
}

/// A fixture with no reading is graded by nothing, and the graders below skip it silently. Adding one
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
        "no expectation under conformance/decompose/expected/ for {missing:?}. Freeze each by \
         reading the third-party-authored fixture (conformance/decompose/PROVENANCE.md says how); \
         {CORRECTION_RULE}"
    );
}

/// Recall on the FROZEN file, before any binary runs: a labelled start no frozen block starts at is
/// a corpus defect, whatever FSA1 then does. Ascending anchors is held per fixture in the same pass,
/// and the corpus's COMPOSITION after it -- that its `appearance` fixtures cover both kinds -- since
/// all of them are properties of the readings rather than of a run.
#[test]
fn every_frozen_file_states_a_reading_its_own_block_list_covers() {
    let mut failures: Vec<String> = Vec::new();
    let (mut grades_recall, mut guards_fragmentation) = (false, false);
    for fixture in fixtures().iter().filter(|f| expectation_of(f).exists()) {
        let (stem, want) = (stem(fixture), frozen(fixture));
        let missed = missed(&stem, &want.regions, &want.blocks);
        if !missed.is_empty() {
            failures.push(format!(
                "=== {stem} ===\nno frozen block starts at {}; recall over the labelled starts is \
                 exactly 1.000, so the reading and the block list disagree; {CORRECTION_RULE}",
                spell(&missed)
            ));
        }
        let anchors: Vec<(u32, u32)> = want.blocks.iter().map(|b| anchor(&stem, b)).collect();
        if anchors.windows(2).any(|w| w[0] >= w[1]) {
            failures.push(format!(
                "=== {stem} ===\nthe frozen blocks {:?} are not ascending by anchor, which is the \
                 order the policy emits them in",
                want.blocks
            ));
        }
        if want.policy == Decomposition::Appearance {
            grades_recall |= want.misses >= 1;
            guards_fragmentation |= want.misses == 0;
        }
    }
    if !grades_recall {
        failures.push(format!(
            "=== the corpus ===\nno `appearance` fixture carries `misses:` of 1 or more, so nothing \
             here grades the RECALL `--decompose occupancy` cannot reach: the corpus may not be \
             authored all-easy; {CORRECTION_RULE}"
        ));
    }
    if !guards_fragmentation {
        failures.push(format!(
            "=== the corpus ===\nno `appearance` fixture carries `misses: 0`, so nothing here \
             guards against FRAGMENTATION -- a sheet `occupancy` cuts correctly and `appearance` \
             could shatter: the corpus must hold at least one; {CORRECTION_RULE}"
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// The graded cut, end to end: the blocks the frozen policy writes, the recall of the labelled
/// starts over what it actually wrote, and `check`'s verdict on the tree. The policy is NAMED, never
/// left to the default -- this corpus grades a codec, and which codec `unpack` reaches for when no
/// one names one is a separate question with its own assertions.
fn grade_default(fixture: &Path, work: &Path, want: &Expectation) -> Result<(), String> {
    let stem = stem(fixture);
    let unpacked = work.join("default");
    let report = fsa1_ingest::import_file_as(fixture, &unpacked, false, want.policy)
        .map_err(|e| format!("unpack failed: {e}"))?;
    if report.decomposition != want.policy {
        return Err(format!(
            "the source resolved to `{}`, not the frozen `{}`; {CORRECTION_RULE}",
            report.decomposition.name(),
            want.policy.name()
        ));
    }
    let got = read_blocks(&stem, &unpacked, &want.sheet)?;
    if got != want.blocks {
        return Err(format!(
            "the blocks the default policy wrote are not the frozen ones; {CORRECTION_RULE}\n\
             --- frozen ---\n{:?}\n--- got ---\n{got:?}",
            want.blocks
        ));
    }
    let missed = missed(&stem, &want.regions, &got);
    if !missed.is_empty() {
        return Err(format!(
            "the run wrote no block starting at {}; recall over the labelled starts is exactly \
             1.000; {CORRECTION_RULE}",
            spell(&missed)
        ));
    }
    let workbook = Workbook::load_dir(&unpacked)
        .expect("the unpacked tree is readable")
        .map_err(|diags| format!("`check` refuses what `unpack` wrote: {diags:?}"))?;
    let lint = workbook.lint();
    if lint.is_empty() {
        return Ok(());
    }
    let named: Vec<&str> = NAMED_REFUSALS.iter().map(|c| c.code_str()).collect();
    Err(format!(
        "`check` reports {} diagnostic(s) on what `unpack` wrote, and it may carry none -- least of \
         all {named:?}: {lint:?}",
        lint.len()
    ))
}

/// What the OLD policy leaves unaddressable, which is the bar this corpus is authored against: a
/// fixture `--decompose occupancy` already cuts at every labelled start grades nothing.
fn grade_occupancy(fixture: &Path, work: &Path, want: &Expectation) -> Result<(), String> {
    let stem = stem(fixture);
    let unpacked = work.join("occupancy");
    fsa1_ingest::import_file_as(fixture, &unpacked, false, Decomposition::Occupancy)
        .map_err(|e| format!("unpack --decompose occupancy failed: {e}"))?;
    let blocks = read_blocks(&stem, &unpacked, &want.sheet)?;
    let missed = missed(&stem, &want.regions, &blocks);
    if missed.len() == want.misses {
        return Ok(());
    }
    Err(format!(
        "`--decompose occupancy` wrote {blocks:?}, missing {} of the labelled starts against the \
         frozen {}: {}; {CORRECTION_RULE}",
        missed.len(),
        want.misses,
        spell(&missed)
    ))
}

/// Every fixture graded in ONE run, so a change that breaks three of them names three rather than
/// the alphabetically first. Each fixture's scratch tree is removed whatever its verdict.
#[test]
fn every_fixture_cuts_into_its_frozen_blocks_under_both_policies() {
    let mut failures: Vec<String> = Vec::new();
    for fixture in fixtures() {
        if !expectation_of(&fixture).exists() {
            continue;
        }
        let (stem, want) = (stem(&fixture), frozen(&fixture));
        let work = workdir(&stem);
        let verdict = grade_default(&fixture, &work, &want)
            .and_then(|()| grade_occupancy(&fixture, &work, &want));
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
