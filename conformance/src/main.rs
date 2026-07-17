// Concern: the conformance CLI entry — parse argv and dispatch the three verbs (`report` | `backslide` | `resnapshot`), rendering their output and encoding each verdict in the EXIT CODE the pre-commit hook reads (backslide: 0 clean / 1 ≥1 lost Match / 2 anchor unreadable; resnapshot: 0 written / 1 refused regression / 2 bad existing anchor); bare invocation → `report` | Non-concern: the model + capture/anchor IO (the lib owns `capture`/`read_anchor`/`backslides`) — this only parses args, renders, and sets exit codes | IO: (argv) -> stdout/stderr + an exit code
//! The conformance CLI. Three verbs over the ONE `Facts` model:
//! - `report` (bare default) — SURFACE the coverage ratchet + every fixture's verdict (never fails;
//!   a Diverge is a fact, not an error);
//! - `backslide` — the pre-commit GUARD: compare current facts to the committed anchor and exit
//!   `0`/`1`/`2` on clean / lost-Match / unreadable-anchor;
//! - `resnapshot [--allow-backslide]` — (re)write the committed anchor, REFUSING (exit 1) to bake a
//!   regression unless the conscious override is given.

use std::process::ExitCode;

use conformance::snapshot::{self, VerdictKind};
use conformance::{Facts, anchor_path, capture, read_anchor};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let verb = args.first().map(String::as_str).unwrap_or("report");
    // Bare invocation (`conformance` with no verb) → `report`: guard the tail slice so an empty argv
    // does not panic (`&args[1..]` on a 0-length vec is out of range).
    let rest = args.get(1..).unwrap_or(&[]);
    match verb {
        "report" | "facts" => run_report(),
        "backslide" => run_backslide(),
        "resnapshot" => run_resnapshot(rest.iter().any(|a| a == "--allow-backslide")),
        "-h" | "--help" | "help" => {
            print_help();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("conformance: unknown verb {other:?}\n");
            print_help();
            ExitCode::from(2)
        }
    }
}

fn print_help() {
    eprintln!(
        "conformance — the formula-conformance ratchet\n\n\
         USAGE: conformance [report|backslide|resnapshot [--allow-backslide]]\n\n\
         report      surface the coverage ratchet + per-fixture verdicts (default; never fails)\n\
         backslide   guard current facts vs the committed anchor; exit 0 clean / 1 lost-Match / 2 no anchor\n\
         resnapshot  (re)write the committed facts anchor; refuses (exit 1) to bake a regression\n\
                     unless --allow-backslide is given"
    );
}

/// `report` — surface the ratchet and every fixture verdict. Facts are facts: always exit 0.
fn run_report() -> ExitCode {
    let facts = match capture() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("report: {e}");
            return ExitCode::from(2);
        }
    };
    let (matched, diverged): (Vec<_>, Vec<_>) = facts
        .verdicts
        .values()
        .partition(|v| v.kind == VerdictKind::Match);

    println!("=== FORMULA CONFORMANCE FACTS ===");
    println!("{}", facts.coverage.line());
    println!(
        "fixtures: {} total — {} Match, {} Diverge",
        facts.verdicts.len(),
        matched.len(),
        diverged.len()
    );
    if !facts.coverage.functions.is_empty() {
        println!("modeled functions: {}", facts.coverage.functions.join(", "));
    }
    if !diverged.is_empty() {
        println!("\n--- Diverge (surfaced facts, not gate failures) ---");
        for v in &diverged {
            println!("  {}  ::  {}", v.key, v.detail);
        }
    }
    ExitCode::SUCCESS
}

/// `backslide` — the pre-commit guard. Exit contract: `2` when the anchor cannot be read (fail
/// SAFE), `1` when ≥1 fixture lost its Match, `0` otherwise.
fn run_backslide() -> ExitCode {
    let anchor = match read_anchor() {
        Ok(a) => a,
        Err(e) => {
            eprintln!(
                "backslide: {e}\n\
                 (exit 2 — failing SAFE: cannot tell a regression from a clean tree with no anchor)"
            );
            return ExitCode::from(2);
        }
    };
    let current = match capture() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("backslide: cannot capture current facts: {e}");
            return ExitCode::from(2);
        }
    };
    let backslid = snapshot::backslides(&anchor, &current.verdicts);
    if backslid.is_empty() {
        println!("backslide: CLEAN — no fixture lost a Match vs the committed anchor.");
        println!("  {}", current.coverage.line());
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "backslide: {} fixture(s) LOST a Match vs the committed anchor:",
            backslid.len()
        );
        for b in &backslid {
            eprintln!("  {}  ::  now {}", b.key, b.now_detail);
        }
        ExitCode::from(1)
    }
}

/// `resnapshot` — (re)write the committed anchor. PRE-WRITE guard: refuse (exit 1) to bake a
/// regression (a fixture Match in the committed anchor, Diverge now) unless `--allow-backslide`.
fn run_resnapshot(allow_backslide: bool) -> ExitCode {
    let current = match capture() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("resnapshot: cannot capture current facts: {e}");
            return ExitCode::from(2);
        }
    };

    // Tolerate ABSENCE (the first-ever snapshot), fail-fast on a malformed EXISTING anchor (exit 2).
    let path = anchor_path();
    let committed = if path.exists() {
        match read_anchor() {
            Ok(a) => Some(a),
            Err(e) => {
                eprintln!("resnapshot: existing anchor is unreadable: {e} (exit 2)");
                return ExitCode::from(2);
            }
        }
    } else {
        None
    };

    if let Some(anchor) = &committed {
        let backslid = snapshot::backslides(anchor, &current.verdicts);
        if !backslid.is_empty() {
            eprintln!(
                "resnapshot: this would re-bless {} regression(s), baking a lost Match into the anchor:",
                backslid.len()
            );
            for b in &backslid {
                eprintln!("  {}  ::  now {}", b.key, b.now_detail);
            }
            if allow_backslide {
                eprintln!(
                    "resnapshot: --allow-backslide — consciously baking them in. Record WHY in the \
                     commit that carries the re-blessed anchor."
                );
            } else {
                eprintln!(
                    "resnapshot: REFUSING (exit 1). Fix the regression(s), or re-run with \
                     --allow-backslide to consciously re-bless (record WHY in the commit)."
                );
                return ExitCode::from(1);
            }
        }
    }

    if let Err(e) = write_anchor(&current) {
        eprintln!("resnapshot: {e}");
        return ExitCode::from(2);
    }
    eprintln!("resnapshot: wrote anchor → {}", path.display());
    eprintln!("  {}", current.coverage.line());
    println!("{}", path.display());
    ExitCode::SUCCESS
}

/// Write the facts snapshot to the committed anchor path (creating parent dirs on demand).
fn write_anchor(facts: &Facts) -> Result<(), String> {
    let path = anchor_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    std::fs::write(&path, facts.to_tsv()).map_err(|e| format!("write {}: {e}", path.display()))
}
