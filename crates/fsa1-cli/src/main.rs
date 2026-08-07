// Concern: the CLI argv surface — every verb, its flags, and its help text | Non-concern: cell values, path resolution, drawing | IO: (argv) -> stdout + stderr + an exit code

mod guide;
mod output;

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fsa1_ingest::{Decomposition, UnpackCategory};
use fsa1_model::{Direction, FormulaOutcome, RenderMode, Workbook};

use crate::output::{
    ErrorCode, emit_error, emit_eval_error_value, emit_eval_value, emit_trace,
    emit_validation_diagnostics, emit_version,
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(run(&args))
}

fn run(args: &[String]) -> u8 {
    if args.iter().any(|a| a == "--version" || a == "-V") {
        emit_version();
        return 0;
    }
    if args.iter().any(|a| a == "--guide") {
        print!("{}", guide_text());
        return 0;
    }
    if args.is_empty() {
        print_help(None);
        return 0;
    }
    if args.iter().any(|a| a == "--help") {
        let cmd = args.iter().map(String::as_str).find(|a| {
            matches!(
                *a,
                "render"
                    | "check"
                    | "eval"
                    | "trace"
                    | "tree"
                    | "sample"
                    | "unpack"
                    | "pack"
                    | "convert"
            )
        });
        print_help(cmd);
        return 0;
    }

    match args[0].as_str() {
        "render" => cmd_view(&args[1..], Presenter::Table),
        "check" => cmd_check(&args[1..]),
        "eval" => cmd_eval(&args[1..]),
        "trace" => cmd_trace(&args[1..]),
        "tree" => cmd_view(&args[1..], Presenter::Tree),
        "sample" => cmd_sample(&args[1..]),
        "unpack" => cmd_unpack(&args[1..]),
        "pack" => cmd_pack(&args[1..]),
        "convert" => cmd_convert(&args[1..]),
        other => {
            let msg = format!("unknown command {other:?}");
            fail(ErrorCode::InvalidArguments, &msg)
        }
    }
}

pub(crate) const MODE_USAGE: &str = "--mode needs one of: combined, values, functions";

pub(crate) fn parse_mode(v: &str) -> Result<RenderMode, u8> {
    match v {
        "combined" => Ok(RenderMode::Combined),
        "values" => Ok(RenderMode::Values),
        "functions" => Ok(RenderMode::Functions),
        other => Err(bad_arg(&format!(
            "unknown --mode {other:?}; choose combined, values, or functions"
        ))),
    }
}

const FORMAT_USAGE: &str = "--format needs one of: ascii, html";

/// The carrier, orthogonal to `--mode`: the mode picks what a cell SAYS, the format what it is
/// carried in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Ascii,
    Html,
}

fn parse_format(v: &str) -> Result<OutputFormat, u8> {
    match v {
        "ascii" => Ok(OutputFormat::Ascii),
        "html" => Ok(OutputFormat::Html),
        other => Err(bad_arg(&format!(
            "unknown --format {other:?}; choose ascii or html"
        ))),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Presenter {
    Table,
    Tree,
}

/// One plan+evaluate pass feeds both presenters, so `render` and `tree` cannot disagree about a cell.
fn cmd_view(rest: &[String], presenter: Presenter) -> u8 {
    let verb = match presenter {
        Presenter::Table => "render",
        Presenter::Tree => "tree",
    };
    let mut path: Option<String> = None;
    let mut mode = RenderMode::Combined;
    let mut format = OutputFormat::Ascii;
    let mut full = false;

    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = split_flag(arg);
        match flag {
            "--mode" => match take_value(inline, &mut it) {
                Some(v) => match parse_mode(&v) {
                    Ok(m) => mode = m,
                    Err(code) => return code,
                },
                None => return bad_arg(MODE_USAGE),
            },
            "--format" if presenter == Presenter::Table => match take_value(inline, &mut it) {
                Some(v) => match parse_format(&v) {
                    Ok(f) => format = f,
                    Err(code) => return code,
                },
                None => return bad_arg(FORMAT_USAGE),
            },
            "--full" if presenter == Presenter::Tree => full = true,
            f if f.starts_with('-') => return bad_arg(&format!("unknown flag {f:?}")),
            _ => {
                if path.replace(arg.clone()).is_some() {
                    return bad_arg(&format!("{verb} takes exactly one <path>"));
                }
            }
        }
    }
    let Some(path) = path else {
        return bad_arg(&format!(
            "{verb} needs a <path> like ./budget or ./budget/Summary/A1:D9"
        ));
    };

    let r = match fsa1_verbs::ops::view_at(
        &path,
        mode,
        match presenter {
            Presenter::Table => fsa1_verbs::ops::Presenter::Table,
            Presenter::Tree => fsa1_verbs::ops::Presenter::Tree,
        },
        match format {
            OutputFormat::Ascii => fsa1_verbs::ops::Format::Ascii,
            OutputFormat::Html => fsa1_verbs::ops::Format::Html,
        },
        full,
    ) {
        Ok(r) => r,
        Err(e) => return refused(e),
    };
    for note in &r.notes {
        eprintln!("fsa1-cli: {note}");
    }
    // An HTML carrier still emits its document: a caller redirecting stdout gets a file either way.
    if r.empty && presenter == Presenter::Table && format == OutputFormat::Ascii {
        return 0;
    }
    println!("{}", r.text);
    0
}

/// A workbook that will not load is itself the failure, so this consumes a `Decomposed` rather than a
/// `Resolved`: it must still scope and report against a root that never loaded.
fn cmd_check(rest: &[String]) -> u8 {
    let mut path: Option<String> = None;

    for arg in rest {
        let (flag, _inline) = split_flag(arg);
        match flag {
            f if f.starts_with('-') => return bad_arg(&format!("unknown flag {f:?}")),
            _ => {
                if path.replace(arg.clone()).is_some() {
                    return bad_arg("check takes exactly one <path>");
                }
            }
        }
    }
    let Some(path) = path else {
        return bad_arg("check needs a <path> like ./budget or ./budget/Sheet1/H3");
    };

    match fsa1_verbs::ops::check(&path) {
        Ok(diags) => output::emit_diagnostics(&diags),
        Err(e) => refused(e),
    }
}

fn cmd_eval(rest: &[String]) -> u8 {
    let mut path: Option<String> = None;
    let mut formula: Option<String> = None;

    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = split_flag(arg);
        match flag {
            "--formula" => match take_value(inline, &mut it) {
                Some(v) => formula = Some(v),
                None => {
                    return bad_arg("--formula needs a formula, e.g. --formula '=SUM(A1:A5)'");
                }
            },
            f if f.starts_with('-') => return bad_arg(&format!("unknown flag {f:?}")),
            _ => {
                if path.replace(arg.clone()).is_some() {
                    return bad_arg("eval takes exactly one <path> (the formula is --formula)");
                }
            }
        }
    }

    let Some(path) = path else {
        return bad_arg("eval needs a <path> like ./budget or ./budget/Orders");
    };
    let Some(formula) = formula else {
        return bad_arg(
            "eval needs --formula, e.g. fsa1-cli eval ./budget --formula '=SUM(A1:A5)'",
        );
    };

    match fsa1_verbs::ops::eval(&path, &formula) {
        Ok(FormulaOutcome::Value(v)) => {
            emit_eval_value(&v);
            0
        }
        Ok(FormulaOutcome::Error(v)) => emit_eval_error_value(&v),
        Err(e) => refused(e),
    }
}

fn cmd_trace(rest: &[String]) -> u8 {
    let mut path: Option<String> = None;
    let mut dependents = false;
    let mut depth: Option<u32> = None;

    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = split_flag(arg);
        match flag {
            "--depth" => match take_value(inline, &mut it) {
                Some(v) => match v.parse::<u32>() {
                    Ok(n) => depth = Some(n),
                    Err(_) => return bad_arg(&format!("--depth needs a number, not {v:?}")),
                },
                None => return bad_arg("--depth needs a number, e.g. --depth 3"),
            },
            "--dependents" => dependents = true,
            f if f.starts_with('-') => return bad_arg(&format!("unknown flag {f:?}")),
            _ => {
                if path.replace(arg.clone()).is_some() {
                    return bad_arg("trace takes exactly one <path>");
                }
            }
        }
    }

    let Some(path) = path else {
        return bad_arg("trace needs a <path> like ./budget/Sheet1/C3");
    };

    let dir = if dependents {
        Direction::Downstream
    } else {
        Direction::Upstream
    };

    match fsa1_verbs::ops::trace(&path, dir, depth) {
        Ok(node) => {
            emit_trace(&node);
            0
        }
        Err(e) => refused(e),
    }
}

/// The one command that writes to disk. The sample content is the model's; this only lays it out,
/// each range file taking the name `range_file_path` says this host can carry.
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

    if dir.exists() {
        let non_empty = match std::fs::read_dir(dir) {
            Ok(mut entries) => entries.next().is_some(),
            Err(e) => {
                let msg = format!("cannot read {path:?}: {e}");
                return fail(ErrorCode::Io, &msg);
            }
        };
        if non_empty {
            let msg = format!(
                "{path:?} already exists and is not empty -- refusing to overwrite; pick an empty or new directory"
            );
            return fail(ErrorCode::Conflict, &msg);
        }
    }

    let content = fsa1_model::sample_workbook();
    for (rel, body) in &content {
        let full = dir.join(fsa1_model::range_file_path(rel));
        if let Some(parent) = full.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            let msg = format!("cannot create {:?}: {e}", parent.display());
            return fail(ErrorCode::Io, &msg);
        }
        if let Err(e) = std::fs::write(&full, body) {
            let msg = format!("cannot write {:?}: {e}", full.display());
            return fail(ErrorCode::Io, &msg);
        }
    }
    // A workbook is meant to live in git; pin it to LF so a Windows checkout cannot CRLF-mangle grids.
    if let Err(e) = fsa1_model::write_workbook_gitattributes(dir) {
        let msg = format!(
            "cannot write {:?}: {e}",
            dir.join(".gitattributes").display()
        );
        return fail(ErrorCode::Io, &msg);
    }

    print!(
        "wrote a sample workbook to {path} (tabs: Orders, Summary)\n\
         \n\
         next:\n  \
         fsa1-cli tree   {path}                    # see the whole workbook (both tabs, every cell)\n  \
         fsa1-cli render {path}                    # draw the Orders tab as a grid\n  \
         fsa1-cli render {path} --mode functions   # show the formulas, not their values\n  \
         fsa1-cli check  {path}                    # lint it (clean)\n  \
         fsa1-cli eval   {path} --formula '=Orders!D5'  # evaluate a cell (110)\n  \
         then edit a cell file and re-render\n"
    );
    0
}

/// Re-spell a workbook's range file NAMES between `A1:C1` (POSIX) and `A1-C1` (portable/Windows-safe)
/// so a tree authored on one OS loads on the other. Only range file names change; cell contents,
/// single cells, and defined names are untouched, and the reader accepts both spellings everywhere.
fn cmd_convert(rest: &[String]) -> u8 {
    let mut path: Option<String> = None;
    let mut to: Option<char> = None;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = split_flag(arg);
        match flag {
            "--to" => {
                let Some(v) = take_value(inline, &mut it) else {
                    return bad_arg("--to needs one of: posix, windows, auto");
                };
                to = Some(match v.as_str() {
                    "posix" | "unix" => fsa1_model::RANGE_SEP_POSIX,
                    "windows" | "portable" => fsa1_model::RANGE_SEP_WINDOWS,
                    "auto" => fsa1_model::RANGE_SEP,
                    other => {
                        return bad_arg(&format!(
                            "unknown --to {other:?}; choose posix, windows, or auto"
                        ));
                    }
                });
            }
            f if f.starts_with('-') => return bad_arg(&format!("unknown flag {f:?}")),
            _ => {
                if path.replace(arg.clone()).is_some() {
                    return bad_arg("convert takes exactly one <workbook-dir>");
                }
            }
        }
    }
    let Some(path) = path else {
        return bad_arg("convert needs a <workbook-dir>");
    };
    let target = to.unwrap_or(fsa1_model::RANGE_SEP);
    let root = Path::new(&path);

    // A ':' name cannot be created on Windows — NTFS reads it as an alternate-data-stream separator.
    if target == fsa1_model::RANGE_SEP_POSIX && cfg!(windows) {
        return bad_arg(
            "cannot convert to the posix ':' spelling on Windows: ':' is not a legal filename \
             character here. Run the conversion on a POSIX host, or use --to windows.",
        );
    }
    if !root.is_dir() {
        return fail(
            ErrorCode::NotFound,
            &format!("no such workbook directory {path:?}"),
        );
    }

    let read_err = |p: &Path, e: std::io::Error| format!("cannot read {:?}: {e}", p.display());
    let tabs = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(e) => return fail(ErrorCode::Io, &read_err(root, e)),
    };
    let mut renamed = 0usize;
    for tab in tabs {
        let tab = match tab {
            Ok(t) => t,
            Err(e) => return fail(ErrorCode::Io, &read_err(root, e)),
        };
        let tab_name = tab.file_name().to_string_lossy().into_owned();
        // Range files live inside tab folders; skip git/.cache and workbook-scoped name files.
        if Workbook::is_reserved_entry(&tab_name) {
            continue;
        }
        let tab_path = tab.path();
        if !tab_path.is_dir() {
            continue;
        }
        let entries = match std::fs::read_dir(&tab_path) {
            Ok(e) => e,
            Err(e) => return fail(ErrorCode::Io, &read_err(&tab_path, e)),
        };
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => return fail(ErrorCode::Io, &read_err(&tab_path, e)),
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(new_name) = fsa1_model::reseparate_entry_name(&name, target) else {
                continue;
            };
            let to_path = tab_path.join(&new_name);
            if to_path.exists() {
                return fail(
                    ErrorCode::Conflict,
                    &format!(
                        "cannot rename {name:?} -> {new_name:?} in {tab_name:?}: target already exists"
                    ),
                );
            }
            if let Err(e) = std::fs::rename(entry.path(), &to_path) {
                return fail(
                    ErrorCode::Io,
                    &format!("cannot rename {name:?} -> {new_name:?} in {tab_name:?}: {e}"),
                );
            }
            renamed += 1;
        }
    }

    let spelling = if target == fsa1_model::RANGE_SEP_POSIX {
        "posix (A1:C1)"
    } else {
        "portable (A1-C1)"
    };
    if renamed == 0 {
        println!("convert: {path} already uses the {spelling} spelling (no range files renamed)");
    } else {
        println!(
            "convert: rewrote {renamed} range file name(s) to the {spelling} spelling in {path}"
        );
    }
    0
}

/// Every spelling of the choice, read out of the one array that has them: the refusal and `--help`
/// cannot name a different set than `--decompose` accepts.
fn decomposition_choices() -> String {
    Decomposition::ALL.map(Decomposition::name).join(", ")
}

fn parse_decomposition(v: &str) -> Result<Decomposition, u8> {
    v.parse().map_err(|()| {
        bad_arg(&format!(
            "unknown --decompose {v:?}; choose {}",
            decomposition_choices()
        ))
    })
}

/// The whole conversion is `fsa1-ingest`'s, behind the format firewall — including which policy an
/// unflagged run resolves to, which the source's channels decide and this shell never guesses.
fn cmd_unpack(rest: &[String]) -> u8 {
    let mut positionals: Vec<String> = Vec::new();
    let mut strict = false;
    let mut decompose: Option<Decomposition> = None;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = split_flag(arg);
        match flag {
            "--strict" => strict = true,
            "--decompose" => match take_value(inline, &mut it) {
                Some(v) => match parse_decomposition(&v) {
                    Ok(d) => decompose = Some(d),
                    Err(code) => return code,
                },
                None => {
                    return bad_arg(&format!(
                        "--decompose needs one of: {}",
                        decomposition_choices()
                    ));
                }
            },
            f if f.starts_with('-') => return bad_arg(&format!("unknown flag {f:?}")),
            _ => positionals.push(arg.clone()),
        }
    }
    let (src, dest): (String, Option<PathBuf>) = match positionals.as_slice() {
        [src, dst] => (src.clone(), Some(PathBuf::from(dst))),
        [src] => (src.clone(), None),
        [] => {
            return bad_arg("unpack needs a <src> (.ods or .xlsx), e.g. fsa1-cli unpack book.xlsx");
        }
        _ => return bad_arg("unpack takes <src> [<dest-workbook-dir>]"),
    };

    let src_path = Path::new(&src);
    match fsa1_verbs::ops::unpack(src_path, dest.as_deref(), decompose, strict) {
        Ok(u) => {
            let report = u.report;
            let dest = u.dest.display();
            print!(
                "unpacked {src} -> {dest} ({} tab(s), {} range file(s) written, decomposed by {})\n\
                 \n\
                 next:\n  \
                 fsa1-cli tree   {dest}          # see the whole workbook — every tab, cell, name\n  \
                 fsa1-cli render {dest}          # draw one tab as a grid\n  \
                 fsa1-cli check  {dest}          # lint it\n",
                report.tabs.len(),
                report.files,
                report.decomposition.name(),
            );
            eprint!(
                "{}",
                render_unpack_report(&report.warnings, &report.inspected)
            );
            0
        }
        Err(e) => refused(e),
    }
}

/// Every account of what the report covers reads this one table: the section a category prints under,
/// and the noun the clean-run line credits it with. Prose kept by hand beside it drifts, and a
/// category the report emits but no account names is one an agent must go and look for itself.
const SECTIONS: [(UnpackCategory, &str, &str); 7] = [
    (
        UnpackCategory::NumberFormat,
        "number formats coerced to plain",
        "number formats",
    ),
    (UnpackCategory::Table, "tables dropped", "tables"),
    (
        UnpackCategory::Name,
        "defined names skipped",
        "defined names",
    ),
    (
        UnpackCategory::Formula,
        "formulas kept verbatim",
        "formulas",
    ),
    (
        UnpackCategory::Styling,
        "appearance narrowed or dropped",
        "appearance",
    ),
    (
        UnpackCategory::Geometry,
        "column widths and row heights dropped",
        "column widths, row heights",
    ),
    (
        UnpackCategory::WorkbookPart,
        "workbook parts not carried",
        "workbook parts",
    ),
];

/// Uncapped: the consumer is an agent, and under-reporting a lossy conversion is the failure mode.
/// `inspected` is what the run EXAMINED, and it bounds what either rendering may say: a category no
/// reader on this source's path opened is neither reported nor vouched for, it is declared unlooked-at.
fn render_unpack_report(
    warnings: &[fsa1_ingest::UnpackWarning],
    inspected: &[UnpackCategory],
) -> String {
    let (mut vouched, mut blind): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
    for (category, _, noun) in SECTIONS {
        if inspected.contains(&category) {
            vouched.push(noun);
        } else {
            blind.push(noun);
        }
    }

    if warnings.is_empty() && blind.is_empty() {
        return format!(
            "unpack fidelity: nothing lost -- every cell value crossed, and no loss in any category \
             the report tracks: {}.\n",
            vouched.join(", ")
        );
    }

    let mut out = if warnings.is_empty() {
        format!(
            "unpack fidelity: no loss in any category this source was inspected for -- every cell \
             value crossed, and no loss in: {}.\n",
            vouched.join(", ")
        )
    } else {
        format!(
            "unpack fidelity report ({} item(s)) -- this conversion changed the following; each is located \
             and visible in `fsa1-cli check`, none is silently wrong:\n",
            warnings.len()
        )
    };
    for (cat, label, _) in SECTIONS {
        let items: Vec<&fsa1_ingest::UnpackWarning> =
            warnings.iter().filter(|w| w.category() == cat).collect();
        if items.is_empty() {
            continue;
        }
        out.push('\n');
        out.push_str(&format!("{label} ({} item(s)):\n", items.len()));
        for w in items {
            out.push_str(&format!("  {w}\n"));
        }
    }
    if !blind.is_empty() {
        out.push_str(&format!(
            "not inspected on this source, so not vouched for: {} -- a loss in one of those would \
             not appear in this report.\n",
            blind.join(", ")
        ));
    }
    out
}

/// Only `.`, `..` and `/` have no stem. A dot-prefixed name (`.xlsx`) has one, so it derives a
/// directory here and is refused downstream by `import_file`.
/// `--target` accepts only the one format fsa1-xlsx writes; it exists as the seam for a future one.
fn cmd_pack(rest: &[String]) -> u8 {
    let mut positionals: Vec<String> = Vec::new();
    let mut target = "xlsx".to_string();
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = split_flag(arg);
        match flag {
            "--target" => match take_value(inline, &mut it) {
                Some(v) if v == "xlsx" => target = v,
                Some(v) => {
                    return bad_arg(&format!(
                        "only --target xlsx is supported (got {v:?}); ods export is not yet available"
                    ));
                }
                None => return bad_arg("--target needs a format, e.g. --target xlsx"),
            },
            f if f.starts_with('-') => return bad_arg(&format!("unknown flag {f:?}")),
            _ => positionals.push(arg.clone()),
        }
    }
    let folder = match positionals.as_slice() {
        [folder] => folder.clone(),
        [] => return bad_arg("pack needs a <workbook-dir>, e.g. fsa1-cli pack ./book"),
        _ => return bad_arg("pack takes exactly one <workbook-dir> (the output name is derived)"),
    };
    match fsa1_verbs::ops::pack(Path::new(&folder), None, &target) {
        Ok(p) => {
            let dest = p.dest.display();
            print!(
                "packed {folder} -> {dest} ({} sheet(s) written)\n\
                 \n\
                 next:\n  \
                 open {dest} in a spreadsheet app, or re-unpack it:\n  \
                 fsa1-cli unpack {dest}   # read the packed .xlsx back into a workbook\n",
                p.sheets,
            );
            0
        }
        Err(e) => refused(e),
    }
}

pub(crate) fn split_flag(arg: &str) -> (&str, Option<&str>) {
    match arg.split_once('=') {
        Some((f, v)) if f.starts_with('-') => (f, Some(v)),
        _ => (arg, None),
    }
}

pub(crate) fn take_value(
    inline: Option<&str>,
    it: &mut std::slice::Iter<'_, String>,
) -> Option<String> {
    match inline {
        Some(v) => Some(v.to_string()),
        None => it.next().cloned(),
    }
}

/// The one place a Refusal becomes an exit code: its kind picks the code, its diagnostics print the
/// way a load's always have, and its message goes out under the program name.
pub(crate) fn refused(r: fsa1_verbs::Refusal) -> u8 {
    let code = match r.kind {
        fsa1_verbs::Kind::InvalidArguments => ErrorCode::InvalidArguments,
        fsa1_verbs::Kind::Validation => ErrorCode::Validation,
        fsa1_verbs::Kind::Conflict => ErrorCode::Conflict,
        fsa1_verbs::Kind::NotFound => ErrorCode::NotFound,
        fsa1_verbs::Kind::Io => ErrorCode::Io,
    };
    if !r.diagnostics.is_empty() {
        return emit_validation_diagnostics(&r.diagnostics);
    }
    fail(code, &r.message)
}

pub(crate) fn fail(code: ErrorCode, message: &str) -> u8 {
    emit_error(code, message);
    code.exit()
}

pub(crate) fn bad_arg(msg: &str) -> u8 {
    fail(ErrorCode::InvalidArguments, msg)
}

fn print_help(cmd: Option<&str>) {
    let text: Cow<'_, str> = match cmd {
        Some("render") => RENDER_HELP.into(),
        Some("check") => CHECK_HELP.into(),
        Some("eval") => EVAL_HELP.into(),
        Some("trace") => TRACE_HELP.into(),
        Some("tree") => TREE_HELP.into(),
        Some("sample") => SAMPLE_HELP.into(),
        Some("unpack") => unpack_help().into(),
        Some("pack") => PACK_HELP.into(),
        Some("convert") => CONVERT_HELP.into(),
        _ => GLOBAL_HELP.into(),
    };
    print!("{text}");
}

/// The terse index's one hole, filled exactly as [`unpack_help`] fills `--help`'s, so a new policy
/// cannot reach the surface while EITHER text still names the old set.
fn guide_text() -> String {
    guide::GUIDE.replace("{DECOMPOSITIONS}", &decomposition_choices())
}

/// The one help text with holes in it: its account of the fidelity report is [`SECTIONS`] rendered
/// and its choice list is [`Decomposition::ALL`], so neither a new category nor a new policy can reach
/// the surface while `--help` still names the old set.
fn unpack_help() -> String {
    let listed: Vec<String> = SECTIONS
        .iter()
        .map(|(_, label, _)| format!("    {label}"))
        .collect();
    UNPACK_HELP
        .replace("{SECTIONS}", &listed.join("\n"))
        .replace("{DECOMPOSITIONS}", &decomposition_choices())
}

const GLOBAL_HELP: &str = r#"fsa1-cli — render, lint, and evaluate a spreadsheet stored as a filesystem (tabs = folders, cells/ranges = files)

USAGE:
  fsa1-cli render <path> [--mode <combined|values|functions>] [--format <ascii|html>]   # <path>: <wb>[/<tab>[/<A1>]]
  fsa1-cli check  <path>                                         # <path>: <wb>[/<tab>[/<A1>]]
  fsa1-cli eval   <path> --formula '=<formula>'                  # <path>: <wb>[/<tab>]
  fsa1-cli trace  <path> [--dependents] [--depth <N>]           # <path>: <wb>/<tab>/<A1> (one cell)
  fsa1-cli tree   <path> [--mode <combined|values|functions>]    # <path>: <wb>[/<tab>[/<A1>]]
  fsa1-cli sample <dir>
  fsa1-cli unpack [--strict] [--decompose <policy>] <src> [<dst>]   # <src> is .ods/.xlsx; <dst> derives to ./<src-stem>/
  fsa1-cli pack   <workbook-dir> [--target xlsx]  # serialize a workbook to a fresh ./<basename>.xlsx
  fsa1-cli convert <workbook-dir> [--to posix|windows|auto]  # re-spell range file names for another OS
  fsa1-cli --version | --help | --guide

  The tab and A1 cell/range are PART OF THE PATH (tabs = folders, cells/ranges = A1 selectors):
  `render wb/Summary` draws the Summary tab; `render wb/Summary/A1:D9` draws that region.

  Per-command help (its own args and exit codes): fsa1-cli <command> --help

COMMANDS:
  render   Draw the path's scope as ASCII tables with a column-letter header and a row-number gutter:
           a bare <wb> path draws EVERY tab (each headed by its name), a <wb>/<tab> path one tab, a
           <wb>/<tab>/<A1:B9> path one region. `tree` shows the SAME scopes as a nested view — the two
           differ only in the output form. Default mode is COMBINED: a literal shows its value; a
           formula shows `<value> ← =<formula>` (value AND source in one glance). Narrow with --mode
           values (computed only) or --mode functions (authored source only). --format html carries the
           SAME cells as one standalone HTML document on stdout, styled by the tab's sidecars.
  check    Lint the workbook — overlap, dimension-mismatch, and cycle diagnostics — as an ASCII table
           pointing at the offending file(s). Exits non-zero if any error-severity diagnostic. Name a
           tab/region IN THE PATH (wb/Tab or wb/Tab/A1:B2) to report ONLY the diagnostics inside that
           tab/range (exits 0 if that scope is clean) — validate just the cells you authored on an
           import that carries unrelated pre-existing error cells.
  eval     Evaluate an ad-hoc --formula against the loaded workbook and emit its value. Unqualified refs
           bind to the path tab (wb/Tab), else the first tab (wb). Read-only.
  trace    Report a cell's upstream dependencies (or downstream consumers with --dependents) as a
           tree; each node carries its value and computation hash. The cell is named in the path
           (wb/Tab/A1). Read-only.
  tree     Present the workbook's COMPLETE structure — every tab, every cell, every name — as one
           read-only nested view; never a reserved entry like .cache/. Content mode mirrors render:
           COMBINED is the default (a formula name/cell shows `<value> ← =<formula>`), narrow with
           --mode values|functions. Scope it to a tab with a wb/Tab path; a wb/Tab/A1:B9 path shows
           exactly that viewport's cells, ALL of them, uncapped. Read-only.
  sample   Write a live tutorial workbook into <dir>, then report. Refuses to overwrite a non-empty
           directory.
  unpack   Convert a real spreadsheet file (.ods or .xlsx) into an FSA1 workbook the engine reads.
           Each rectangular block of a sheet's content becomes ONE grid file named by the A1 range it
           fills. <dst> is optional — omitted, the workbook is written to ./<src-stem>/ in the CWD.
           Refuses a non-empty destination.
  pack     Serialize an FSA1 workbook back into a single .xlsx file (the inverse of unpack): cell
           values, formulas, and multiple sheets, in default (General) format. The output name is
           DERIVED — ./<workbook-basename>.xlsx in the CWD; --target defaults to xlsx (the only format).
           Derived wholly from the workbook (no cell content on argv). Refuses an already-occupied
           output (never clobbers) and leaves the source workbook byte-identical.
  convert  Re-spell a workbook's range file names between `A1:C1` (POSIX) and `A1-C1` (portable /
           Windows-safe, since `:` is illegal in a Windows filename) so a raw tree authored on one OS
           checks out and loads on the other. Only range file NAMES change; contents, single cells,
           and defined names are untouched, and the reader accepts both spellings on every platform.

AUTHORING (there is NO write command by design — the filesystem IS the write surface):
  You author and edit a workbook by writing the A1-named cell files DIRECTLY on disk with ordinary
  file tools; a cell is its own file (its name is its A1 range, its content is its grid). fsa1-cli
  only reads: render/check/eval/trace verify what you wrote. To add =SUM(A1:A2) at H3 on Sheet1:
    mkdir -p ./budget/Sheet1
    printf '=SUM(A1:A2)' > ./budget/Sheet1/H3      # the file name IS the cell address
    fsa1-cli check ./budget/Sheet1/H3           # scoped validation of just that cell
  Then `render`/`check` to verify. See `fsa1-cli --guide` for the filename + body grammar.

NAMES (a named cell/range/formula, referenced in a formula by its identifier):
  A name lives in its SCOPE folder — a tab folder is sheet-scoped, the workbook root is workbook-scoped
  (a sheet-scoped name shadows a workbook one). A single cell / range is a SYMLINK to the cell file(s)
  (`.begin`/`.end` name a range's two corners); a computed name is a regular file holding `=ref/expr`:
    ln -s B5 ./budget/Sheet1/total                 # =total resolves to Sheet1!B5 (write-through)
    ln -s A2 ./budget/Sheet1/Days.begin; ln -s A366 ./budget/Sheet1/Days.end   # =SUM(Days)
    printf '=Base*1.05' > ./budget/Sheet1/Rate     # a named formula (a cell not at a coordinate)
  A name's identifier must NOT parse as an A1 address; distribute the tree as a tar (it keeps symlinks).

EXAMPLES:
  fsa1-cli sample ./demo && fsa1-cli render ./demo
  fsa1-cli unpack book.xlsx && fsa1-cli render ./book
  fsa1-cli render ./budget/Summary
  fsa1-cli check  ./budget
  fsa1-cli eval   ./budget/Orders --formula '=SUMPRODUCT(--(C2:C11>5))'

EXIT CODES:
  0   Success (render drawn, or check found no error-severity diagnostics)
  1   I/O failure (could not read, create, or write a file or directory)
  2   Invalid arguments
  3   Validation error (check found error-severity diagnostics, or a workbook would not load)
  4   Conflict (a never-clobber refusal: sample/unpack <dir> exists and is non-empty, or pack's
      derived ./<basename>.xlsx already exists)
  24  Not found (no such workbook directory, or no such tab)

SEE ALSO:
  fsa1-cli --guide      Terse guide to the on-disk model (structure, filenames, body grammar)
  fsa1-cli --version    Show version information
"#;

const RENDER_HELP: &str = r#"fsa1-cli render — draw a tab (or a sub-range) of a filesystem spreadsheet

USAGE:
  fsa1-cli render <path> [--mode <combined|values|functions>] [--format <ascii|html>]

  <path> is <wb>[/<tab>[/<A1>|<Name>]] — the tab and A1 cell/range or defined NAME are PART OF THE PATH:
    render ./budget                draw EVERY tab, each at its used region
    render ./budget/Summary        draw the Summary tab
    render ./budget/Summary/A1:E14 draw exactly that A1 rectangle on Summary
    render ./budget/Summary/total  draw the region an FS4 defined name resolves to

DESCRIPTION:
  Render the path's scope to grids. The whole workbook loads (so cross-tab refs resolve); the path
  selects the scope — every tab, one tab, or one region — and `render` and `tree` are the same code over
  it, differing only in whether it is drawn as a table or as nested nodes. Values are demand-driven and
  computed in ONE pass over the whole scope, so no two cells of a view can disagree. Default mode is
  COMBINED (a literal shows its value; a formula shows `<value> ← =<formula>`, reusing the same value
  and source spellings --mode values|functions produce). A multi-tab view heads each grid with its tab
  name and separates them by a blank line; a single tab is the bare grid. Default viewport is the tab's
  used region. A region is the LITERAL rectangle — cells outside the used region pad blank (no
  clipping); a region wholly outside the used region prints a stderr note but still draws the padded
  grid and exits 0.

ARGUMENTS:
  <path>            (required) <wb>[/<tab>[/<A1>]] (tabs = sub-folders; the A1 selector is logical).
  --mode <m>        (optional) One of: combined (default), values (computed only), functions (authored
                    source only: a formula shows its =… text, a literal shows its value).
  --format <f>      (optional) The CARRIER, orthogonal to --mode: ascii (default) or html. --mode picks
                    what a cell says; --format picks what it is carried in.

EXAMPLES:
  fsa1-cli render ./budget
  fsa1-cli render ./budget/Summary/A1:E14 --mode functions
  fsa1-cli render ./budget --format html > budget.html

OUTPUT:
  An ASCII table per in-scope tab on stdout: a column-letter header row, a row-number gutter, and one
  cell per coordinate (the computed value, the formula source, or `<value> ← =<formula>` per the chosen
  mode). A multi-tab view names each tab above its grid.
  --format html emits ONE standalone, JavaScript-free HTML document on stdout instead (redirect it to a
  file): a <table> per in-scope tab with the same header row and gutter as <th>, carrying each cell's
  resolved presentation as a class — one CSS rule per distinct style. Nothing else is written to stdout.

EXIT CODES:
  0   Success (grid drawn, incl. a region outside the used region — with a stderr note)
  1   I/O failure
  2   Invalid arguments (unknown flag/mode/format, an oversized region, a trailing segment that is
      neither canonical A1 nor a known defined name, or a name that resolves to a formula/constant)
  3   Validation error (the workbook would not load, or the path has no tabs)
  24  Not found (no such workbook directory, or no such tab)

SEE ALSO:
  fsa1-cli tree       The same scopes drawn as a nested view instead of tables
  fsa1-cli check      Lint a workbook
  fsa1-cli --guide    Terse guide to the on-disk model
"#;

const CHECK_HELP: &str = r#"fsa1-cli check — lint a filesystem spreadsheet

USAGE:
  fsa1-cli check <path>

  <path> is <wb>[/<tab>[/<A1>|<Name>]] — a tab/region or defined NAME in the path scopes the report:
    check ./budget                 lint the whole workbook
    check ./budget/Sheet1          lint only the Sheet1 tab
    check ./budget/Sheet1/H3       lint only cell H3
    check ./budget/Sheet1/Days     lint only the region a defined name resolves to

DESCRIPTION:
  Lint the workbook: overlap, dimension-mismatch, cycle, and the load-time filename refusals.
  Exits non-zero if any error-severity diagnostic fires. READ-ONLY — check writes nothing under the
  workbook at all, .cache/ included.

  SCOPE (a tab/region named IN THE PATH): report ONLY the diagnostics whose location falls within that
  tab/range, and exit 0 iff nothing in scope is faulty — even when the wider workbook has errors. This
  lets an agent validate just the cells IT authored on an import that carries pre-existing (GRID6) error
  cells elsewhere. A file-level diagnostic (no single cell, e.g. a whole-tab overlap) is reported
  whenever its tab is in scope. An unscoped check (a bare <wb> path) is the whole workbook — and a
  workbook that will not load is itself the failure (exit 3), scoped or not.

ARGUMENTS:
  <path>            (required) <wb>[/<tab>[/<A1>]] — the workbook, optionally narrowed to a tab/region.

EXAMPLES:
  fsa1-cli check ./budget
  fsa1-cli check ./budget/Sheet1/H3             # validate just the cell you authored
  fsa1-cli check ./budget/Sheet1/A1:D20

OUTPUT:
  An ASCII report on stdout: one row per diagnostic with its severity, stable code, located pointer
  (the offending file / body position / tab), message, and `help` remediation. A clean workbook shows
  a single "no diagnostics" row. Each diagnostic carries a stable `code`, `severity`, `message`,
  `help`, and `location`.

EXIT CODES:
  0   Success (no error-severity diagnostics in scope)
  1   I/O failure
  2   Invalid arguments (unknown flag, a trailing segment that is neither canonical A1 nor a known
      defined name, or a name that resolves to a formula/constant)
  3   Validation error (an in-scope error-severity diagnostic, or the workbook would not load)
  24  Not found (no such workbook directory, or no such scope tab)

SEE ALSO:
  fsa1-cli render     Draw a workbook
  fsa1-cli --guide    Terse guide to the on-disk model
"#;

const EVAL_HELP: &str = r##"fsa1-cli eval — evaluate an ad-hoc formula against a workbook

USAGE:
  fsa1-cli eval <path> --formula '=<formula>'

  <path> is <wb> or <wb>/<tab> (NO A1 selector — eval has no region):
    eval ./budget --formula …          unqualified refs bind to the first tab
    eval ./budget/Orders --formula …   unqualified refs bind to the Orders tab

DESCRIPTION:
  Evaluate a formula against a loaded workbook and emit its value. Read-only — no writes, no mutation.
  Unqualified references (A1, A1:A5) bind to the path tab (wb/Tab), else the first tab (wb); cross-tab
  (Tab!A1) and ranges resolve. A region selector on the path (wb/Tab/A1) is refused — eval has no
  region.

ARGUMENTS:
  <path>            (required) <wb> or <wb>/<tab> — the tab unqualified references resolve against.
  --formula '=…'    (required) The formula to evaluate.

EXAMPLES:
  fsa1-cli eval ./budget --formula '=SUM(A1:A5)'
  fsa1-cli eval ./budget/Orders --formula '=SUMPRODUCT(--(C2:C11>5))'

OUTPUT:
  The bare value on stdout (e.g. `6`). An error-valued result prints the error value (e.g. `#DIV/0!`)
  and exits 3; an unparseable formula prints its located diagnostic and exits 3.

EXIT CODES:
  0   Success (a value)
  1   I/O failure
  2   Invalid arguments (missing --formula, or a region selector on the path)
  3   Validation error (a parse error, or an error-valued result like #DIV/0!)
  24  Not found (no such workbook directory, or no such tab)

SEE ALSO:
  fsa1-cli render     Draw a workbook
  fsa1-cli --guide    Terse guide to the on-disk model
"##;

const TRACE_HELP: &str = r#"fsa1-cli trace — inspect a cell's dependency tree

USAGE:
  fsa1-cli trace <path> [--dependents] [--depth <N>]

  <path> is <wb>/<tab>/<A1>|<Name> naming a SINGLE cell (a range selector or named range is refused):
    trace ./budget/Sheet1/D1               trace D1's upstream dependencies
    trace ./budget/Sheet1/D1 --dependents  trace D1's downstream consumers
    trace ./budget/Sheet1/anchor           trace the single cell a defined name resolves to

DESCRIPTION:
  Report a cell's UPSTREAM dependencies (the cells it reads, transitively) or, with --dependents, its
  DOWNSTREAM consumers (the cells that read it) — the same engine dependency relation, transposed.
  The walk is cycle-safe (a cycle is reported, not looped) and shows a shared cell once (marked
  repeated). Each node carries its value and, unless it lies on a cycle, its computation hash.

ARGUMENTS:
  <path>            (required) <wb>/<tab>/<A1> — the single cell to trace (a range is refused).
  --dependents      (optional) Trace downstream consumers instead of upstream dependencies.
  --depth <N>       (optional) Cap the DISPLAYED tree depth. Default: the whole cone, however deep.

EXAMPLES:
  fsa1-cli trace ./budget/Sheet1/D1
  fsa1-cli trace ./budget/Sheet1/A1 --dependents
  fsa1-cli trace ./budget/Sheet1/D1 --depth 2

OUTPUT:
  An indented text tree on stdout: one line per node, `<cell>  <formula>  -> <value>  [<hash|status>]`,
  with `(repeated)` on a shared node. The root is the traced cell; children are its dependencies (or,
  with --dependents, its consumers).

EXIT CODES:
  0   Success (tree reported)
  1   I/O failure
  2   Invalid arguments (a missing cell, a range selector, or a bad --depth)
  3   Validation error (the workbook would not load, or an out-of-range trace target)
  24  Not found (no such workbook directory, or no such tab)

SEE ALSO:
  fsa1-cli eval       Evaluate an ad-hoc formula
  fsa1-cli --guide    Terse guide to the on-disk model
"#;

const TREE_HELP: &str = r#"fsa1-cli tree — present a workbook's complete structure as a read-only nested view

USAGE:
  fsa1-cli tree <path> [--mode <combined|values|functions>] [--full]

  <path> is <wb>[/<tab>[/<A1>]] — the tab and A1 region are PART OF THE PATH:
    tree ./budget                  the whole workbook
    tree ./budget/Summary          just the Summary tab
    tree ./budget/Summary/A1:A60   exactly that viewport's cells, ALL of them (uncapped)

DESCRIPTION:
  Present the workbook's COMPLETE authored structure (CLI3) — every tab, every cell of every cell/range
  file, and every name — as a single read-only nested tree. Never shows a reserved entry such as
  .cache/ (FS3). A single-cell file is one node; a multi-cell range file expands to one node per A1
  coordinate (row then column), capped so a large range never floods the view (the remainder is shown
  as an elided count with a "use --full to expand" hint — pass --full to lift the cap and show every
  coordinate). Content mode mirrors render: COMBINED is the DEFAULT (a formula cell/name shows
  `<value> ← =<formula>`; a literal shows its value), narrow with --mode functions (authored source) or
  --mode values (computed). A GRID5 array-formula file is one node under functions (the formula) and
  expands under values/combined. A name shows what it resolves to: a symlinked cell/range shows its
  target A1 reference; a named formula/constant shows its definition (functions), computed value
  (values), or both (combined). Rooting at a <wb>/<tab> path shows that tab's cells and its sheet-scoped
  names only; workbook-scoped names appear in the whole-workbook view. A <wb>/<tab>/<A1:B9> path shows
  EXACTLY that viewport's cells — ALL of them, the per-range cap does NOT apply (an explicit region is
  shown in full). READ-ONLY: leaves the workbook byte-identical (CORE3) and writes nothing under it at
  all, .cache/ included — as does every other command.

ARGUMENTS:
  <path>            (required) <wb>[/<tab>[/<A1>]] — the workbook, a tab, or a tab region.
  --mode <m>        (optional) One of: combined (default: value AND source, `<value> ← =<formula>`),
                    values (computed only), functions (authored source only).
  --full            (optional) Lift the per-range coordinate cap on the whole-structure view: expand
                    every cell, eliding nothing (what the "use --full to expand" hint invites).

EXAMPLES:
  fsa1-cli tree ./budget                        # the whole workbook, combined (value + source)
  fsa1-cli tree ./budget --mode functions       # authored source only
  fsa1-cli tree ./budget --mode values          # computed values only
  fsa1-cli tree ./budget --full                 # every coordinate of every range, nothing elided
  fsa1-cli tree ./budget/Summary                # just the Summary tab
  fsa1-cli tree ./budget/Summary/A1:A60         # exactly this viewport, all 60 cells, uncapped

OUTPUT:
  A nested text tree on stdout: each tab, its cells (A1 coordinate  # content), and its names.

EXIT CODES:
  0   Success (tree drawn)
  1   I/O failure
  2   Invalid arguments (unknown flag/mode, or a non-A1 trailing selector)
  3   Validation error (the workbook would not load)
  24  Not found (no such workbook directory, or no such tab)

SEE ALSO:
  fsa1-cli render     The same scopes drawn as ASCII tables instead of a nested view
  fsa1-cli --guide    Terse guide to the on-disk model
"#;

const SAMPLE_HELP: &str = r#"fsa1-cli sample — write a live tutorial workbook to disk

USAGE:
  fsa1-cli sample <dir>

DESCRIPTION:
  Write the canonical tutorial workbook (two tabs, a header row, per-row formulas, a SUM, and a
  cross-sheet reference) into <dir>. The one command that writes to disk. Refuses to overwrite a
  non-empty directory (never clobbers).

ARGUMENTS:
  <dir>             (required) The directory to write into (created if absent; must be empty if it exists).

EXAMPLES:
  fsa1-cli sample ./demo && fsa1-cli render ./demo

OUTPUT:
  A terse next-steps hint on stdout naming the written tabs and a few commands to try.

EXIT CODES:
  0   Success (workbook written)
  1   I/O failure
  2   Invalid arguments
  4   Conflict (<dir> exists and is non-empty — refused, nothing written)

SEE ALSO:
  fsa1-cli render     Draw the written workbook
  fsa1-cli --guide    Terse guide to the on-disk model
"#;

const UNPACK_HELP: &str = r#"fsa1-cli unpack — convert a real spreadsheet file into an FSA1 workbook

USAGE:
  fsa1-cli unpack [--strict] [--decompose <policy>] <src> [<dst>]

DESCRIPTION:
  Convert a real spreadsheet file — an OpenDocument (.ods) or an Excel (.xlsx) workbook, dispatched by
  extension — into an FSA1 workbook the format-blind engine renders and evaluates. Each sheet becomes
  a tab folder; each rectangular block the decomposition cuts becomes ONE grid file named by the closed
  A1 range it fills (a sheet with no content makes no file), and the appearance of its cells rides along
  in a <range>.css sidecar beside it. Cell values map to FSA1's value model (a date-typed cell becomes
  its Excel serial); formulas are translated to FSA1's Excel-A1 grammar (an xlsx formula is already
  Excel-A1), and a formula FSA1 cannot parse is preserved verbatim and flagged as a located error at
  load (check reports it), not aborted. Refuses to overwrite a non-empty destination (never clobbers);
  a failed unpack cleans up its partial output. Every failure is a located diagnostic (sheet!cell) — an
  unrepresentable value or an unsupported source format is refused, never silently wrong.

  <dst> is OPTIONAL. Omitted, the workbook is written to ./<src-stem>/ in the current directory, where
  <src-stem> is the source filename with its final extension removed (/path/to/acme-dcf.xlsx -> ./acme-
  dcf/; the source's directory prefix is discarded — the workbook lands in the CWD, not beside the
  source). Given, <dst> is used verbatim.

  --decompose picks WHICH POLICY cuts a sheet into those blocks: {DECOMPOSITIONS}. occupancy cuts at
  a sheet's widest fully-empty rows and columns, reading no appearance at all, and writes ONE block per
  sheet-sized region of occupancy — the shape unpack wrote before this flag existed. appearance grows
  and joins rectangles over runs of one cell appearance, and so writes MORE, SMALLER range files than
  occupancy. cell writes one file per occupied coordinate and joins nothing: the identity cut, which
  the other two are graded against and which no tree of any size should be written with. What it buys is that a structure an author expressed is addressable as ONE file more often
  than under occupancy; no block either policy cuts is a semantic unit. WITHOUT the flag the policy is
  ALWAYS occupancy, whatever the source, so an unflagged unpack writes the shape it always wrote; the
  policy in force is named on the success line. --decompose appearance on a source whose format has
  no appearance channel is REFUSED before anything is written, because no source in that format could
  ever state one; an xlsx stating no appearance anywhere is ACCEPTED, and cut on the structure its
  occupancy expresses. --decompose occupancy is always accepted.

  --strict selects the round-trip contract (the inverse of a faithful `pack`): an xlsx that the
  skeleton cannot serialize back identically is REFUSED with a located diagnostic rather than unpacked
  lossily — one carrying a non-default number format on any cell (the skeleton models General format
  only), a package part FSA1 neither models nor can regenerate (a chart, drawing, pivotTable, table,
  media object, external link, or macro project), or a column width / row height no range file ends up
  carrying (which the CUT decides, so that one is found after the blocks are chosen and the partial
  output is removed — the destination is left absent either way). The default (no --strict) is the
  unchanged lossy unpack, which accepts those files, imports their values, and NAMES each loss.

ARGUMENTS:
  <src>                  (required) The source spreadsheet to read — a .ods or .xlsx file.
  <dst>                  (optional) The workbook directory to write (created; must be empty if it
                         exists). Omitted, it derives to ./<src-stem>/ in the current directory.

OPTIONS:
  --strict               Refuse any xlsx the skeleton cannot round-trip identically (a non-default number
                         format, an out-of-scope package part, or a size no range file carries), naming
                         the offending cell, part or axis. Nothing is written on a refusal.
  --decompose <policy>   Which policy cuts a sheet into range files: {DECOMPOSITIONS}. Defaults to
                         occupancy for every source; appearance is only ever used when named.
                         appearance writes more,
                         smaller files than occupancy; occupancy writes one block per sheet-sized
                         region of occupancy. appearance on a source whose format has no appearance
                         channel is refused before anything is written (exit 3).

EXAMPLES:
  fsa1-cli unpack book.xlsx && fsa1-cli render ./book            # <dst> derives to ./book/
  fsa1-cli unpack book.ods  ./out/wb && fsa1-cli render ./out/wb # explicit <dst>
  fsa1-cli unpack --strict book.xlsx   # refuse a formatted/tail-part file instead of lossily unpacking
  fsa1-cli unpack --decompose occupancy book.xlsx   # one file per content block, ignoring appearance

OUTPUT:
  A terse next-steps hint on stdout: the source, the destination, the tab and range-file counts, the
  policy the sheets were decomposed by, and a couple of commands to try on the unpacked workbook.

  A DEFAULT-ON fidelity report on STDERR (exit code unchanged): after the success line, unpack prints a
  full, grouped, uncapped account of everything the conversion changed, each item located (sheet!cell /
  name / table / column / row / part) and visible in `check`. Its sections, in the order they print:
{SECTIONS}
  A faithful conversion prints a single "nothing lost" line instead. The report fires on any completed
  import whose losses are non-empty, WITH or WITHOUT --strict (--strict only turns some of those losses
  into refusals; on a refusal nothing is imported, so there is no report).

  Either rendering vouches only for the categories the SOURCE's readers examined. A .ods is read for
  its values and formulas alone, so its run names the categories it did not inspect ("not inspected on
  this source, so not vouched for: ...") and never prints "nothing lost" — a loss in one of those is
  neither reported nor ruled out. A .xlsx is read for all seven.

EXIT CODES:
  0   Success (workbook written)
  1   I/O failure (source unreadable, or a destination write failed)
  2   Invalid arguments (incl. a <src> with no derivable stem, e.g. `.`, `..`, `/`, when <dst> is
      omitted, or a --decompose naming no policy)
  3   Validation error (an unrepresentable value, bad date/sheet name, unsupported source format, a
      --decompose the source cannot feed, or a --strict round-trip refusal: a non-default number
      format, an out-of-scope package part, or a size no range file carries)
  4   Conflict (<dst> exists and is non-empty — refused, nothing written)
  24  Not found (no such source file)

SEE ALSO:
  fsa1-cli render     Draw the unpacked workbook
  fsa1-cli --guide    Terse guide to the on-disk model
"#;

const PACK_HELP: &str = r#"fsa1-cli pack — serialize an FSA1 workbook back into a single .xlsx file

USAGE:
  fsa1-cli pack <workbook-dir> [--target xlsx]

DESCRIPTION:
  Serialize an FSA1 workbook (the filesystem spreadsheet) back into one Excel (.xlsx) file — the
  inverse of unpack. Emits the simple core: cell values, formulas, multiple sheets, per-cell display
  formats (GRID7), and the workbook's defined names (SER3). Formula cells carry NO cached value, so the
  opening spreadsheet recomputes. The output is DERIVED WHOLLY from the source workbook: the command
  takes no cell content on the command line, and it reads the source READ-ONLY, leaving it byte-identical
  — it writes nothing under it, .cache/ included. The output name is DERIVED — ./<workbook-basename>.xlsx in the current directory
  (pack path/to/acme-dcf -> ./acme-dcf.xlsx, basename only). It lands only at a FRESH, not-already-
  occupied path — an existing file is refused (never clobbered). Number formats and rich parts (charts,
  pivots, tables, media) are not modeled by the skeleton.

ARGUMENTS:
  <workbook-dir>    (required) The FSA1 workbook directory to serialize (tabs = sub-folders). Its
                    basename names the derived ./<basename>.xlsx output in the current directory.

OPTIONS:
  --target <fmt>    (optional) The output format. Defaults to xlsx, the only format supported today;
                    any other value is refused. Exists so a future serializer slots in without a
                    surface change.

EXAMPLES:
  fsa1-cli pack ./book                  # -> ./book.xlsx
  fsa1-cli unpack book.xlsx && fsa1-cli pack ./book   # -> ./book.xlsx

OUTPUT:
  A terse confirmation on stdout naming the source, the derived destination, and the sheet count.

EXIT CODES:
  0   Success (.xlsx written)
  1   I/O failure (the destination could not be written)
  2   Invalid arguments (incl. a <workbook-dir> with no basename, e.g. `.`, `..`, `/`, or a --target
      other than xlsx)
  3   Validation error (the workbook would not load, or has no tabs to pack)
  4   Conflict (the derived ./<basename>.xlsx already exists — refused, nothing written)
  24  Not found (no such workbook directory)

SEE ALSO:
  fsa1-cli unpack     Convert a real spreadsheet file into an FSA1 workbook
  fsa1-cli --guide    Terse guide to the on-disk model
"#;

const CONVERT_HELP: &str = r#"fsa1-cli convert — re-spell a workbook's range file names for another OS

USAGE:
  fsa1-cli convert <workbook-dir> [--to posix|windows|auto]

DESCRIPTION:
  A range cell is a file whose NAME is its A1 range. That name has two legal spellings: `A1:C1`
  (POSIX-native) and `A1-C1` (portable / Windows-safe — `:` is not a legal filename character on
  Windows). `convert` renames a workbook's range files between the two so a tree authored on one OS
  can be checked out and read on the other.

  ONLY range file names change. Cell contents, single-cell files (`A1`), defined names (`Tax_Rate`,
  `Days.begin`), and every formula's logical `:` operator are left byte-identical. The reader accepts
  BOTH spellings on every platform, so a converted tree still loads on the host you converted it on.

ARGUMENTS:
  <workbook-dir>    (required) The FSA1 workbook directory to convert in place.

OPTIONS:
  --to <target>     posix   -> `A1:C1`  (only creatable on POSIX; refused on Windows)
                    windows -> `A1-C1`  (portable; creatable anywhere)
                    auto    -> the spelling native to THIS host (default)

EXAMPLES:
  fsa1-cli convert ./book --to windows   # make ./book checkoutable on Windows (A1:C1 -> A1-C1)
  fsa1-cli convert ./book --to posix     # back to A1:C1 (run on Linux/macOS)
  fsa1-cli convert ./book                # normalize to this host's spelling

EXIT CODES:
  0   Success (files renamed, or already normalized)
  1   I/O failure (a rename could not be performed)
  2   Invalid arguments (incl. --to posix on Windows, where ':' is not a legal filename char)
  4   Conflict (a target name already exists — refused, nothing renamed)
  24  Not found (no such workbook directory)

SEE ALSO:
  fsa1-cli --guide    Terse guide to the on-disk model (the filename grammar)
"#;

#[cfg(test)]
mod tests {
    use super::{SECTIONS, guide_text, render_unpack_report, unpack_help};
    use fsa1_ingest::{Decomposition, UnpackCategory, UnpackWarning};

    /// One warning per category, so a report rendered from these emits every section there is.
    fn one_of_each() -> Vec<UnpackWarning> {
        vec![
            UnpackWarning::NumberFormatCoerced {
                sheet: "S".into(),
                cell: "D2".into(),
                num_fmt_id: 14,
                format_code: None,
            },
            UnpackWarning::TableDropped {
                table: "Sales".into(),
                reason: "r".into(),
            },
            UnpackWarning::NameSkipped {
                name: "A1".into(),
                scope: None,
                reason: "r".into(),
            },
            UnpackWarning::FormulaKeptVerbatim {
                sheet: "S".into(),
                cell: "A2".into(),
                source: "x".into(),
                reason: "r".into(),
            },
            UnpackWarning::CellAttributeDropped {
                sheet: "S".into(),
                cell: "A1".into(),
                attribute: "indent level 2".into(),
            },
            UnpackWarning::ColumnWidthUnspellable {
                sheet: "S".into(),
                column: "C".into(),
                width: "300".into(),
            },
            UnpackWarning::WorkbookPartNotCarried {
                part: "conditional formatting".into(),
            },
        ]
    }

    /// An xlsx run, the one that opens every category there is.
    fn all() -> Vec<UnpackCategory> {
        UnpackCategory::ALL.to_vec()
    }

    #[test]
    fn a_clean_import_renders_the_nothing_lost_line() {
        let out = render_unpack_report(&[], &all());
        assert!(out.starts_with("unpack fidelity: nothing lost"), "{out}");
        assert!(out.ends_with('\n'));
    }

    /// Both accounts of what the report covers read SECTIONS, so a category with no row there is one
    /// the clean line silently omits and the blind-spot line silently forgives.
    #[test]
    fn every_category_has_a_section_row() {
        for category in UnpackCategory::ALL {
            assert!(
                SECTIONS.iter().any(|(c, _, _)| *c == category),
                "{category:?} has no section row"
            );
        }
        assert_eq!(SECTIONS.len(), UnpackCategory::ALL.len());
    }

    /// The root of both blocking findings: a category is vouched for ONLY where something looked at
    /// it. An .ods run reads values and formulas, so its line credits formulas, withholds the other
    /// six by name, and does not say "nothing lost" at all.
    #[test]
    fn a_category_the_run_never_inspected_is_declared_rather_than_vouched_for() {
        let out = render_unpack_report(&[], &[UnpackCategory::Formula]);
        assert!(
            !out.contains("nothing lost"),
            "an unlooked-at category cannot be part of a nothing-lost claim:\n{out}"
        );
        assert!(
            out.contains("no loss in: formulas."),
            "what WAS inspected stays specific:\n{out}"
        );
        assert!(out.contains("not inspected on this source"), "{out}");
        for (category, _, noun) in SECTIONS {
            if category == UnpackCategory::Formula {
                continue;
            }
            let blind = out
                .split("not inspected on this source")
                .nth(1)
                .expect("{out}");
            assert!(
                blind.contains(noun),
                "{noun:?} is not withheld by name:\n{out}"
            );
        }
    }

    /// The same discipline on the lossy rendering: a report that lists what it found, over a source
    /// half of whose categories nobody opened, is read as a clean bill for the rest unless it says so.
    #[test]
    fn a_lossy_report_on_a_partly_inspected_source_still_declares_the_blind_categories() {
        let out = render_unpack_report(
            &[UnpackWarning::FormulaKeptVerbatim {
                sheet: "Calc".into(),
                cell: "A1".into(),
                source: "SUM({1;2;3})".into(),
                reason: "an inline array".into(),
            }],
            &[UnpackCategory::Formula],
        );
        assert!(out.contains("unpack fidelity report (1 item(s))"), "{out}");
        assert!(out.contains("formulas kept verbatim (1 item(s)):"), "{out}");
        assert!(
            out.contains("not inspected on this source, so not vouched for: number formats"),
            "{out}"
        );
    }

    /// An agent reads `--help` to decide whether unpack reports a loss at all; a help text naming
    /// fewer sections than the report emits sends it to inspect the tree itself.
    #[test]
    fn unpack_help_names_every_section_the_report_can_emit() {
        let help = unpack_help();
        let report = render_unpack_report(&one_of_each(), &all());
        let emitted: Vec<&str> = report
            .lines()
            .filter(|l| !l.starts_with(' ') && l.ends_with("item(s)):"))
            .filter_map(|l| l.split_once(" (").map(|(label, _)| label))
            .collect();
        assert_eq!(emitted.len(), SECTIONS.len(), "{report}");
        for label in emitted {
            assert!(help.contains(label), "unpack --help never names {label:?}");
        }
    }

    /// The same contract on the other surface `--help` documents: a policy `--decompose` accepts but
    /// the help never names is one an agent can only find by guessing.
    #[test]
    fn unpack_help_names_every_decomposition() {
        let help = unpack_help();
        for decomposition in Decomposition::ALL {
            assert!(
                help.contains(decomposition.name()),
                "unpack --help never names {:?}",
                decomposition.name()
            );
        }
    }

    /// And on the surface an agent reads FIRST. `--guide` is the terse index, so a policy it omits is
    /// one an agent never learns exists without opening `unpack --help`.
    #[test]
    fn the_guide_names_every_decomposition() {
        let guide = guide_text();
        assert!(!guide.contains("{DECOMPOSITIONS}"), "the hole is unfilled");
        for decomposition in Decomposition::ALL {
            assert!(
                guide.contains(decomposition.name()),
                "--guide never names {:?}",
                decomposition.name()
            );
        }
    }

    /// The other half of the same contract: the line a FAITHFUL conversion prints names what it is
    /// vouching for, so an agent is not told appearance and geometry went unchecked.
    #[test]
    fn the_nothing_lost_line_names_every_category_the_report_covers() {
        let out = render_unpack_report(&[], &all());
        for (_, _, covered) in SECTIONS {
            assert!(
                out.contains(covered),
                "nothing-lost omits {covered:?}: {out}"
            );
        }
    }

    #[test]
    fn a_lossy_import_groups_its_categories_in_section_order_uncapped() {
        // The Table category has no natural file fixture, so its rendering is pinned here.
        let warnings = vec![
            UnpackWarning::NumberFormatCoerced {
                sheet: "Data".into(),
                cell: "D2".into(),
                num_fmt_id: 14,
                format_code: None,
            },
            UnpackWarning::TableDropped {
                table: "Sales".into(),
                reason: "could not map to a sheet (displayName/sheet divergence); structured refs load as #NAME?".into(),
            },
            UnpackWarning::NameSkipped {
                name: "A1".into(),
                scope: None,
                reason: "identifier parses as an A1 address".into(),
            },
            UnpackWarning::FormulaKeptVerbatim {
                sheet: "Data".into(),
                cell: "B1".into(),
                source: "SUM({1,2,3})".into(),
                reason: "an inline array is not translatable".into(),
            },
        ];
        let out = render_unpack_report(&warnings, &all());
        assert!(out.contains("unpack fidelity report (4 item(s))"), "{out}");
        let nf = out.find("number formats coerced to plain").unwrap();
        let tb = out.find("tables dropped").unwrap();
        let nm = out.find("defined names skipped").unwrap();
        let fm = out.find("formulas kept verbatim").unwrap();
        assert!(
            nf < tb && tb < nm && nm < fm,
            "sections out of order:\n{out}"
        );
        assert!(out.contains("\"Sales\": could not map to a sheet"), "{out}");
        assert!(
            out.contains("\"A1\" (workbook): identifier parses as an A1 address"),
            "{out}"
        );
    }

    /// A category with no section is a loss the report swallows, which is the one thing this report
    /// exists to prevent — so EVERY category reaches it.
    #[test]
    fn every_warning_category_reaches_a_section_of_its_own() {
        for warning in [
            UnpackWarning::MergedRegionFlattened {
                sheet: "S".into(),
                region: "D1:E1".into(),
            },
            UnpackWarning::CellAttributeDropped {
                sheet: "S".into(),
                cell: "A1".into(),
                attribute: "indent level 2".into(),
            },
            UnpackWarning::ColumnWidthUnspellable {
                sheet: "S".into(),
                column: "C".into(),
                width: "300".into(),
            },
            UnpackWarning::WorkbookPartNotCarried {
                part: "conditional formatting".into(),
            },
        ] {
            let out = render_unpack_report(std::slice::from_ref(&warning), &all());
            assert!(
                out.contains(&warning.to_string()),
                "{warning:?} never reached the report:\n{out}"
            );
        }
    }

    #[test]
    fn a_section_with_no_items_is_omitted() {
        let out = render_unpack_report(
            &[UnpackWarning::FormulaKeptVerbatim {
                sheet: "S".into(),
                cell: "A2".into(),
                source: "x".into(),
                reason: "r".into(),
            }],
            &all(),
        );
        assert!(out.contains("formulas kept verbatim (1 item(s)):"), "{out}");
        assert!(!out.contains("number formats coerced"), "{out}");
        assert!(!out.contains("tables dropped"), "{out}");
        assert!(!out.contains("defined names skipped"), "{out}");
    }
}
