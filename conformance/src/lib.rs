// Concern: the conformance crate root — wire the modules (literal grammar, corpus loader, stub resolver, grader, facts snapshot, vendored sha256) and own the CAPTURE + anchor-IO seam the CLI and tests share: grade the whole `formula/` corpus into a `Facts` crumb, resolve the committed `facts-snapshot.tsv` anchor path, and read it back (fail-fast on a foreign/unreadable anchor) | Non-concern: CLI arg parsing + exit-code dispatch (main.rs owns the verbs) and the per-module logic (each module owns its concern) — this is the assembly + the capture/read boundary | IO: (the `formula/` corpus + git) -> a `Facts`; (the anchor path) -> parsed verdicts
//! `conformance` — the FORMULA-conformance ratchet for charlie. It grades charlie-ast against a
//! frozen corpus of value probes whose EXPECTED values are authored externally (`formula/PROVENANCE.md`),
//! and gates the W3b function grind with a `backslide` guard: a commit may add non-conforming fixtures
//! freely, but may never make a fixture that Matched stop Matching. The guard's verdict is its EXIT
//! CODE (0 clean / 1 ≥1 lost Match / 2 anchor unreadable — fail SAFE), wired into `.githooks/pre-commit`.

use std::path::{Path, PathBuf};

pub mod corpus;
pub mod grade;
pub mod literal;
pub mod resolver;
pub mod sha256;
pub mod snapshot;

pub use snapshot::{Backslide, Coverage, Facts, Meta, Verdict, VerdictKind};

/// The COMMITTED anchor path: `<crate>/formula/facts-snapshot.tsv`, a TRACKED file that travels with
/// the commit (so "what Matched at HEAD" is versioned alongside the corpus it grades). Anchored to
/// the manifest dir so it resolves identically from any cwd.
pub fn anchor_path() -> PathBuf {
    corpus::corpus_dir().join("facts-snapshot.tsv")
}

/// Capture the CURRENT facts: load + grade the whole corpus, compute coverage, stamp fresh
/// provenance. Fail-fast (`Err`) on a broken corpus (an unreadable dir, a malformed record).
pub fn capture() -> Result<Facts, String> {
    let fixtures = corpus::load_all()?;
    let verdicts = grade::grade_all(&fixtures);
    Ok(Facts::capture(&fixtures, verdicts, capture_meta()))
}

/// Read the committed anchor's verdicts, or `Err` if the file is missing / unreadable / malformed
/// (the caller maps this to the fail-SAFE exit 2 — "can't verify" is never "clean").
pub fn read_anchor() -> Result<std::collections::BTreeMap<String, Verdict>, String> {
    let path = anchor_path();
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read anchor {}: {e}", path.display()))?;
    Facts::parse_verdicts(&text)
}

/// Stamp fresh provenance around a capture: the wall clock plus a FAILURE-TOLERANT git shell-out
/// (honest-or-absent — never a fabricated sha).
fn capture_meta() -> Meta {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    Meta {
        captured_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        git_commit: git(dir, &["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_string()),
        git_dirty: git(dir, &["status", "--porcelain"]).is_some_and(|s| !s.trim().is_empty()),
        tool: format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
    }
}

/// Run `git <args>` in `dir`, returning trimmed stdout on success and `None` on ANY failure.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout)
        .ok()
        .map(|s| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole live corpus loads, grades without panic, and every fixture parses. This is the
    /// corpus's own well-formedness gate (an integrity floor: no swallowed crash).
    #[test]
    fn the_live_corpus_loads_and_grades_without_panic() {
        let facts = capture().expect("the corpus must load and grade cleanly");
        assert!(
            !facts.verdicts.is_empty(),
            "the seed corpus must not be empty"
        );
    }

    /// The COMMITTED anchor is self-consistent with the current tree: no fixture that Matched in the
    /// anchor diverges now. This makes `cargo test` a HARD backslide gate (the un-overridable twin of
    /// the pre-commit hook), so a regression cannot land even if the hook is bypassed.
    #[test]
    fn the_committed_anchor_shows_no_backslide() {
        let anchor = read_anchor().expect("the committed anchor must be readable");
        let current = capture().expect("capture").verdicts;
        let backslid = snapshot::backslides(&anchor, &current);
        assert!(
            backslid.is_empty(),
            "committed anchor backslides: {:?}",
            backslid.iter().map(|b| &b.key).collect::<Vec<_>>()
        );
    }
}
