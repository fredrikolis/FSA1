// Concern: the conformance CLI — surfaces the corpus's graded facts | Non-concern: grading a fixture, loading the corpus | IO: (argv) -> stdout/stderr + an exit code

use std::process::ExitCode;

use conformance::capture;
use conformance::snapshot::VerdictKind;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let verb = args.first().map(String::as_str).unwrap_or("report");
    match verb {
        "report" | "facts" => run_report(),
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
