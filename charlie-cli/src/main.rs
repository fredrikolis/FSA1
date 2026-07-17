// Concern: charlie-cli — the THIN binary shell (`charlie-cli render` / `check` / `eval` / `sample` / `--guide`): parse argv, drive `charlie-model` (load a workbook, ask for a render grid, a lint report, or an ad-hoc `=formula`'s value; or WRITE the model's tutorial workbook to disk for `sample`), print the result to stdout — an ASCII table for render/check, a single scalar value for eval, the terse guide for `--guide`, a sample tree + next-steps for `sample` — and set the exit code an agent branches on (0 clean · 1 I/O failure · 2 bad args · 3 error-severity diagnostics · 4 target-dir conflict · 24 path not found); it holds NO spreadsheet logic — the demand-driven eval, value spelling, diagnostics, and the sample CONTENT all live in the model, the guide text in `guide`, and comfy-table drawing in `ascii` | Non-concern: WHAT a cell computes to or WHY a diagnostic fires (charlie-model owns the render model + lint + ad-hoc formula eval + sample content), the formula language (charlie-ast), and xlsx serde | IO: (argv, a workbook directory on disk) -> an ASCII table (render/check), a scalar value (eval), the guide text (`--guide`), or a freshly-written sample workbook tree (`sample`) on stdout/disk + an exit code; usage/errors to stderr
//! `charlie-cli` — render and lint a filesystem spreadsheet. The binary is a thin consumer of
//! `charlie-model`: it parses arguments, calls the model's `render`/`lint` surface, and lays the
//! returned plain-data grid into an ASCII table with `comfy-table` (see [`ascii`]). All spreadsheet
//! logic stays in the model (`repo-standards.md`: logic in the engine, CLI a thin shell).
//!
//! Stack-native entrypoint: `cargo run -p charlie-cli -- render <path>` (binary name `charlie-cli`).

mod ascii;
mod guide;

use std::path::Path;
use std::process::ExitCode;

use charlie_model::{FormulaOutcome, RenderMode, Workbook, parse_viewport, render};

use crate::ascii::{diagnostics_table, grid_table};

/// Exit codes, aligned with `cli-interface-standards.md`.
mod exit {
    /// Invalid CLI usage (unknown command/flag, missing/duplicate argument).
    pub const BAD_ARGS: u8 = 2;
    /// An error-severity diagnostic rejected the input (lint failed, or a workbook won't load).
    pub const VALIDATION: u8 = 3;
    /// A runtime-precondition refusal: the argv is valid, but the target state conflicts with the
    /// operation (e.g. `sample <dir>` where `<dir>` already exists and is non-empty — the never-clobber
    /// guarantee). Distinct from `BAD_ARGS` (the command is well-formed) per `cli-interface-standards.md`.
    pub const CONFLICT: u8 = 4;
    /// A path (workbook directory) does not exist.
    pub const NOT_FOUND: u8 = 24;
    /// An unexpected I/O failure.
    pub const IO: u8 = 1;
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(run(&args))
}

fn run(args: &[String]) -> u8 {
    // `--version`/`-V` overrides everything (cli-interface-standards.md).
    if args.iter().any(|a| a == "--version" || a == "-V") {
        print_version();
        return 0;
    }
    // `--guide` prints the terse on-disk-model tour (its single home is `guide::GUIDE`).
    if args.iter().any(|a| a == "--guide") {
        print!("{}", guide::GUIDE);
        return 0;
    }
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return 0;
    }
    match args[0].as_str() {
        "render" => cmd_render(&args[1..]),
        "check" => cmd_check(&args[1..]),
        "eval" => cmd_eval(&args[1..]),
        "sample" => cmd_sample(&args[1..]),
        other => {
            eprintln!("charlie-cli: unknown command {other:?}\n");
            print_help();
            exit::BAD_ARGS
        }
    }
}

/// `charlie-cli render <path> [--tab NAME] [--range A3:G8] [--values|--functions|--annotation]`.
fn cmd_render(rest: &[String]) -> u8 {
    let mut path: Option<String> = None;
    let mut tab: Option<String> = None;
    let mut range: Option<String> = None;
    let mut modes: Vec<RenderMode> = Vec::new();

    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = split_flag(arg);
        match flag {
            "--tab" => match take_value(inline, &mut it) {
                Some(v) => tab = Some(v),
                None => return bad_arg("--tab needs a tab name"),
            },
            "--range" => match take_value(inline, &mut it) {
                Some(v) => range = Some(v),
                None => return bad_arg("--range needs an A1 range like A3:G8"),
            },
            "--values" => modes.push(RenderMode::Values),
            "--functions" => modes.push(RenderMode::Functions),
            "--annotation" => modes.push(RenderMode::Annotation),
            f if f.starts_with('-') => return bad_arg(&format!("unknown flag {f:?}")),
            _ => {
                if path.replace(arg.clone()).is_some() {
                    return bad_arg("render takes exactly one <path>");
                }
            }
        }
    }
    if modes.len() > 1 {
        return bad_arg("choose at most one of --values / --functions / --annotation");
    }
    let mode = modes.first().copied().unwrap_or(RenderMode::Values);
    let Some(path) = path else {
        return bad_arg("render needs a <path> to a workbook directory");
    };

    let wb = match load(Path::new(&path)) {
        Ok(wb) => wb,
        Err(code) => return code,
    };
    if wb.sheet_names().is_empty() {
        eprintln!("charlie-cli: {path:?} has no tabs (a tab is a sub-folder of cell/range files)");
        return exit::VALIDATION;
    }

    // Pick the tab: an explicit --tab by name, else tab 0 (the first sheet).
    let sheet = match &tab {
        Some(name) => match wb.tab_index(name) {
            Some(i) => i,
            None => {
                eprintln!(
                    "charlie-cli: no tab named {name:?} in {path:?} (tabs: {:?})",
                    wb.sheet_names()
                );
                return exit::NOT_FOUND;
            }
        },
        None => 0,
    };

    // Pick the viewport: an explicit --range, else the tab's whole used region.
    let viewport = match &range {
        Some(r) => match parse_viewport(r) {
            Ok(rect) => rect,
            Err(msg) => return bad_arg(&msg),
        },
        None => match wb.used_region(sheet) {
            Some(rect) => rect,
            None => {
                eprintln!(
                    "charlie-cli: tab {:?} is empty (no cells to render)",
                    sheet_name(&wb, sheet)
                );
                return 0;
            }
        },
    };

    // Bound the viewport before rendering: `render` allocates a string per cell, so a
    // syntactically-valid but enormous `--range` (e.g. `A1:A4294967295`) would abort the process on
    // allocation. Refuse with a located diagnostic instead (fail-fast, never a crash).
    let cells = charlie_model::viewport_cell_count(viewport);
    if cells > charlie_model::MAX_VIEWPORT_CELLS {
        return bad_arg(&format!(
            "--range spans {cells} cells, over the render bound of {} -- narrow the range",
            charlie_model::MAX_VIEWPORT_CELLS
        ));
    }

    let grid = render(&wb, sheet, viewport, mode);
    println!("{}", grid_table(&grid));
    0
}

/// `charlie-cli check <path>` — lint the workbook and render the diagnostics as ASCII. Exits non-zero if
/// any error-severity diagnostic fires (a workbook that won't even load is itself the failure).
fn cmd_check(rest: &[String]) -> u8 {
    let mut path: Option<String> = None;
    for arg in rest {
        let (flag, _) = split_flag(arg);
        if flag.starts_with('-') {
            return bad_arg(&format!("unknown flag {flag:?}"));
        }
        if path.replace(arg.clone()).is_some() {
            return bad_arg("check takes exactly one <path>");
        }
    }
    let Some(path) = path else {
        return bad_arg("check needs a <path> to a workbook directory");
    };

    // Load-time refusals (overlap, literal dimension mismatch, bad filename) surface from the loader;
    // eval-time ones (cycle, formula dimension mismatch, unparseable body) come from `lint`.
    let diags = match Workbook::load_dir(Path::new(&path)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("charlie-cli: no such workbook directory {path:?}");
            return exit::NOT_FOUND;
        }
        Err(e) => {
            eprintln!("charlie-cli: cannot read {path:?}: {e}");
            return exit::IO;
        }
        Ok(Err(load_diags)) => load_diags,
        Ok(Ok(wb)) => wb.lint(),
    };

    println!("{}", diagnostics_table(&diags));
    let has_error = diags
        .iter()
        .any(|d| matches!(d.code.severity(), charlie_model::Severity::Error));
    if has_error { exit::VALIDATION } else { 0 }
}

/// `charlie-cli eval <path> [--tab <name>] '=<formula>'` — evaluate an ad-hoc formula against a loaded
/// workbook and print the resulting value with the same formatting `render` uses. Read-only: no file
/// writes, no mutation. Unqualified refs resolve against `--tab` (default: the first tab). A parse
/// error prints the located diagnostic; an error-valued result prints the error text; both exit
/// non-zero.
fn cmd_eval(rest: &[String]) -> u8 {
    let mut path: Option<String> = None;
    let mut tab: Option<String> = None;
    let mut formula: Option<String> = None;

    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = split_flag(arg);
        match flag {
            "--tab" => match take_value(inline, &mut it) {
                Some(v) => tab = Some(v),
                None => return bad_arg("--tab needs a tab name"),
            },
            // A formula begins with `=` (not `-`), so only a genuine `-`/`--` token is an unknown flag.
            f if f.starts_with('-') => return bad_arg(&format!("unknown flag {f:?}")),
            _ => {
                if path.is_none() {
                    path = Some(arg.clone());
                } else if formula.is_none() {
                    formula = Some(arg.clone());
                } else {
                    return bad_arg("eval takes exactly one <path> and one '=formula'");
                }
            }
        }
    }

    let Some(path) = path else {
        return bad_arg("eval needs a <path> to a workbook directory");
    };
    let Some(formula) = formula else {
        return bad_arg("eval needs a formula, e.g. charlie-cli eval ./budget '=SUM(A1:A5)'");
    };

    let wb = match load(Path::new(&path)) {
        Ok(wb) => wb,
        Err(code) => return code,
    };
    if wb.sheet_names().is_empty() {
        eprintln!("charlie-cli: {path:?} has no tabs (a tab is a sub-folder of cell/range files)");
        return exit::VALIDATION;
    }

    // Resolve the tab unqualified refs bind to: an explicit --tab by name, else tab 0 (the first).
    let sheet = match &tab {
        Some(name) => match wb.tab_index(name) {
            Some(i) => i,
            None => {
                eprintln!(
                    "charlie-cli: no tab named {name:?} in {path:?} (tabs: {:?})",
                    wb.sheet_names()
                );
                return exit::NOT_FOUND;
            }
        },
        None => 0,
    };

    match wb.eval_formula(sheet, &formula) {
        Ok(FormulaOutcome::Value(s)) => {
            println!("{s}");
            0
        }
        // An error-valued result (#DIV/0!, #REF!, …) prints its text and is a non-zero outcome.
        Ok(FormulaOutcome::Error(s)) => {
            println!("{s}");
            exit::VALIDATION
        }
        // A parse refusal is a located diagnostic on stderr; non-zero.
        Err(diag) => {
            eprintln!("charlie-cli: {diag}");
            exit::VALIDATION
        }
    }
}

/// `charlie-cli sample <dir>` — write the model's canonical tutorial workbook
/// (`charlie_model::sample_workbook`) into `<dir>`, creating a sub-folder per tab, then print a terse
/// next-steps hint. REFUSES (never clobbers) if `<dir>` already exists and is non-empty. This is the
/// one command that WRITES to disk; the sample CONTENT is the model's — the CLI only lays it onto the
/// filesystem (no spreadsheet logic here).
fn cmd_sample(rest: &[String]) -> u8 {
    let mut path: Option<String> = None;
    for arg in rest {
        let (flag, _) = split_flag(arg);
        if flag.starts_with('-') {
            return bad_arg(&format!("unknown flag {flag:?}"));
        }
        if path.replace(arg.clone()).is_some() {
            return bad_arg("sample takes exactly one <dir>");
        }
    }
    let Some(path) = path else {
        return bad_arg("sample needs a <dir> to write the tutorial workbook into");
    };
    let dir = Path::new(&path);

    // Never clobber: refuse a `<dir>` that already exists and is non-empty. An empty existing dir, or
    // a not-yet-created one, is fine (the writes create it).
    if dir.exists() {
        let non_empty = match std::fs::read_dir(dir) {
            Ok(mut entries) => entries.next().is_some(),
            Err(e) => {
                eprintln!("charlie-cli: cannot read {path:?}: {e}");
                return exit::IO;
            }
        };
        if non_empty {
            // A runtime-precondition refusal, NOT a usage error: the argv is well-formed, but the
            // target directory's state conflicts with the never-clobber guarantee. That is exit 4
            // (CONFLICT) per `cli-interface-standards.md`, not exit 2 (BAD_ARGS) — and it prints no
            // `--help` pointer, since the command was used correctly.
            eprintln!(
                "charlie-cli: {path:?} already exists and is not empty -- refusing to overwrite; pick an empty or new directory"
            );
            return exit::CONFLICT;
        }
    }

    for (rel, content) in charlie_model::sample_workbook() {
        let full = dir.join(&rel);
        if let Some(parent) = full.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            eprintln!("charlie-cli: cannot create {:?}: {e}", parent.display());
            return exit::IO;
        }
        if let Err(e) = std::fs::write(&full, content) {
            eprintln!("charlie-cli: cannot write {:?}: {e}", full.display());
            return exit::IO;
        }
    }

    // These next-steps strings mirror the model's FIXED sample content (tab names, the `Orders!D5`
    // cell and its `110` value, from `charlie_model::sample_workbook`); if that sample is ever
    // changed, update these hints in lockstep — nothing else pins them together.
    println!("wrote a sample workbook to {path} (tabs: Orders, Summary)");
    println!();
    println!("next:");
    println!("  charlie-cli render {path}               # draw the Orders tab");
    println!("  charlie-cli render {path} --functions   # show the formulas, not their values");
    println!("  charlie-cli check  {path}               # lint it (clean)");
    println!("  charlie-cli eval   {path} '=Orders!D5'  # evaluate a cell (110)");
    println!("  then edit a cell file and re-render");
    0
}

/// Load a workbook directory for `render`, mapping loader failures to exit codes and printing the
/// load-time refusal table (a workbook that won't load can't be rendered — run `check` for detail).
fn load(path: &Path) -> Result<Workbook, u8> {
    match Workbook::load_dir(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "charlie-cli: no such workbook directory {:?}",
                path.display()
            );
            Err(exit::NOT_FOUND)
        }
        Err(e) => {
            eprintln!("charlie-cli: cannot read {:?}: {e}", path.display());
            Err(exit::IO)
        }
        Ok(Err(diags)) => {
            eprintln!(
                "charlie-cli: {:?} has load errors -- run `charlie-cli check` for detail:",
                path.display()
            );
            eprintln!("{}", diagnostics_table(&diags));
            Err(exit::VALIDATION)
        }
        Ok(Ok(wb)) => Ok(wb),
    }
}

/// The tab name for a sheet index, for messages (falls back to the index if out of range).
fn sheet_name(wb: &Workbook, sheet: u32) -> String {
    wb.sheet_names()
        .get(sheet as usize)
        .map_or_else(|| sheet.to_string(), |s| (*s).to_string())
}

/// Split `--flag=value` into `("--flag", Some("value"))`; a plain `--flag` yields `(.., None)`.
fn split_flag(arg: &str) -> (&str, Option<&str>) {
    match arg.split_once('=') {
        Some((f, v)) if f.starts_with('-') => (f, Some(v)),
        _ => (arg, None),
    }
}

/// The value for a valued flag: the inline `=value`, else the next argument.
fn take_value(inline: Option<&str>, it: &mut std::slice::Iter<'_, String>) -> Option<String> {
    match inline {
        Some(v) => Some(v.to_string()),
        None => it.next().cloned(),
    }
}

/// Print a usage error to stderr and return the bad-args exit code.
fn bad_arg(msg: &str) -> u8 {
    eprintln!("charlie-cli: {msg}\n\nrun `charlie-cli --help` for usage");
    exit::BAD_ARGS
}

/// `--version` — a machine-parseable JSON envelope (cli-interface-standards.md).
fn print_version() {
    println!(
        "{{\"status\":\"success\",\"data\":{{\"name\":\"charlie-cli\",\"version\":\"{}\"}}}}",
        env!("CARGO_PKG_VERSION")
    );
}

fn print_help() {
    print!(
        r#"charlie-cli — render and lint a filesystem spreadsheet (tabs = folders, cells/ranges = files)

USAGE:
  charlie-cli render <path> [--tab <name>] [--range <A3:G8>] [--values|--functions|--annotation]
  charlie-cli check  <path>
  charlie-cli eval   <path> [--tab <name>] '=<formula>'
  charlie-cli sample <dir>
  charlie-cli --version | --help | --guide

COMMANDS:
  render   Draw a tab (or a sub-range) as an ASCII table with a column-letter header and a
           row-number gutter. Default mode is --values.
             --tab <name>     Which tab (sub-folder) to render. Default: the first tab.
             --range <A3:G8>  Only this rectangle (canonical A1). Default: the tab's used region.
             --values         Computed values (demand-driven — only the viewport's cone evaluates).
             --functions      Source text: a formula cell shows its =… text, a literal shows its value.
             --annotation     Each range's line-1 '# ' annotation.
  check    Lint the workbook — overlap, dimension-mismatch, and cycle diagnostics — as an ASCII
           table pointing at the offending file(s). Exits non-zero if any error-severity diagnostic.
  eval     Evaluate an ad-hoc =formula against the loaded workbook and print its value (same
           formatting as render). Read-only — no file writes, no mutation. The formula may reference
           cells, ranges, and other tabs; unqualified refs (A1, A1:A5) bind to --tab.
             --tab <name>     Which tab unqualified references resolve against. Default: the first tab.
           A parse error prints the located diagnostic; an error-valued result (#DIV/0!, …) prints
           the error text. Both exit non-zero.
  sample   Write a live tutorial workbook (two tabs, a header row, explicit per-row formulas, a SUM,
           and a cross-sheet reference) into <dir>, then print next steps. Render/check/eval it, edit
           a cell, re-render — it teaches the format by being a real workbook. Refuses to overwrite a
           non-empty directory.

EXAMPLES:
  charlie-cli sample ./demo && charlie-cli render ./demo
  charlie-cli render ./budget --tab Summary
  charlie-cli render ./budget --tab Sales --range A1:E14 --functions
  charlie-cli check  ./budget
  charlie-cli eval   ./budget --tab Orders '=SUM(C2:C11)'
  charlie-cli eval   ./budget --tab Orders '=SUMPRODUCT(--(C2:C11>5))'

EXIT CODES:
  0   Success (render drawn, or check found no error-severity diagnostics)
  1   I/O failure (could not read, create, or write a file or directory)
  2   Invalid arguments
  3   Validation error (check found error-severity diagnostics, or a workbook would not load)
  4   Conflict (sample refuses: <dir> already exists and is non-empty — never clobbers)
  24  Not found (no such workbook directory, or no such tab)

SEE ALSO:
  charlie-cli --guide      Terse guide to the on-disk model (structure, filenames, body grammar)
  charlie-cli --version    Show version as a JSON envelope
"#
    );
}
