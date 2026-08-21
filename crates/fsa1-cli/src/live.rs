// Concern: runs the guide's five verbs against a throwaway sample workbook and returns what they printed | Non-concern: the guide's prose, the verb bodies | IO: () -> String
use fsa1_model::{Direction, FormulaOutcome};
use fsa1_verbs::{Refusal, ops, ops::Format, present};
use std::path::{Path, PathBuf};

/// Removes its directory on drop. Constructed BEFORE the first write, so a failure part-way
/// through still unwinds the partial tree rather than stranding it in `$TMPDIR`.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_sample() -> Result<Scratch, String> {
    let dir = std::env::temp_dir().join(format!("fsa1-guide.{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let scratch = Scratch(dir);
    for (rel, body) in fsa1_model::sample_workbook() {
        let path = scratch.0.join(&rel);
        let parent = path
            .parent()
            .ok_or_else(|| format!("{} has no parent", path.display()))?;
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        std::fs::write(&path, body).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(scratch)
}

/// `$ <cmd>` then verbatim what it printed, indented so the transcript reads as one block.
fn shot(cmd: &str, out: &str) -> String {
    let body: String = out.lines().map(|l| format!("  {l}\n")).collect::<String>();
    format!("  $ {cmd}\n{body}\n")
}

/// A refused verb is REPORTED, never skipped: a transcript missing a section would be
/// indistinguishable from one that was never attempted.
fn shot_or(cmd: &str, out: Result<String, Refusal>) -> String {
    match out {
        Ok(text) => shot(cmd, &text),
        Err(r) => shot(cmd, &format!("REFUSED: {}", r.message)),
    }
}

fn at(root: &Path, tail: &str) -> String {
    let r = root.display();
    if tail.is_empty() {
        r.to_string()
    } else {
        format!("{r}/{tail}")
    }
}

/// The five verbs below, RUN. Nothing here is transcribed, so nothing here can go stale; a verb
/// that refuses says so in place. An unwritable `$TMPDIR` yields one named line and no transcript.
pub fn transcript() -> String {
    let scratch = match write_sample() {
        Ok(s) => s,
        Err(why) => return format!("\nTRANSCRIPT unavailable -- cannot write the sample: {why}\n"),
    };
    let root = &scratch.0;
    let mut out =
        String::from("TRANSCRIPT -- every line below was printed by the verb named above it.\n\n");

    out.push_str(&shot(
        "fsa1-cli sample ./demo",
        "wrote the workbook this transcript runs against",
    ));
    out.push_str(&shot_or(
        "fsa1-cli tree ./demo",
        ops::tree(ops::TreeArgs {
            target: &at(root, ""),
            mode: None,
            full: false,
        })
        .map(|r| r.text),
    ));
    out.push_str(&shot_or(
        "fsa1-cli render ./demo/Orders",
        ops::render(ops::RenderArgs {
            target: &at(root, "Orders"),
            mode: None,
            format: Format::Ascii,
        })
        .map(|r| r.text),
    ));
    out.push_str(&shot_or(
        "fsa1-cli eval ./demo/Orders --formula '=SUM(D2:D4)'",
        ops::eval(ops::EvalArgs {
            target: &at(root, "Orders"),
            formula: "=SUM(D2:D4)",
        })
        .map(|o| {
            let (FormulaOutcome::Value(v) | FormulaOutcome::Error(v)) = o;
            v
        }),
    ));
    out.push_str(&shot_or(
        "fsa1-cli check ./demo",
        ops::check(ops::CheckArgs {
            target: &at(root, ""),
            format: None,
        })
        .map(|d| present::diagnostics_table(&d)),
    ));
    out.push_str(&shot_or(
        "fsa1-cli trace ./demo/Orders/D5",
        ops::trace(ops::TraceArgs {
            target: &at(root, "Orders/D5"),
            dir: Direction::Upstream,
            depth: None,
        })
        .map(|n| present::trace(&n)),
    ));
    out
}
