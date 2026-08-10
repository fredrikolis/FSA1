// Concern: binds PRES2's carriage guarantee to the presentation corpus, sidecar by sidecar and placement by placement | Non-concern: what the page paints | IO: (presentation/fixtures) -> pass/fail

use std::path::{Path, PathBuf};

use fsa1_model::{Overlay, RenderMode, ViewScope, Workbook, view};

fn fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("presentation/fixtures");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{} must be readable: {e}", dir.display()))
        .map(|e| e.expect("a readable entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "xlsx"))
        .collect();
    found.sort();
    found
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .expect("a corpus path always names its last segment")
        .to_string_lossy()
        .into_owned()
}

fn workdir(stem: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fsa1-transparent-{}-{stem}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch dir");
    dir
}

/// Every `<stem>.css` beside a `<stem>.json`: a figure's PLACEMENT, which is no scope and states no
/// presentation, so the document that carries every sidecar must carry none of these.
fn placements(root: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for tab in std::fs::read_dir(root).expect("the unpacked tree is readable") {
        let tab = tab.expect("a readable entry").path();
        if !tab.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&tab).expect("the tab is readable") {
            let css = entry.expect("a readable entry").path();
            if css.extension().is_some_and(|x| x == "css") && css.with_extension("json").exists() {
                let text = std::fs::read_to_string(&css).expect("a UTF-8 sidecar");
                out.push((css.display().to_string(), text));
            }
        }
    }
    out
}

/// The whole of PRES2's first half over one fixture: the exporter WRAPS a sidecar's bytes and does
/// nothing else to them, so each one is a literal substring of the page. Nothing here parses CSS —
/// a harness that re-derived the text would agree with itself and prove nothing. `false` is a
/// fixture no viewport spans, which has no document to read and is counted rather than passed.
fn grade(fixture: &Path, work: &Path) -> Result<bool, String> {
    let unpacked = work.join("wb");
    fsa1_ingest::import_file(fixture, &unpacked, false)
        .map_err(|e| format!("unpack failed: {e}"))?;
    let wb = Workbook::load_dir(&unpacked)
        .expect("the unpacked tree is readable")
        .map_err(|diags| format!("the tree does not load: {diags:?}"))?;
    let overlay = Overlay::load_dir(&unpacked)
        .expect("the unpacked tree is readable")
        .map_err(|diags| format!("its sidecars do not load: {diags:?}"))?;
    let Ok(v) = view(
        &wb,
        Some(&overlay),
        ViewScope::Workbook,
        RenderMode::Values,
        &[],
    ) else {
        return Ok(false);
    };
    let html = fsa1_html::document(&wb, &overlay, &v, &[]);

    for sheet in &v.sheets {
        for scope in overlay.scopes(&wb, sheet.sheet) {
            if !html.contains(scope.text) {
                return Err(format!(
                    "{}'s bytes are not in the document VERBATIM; the exporter may wrap a sidecar \
                     and nothing else -- never sort, dedupe, re-indent or re-spell one\n--- \
                     authored ---\n{}",
                    scope.file, scope.text
                ));
            }
        }
    }
    for (name, text) in placements(&unpacked) {
        if html.contains(&text) {
            return Err(format!(
                "{name} is a figure's PLACEMENT, not presentation; it states where a chart sits in \
                 EMU and must reach no <style>\n--- carried ---\n{text}"
            ));
        }
    }
    Ok(true)
}

/// Every fixture graded in ONE run, so a change that breaks eighteen of them names eighteen rather
/// than the alphabetically first. Each fixture's scratch tree is removed whatever its verdict, and
/// the drawn count is asserted so a corpus that quietly stopped rendering fails rather than passes.
#[test]
fn every_fixtures_sidecars_reach_the_html_export_byte_for_byte() {
    let mut failures: Vec<String> = Vec::new();
    let mut drawn = 0;
    for fixture in fixtures() {
        let stem = stem(&fixture);
        let work = workdir(&stem);
        let verdict = grade(&fixture, &work);
        let _ = std::fs::remove_dir_all(&work);
        match verdict {
            Ok(graded) => drawn += u32::from(graded),
            Err(why) => failures.push(format!("=== {stem} ===\n{why}")),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} fixture(s) failed:\n\n{}",
        failures.len(),
        fixtures().len(),
        failures.join("\n\n")
    );
    assert_eq!(
        drawn,
        fixtures().len() as u32 - 1,
        "every fixture but stray_cell_sheet, whose stray coordinate spans a viewport over the \
         render bound, draws a document to read"
    );
}
