// Concern: charlie-cli — the THIN binary shell (`charlie-cli render` / `check` / `eval` / `sample` / `import` / `--guide`): parse argv (including the global `--format text|json` selector), drive `charlie-model` (load a workbook, ask for a render grid, a lint report, or an ad-hoc `=formula`'s value; or WRITE the model's tutorial workbook to disk for `sample`) or `charlie-ingest` (CONVERT an `.ods`/`.xlsx` into a workbook for `import`), and hand the structured outcome to the `output` envelope layer — which dual-renders it as EITHER a human ASCII table / scalar / prose (`--format text`, the default) OR a `{status,data|error}` JSON envelope on stdout (`--format json`, the machine surface) — then set the exit code an agent branches on (0 clean · 1 I/O failure · 2 bad args · 3 error-severity diagnostics or error-valued eval or an untranslatable import · 4 target-dir conflict · 24 path not found); it holds NO spreadsheet logic — the demand-driven eval, value spelling, diagnostics, and the sample CONTENT all live in the model, the ODS/xlsx conversion in `charlie-ingest`, the guide text in `guide`, comfy-table drawing in `ascii`, and the envelope/JSON in `output` | Non-concern: WHAT a cell computes to or WHY a diagnostic fires (charlie-model owns the render model + lint + ad-hoc formula eval + sample content, charlie-ingest owns the ODS/xlsx import + the format firewall), the formula language (charlie-ast), and the envelope serialization itself (`output` owns the JSON + the dual-render) | IO: (argv incl. `--format`, a workbook directory on disk, a `.ods`/`.xlsx` source) -> a render grid / lint report / scalar value / sample-write / imported workbook, dual-rendered by `output` to stdout (text or JSON envelope) + an exit code; a freshly-written sample or imported workbook tree on disk; text-mode operational errors to stderr
//! `charlie-cli` — render and lint a filesystem spreadsheet. The binary is a thin consumer of
//! `charlie-model`: it parses arguments, calls the model's `render`/`lint`/`eval` surface, and hands
//! the returned plain-data outcome to the [`output`] envelope layer, which dual-renders it as a human
//! ASCII table (`--format text`) or a machine JSON envelope (`--format json`). All spreadsheet logic
//! stays in the model (`repo-standards.md`: logic in the engine, CLI a thin shell).
//!
//! Stack-native entrypoint: `cargo run -p charlie-cli -- render <path>` (binary name `charlie-cli`).

mod ascii;
mod guide;
mod output;

use std::path::Path;
use std::process::ExitCode;

use charlie_model::{Direction, FormulaOutcome, RenderMode, Workbook, parse_viewport, render};

use crate::output::{
    ErrorCode, Format, emit_error, emit_eval_error_value, emit_eval_value, emit_grid, emit_import,
    emit_sample, emit_trace, emit_validation_diagnostics, emit_version,
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(run(&args))
}

fn run(args: &[String]) -> u8 {
    // `--version`/`-V` overrides everything (cli-interface-standards.md) — always a JSON envelope.
    if args.iter().any(|a| a == "--version" || a == "-V") {
        emit_version();
        return 0;
    }
    // `--guide` prints the terse on-disk-model tour (its single home is `guide::GUIDE`).
    if args.iter().any(|a| a == "--guide") {
        print!("{}", guide::GUIDE);
        return 0;
    }
    if args.is_empty() {
        print_help(None);
        return 0;
    }
    // `--help` prints per-command help when a known subcommand token is present (regardless of flag
    // order), else the global banner — so `render --help` documents render's own args, JSON OUTPUT
    // shape, and exit codes (cli-interface-standards Part 4: help is per-command). No `-h` short form:
    // the standard permits only `-V` and `--help` (Part 1 "Version Flag"), never other short flags.
    if args.iter().any(|a| a == "--help") {
        let cmd = args.iter().map(String::as_str).find(|a| {
            matches!(
                *a,
                "render" | "check" | "eval" | "trace" | "sample" | "import"
            )
        });
        print_help(cmd);
        return 0;
    }

    // The global `--format text|json` selector is stripped before command dispatch (it may appear
    // anywhere). Default: text (the human form); `--format json` is the machine envelope surface.
    let (fmt, rest) = match extract_format(args) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    if rest.is_empty() {
        print_help(None);
        return 0;
    }

    match rest[0].as_str() {
        "render" => cmd_render(fmt, &rest[1..]),
        "check" => cmd_check(fmt, &rest[1..]),
        "eval" => cmd_eval(fmt, &rest[1..]),
        "trace" => cmd_trace(fmt, &rest[1..]),
        "sample" => cmd_sample(fmt, &rest[1..]),
        "import" => cmd_import(fmt, &rest[1..]),
        other => {
            let msg = format!("unknown command {other:?}");
            fail(fmt, ErrorCode::InvalidArguments, &msg)
        }
    }
}

/// Strip the global `--format text|json` selector (in either `--format json` or `--format=json`
/// spelling) from `args`, returning the chosen [`Format`] and the remaining argv. Defaults to
/// [`Format::Text`]. An unknown value is a bad-args refusal (emitted as text, since format is unknown).
fn extract_format(args: &[String]) -> Result<(Format, Vec<String>), u8> {
    let mut fmt = Format::Text;
    let mut rest: Vec<String> = Vec::with_capacity(args.len());
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let value = if arg == "--format" {
            match it.next() {
                Some(v) => Some(v.as_str()),
                None => {
                    emit_error(
                        Format::Text,
                        ErrorCode::InvalidArguments,
                        "--format needs a value (text or json)",
                    );
                    return Err(ErrorCode::InvalidArguments.exit());
                }
            }
        } else {
            arg.strip_prefix("--format=")
        };
        match value {
            Some("text") => fmt = Format::Text,
            Some("json") => fmt = Format::Json,
            Some(other) => {
                let msg = format!("--format must be text or json, not {other:?}");
                emit_error(Format::Text, ErrorCode::InvalidArguments, &msg);
                return Err(ErrorCode::InvalidArguments.exit());
            }
            None => rest.push(arg.clone()),
        }
    }
    Ok((fmt, rest))
}

/// `charlie-cli render <path> [--tab NAME] [--range A3:G8] [--values|--functions]`.
fn cmd_render(fmt: Format, rest: &[String]) -> u8 {
    let mut path: Option<String> = None;
    let mut tab: Option<String> = None;
    let mut range: Option<String> = None;
    let mut modes: Vec<RenderMode> = Vec::new();
    let mut no_cache = false;

    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = split_flag(arg);
        match flag {
            "--tab" => match take_value(inline, &mut it) {
                Some(v) => tab = Some(v),
                None => return bad_arg(fmt, "--tab needs a tab name"),
            },
            "--range" => match take_value(inline, &mut it) {
                Some(v) => range = Some(v),
                None => return bad_arg(fmt, "--range needs an A1 range like A3:G8"),
            },
            "--values" => modes.push(RenderMode::Values),
            "--functions" => modes.push(RenderMode::Functions),
            "--no-cache" => no_cache = true,
            f if f.starts_with('-') => return bad_arg(fmt, &format!("unknown flag {f:?}")),
            _ => {
                if path.replace(arg.clone()).is_some() {
                    return bad_arg(fmt, "render takes exactly one <path>");
                }
            }
        }
    }
    if modes.len() > 1 {
        return bad_arg(fmt, "choose at most one of --values / --functions");
    }
    let mode = modes.first().copied().unwrap_or(RenderMode::Values);
    let Some(path) = path else {
        return bad_arg(fmt, "render needs a <path> to a workbook directory");
    };

    let wb = match load(fmt, Path::new(&path), no_cache) {
        Ok(wb) => wb,
        Err(code) => return code,
    };
    if wb.sheet_names().is_empty() {
        let msg = format!("{path:?} has no tabs (a tab is a sub-folder of cell/range files)");
        return fail(fmt, ErrorCode::Validation, &msg);
    }

    // Pick the tab: an explicit --tab by name, else tab 0 (the first sheet).
    let sheet = match &tab {
        Some(name) => match wb.tab_index(name) {
            Some(i) => i,
            None => {
                let msg = format!(
                    "no tab named {name:?} in {path:?} (tabs: {:?})",
                    wb.sheet_names()
                );
                return fail(fmt, ErrorCode::NotFound, &msg);
            }
        },
        None => 0,
    };

    // Pick the viewport: an explicit --range, else the tab's whole used region.
    let viewport = match &range {
        Some(r) => match parse_viewport(r) {
            Ok(rect) => rect,
            Err(msg) => return bad_arg(fmt, &msg),
        },
        None => match wb.used_region(sheet) {
            Some(rect) => rect,
            None => return emit_empty_tab(fmt, &wb, sheet),
        },
    };

    // Bound the viewport before rendering: `render` allocates a string per cell, so a
    // syntactically-valid but enormous `--range` (e.g. `A1:A4294967295`) would abort the process on
    // allocation. Refuse with a located diagnostic instead (fail-fast, never a crash).
    let cells = charlie_model::viewport_cell_count(viewport);
    if cells > charlie_model::MAX_VIEWPORT_CELLS {
        let msg = format!(
            "--range spans {cells} cells, over the render bound of {} -- narrow the range",
            charlie_model::MAX_VIEWPORT_CELLS
        );
        return bad_arg(fmt, &msg);
    }

    let grid = render(&wb, sheet, viewport, mode);
    emit_grid(fmt, &grid);
    0
}

/// An empty tab has no cells to render: emit an empty grid (JSON) or a stderr note (text). Exit 0 —
/// an empty tab is not a failure.
fn emit_empty_tab(fmt: Format, wb: &Workbook, sheet: u32) -> u8 {
    match fmt {
        Format::Json => {
            let empty = charlie_model::RenderGrid {
                col_labels: Vec::new(),
                rows: Vec::new(),
            };
            emit_grid(fmt, &empty);
        }
        Format::Text => {
            eprintln!(
                "charlie-cli: tab {:?} is empty (no cells to render)",
                sheet_name(wb, sheet)
            );
        }
    }
    0
}

/// `charlie-cli check <path>` — lint the workbook and render the diagnostics. Exits non-zero if any
/// error-severity diagnostic fires (a workbook that won't even load is itself the failure).
fn cmd_check(fmt: Format, rest: &[String]) -> u8 {
    let mut path: Option<String> = None;
    let mut no_cache = false;
    for arg in rest {
        let (flag, _) = split_flag(arg);
        if flag == "--no-cache" {
            no_cache = true;
            continue;
        }
        if flag.starts_with('-') {
            return bad_arg(fmt, &format!("unknown flag {flag:?}"));
        }
        if path.replace(arg.clone()).is_some() {
            return bad_arg(fmt, "check takes exactly one <path>");
        }
    }
    let Some(path) = path else {
        return bad_arg(fmt, "check needs a <path> to a workbook directory");
    };

    // Load-time refusals (overlap, literal dimension mismatch, bad filename) surface from the loader;
    // eval-time ones (cycle, formula dimension mismatch, unparseable body) come from `lint`.
    let diags = match Workbook::load_dir(Path::new(&path)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let msg = format!("no such workbook directory {path:?}");
            return fail(fmt, ErrorCode::NotFound, &msg);
        }
        Err(e) => {
            let msg = format!("cannot read {path:?}: {e}");
            return fail(fmt, ErrorCode::Io, &msg);
        }
        Ok(Err(load_diags)) => load_diags,
        Ok(Ok(mut wb)) => {
            apply_no_cache(&mut wb, no_cache);
            wb.lint()
        }
    };

    output::emit_diagnostics(fmt, &diags)
}

/// `charlie-cli eval <path> --formula '=<formula>' [--tab <name>]` — evaluate an ad-hoc formula against
/// a loaded workbook and emit the resulting value. Read-only: no file writes, no mutation. Unqualified
/// refs resolve against `--tab` (default: the first tab). A parse error is a located diagnostic; an
/// error-valued result carries the error text; both exit non-zero. Every eval outcome renders through
/// the same channel (stdout / the JSON envelope) so the failure detail is uniformly locatable.
fn cmd_eval(fmt: Format, rest: &[String]) -> u8 {
    let mut path: Option<String> = None;
    let mut tab: Option<String> = None;
    let mut formula: Option<String> = None;
    let mut no_cache = false;

    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = split_flag(arg);
        match flag {
            "--tab" => match take_value(inline, &mut it) {
                Some(v) => tab = Some(v),
                None => return bad_arg(fmt, "--tab needs a tab name"),
            },
            "--formula" => match take_value(inline, &mut it) {
                Some(v) => formula = Some(v),
                None => {
                    return bad_arg(
                        fmt,
                        "--formula needs a formula, e.g. --formula '=SUM(A1:A5)'",
                    );
                }
            },
            "--no-cache" => no_cache = true,
            f if f.starts_with('-') => return bad_arg(fmt, &format!("unknown flag {f:?}")),
            _ => {
                if path.replace(arg.clone()).is_some() {
                    return bad_arg(
                        fmt,
                        "eval takes exactly one <path> (the formula is --formula)",
                    );
                }
            }
        }
    }

    let Some(path) = path else {
        return bad_arg(fmt, "eval needs a <path> to a workbook directory");
    };
    let Some(formula) = formula else {
        return bad_arg(
            fmt,
            "eval needs --formula, e.g. charlie-cli eval ./budget --formula '=SUM(A1:A5)'",
        );
    };

    let wb = match load(fmt, Path::new(&path), no_cache) {
        Ok(wb) => wb,
        Err(code) => return code,
    };
    if wb.sheet_names().is_empty() {
        let msg = format!("{path:?} has no tabs (a tab is a sub-folder of cell/range files)");
        return fail(fmt, ErrorCode::Validation, &msg);
    }

    // Resolve the tab unqualified refs bind to: an explicit --tab by name, else tab 0 (the first).
    let sheet = match &tab {
        Some(name) => match wb.tab_index(name) {
            Some(i) => i,
            None => {
                let msg = format!(
                    "no tab named {name:?} in {path:?} (tabs: {:?})",
                    wb.sheet_names()
                );
                return fail(fmt, ErrorCode::NotFound, &msg);
            }
        },
        None => 0,
    };

    match wb.eval_formula(sheet, &formula) {
        Ok(FormulaOutcome::Value(s)) => {
            emit_eval_value(fmt, &s);
            0
        }
        // An error-valued result (#DIV/0!, #REF!, …) is a validation refusal that carries its value.
        Ok(FormulaOutcome::Error(s)) => emit_eval_error_value(fmt, &s),
        // A parse refusal is a located diagnostic — the same validation channel as the error value.
        Err(diag) => emit_validation_diagnostics(fmt, std::slice::from_ref(&diag)),
    }
}

/// `charlie-cli trace <path> --tab <name> --cell <A1> [--dependents] [--depth N]` — report a cell's
/// upstream dependencies (default) or downstream consumers (`--dependents`), as an indented tree
/// (`text`) or a nested `TraceNode` (`json`). The `--cell` may be sheet-qualified (`Tab!A1`), which
/// overrides `--tab`. Read-only. A bad cell/tab/depth is a located refusal (CORE2), never a panic.
fn cmd_trace(fmt: Format, rest: &[String]) -> u8 {
    let mut path: Option<String> = None;
    let mut tab: Option<String> = None;
    let mut cell: Option<String> = None;
    let mut dependents = false;
    let mut depth: Option<u32> = None;

    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = split_flag(arg);
        match flag {
            "--tab" => match take_value(inline, &mut it) {
                Some(v) => tab = Some(v),
                None => return bad_arg(fmt, "--tab needs a tab name"),
            },
            "--cell" => match take_value(inline, &mut it) {
                Some(v) => cell = Some(v),
                None => return bad_arg(fmt, "--cell needs a cell address like C3 (or Tab!C3)"),
            },
            "--depth" => match take_value(inline, &mut it) {
                Some(v) => match v.parse::<u32>() {
                    Ok(n) => depth = Some(n),
                    Err(_) => return bad_arg(fmt, &format!("--depth needs a number, not {v:?}")),
                },
                None => return bad_arg(fmt, "--depth needs a number, e.g. --depth 3"),
            },
            "--dependents" => dependents = true,
            f if f.starts_with('-') => return bad_arg(fmt, &format!("unknown flag {f:?}")),
            _ => {
                if path.replace(arg.clone()).is_some() {
                    return bad_arg(fmt, "trace takes exactly one <path>");
                }
            }
        }
    }

    let Some(path) = path else {
        return bad_arg(fmt, "trace needs a <path> to a workbook directory");
    };
    let Some(cell) = cell else {
        return bad_arg(
            fmt,
            "trace needs --cell, e.g. charlie-cli trace ./budget --tab Sheet1 --cell C3",
        );
    };

    // A sheet-qualified `Tab!A1` names its own tab (overriding --tab); a bare address uses --tab.
    let (tab, cell_addr) = match cell.split_once('!') {
        Some((t, c)) => (Some(t.to_string()), c.to_string()),
        None => (tab, cell),
    };

    let wb = match load(fmt, Path::new(&path), false) {
        Ok(wb) => wb,
        Err(code) => return code,
    };
    if wb.sheet_names().is_empty() {
        let msg = format!("{path:?} has no tabs (a tab is a sub-folder of cell/range files)");
        return fail(fmt, ErrorCode::Validation, &msg);
    }

    // Resolve the tab: an explicit (or `Tab!`-qualified) name, else the first tab.
    let sheet = match &tab {
        Some(name) => match wb.tab_index(name) {
            Some(i) => i,
            None => {
                let msg = format!(
                    "no tab named {name:?} in {path:?} (tabs: {:?})",
                    wb.sheet_names()
                );
                return fail(fmt, ErrorCode::NotFound, &msg);
            }
        },
        None => 0,
    };

    // Parse the cell address as a single canonical A1 cell (a range is refused).
    let rect = match parse_viewport(&cell_addr) {
        Ok(r) => r,
        Err(msg) => return bad_arg(fmt, &msg),
    };
    if rect.min_col != rect.max_col || rect.min_row != rect.max_row {
        let msg = format!("--cell takes a single cell like C3, not a range ({cell_addr:?})");
        return bad_arg(fmt, &msg);
    }

    let dir = if dependents {
        Direction::Downstream
    } else {
        Direction::Upstream
    };

    match wb.trace(sheet, rect.min_col, rect.min_row, dir, depth) {
        Ok(node) => {
            emit_trace(fmt, &node);
            0
        }
        Err(diag) => emit_validation_diagnostics(fmt, std::slice::from_ref(&diag)),
    }
}

/// `charlie-cli sample <dir>` — write the model's canonical tutorial workbook
/// (`charlie_model::sample_workbook`) into `<dir>`, creating a sub-folder per tab, then emit a terse
/// result. REFUSES (never clobbers) if `<dir>` already exists and is non-empty. This is the one command
/// that WRITES to disk; the sample CONTENT is the model's — the CLI only lays it onto the filesystem.
fn cmd_sample(fmt: Format, rest: &[String]) -> u8 {
    let mut path: Option<String> = None;
    for arg in rest {
        let (flag, _) = split_flag(arg);
        if flag.starts_with('-') {
            return bad_arg(fmt, &format!("unknown flag {flag:?}"));
        }
        if path.replace(arg.clone()).is_some() {
            return bad_arg(fmt, "sample takes exactly one <dir>");
        }
    }
    let Some(path) = path else {
        return bad_arg(
            fmt,
            "sample needs a <dir> to write the tutorial workbook into",
        );
    };
    let dir = Path::new(&path);

    // Never clobber: refuse a `<dir>` that already exists and is non-empty. An empty existing dir, or
    // a not-yet-created one, is fine (the writes create it).
    if dir.exists() {
        let non_empty = match std::fs::read_dir(dir) {
            Ok(mut entries) => entries.next().is_some(),
            Err(e) => {
                let msg = format!("cannot read {path:?}: {e}");
                return fail(fmt, ErrorCode::Io, &msg);
            }
        };
        if non_empty {
            // A runtime-precondition refusal, NOT a usage error: the argv is well-formed, but the
            // target directory's state conflicts with the never-clobber guarantee. That is CONFLICT
            // (exit 4) per `cli-interface-standards.md`, not bad-args (exit 2).
            let msg = format!(
                "{path:?} already exists and is not empty -- refusing to overwrite; pick an empty or new directory"
            );
            return fail(fmt, ErrorCode::Conflict, &msg);
        }
    }

    let content = charlie_model::sample_workbook();
    for (rel, body) in &content {
        let full = dir.join(rel);
        if let Some(parent) = full.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            let msg = format!("cannot create {:?}: {e}", parent.display());
            return fail(fmt, ErrorCode::Io, &msg);
        }
        if let Err(e) = std::fs::write(&full, body) {
            let msg = format!("cannot write {:?}: {e}", full.display());
            return fail(fmt, ErrorCode::Io, &msg);
        }
    }

    // The tab list is the unique first path-component of each written file, in first-seen order.
    let mut tabs: Vec<String> = Vec::new();
    for (rel, _) in &content {
        if let Some(tab) = Path::new(rel).components().next() {
            let name = tab.as_os_str().to_string_lossy().into_owned();
            if !tabs.contains(&name) {
                tabs.push(name);
            }
        }
    }

    // These next-steps strings mirror the model's FIXED sample content (tab names, the `Orders!D5`
    // cell and its `110` value, from `charlie_model::sample_workbook`); if that sample is ever
    // changed, update these hints in lockstep — nothing else pins them together.
    let text_lines = format!(
        "wrote a sample workbook to {path} (tabs: Orders, Summary)\n\
         \n\
         next:\n  \
         charlie-cli render {path}               # draw the Orders tab\n  \
         charlie-cli render {path} --functions   # show the formulas, not their values\n  \
         charlie-cli check  {path}               # lint it (clean)\n  \
         charlie-cli eval   {path} --formula '=Orders!D5'  # evaluate a cell (110)\n  \
         then edit a cell file and re-render\n"
    );
    emit_sample(fmt, &path, &tabs, &text_lines);
    0
}

/// `charlie-cli import <src> <dest-dir>` — convert a real spreadsheet file (`.ods` or `.xlsx`,
/// dispatched by extension) into a charlie workbook the format-blind engine then renders/evaluates.
/// Delegates the whole conversion to `charlie-ingest` (the format firewall); the CLI only parses argv,
/// maps a located `IngestError` onto the envelope's exit code (CORE2), and reports the written tabs.
/// Refuses (never clobbers) a non-empty destination.
fn cmd_import(fmt: Format, rest: &[String]) -> u8 {
    let mut positionals: Vec<String> = Vec::new();
    for arg in rest {
        let (flag, _) = split_flag(arg);
        if flag.starts_with('-') {
            return bad_arg(fmt, &format!("unknown flag {flag:?}"));
        }
        positionals.push(arg.clone());
    }
    let (src, dest) = match positionals.as_slice() {
        [src, dest] => (src.clone(), dest.clone()),
        [_] => {
            return bad_arg(
                fmt,
                "import needs a <dest-workbook-dir> after the <src> (.ods or .xlsx)",
            );
        }
        [] => {
            return bad_arg(
                fmt,
                "import needs a <src> (.ods or .xlsx) and a <dest-workbook-dir>, e.g. charlie-cli import book.xlsx ./book",
            );
        }
        _ => return bad_arg(fmt, "import takes exactly <src> <dest-workbook-dir>"),
    };

    match charlie_ingest::import_file(Path::new(&src), Path::new(&dest)) {
        Ok(report) => {
            let text_lines = format!(
                "imported {src} -> {dest} ({} tab(s), {} range file(s) written)\n\
                 \n\
                 next:\n  \
                 charlie-cli render {dest}          # draw the first tab\n  \
                 charlie-cli check  {dest}          # lint the imported workbook\n",
                report.tabs.len(),
                report.files,
            );
            emit_import(fmt, &dest, &report.tabs, report.files, &text_lines);
            0
        }
        Err(e) => fail(fmt, import_error_code(e.kind), &e.to_string()),
    }
}

/// Map a located [`charlie_ingest::ErrorKind`] onto the CLI's [`ErrorCode`] (and thus its exit code),
/// so an agent branches on `import`'s failure exactly as it does on the other subcommands' — a missing
/// source is `not_found` (24), a non-empty destination is `conflict` (4), a source/dest I/O failure is
/// `internal_error` (1), and an untranslatable/unrepresentable cell is `validation_error` (3).
fn import_error_code(kind: charlie_ingest::ErrorKind) -> ErrorCode {
    use charlie_ingest::ErrorKind;
    match kind {
        ErrorKind::SourceNotFound => ErrorCode::NotFound,
        ErrorKind::DestConflict => ErrorCode::Conflict,
        ErrorKind::SourceIo | ErrorKind::DestIo => ErrorCode::Io,
        ErrorKind::Invalid => ErrorCode::Validation,
    }
}

/// Load a workbook directory, mapping loader failures to the envelope. A load-time refusal carries
/// located diagnostics (a workbook that won't load can't be rendered/evaluated); a missing path or an
/// I/O failure is an operational error. Returns the exit code in `Err`.
fn load(fmt: Format, path: &Path, no_cache: bool) -> Result<Workbook, u8> {
    match Workbook::load_dir(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let msg = format!("no such workbook directory {:?}", path.display());
            Err(fail(fmt, ErrorCode::NotFound, &msg))
        }
        Err(e) => {
            let msg = format!("cannot read {:?}: {e}", path.display());
            Err(fail(fmt, ErrorCode::Io, &msg))
        }
        Ok(Err(diags)) => Err(emit_validation_diagnostics(fmt, &diags)),
        Ok(Ok(mut wb)) => {
            apply_no_cache(&mut wb, no_cache);
            Ok(wb)
        }
    }
}

/// Apply the `--no-cache` bypass: turn the persistent result cache off when the flag is set (the
/// ENG4/ENG7 testing bypass — no `.cache/` reads or writes; values are identical either way, VAL2).
/// Single-homes the flag -> [`Workbook::disable_cache`] step that both [`load`] (render/eval/trace) and
/// [`cmd_check`] (which bypasses `load` because it needs the loader's OWN diagnostics as lint output)
/// must perform identically, so the two sites cannot drift.
fn apply_no_cache(wb: &mut Workbook, no_cache: bool) {
    if no_cache {
        wb.disable_cache();
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

/// Emit an operational error through the envelope layer and return its paired exit code.
fn fail(fmt: Format, code: ErrorCode, message: &str) -> u8 {
    emit_error(fmt, code, message);
    code.exit()
}

/// A bad-args (invalid-usage) refusal — the most common operational error.
fn bad_arg(fmt: Format, msg: &str) -> u8 {
    fail(fmt, ErrorCode::InvalidArguments, msg)
}

/// Print help: per-command when `cmd` names a subcommand (its own args, JSON OUTPUT shape, and exit
/// codes — cli-interface-standards Part 4), else the global banner.
fn print_help(cmd: Option<&str>) {
    let text = match cmd {
        Some("render") => RENDER_HELP,
        Some("check") => CHECK_HELP,
        Some("eval") => EVAL_HELP,
        Some("trace") => TRACE_HELP,
        Some("sample") => SAMPLE_HELP,
        Some("import") => IMPORT_HELP,
        _ => GLOBAL_HELP,
    };
    print!("{text}");
}

const GLOBAL_HELP: &str = r#"charlie-cli — render and lint a filesystem spreadsheet (tabs = folders, cells/ranges = files)

USAGE:
  charlie-cli render <path> [--tab <name>] [--range <A3:G8>] [--values|--functions]
  charlie-cli check  <path>
  charlie-cli eval   <path> --formula '=<formula>' [--tab <name>]
  charlie-cli trace  <path> --cell <A1> [--tab <name>] [--dependents] [--depth <N>]
  charlie-cli sample <dir>
  charlie-cli import <src> <dest-workbook-dir>       # <src> is a .ods or .xlsx file
  charlie-cli --version | --help | --guide

  Per-command help (its own args, JSON OUTPUT shape, and exit codes): charlie-cli <command> --help

GLOBAL:
  --format <text|json>   Output form. Default: text (human ASCII/prose). `json` emits a
                         {status, data|error} envelope on stdout — the machine-parseable surface
                         (errors and located diagnostics are data in the envelope, never scraped).

COMMANDS:
  render   Draw a tab (or a sub-range). Text: an ASCII table with a column-letter header and a
           row-number gutter. JSON: {columns, rows}. Default mode is --values.
  check    Lint the workbook — overlap, dimension-mismatch, and cycle diagnostics. Text: an ASCII
           table pointing at the offending file(s). JSON: a diagnostics[] array. Exits non-zero if
           any error-severity diagnostic.
  eval     Evaluate an ad-hoc --formula against the loaded workbook and emit its value. Read-only.
  trace    Report a cell's upstream dependencies (or downstream consumers with --dependents) as a
           tree; each node carries its value and computation hash. Read-only.
  sample   Write a live tutorial workbook into <dir>, then report. Refuses to overwrite a non-empty
           directory.
  import   Convert a real spreadsheet file (.ods or .xlsx) into a charlie workbook the engine reads.
           Each sheet becomes a tab folder of grid-only range file(s). Refuses a non-empty destination.

EXAMPLES:
  charlie-cli sample ./demo && charlie-cli render ./demo
  charlie-cli import book.xlsx ./book && charlie-cli render ./book
  charlie-cli render ./budget --tab Summary
  charlie-cli check  ./budget --format json
  charlie-cli eval   ./budget --tab Orders --formula '=SUMPRODUCT(--(C2:C11>5))'

OUTPUT (--format json):
  Every command emits a {status, data|error} envelope on stdout:
    success:  {"status":"success","data": <command-specific object>}
    error:    {"status":"error","error":{"code":"...","message":"..."},"data": <payload or null>}
  Per-command data shapes and a worked example: charlie-cli <command> --help.
  --version emits {"status":"success","data":{"name":"charlie-cli","version":"..."}}.

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
"#;

const RENDER_HELP: &str = r#"charlie-cli render — draw a tab (or a sub-range) of a filesystem spreadsheet

USAGE:
  charlie-cli render <path> [--tab <name>] [--range <A3:G8>] [--values|--functions] [--no-cache] [--format <text|json>]

DESCRIPTION:
  Render a workbook tab to a grid. Values mode is demand-driven — only the viewport's dependency cone
  evaluates. Default mode is --values; default tab is the first; default viewport is the tab's used region.

ARGUMENTS:
  <path>            (required) The workbook directory (tabs = sub-folders).
  --tab <name>      (optional) Which tab to render. Default: the first tab.
  --range <A3:G8>   (optional) Only this rectangle (canonical A1). Default: the tab's used region.
  --values          (optional) Computed values (the default mode).
  --functions       (optional) Source text: a formula shows its =… text, a literal shows its value.
  --no-cache        (optional) Bypass the persistent result cache (.cache/) for this run — no reads
                    or writes. Values are identical; only the work to compute them changes.
  --format <fmt>    (optional) text (default, human ASCII table) or json (the machine envelope).

EXAMPLES:
  charlie-cli render ./budget
  charlie-cli render ./budget --tab Summary --range A1:E14 --functions
  charlie-cli render ./budget --format json

OUTPUT (--format json):
  {
    "status": "success",
    "data": { "columns": ["A","B"], "rows": [ { "label": "1", "cells": ["20000","40000"] } ] }
  }

EXIT CODES:
  0   Success (grid drawn)
  1   I/O failure
  2   Invalid arguments (unknown flag, non-canonical or oversized --range)
  3   Validation error (the workbook would not load)
  24  Not found (no such workbook directory, or no such tab)

SEE ALSO:
  charlie-cli check      Lint a workbook
  charlie-cli --guide    Terse guide to the on-disk model
"#;

const CHECK_HELP: &str = r#"charlie-cli check — lint a filesystem spreadsheet

USAGE:
  charlie-cli check <path> [--no-cache] [--format <text|json>]

DESCRIPTION:
  Lint the workbook: overlap, dimension-mismatch, cycle, and the load-time filename refusals.
  Exits non-zero if any error-severity diagnostic fires.

ARGUMENTS:
  <path>            (required) The workbook directory.
  --no-cache        (optional) Bypass the persistent result cache (.cache/) — no reads or writes.
  --format <fmt>    (optional) text (default, ASCII table) or json (the machine envelope).

EXAMPLES:
  charlie-cli check ./budget
  charlie-cli check ./budget --format json

OUTPUT (--format json):
  A clean workbook: {"status":"success","data":{"diagnostics":[]}}
  With findings (exit 3):
  {
    "status": "error",
    "error": { "code": "validation_error", "message": "1 error-severity diagnostic" },
    "data": { "diagnostics": [ {
      "code": "cycle", "severity": "error", "message": "circular reference",
      "help": "break the dependency cycle: ...", "location": { "tab": "Sheet1", "file": "A1" }
    } ] }
  }
  Each diagnostic carries a stable `code`, `severity`, `message`, structured `help` remediation, and `location`.

EXIT CODES:
  0   Success (no error-severity diagnostics)
  1   I/O failure
  2   Invalid arguments
  3   Validation error (an error-severity diagnostic, or the workbook would not load)
  24  Not found (no such workbook directory)

SEE ALSO:
  charlie-cli render     Draw a workbook
  charlie-cli --guide    Terse guide to the on-disk model
"#;

const EVAL_HELP: &str = r##"charlie-cli eval — evaluate an ad-hoc formula against a workbook

USAGE:
  charlie-cli eval <path> --formula '=<formula>' [--tab <name>] [--no-cache] [--format <text|json>]

DESCRIPTION:
  Evaluate a formula against a loaded workbook and emit its value. Read-only — no writes, no mutation.
  Unqualified references (A1, A1:A5) bind to --tab; cross-tab (Tab!A1) and ranges resolve.

ARGUMENTS:
  <path>            (required) The workbook directory.
  --formula '=…'    (required) The formula to evaluate.
  --tab <name>      (optional) Which tab unqualified references resolve against. Default: the first tab.
  --no-cache        (optional) Bypass the persistent result cache (.cache/) — no reads or writes.
  --format <fmt>    (optional) text (default) or json (the machine envelope).

EXAMPLES:
  charlie-cli eval ./budget --formula '=SUM(A1:A5)'
  charlie-cli eval ./budget --tab Orders --formula '=SUMPRODUCT(--(C2:C11>5))'

OUTPUT (--format json):
  Success:            {"status":"success","data":{"value":"6"}}
  Error-valued (3):   {"status":"error","error":{"code":"validation_error",...},"data":{"value":"#DIV/0!"}}
  Parse refusal (3):  {"status":"error",...,"data":{"diagnostics":[ ... located ... ]}}

EXIT CODES:
  0   Success (a value)
  1   I/O failure
  2   Invalid arguments (missing --formula)
  3   Validation error (a parse error, or an error-valued result like #DIV/0!)
  24  Not found (no such workbook directory, or no such tab)

SEE ALSO:
  charlie-cli render     Draw a workbook
  charlie-cli --guide    Terse guide to the on-disk model
"##;

const TRACE_HELP: &str = r#"charlie-cli trace — inspect a cell's dependency tree

USAGE:
  charlie-cli trace <path> --cell <A1> [--tab <name>] [--dependents] [--depth <N>] [--format <text|json>]

DESCRIPTION:
  Report a cell's UPSTREAM dependencies (the cells it reads, transitively) or, with --dependents, its
  DOWNSTREAM consumers (the cells that read it) — the same engine dependency relation, transposed.
  The walk is cycle-safe (a cycle is reported, not looped) and shows a shared cell once (marked
  repeated). Each node carries its value and, unless it lies on a cycle, its computation hash.

ARGUMENTS:
  <path>            (required) The workbook directory.
  --cell <A1>       (required) The cell to trace. May be sheet-qualified (Tab!A1), overriding --tab.
  --tab <name>      (optional) Which tab the cell is on. Default: the first tab.
  --dependents      (optional) Trace downstream consumers instead of upstream dependencies.
  --depth <N>       (optional) Cap the tree depth. Default: unbounded (still bounded by the engine).
  --format <fmt>    (optional) text (default, indented tree) or json (a nested TraceNode envelope).

EXAMPLES:
  charlie-cli trace ./budget --tab Sheet1 --cell D1
  charlie-cli trace ./budget --cell Sheet1!A1 --dependents
  charlie-cli trace ./budget --cell D1 --depth 2 --format json

OUTPUT (--format json):
  {
    "status": "success",
    "data": { "cell": "Sheet1!D1", "formula": "=C1+C3", "value": "4", "status": "ok",
              "hash": "…", "repeated": false, "children": [ … ] }
  }

EXIT CODES:
  0   Success (tree reported)
  1   I/O failure
  2   Invalid arguments (missing/duplicate --cell, a range, or a bad --depth)
  3   Validation error (the workbook would not load, or an out-of-range trace target)
  24  Not found (no such workbook directory, or no such tab)

SEE ALSO:
  charlie-cli eval       Evaluate an ad-hoc formula
  charlie-cli --guide    Terse guide to the on-disk model
"#;

const SAMPLE_HELP: &str = r#"charlie-cli sample — write a live tutorial workbook to disk

USAGE:
  charlie-cli sample <dir> [--format <text|json>]

DESCRIPTION:
  Write the canonical tutorial workbook (two tabs, a header row, per-row formulas, a SUM, and a
  cross-sheet reference) into <dir>. The one command that writes to disk. Refuses to overwrite a
  non-empty directory (never clobbers).

ARGUMENTS:
  <dir>             (required) The directory to write into (created if absent; must be empty if it exists).
  --format <fmt>    (optional) text (default, next-steps prose) or json (the machine envelope).

EXAMPLES:
  charlie-cli sample ./demo && charlie-cli render ./demo

OUTPUT (--format json):
  {"status":"success","data":{"path":"./demo","tabs":["Orders","Summary"]}}

EXIT CODES:
  0   Success (workbook written)
  1   I/O failure
  2   Invalid arguments
  4   Conflict (<dir> exists and is non-empty — refused, nothing written)

SEE ALSO:
  charlie-cli render     Draw the written workbook
  charlie-cli --guide    Terse guide to the on-disk model
"#;

const IMPORT_HELP: &str = r#"charlie-cli import — convert a real spreadsheet file into a charlie workbook

USAGE:
  charlie-cli import <src> <dest-workbook-dir> [--format <text|json>]

DESCRIPTION:
  Convert a real spreadsheet file — an OpenDocument (.ods) or an Excel (.xlsx) workbook, dispatched by
  extension — into a charlie workbook the format-blind engine renders and evaluates. Each sheet becomes
  a tab folder; the sheet's used rectangle becomes one grid-only range file (A1:<lastcol><lastrow>).
  Cell values map to charlie's value model (a date-typed cell becomes its Excel serial); formulas are
  translated to charlie's Excel-A1 grammar (an xlsx formula is already Excel-A1). Refuses to overwrite a
  non-empty destination (never clobbers). Every failure is a located diagnostic (sheet!cell) — an
  untranslatable formula, an unrepresentable value, or an unsupported source format is refused, never
  silently wrong.

ARGUMENTS:
  <src>                  (required) The source spreadsheet to read — a .ods or .xlsx file.
  <dest-workbook-dir>    (required) The workbook directory to write (created; must be empty if it exists).
  --format <fmt>         (optional) text (default, next-steps prose) or json (the machine envelope).

EXAMPLES:
  charlie-cli import book.xlsx ./book && charlie-cli render ./book
  charlie-cli import book.ods  ./book && charlie-cli render ./book

OUTPUT (--format json):
  {"status":"success","data":{"path":"./book","tabs":["Sheet1","Sheet2"],"files":2}}

EXIT CODES:
  0   Success (workbook written)
  1   I/O failure (source unreadable, or a destination write failed)
  2   Invalid arguments
  3   Validation error (an untranslatable formula, unrepresentable value, bad date/sheet name, or an unsupported source format)
  4   Conflict (<dest> exists and is non-empty — refused, nothing written)
  24  Not found (no such source file)

SEE ALSO:
  charlie-cli render     Draw the imported workbook
  charlie-cli --guide    Terse guide to the on-disk model
"#;
