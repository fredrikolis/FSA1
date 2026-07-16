// Concern: the ORACLE-INPUT-PURITY gate — recompute the SHA-256 of every fingerprinted corpus file with the crate's vendored hash and assert it equals the committed `MANIFEST.sha256`, and assert every `.fixtures` file in `formula/` is covered by the manifest, so a silent tamper of an oracle input (a fixture's context or its EXPECTED value) or an unfingerprinted new corpus file fails the gate LOUDLY | Non-concern: GRADING the fixtures (the crate's lib tests + the runner do that) and the hash transform itself (sha256.rs owns + self-tests it against the NIST vectors) — this only binds the corpus bytes to the manifest | IO: (the committed corpus + MANIFEST.sha256) -> pass/fail
//! Corpus fingerprint gate. `PROVENANCE.md` promises the EXPECTED values were authored externally and
//! the `.fixtures` bytes are frozen; this test makes that promise mechanically enforced — every
//! manifest entry must still hash to its recorded digest, and no `.fixtures` file may escape the
//! manifest. A deliberate corpus edit regenerates the manifest (see its header comment); an
//! accidental one reddens here.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn formula_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("formula")
}

/// Parse the `sha256sum`-format manifest into `(filename, hex)` pairs, skipping `#` comment lines.
fn manifest_entries(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            // `<hex>  <filename>` — two spaces per the sha256sum convention, but split leniently.
            let (hex, name) = l.split_once("  ").or_else(|| l.split_once(' '))?;
            Some((name.trim().to_string(), hex.trim().to_string()))
        })
        .collect()
}

#[test]
fn every_manifest_entry_still_hashes_to_its_recorded_digest() {
    let dir = formula_dir();
    let manifest = std::fs::read_to_string(dir.join("MANIFEST.sha256"))
        .expect("MANIFEST.sha256 must be readable");
    let entries = manifest_entries(&manifest);
    assert!(
        !entries.is_empty(),
        "the manifest must fingerprint at least the seed corpus"
    );
    for (name, want) in &entries {
        let bytes = std::fs::read(dir.join(name))
            .unwrap_or_else(|e| panic!("fingerprinted file {name} must be readable: {e}"));
        let got = conformance::sha256::hex_digest(&bytes);
        assert_eq!(
            &got, want,
            "corpus file {name} has been tampered (digest changed) — a Diverge means fix charlie, \
             never edit an oracle; if the edit was DELIBERATE, regenerate MANIFEST.sha256"
        );
    }
}

#[test]
fn provenance_stays_fingerprinted_by_the_manifest() {
    // The `.fixtures` coverage gate below only guards files with the `.fixtures` extension, so
    // PROVENANCE.md — the promise that the EXPECTED values were authored externally — could be
    // dropped from the manifest and lose its tamper-evidence undetected (its bytes are only checked
    // WHILE it remains listed). Pin its coverage explicitly so removing its manifest line reddens.
    let dir = formula_dir();
    let manifest = std::fs::read_to_string(dir.join("MANIFEST.sha256")).expect("manifest readable");
    let covered: BTreeSet<String> = manifest_entries(&manifest)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert!(
        covered.contains("PROVENANCE.md"),
        "PROVENANCE.md must stay fingerprinted in MANIFEST.sha256 — dropping its line would remove \
         the provenance doc's tamper-evidence silently"
    );
}

#[test]
fn every_fixtures_file_is_covered_by_the_manifest() {
    let dir = formula_dir();
    let manifest = std::fs::read_to_string(dir.join("MANIFEST.sha256")).expect("manifest readable");
    let covered: BTreeSet<String> = manifest_entries(&manifest)
        .into_iter()
        .map(|(name, _)| name)
        .collect();

    for entry in std::fs::read_dir(&dir).expect("formula dir readable") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_some_and(|x| x == "fixtures") {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            assert!(
                covered.contains(&name),
                "corpus file {name} is not fingerprinted in MANIFEST.sha256 — add it (an \
                 unfingerprinted oracle input can be tampered silently)"
            );
        }
    }
}
