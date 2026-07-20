// Concern: the `charlie-cli tree` subcommand (CLI3) — present a workbook's COMPLETE authored structure (every tab, every cell of every cell/range file, every FS4 name) as a single read-only nested text view: parse `tree <scope> [--values|--functions] [--range A1:B9]` (combined content mode is the default; an explicit range is a tab-scoped viewport shown IN FULL, cap overridden), load the workbook (`Workbook::load_dir`, cache disabled so the view writes nothing), enumerate each tab's files (`Workbook::tab_files`) + the resolved names (`Workbook::name_table`), turn each into an annotated-tree `DirNode`/`FileNode` (a single-cell file → one node; a multi-cell range → one node per A1-ordered coordinate, capped at TREE_CELL_CAP with the remainder as an elided count; a GRID5 array-formula file → ONE node under `--functions`, expanded under `--values`), EXCLUDING `.cache/` (the loader already drops it, FS3), and draw it with `for_format(Format::Text)` to stdout | Non-concern: WHAT a cell computes to or how a name resolves (charlie-model owns render/value spelling via `render`/`eval_formula` and the A1-vs-name classification via `NameTable`/`tab_files` — this never re-parses), the other subcommands (main.rs owns dispatch), and the tree GLYPHS (annotated-tree owns the text renderer) | IO: (a workbook `<scope>` path + a content mode) -> the structure tree on stdout; a located refusal (bad args / not found / load diagnostics) via `output` + its exit code
//! `charlie-cli tree` (CLI3): the workbook's complete structure as a read-only nested text view.
//!
//! The RENDERING MECHANISM lives in `plans/tree-command-plan.md` (the spec states only the need). This
//! module builds annotated-tree's `DirNode`/`FileNode` model BY HAND (its `build` is comment-grammar
//! coupled) and hands it to the text renderer. Every content string and every A1 coordinate label comes
//! from charlie-model's `render` surface, so the value/formula spelling is single-sourced (HARD RULE 2)
//! and the CLI never reaches into a grid or reimplements A1 parsing (SoC / HARD RULE 4).

use std::path::Path;

use annotated_tree::{CodebaseMap, DirNode, FileNode, Format as TreeFormat, for_format};
use charlie_model::{
    FormulaOutcome, MAX_VIEWPORT_CELLS, Name, NameScope, NameTarget, Rect, RenderMode, Workbook,
    combined_cell, parse_viewport, render, viewport_cell_count,
};

use crate::output::{ErrorCode, emit_validation_diagnostics};
use crate::{bad_arg, fail, split_flag, take_value};

/// The per-file coordinate cap: a range file expands to at most this many A1-ordered per-cell nodes,
/// the remainder folded into the tab's elided-count marker so a large range never floods the view
/// (plan: "capped at a bounded N cells ~ 50"). Tunable — a display concern, not a contract. `--full`
/// lifts it to [`u32::MAX`] so the elided-count marker's own "use --full to expand" hint is truthful.
const TREE_CELL_CAP: u32 = 50;

/// `charlie-cli tree <scope> [--values|--functions] [--range <A1:B9>] [--full]` — draw the workbook's
/// complete authored structure.
///
/// `<scope>` is a filesystem path: the workbook directory (the whole workbook) or a `<workbook>/<Tab>`
/// path (rooted at that tab). Content mode mirrors `render`: `Combined` is the DEFAULT (a formula
/// cell/name shows `<value> ← =<formula>`), narrowed by `--functions` (authored source) or `--values`
/// (computed). `--range <A1:B9>` (requires a `<workbook>/<Tab>` scope) shows EXACTLY that viewport's
/// cells, ALL of them, with the per-range cap OVERRIDDEN (an explicit range is shown in full). `--full`
/// lifts the per-range coordinate cap on the whole-structure view so nothing is elided (making the
/// elided-count marker's own "use --full to expand" instruction honest). Read-only (CORE3): the cache
/// is disabled, so the command writes nothing.
pub fn cmd_tree(rest: &[String]) -> u8 {
    let mut path: Option<String> = None;
    let mut modes: Vec<RenderMode> = Vec::new();
    let mut range: Option<String> = None;
    let mut full = false;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = split_flag(arg);
        match flag {
            "--values" => modes.push(RenderMode::Values),
            "--functions" => modes.push(RenderMode::Functions),
            // An explicit viewport: show EXACTLY these cells, uncapped (an agent that asks for a range
            // gets every cell of it). Requires a tab scope so the range names one tab.
            "--range" => match take_value(inline, &mut it) {
                Some(v) => range = Some(v),
                None => return bad_arg("--range needs an A1 range like A1:B9"),
            },
            // Lift the per-range coordinate cap: expand every cell, eliding nothing. The elided-count
            // marker (annotated-tree's own "use --full to expand") thus names a flag that truly exists.
            "--full" => full = true,
            f if f.starts_with('-') => return bad_arg(&format!("unknown flag {f:?}")),
            _ => {
                if path.replace(arg.clone()).is_some() {
                    return bad_arg("tree takes exactly one <scope>");
                }
            }
        }
    }
    if modes.len() > 1 {
        return bad_arg("choose at most one of --values / --functions");
    }
    // `--full` removes the cap entirely (u32::MAX coordinates), so `expand_range` never elides and the
    // borrowed "use --full to expand" hint never contradicts what the tool accepts.
    let cap = if full { u32::MAX } else { TREE_CELL_CAP };
    // DEFAULT `Combined` (`<value> ← =<formula>` per formula cell/name — value AND provenance in one
    // glance), matching `render`'s default; `--values`/`--functions` narrow to a single facet.
    let mode = modes.first().copied().unwrap_or(RenderMode::Combined);

    let Some(path) = path else {
        return bad_arg(
            "tree needs a <scope> — the workbook directory, or <workbook>/<Tab> to root at a tab",
        );
    };

    let (wb, tab_filter) = match resolve_scope(&path) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let map = match range {
        // An explicit range is a tab-scoped viewport shown IN FULL (cap overridden). It needs a tab so
        // the range names one tab's grid — a whole-workbook scope leaves the tab ambiguous.
        Some(r) => {
            let Some(sheet) = tab_filter else {
                return bad_arg(
                    "tree --range needs a tab scope: pass <workbook>/<Tab> so the range names one tab",
                );
            };
            let viewport = match parse_viewport(&r) {
                Ok(rect) => rect,
                Err(msg) => return bad_arg(&msg),
            };
            // Bound the viewport before materializing a node per cell (a syntactically-valid but
            // enormous range would OOM) — the same guard `render` applies (fail-fast, never a crash).
            let cells = viewport_cell_count(viewport);
            if cells > MAX_VIEWPORT_CELLS {
                let msg = format!(
                    "--range spans {cells} cells, over the bound of {MAX_VIEWPORT_CELLS} -- narrow the range"
                );
                return bad_arg(&msg);
            }
            build_range_map(&wb, sheet, viewport, mode)
        }
        None => build_map(&wb, tab_filter, mode, cap),
    };
    println!("{}", for_format(TreeFormat::Text, false).render(&map));
    0
}

/// The `--range` view: EXACTLY the requested viewport's cells on `sheet`, A1-ordered, shown IN FULL —
/// the per-range cap does NOT apply to an explicit range (plan: "if an agent asks for a range, show
/// every cell of it, never elided"). Reuses [`expand_range`] with an unbounded cap ([`u32::MAX`]), so
/// nothing is elided and every coordinate's value/source spelling stays single-sourced through the
/// model's [`render`]. The tab node's own name is not printed (its cells show directly).
fn build_range_map(wb: &Workbook, sheet: u32, viewport: Rect, mode: RenderMode) -> CodebaseMap {
    let name = wb.sheet_names()[sheet as usize].to_string();
    let (cells, _elided) = expand_range(wb, sheet, viewport, mode, u32::MAX);
    let root = dir_node(name, Vec::new(), cells, 0);
    CodebaseMap {
        roots: vec![root],
        warnings: Vec::new(),
    }
}

/// Resolve `<scope>` into a loaded workbook plus an optional tab index to root the view at.
///
/// * The path loads as a workbook WITH tabs → the whole-workbook view (`None`).
/// * Else its PARENT loads as a workbook and the last path component names one of that workbook's tabs
///   → root the view at that tab (a `<workbook>/<Tab>` scope; the tab folder itself does not load as a
///   workbook, since its cell files are files, not sub-tabs).
/// * Else the path's own load outcome surfaces its genuine refusal (not-found / load diagnostics), or a
///   valid-but-empty workbook shows an empty whole-workbook view.
///
/// The path is parsed exactly ONCE — the first `load_dir(p)` result is reused for the whole-workbook
/// reading AND the fall-through refusal, never re-loaded (a `<workbook>/<Tab>` scope additionally loads
/// the PARENT workbook once, which is unavoidable). The cache is disabled on every returned workbook,
/// so the read-only `tree` writes nothing (CORE3).
fn resolve_scope(scope: &str) -> Result<(Workbook, Option<u32>), u8> {
    let p = Path::new(scope);
    // Parse `p` once; its outcome drives every interpretation below.
    let loaded = Workbook::load_dir(p);

    // Whole-workbook: the path itself loaded and has at least one tab.
    if matches!(&loaded, Ok(Ok(wb)) if !wb.sheet_names().is_empty()) {
        let mut wb = loaded.expect("guarded Ok").expect("guarded Ok");
        wb.disable_cache();
        return Ok((wb, None));
    }

    // `<workbook>/<Tab>`: the parent loads and the basename names a tab of it. (Preferred over an
    // empty-but-valid `p` load, so a tab folder is read as a tab scope, not an empty workbook.)
    if let Some(base) = p.file_name().map(|b| b.to_string_lossy().into_owned()) {
        let parent = match p.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        };
        if let Ok(Ok(mut wb)) = Workbook::load_dir(parent)
            && let Some(idx) = wb.tab_index(&base)
        {
            wb.disable_cache();
            return Ok((wb, Some(idx)));
        }
    }

    // Neither interpretation rooted at a tab — surface `p`'s OWN first-parse outcome (never re-loaded):
    // a valid-but-empty workbook shows an empty whole-workbook view; an error carries its located
    // diagnostics (a load-time refusal) or its operational exit code (missing path / I/O failure).
    match loaded {
        Ok(Ok(mut wb)) => {
            wb.disable_cache();
            Ok((wb, None))
        }
        Ok(Err(diags)) => Err(emit_validation_diagnostics(&diags)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let msg = format!("no such workbook directory {:?}", p.display());
            Err(fail(ErrorCode::NotFound, &msg))
        }
        Err(e) => {
            let msg = format!("cannot read {:?}: {e}", p.display());
            Err(fail(ErrorCode::Io, &msg))
        }
    }
}

/// Build the annotated-tree model for the requested view: the whole workbook (each tab a sub-directory,
/// workbook-scoped names as files), or a single tab rooted directly. The root node's own name is not
/// printed by the text renderer — its contents show directly (matching `tree`'s default).
fn build_map(wb: &Workbook, tab_filter: Option<u32>, mode: RenderMode, cap: u32) -> CodebaseMap {
    let root = match tab_filter {
        Some(idx) => tab_dirnode(wb, idx, mode, cap),
        None => workbook_dirnode(wb, mode, cap),
    };
    CodebaseMap {
        roots: vec![root],
        warnings: Vec::new(),
    }
}

/// The whole-workbook root: one sub-directory per tab, then the workbook-scoped FS4 names as files.
fn workbook_dirnode(wb: &Workbook, mode: RenderMode, cap: u32) -> DirNode {
    let dirs = (0..wb.sheet_names().len() as u32)
        .map(|idx| tab_dirnode(wb, idx, mode, cap))
        .collect();
    let files = name_nodes(wb, mode, |scope| matches!(scope, NameScope::Workbook));
    dir_node(String::new(), dirs, files, 0)
}

/// One tab's node: every authored cell (single-cell files as one node, range files expanded per A1
/// coordinate and capped at `cap`), then the tab's sheet-scoped names.
fn tab_dirnode(wb: &Workbook, sheet: u32, mode: RenderMode, cap: u32) -> DirNode {
    let name = wb.sheet_names()[sheet as usize].to_string();
    let (mut files, elided) = cell_nodes(wb, sheet, mode, cap);
    let scope_name = name.clone();
    files.extend(name_nodes(
        wb,
        mode,
        |scope| matches!(scope, NameScope::Sheet(s) if *s == scope_name),
    ));
    dir_node(name, Vec::new(), files, elided)
}

/// Every authored cell of `sheet` as file nodes, plus the count of coordinates elided by the per-file
/// cap. A GRID5 array-formula file under `--functions` is ONE node (its formula at the anchor); every
/// other file expands per A1 coordinate (a single-cell file is the 1-coordinate case).
fn cell_nodes(wb: &Workbook, sheet: u32, mode: RenderMode, cap: u32) -> (Vec<FileNode>, u32) {
    let mut nodes = Vec::new();
    let mut elided: u32 = 0;
    for fe in wb.tab_files(sheet).unwrap_or_default() {
        if fe.array_formula && mode == RenderMode::Functions {
            // One node: the array formula lives once, at the region's top-left anchor (VAL1).
            let grid = render(wb, sheet, fe.region, mode);
            let text = grid.rows[0].cells[0].clone();
            nodes.push(file_node(fe.name.to_string(), text));
        } else {
            let (cells, more) = expand_range(wb, sheet, fe.region, mode, cap);
            nodes.extend(cells);
            elided = elided.saturating_add(more);
        }
    }
    (nodes, elided)
}

/// Expand a file's region into per-coordinate nodes, A1-ordered (row then column), capped at `cap`
/// (`--full` passes [`u32::MAX`] to expand everything); returns the nodes and the count of coordinates
/// beyond the cap. Both the coordinate LABEL (a column letter + a row number) and the cell TEXT come
/// from the model's [`render`] surface, so a GRID5 continuation cell, a load-error cell, and
/// value/formula spelling are all single-sourced. Only a bounded sub-rectangle (enough rows to cover
/// the cap) is rendered, so a huge range never materializes a huge grid just to show its first cells.
fn expand_range(
    wb: &Workbook,
    sheet: u32,
    region: Rect,
    mode: RenderMode,
    cap: u32,
) -> (Vec<FileNode>, u32) {
    let cols = u64::from(region.max_col - region.min_col) + 1;
    let rows = u64::from(region.max_row - region.min_row) + 1;
    let total = rows * cols;
    // Render only the first rows_to_render rows — enough to cover `cap` coordinates.
    let rows_to_render = if total <= u64::from(cap) {
        rows
    } else {
        u64::from(cap).div_ceil(cols).min(rows)
    };
    let sub = Rect {
        min_col: region.min_col,
        min_row: region.min_row,
        max_col: region.max_col,
        max_row: region.min_row + (rows_to_render as u32) - 1,
    };
    let grid = render(wb, sheet, sub, mode);

    let mut nodes = Vec::new();
    let mut count: u32 = 0;
    'rows: for row in &grid.rows {
        for (ci, cell) in row.cells.iter().enumerate() {
            if count >= cap {
                break 'rows;
            }
            let label = format!("{}{}", grid.col_labels[ci], row.row_label);
            nodes.push(file_node(label, cell.clone()));
            count += 1;
        }
    }
    let elided = (total - u64::from(count)).min(u64::from(u32::MAX)) as u32;
    (nodes, elided)
}

/// The FS4 name entries whose scope satisfies `want`, each as a file node showing what it resolves to
/// (FS4): a symlinked cell/range name → its target A1 reference (both modes); a named formula/constant
/// → its authored definition (`--functions`) or its computed value (`--values`, via the model's
/// [`Workbook::eval_formula`] against the name's scope sheet). The parsing/resolution authority stays
/// in the model — this only spells the already-resolved target (HARD RULE 4).
fn name_nodes(wb: &Workbook, mode: RenderMode, want: impl Fn(&NameScope) -> bool) -> Vec<FileNode> {
    wb.name_table()
        .names()
        .iter()
        .filter(|n| want(&n.scope))
        .map(|n| file_node(n.ident.clone(), name_text(wb, n, mode)))
        .collect()
}

/// The annotation text for one resolved name under `mode`. A symlinked (ref) name shows its target A1
/// reference in EVERY mode; a formula/constant name shows its authored definition (`--functions`), its
/// computed value (`--values`), or `<value> ← =<expr>` (combined, the default) — the combined spelling
/// composed through the SAME [`combined_cell`] the grid cells use (HARD RULE 4).
fn name_text(wb: &Workbook, name: &Name, mode: RenderMode) -> String {
    match &name.target {
        // A symlinked name always shows its target A1 reference (its resolution IS that reference).
        NameTarget::Ref(a1) => format!("→ {a1}"),
        NameTarget::Expr(expr) => {
            let source = format!("={expr}");
            match mode {
                RenderMode::Functions => source,
                RenderMode::Values => name_value(wb, name, expr),
                RenderMode::Combined => combined_cell(&name_value(wb, name, expr), &source),
            }
        }
    }
}

/// The computed value of a named formula/constant's definition, evaluated against the name's scope sheet
/// (workbook-scoped → the first tab), so it shows the same value a formula referencing the name would
/// see. A definition that will not even parse falls back to its authored `=<expr>` text (never a panic;
/// the fault surfaces in `check`, not here).
fn name_value(wb: &Workbook, name: &Name, expr: &str) -> String {
    let sheet = match &name.scope {
        NameScope::Sheet(s) => wb.tab_index(s).unwrap_or(0),
        NameScope::Workbook => 0,
    };
    match wb.eval_formula(sheet, &format!("=({expr})")) {
        Ok(FormulaOutcome::Value(s) | FormulaOutcome::Error(s)) => s,
        Err(_) => format!("={expr}"),
    }
}

/// A leaf file node carrying `text` as its annotation (an empty string → no annotation, so a blank
/// authored cell renders as just its coordinate). The token/age/symbol fields are all unused here.
fn file_node(name: String, text: String) -> FileNode {
    FileNode {
        name,
        annotation: (!text.is_empty()).then_some(text),
        age_secs: None,
        tokens: None,
        symbols: Vec::new(),
    }
}

/// A directory node with `dirs`/`files` and an elided-file count (the per-node overflow marker). The
/// charter/deps/tokens/elided-dirs fields are unused by this hand-built structure view.
fn dir_node(name: String, dirs: Vec<DirNode>, files: Vec<FileNode>, elided_files: u32) -> DirNode {
    DirNode {
        name,
        charter: None,
        deps: None,
        dirs,
        files,
        tokens: None,
        elided_dirs: 0,
        elided_files,
    }
}
