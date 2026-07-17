// Concern: the DEMAND-DRIVEN evaluation engine — load a sheet-directory (tabs=folders, files=closed ranges) into an in-memory `Workbook` via `parse_file`, then implement `charlie_ast::Resolver` OVER that model so a requested cell is resolved by finding the file that covers it, reading its grid cell at the local offset, and — when that cell is a `=formula` — EVALUATING the parsed `Expr` through `charlie_ast::eval` with THIS workbook as the resolver (the PULL); a cell's value derives only from its own content (VAL1), so there is no drag-fill/offset anywhere; results are memoized PER CELL by resolved `(sheet,col,row)` so a diamond/deep DAG evaluates linearly and never exponentially, a currently-evaluating set turns a reference cycle into a located `#REF!`-class refusal (never a hang/overflow), and only cells transitively requested ever compute (the effectively-infinite-sheet property); an AD-HOC `=formula` string is also evaluated against this loaded workbook via `eval_formula` (the `charlie-cli eval` entry) — parsed through `charlie_ast`, evaluated with THIS workbook as the resolver (unqualified refs resolving against a caller-named tab), and returned as a `FormulaOutcome` (a clean value vs a spreadsheet error value, each already `display_value`-spelled) so the CLI sets its exit code without depending on `charlie-ast` | Non-concern: the formula LANGUAGE (charlie-ast owns lex/parse/eval), the filename/grid/overlap GRAMMAR (this reuses `parse_file`/`detect_overlaps`), xlsx serde, and the CLI render surface | IO: (a sheet-directory or in-memory tabs) -> a `Workbook`; then (a `CellRef`/`RangeRef`, pulling formulas) -> a `Value`/`ArrayView`, or (a tab index + an ad-hoc `=formula` string) -> a `Result<FormulaOutcome, Diagnostic>`, plus the eval-time located `Diagnostic`s it accumulates
//! Demand-driven evaluation: [`Workbook`] loads a sheet-directory and implements [`Resolver`] over
//! it, so `charlie_ast::eval` pulls cell values through the model (the "swap the impl, the engine is
//! unchanged" firewall made live over a real store). Memoized, cycle-safe, and lazy: an off-request
//! cell never computes. Beyond the stored-cell pull, [`Workbook::eval_formula`] evaluates an ad-hoc
//! `=formula` string against the loaded workbook (the `charlie-cli eval` entry), returning a
//! [`FormulaOutcome`] that distinguishes a clean value from a spreadsheet error value.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use charlie_ast::{
    ArrayView, CellRef, ErrKind, RangeRef, Resolver, Shape, SheetId, Value, eval, parse,
    system_now_secs, unix_secs_to_serial,
};

use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::grid::{Cell as GridCell, Grid};
use crate::overlap::{Rect, detect_overlaps};
use crate::{ParsedFile, parse_file};

/// The maximum cross-cell formula-pull depth before the model refuses. Each link in a `=formula`
/// dependency chain (`A1=A2`, `A2=A3`, ...) resolves by *native recursion* (`value -> eval_formula
/// -> charlie_ast::eval -> value`), and every formula gets a fresh [`charlie_ast`] `EvalCtx`, so the
/// engine's own per-tree depth bound never spans cells — a deep-but-acyclic chain would otherwise
/// grow the native stack one frame-group per link and abort the process on overflow.
///
/// This is the model-level counterpart to that engine bound (`ast-standards` PART 9, "every later
/// walk is stack-safe"): a chain deeper than this is a located `#NUM!`-class refusal, never a crash.
/// The value is deliberately conservative — chosen to be safe on the *smallest* stack any of our
/// entry points runs on (the test harness's ~2 MiB worker threads, where a debug build overflows
/// only well above this), not to match a spreadsheet engine's much larger practical limit. A serial
/// dependency chain this long is already pathological; the contract is only that it refuses cleanly.
///
/// Past this bound, whether a deep acyclic chain's top cell *refuses* or *computes* is deliberately
/// left order-sensitive: if an intermediate link was already pulled (and depth-clean-memoized) at a
/// shallower depth, a later deep pull short-circuits on that memo and computes fully rather than
/// tripping the guard. This is never a *wrong value* — the memo only ever caches true, depth-clean
/// values, so returning one is always correct. The dangerous direction (a depth-tainted `#NUM!`
/// poisoning a later shallower, computable pull) is what the `depth_refusals` snapshot guard prevents
/// (see `a_depth_refused_pull_does_not_poison_a_later_shallower_pull`).
const MAX_PULL_DEPTH: u32 = 256;

/// The largest rectangular range (in cells) [`Resolver::range`] will MATERIALIZE before it refuses.
/// A `=formula` may reference a syntactically-valid but pathologically-large rectangle
/// (`=SUM(A2:ZZ100000)` — ~70M cells); materializing a `Value` for every one of them retains an
/// arena buffer for the workbook lifetime and drives the process into an OOM abort. Bounding the
/// materialized area turns that into a located `#NUM!`-class [`Code::RangeTooLarge`] refusal — the
/// range resolves to a single error cell that the referencing aggregation propagates — so no valid
/// invocation can crash the process on allocation. The bound is far above any real sheet's used
/// range (a 1000-column × 1000-row block); only a pathological reference reaches it.
const MAX_RANGE_CELLS: u64 = 1_000_000;

/// One loaded file: its name and claimed region plus its deserialized [`Grid`] (each coordinate a
/// literal value or a parsed formula). A formula cell's value is computed at eval and cached in
/// [`Workbook::memo`], not here.
#[derive(Clone, Debug)]
struct LoadedFile {
    name: String,
    region: Rect,
    grid: Grid,
    /// The file's line-1 `# ` annotation, verbatim (the `# ` prefix included). Preserved at load so
    /// the render surface can show each range's annotation without re-reading the file.
    annotation: String,
}

/// One tab (folder): its sheet name and the files that partition its used region.
#[derive(Clone, Debug)]
struct Tab {
    name: String,
    files: Vec<LoadedFile>,
}

/// The identity of a formula file within the workbook: `(sheet index, file index within the tab)`.
/// The file-level anchor for an eval-time refusal is keyed by this.
type FileId = (u32, usize);

/// The identity of one rendered cell: `(sheet index, zero-based col, zero-based row)`. The per-cell
/// memo and the currently-evaluating (cycle-detection) set are keyed by this — each grid cell is a
/// DISTINCT computation, so the cache and the cycle guard must be per cell, not per file.
type CellKey = (u32, u32, u32);

/// An in-memory charlie workbook that evaluates on demand.
///
/// Load with [`Workbook::from_tabs`] (in-memory) or [`Workbook::load_dir`] (a filesystem tree), then
/// drive evaluation by requesting cells ([`Workbook::value_at`]) or by handing `&Workbook` to
/// [`charlie_ast::eval`] as a [`Resolver`]. Evaluation is **demand-driven** (only requested cells,
/// transitively, compute), **memoized per cell** (each rendered cell computes at most once, so a
/// diamond / deep DAG stays linear and never re-evaluates exponentially), and **cycle-safe** (a
/// reference cycle is a located `#REF!`-class refusal, never a hang). Each grid cell derives its value
/// only from its own content (VAL1) — a range file is an explicit grid, never a drag-filled formula.
#[derive(Debug)]
pub struct Workbook {
    tabs: Vec<Tab>,
    /// The "now" instant [`Resolver::now_serial`] reports. Defaults to the wall clock at load; a test
    /// pins it with [`Workbook::with_now`]. (Production gets wall-clock time for free.)
    now: f64,
    /// The sheet an unqualified reference (`sheet: None`) resolves against — the home sheet of the
    /// formula currently being evaluated. Saved/restored around each formula eval so a nested
    /// cross-sheet pull sees the right context. `Cell` because it is a plain copyable scalar.
    current_sheet: Cell<u32>,
    /// Per-CELL result cache (the memo): each rendered cell computes at most once, keyed by its
    /// resolved `(sheet, col, row)`. Per-cell (not per-file) is what simultaneously makes an EXPLICIT
    /// GRID correct — each cell of a range file is a distinct computation (VAL1) — AND makes a diamond
    /// / deep DAG evaluate LINEARLY: a cell reached through many reference paths computes once and is thereafter
    /// read from here, so re-evaluation can never grow exponentially (the anti-hang guarantee).
    memo: RefCell<HashMap<CellKey, Value>>,
    /// The cells whose evaluation is in progress — the cycle detector. Re-entering a cell (a direct or
    /// transitive self-reference) is a located `#REF!`-class refusal returned at once, never a hang.
    visiting: RefCell<HashSet<CellKey>>,
    /// The live cross-cell pull depth — how many nested formula files are currently on the native
    /// stack. Bounds a finite-but-deep (acyclic) chain to [`MAX_PULL_DEPTH`] so it refuses rather
    /// than overflowing. `Cell` because it is a plain copyable scalar (like `current_sheet`).
    pull_depth: Cell<u32>,
    /// A monotone count of depth-limit refusals raised so far. Snapshotted around each formula's
    /// evaluation so a formula whose subtree tripped the depth guard is recognised as
    /// *depth-tainted* and its (`#NUM!`-carrying) outcome is NOT memoized — the refusal is a
    /// property of the DEPTH the chain was reached at, not of the cell, so a later shallower pull of
    /// the same cell (which is legally computable) must not read a poisoned cache entry.
    depth_refusals: Cell<u64>,
    /// The formula file currently being evaluated, if any — the anchor an eval-time refusal raised
    /// from *inside* eval (e.g. a range-too-large refusal in [`Resolver::range`]) points at. Saved
    /// and restored around each formula eval, like [`Workbook::current_sheet`].
    current_file: Cell<Option<FileId>>,
    /// An append-only arena backing the borrowed [`ArrayView`]s that [`Resolver::range`] returns.
    arena: Arena,
    /// Located refusals surfaced during evaluation (cycles, spills, unparseable bodies). Load-time
    /// refusals are returned by the loader; these accumulate as cells are pulled.
    diagnostics: RefCell<Vec<Diagnostic>>,
}

/// The outcome of [`Workbook::eval_formula`]: a successfully evaluated value or a spreadsheet error
/// value, each already spelled with the render surface's [`display_value`](crate::render::display_value)
/// formatting. The variant lets a caller (the `charlie-cli eval` CLI) set its exit code — a `Value` is
/// clean (exit 0), an `Error` value is a non-zero validation outcome — without re-inspecting the
/// `charlie_ast::Value` (keeping the CLI firewall: `charlie-cli` never depends on `charlie-ast`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormulaOutcome {
    /// A non-error value; the string is its render-formatted display.
    Value(String),
    /// A spreadsheet error value (`#DIV/0!`, `#REF!`, `#VALUE!`, …); the string is its canonical text.
    Error(String),
}

/// A read-only view of the single file that covers a requested cell — the un-evaluated source the
/// render surface shows in `--functions`/`--annotation` mode. Borrows the workbook, so it cannot
/// outlive it.
#[derive(Clone, Copy, Debug)]
pub struct CellSource<'a> {
    /// The covering file's name (`A1`, `A3:G8`).
    pub file_name: &'a str,
    /// The declared region the file claims.
    pub region: Rect,
    /// The file's verbatim line-1 `# ` annotation.
    pub annotation: &'a str,
    /// The specific grid cell at the requested coordinate — a parsed `=formula` (with its source
    /// text) or a literal value (un-evaluated).
    pub cell: &'a GridCell,
}

impl Workbook {
    /// Load an in-memory workbook: `tabs` is `(tab name, [(filename, contents)])`. Each file is run
    /// through the W2 [`parse_file`]; per-tab overlaps are detected. Returns every load-time located
    /// refusal (a bad filename/body, a non-conforming literal, an overlap) if any, else the workbook.
    pub fn from_tabs(tabs: &[(&str, &[(&str, &str)])]) -> Result<Workbook, Vec<Diagnostic>> {
        let owned: Vec<(String, Vec<(String, String)>)> = tabs
            .iter()
            .map(|(t, files)| {
                (
                    (*t).to_string(),
                    files
                        .iter()
                        .map(|(n, c)| ((*n).to_string(), (*c).to_string()))
                        .collect(),
                )
            })
            .collect();
        Workbook::from_owned(owned)
    }

    /// Load a workbook from a filesystem directory: each immediate sub-folder is a tab, and each file
    /// in it is a cell/range file. Reads the tree into memory, then delegates to the same loader as
    /// [`Workbook::from_tabs`]. The outer [`std::io::Result`] reports a filesystem read failure; the
    /// inner `Result` reports the workbook's own load-time refusals (kept separate — an unreadable
    /// directory is not a spreadsheet diagnostic).
    pub fn load_dir(root: &Path) -> std::io::Result<Result<Workbook, Vec<Diagnostic>>> {
        let mut tabs: Vec<(String, Vec<(String, String)>)> = Vec::new();
        let mut entries: Vec<_> = std::fs::read_dir(root)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let tab_name = entry.file_name().to_string_lossy().into_owned();
            let mut files: Vec<(String, String)> = Vec::new();
            let mut file_entries: Vec<_> =
                std::fs::read_dir(entry.path())?.collect::<Result<_, _>>()?;
            file_entries.sort_by_key(|e| e.file_name());
            for f in file_entries {
                if !f.file_type()?.is_file() {
                    continue;
                }
                let name = f.file_name().to_string_lossy().into_owned();
                let contents = std::fs::read_to_string(f.path())?;
                files.push((name, contents));
            }
            tabs.push((tab_name, files));
        }
        Ok(Workbook::from_owned(tabs))
    }

    /// The shared loader over owned strings (so the fs and in-memory paths converge here).
    fn from_owned(tabs: Vec<(String, Vec<(String, String)>)>) -> Result<Workbook, Vec<Diagnostic>> {
        let mut out_tabs = Vec::with_capacity(tabs.len());
        let mut diags = Vec::new();
        for (tab_name, files) in tabs {
            let mut loaded = Vec::new();
            let mut regions: Vec<(String, Rect)> = Vec::new();
            for (fname, contents) in files {
                match parse_file(&fname, &contents) {
                    Ok(ParsedFile {
                        region,
                        declared_shape: _,
                        grid,
                    }) => {
                        // Line 1 is the mandatory `# ` annotation (parse_file verified it); preserve
                        // it verbatim for the render `--annotation` mode. `split_once` mirrors the
                        // loader's own line-1 split.
                        let annotation = contents
                            .split_once('\n')
                            .map_or(contents.as_str(), |(a, _)| a)
                            .to_string();
                        regions.push((fname.clone(), region));
                        loaded.push(LoadedFile {
                            name: fname,
                            region,
                            grid,
                            annotation,
                        });
                    }
                    Err(d) => diags.push(d),
                }
            }
            diags.extend(detect_overlaps(&tab_name, &regions));
            out_tabs.push(Tab {
                name: tab_name,
                files: loaded,
            });
        }
        if diags.is_empty() {
            Ok(Workbook {
                tabs: out_tabs,
                now: system_now_serial(),
                current_sheet: Cell::new(0),
                memo: RefCell::new(HashMap::new()),
                visiting: RefCell::new(HashSet::new()),
                pull_depth: Cell::new(0),
                depth_refusals: Cell::new(0),
                current_file: Cell::new(None),
                arena: Arena::default(),
                diagnostics: RefCell::new(Vec::new()),
            })
        } else {
            Err(diags)
        }
    }

    /// Pin the [`Resolver::now_serial`] clock (for deterministic `TODAY()`/`NOW()` in tests).
    pub fn with_now(mut self, serial: f64) -> Workbook {
        self.now = serial;
        self
    }

    /// The sheet names, in tab order (index == [`SheetId`]).
    pub fn sheet_names(&self) -> Vec<&str> {
        self.tabs.iter().map(|t| t.name.as_str()).collect()
    }

    /// Resolve one cell to its value — the demand-driven entry a consumer calls. `sheet` is the
    /// tab index; `col`/`row` are zero-based. Pulls (and memoizes) any formulas it transitively needs.
    pub fn value_at(&self, sheet: u32, col: u32, row: u32) -> Value {
        self.value(CellRef {
            col,
            row,
            sheet: Some(SheetId(sheet)),
        })
    }

    /// The located refusals accumulated during evaluation so far (cycles, spills, unparseable bodies).
    /// Snapshot — call after driving the cells of interest.
    pub fn eval_diagnostics(&self) -> Vec<Diagnostic> {
        self.diagnostics.borrow().clone()
    }

    /// Evaluate an AD-HOC formula string against this loaded workbook — the `charlie-cli eval` entry.
    /// Parses `formula` through `charlie_ast`, then evaluates it with THIS workbook as the
    /// [`Resolver`], so the formula can reference cells, ranges, and other tabs exactly as a stored
    /// formula would. Unqualified references (`A1`, `A1:A5`) resolve against `sheet` (the tab index).
    /// Read-only: no file writes, no cell mutation — it reuses the same memoized pull path stored
    /// formulas ride, restoring the sheet context afterward.
    ///
    /// The result is spelled with the SAME [`display_value`](crate::render::display_value) formatting
    /// the render surface uses. A parse failure is a located [`Diagnostic`] (`Err`); an evaluation
    /// that yields a spreadsheet **error value** (`#DIV/0!`, `#REF!`, …) is [`FormulaOutcome::Error`]
    /// so a caller can exit non-zero, while any other value is [`FormulaOutcome::Value`].
    pub fn eval_formula(&self, sheet: u32, formula: &str) -> Result<FormulaOutcome, Diagnostic> {
        let expr = parse(formula).map_err(|diag| {
            Diagnostic::new(
                Code::FormulaSyntax,
                // Locate on the formula text at the refusal's 1-based byte column (a formula is a
                // single line; line 1 mirrors the body-line convention).
                Loc::body(formula, 1, (diag.span.start as u32) + 1),
                format!("cannot parse formula {formula:?}: {}", diag.message),
            )
        })?;
        // Resolve unqualified refs against the requested tab: save/restore `current_sheet` exactly as
        // the stored-formula pull path does, so this ad-hoc eval never leaves the context dirty.
        let prev = self.current_sheet.replace(sheet);
        let value = eval(&expr, self);
        self.current_sheet.set(prev);
        let shown = crate::render::display_value(&value);
        Ok(match value {
            Value::Error(_) => FormulaOutcome::Error(shown),
            _ => FormulaOutcome::Value(shown),
        })
    }

    /// The tab index for a sheet name, or `None` if no tab has that name. The index is the
    /// [`SheetId`] the resolver uses; tab order is load order (sorted on the fs path).
    pub fn tab_index(&self, name: &str) -> Option<u32> {
        self.tabs
            .iter()
            .position(|t| t.name == name)
            .map(|i| i as u32)
    }

    /// The bounding rectangle of every file's declared region on `sheet` — the natural default
    /// viewport for rendering the whole tab. `None` for an out-of-range sheet or an empty tab.
    pub fn used_region(&self, sheet: u32) -> Option<Rect> {
        let tab = self.tabs.get(sheet as usize)?;
        let mut files = tab.files.iter();
        let first = files.next()?.region;
        Some(files.fold(first, |acc, f| Rect {
            min_col: acc.min_col.min(f.region.min_col),
            min_row: acc.min_row.min(f.region.min_row),
            max_col: acc.max_col.max(f.region.max_col),
            max_row: acc.max_row.max(f.region.max_row),
        }))
    }

    /// A read-only view of the file that covers `(col,row)` on `sheet` — its name, declared region,
    /// line-1 annotation, and (un-evaluated) body — for the render `--functions`/`--annotation`
    /// surface. `None` for a gap (no file claims the cell). Overlaps are rejected at load, so at
    /// most one file covers a cell.
    pub fn source_at(&self, sheet: u32, col: u32, row: u32) -> Option<CellSource<'_>> {
        let (_, file) = self.covering(sheet, col, row)?;
        let dr = row - file.region.min_row;
        let dc = col - file.region.min_col;
        Some(CellSource {
            file_name: &file.name,
            region: file.region,
            annotation: &file.annotation,
            cell: file.grid.cell_at(dr, dc),
        })
    }

    /// Lint the whole workbook: drive every cell of every file (so every formula evaluates once,
    /// memoized) and return the eval-time located refusals — cycles, over-deep chains (`#NUM!`-class),
    /// formula-result dimension mismatches (`#SPILL!`-class), and unparseable formula bodies.
    /// Load-time refusals (overlap, literal dimension mismatch, bad filenames) surface from the
    /// loader, not here.
    pub fn lint(&self) -> Vec<Diagnostic> {
        // Snapshot the regions first so no `&self.tabs` borrow is held across the `value` pulls.
        let regions: Vec<(u32, Rect)> = self
            .tabs
            .iter()
            .enumerate()
            .flat_map(|(s, t)| t.files.iter().map(move |f| (s as u32, f.region)))
            .collect();
        for (sheet, region) in regions {
            for row in region.min_row..=region.max_row {
                for col in region.min_col..=region.max_col {
                    let _ = self.value_at(sheet, col, row);
                }
            }
        }
        // Each formula file records its refusal at most once during a full drive (memoization
        // returns the cached outcome before re-refusing on a repeat pull), so consecutive-duplicate
        // removal only guards the rare case where two adjacent drives surface the identical located
        // refusal; it is a cheap tidy, not a cross-file de-duplicator.
        let mut diags = self.eval_diagnostics();
        diags.dedup();
        diags
    }

    /// Record one eval-time refusal.
    fn refuse(&self, diag: Diagnostic) {
        self.diagnostics.borrow_mut().push(diag);
    }

    /// The tab a `sheet: None` reference resolves against, or the explicit sheet index.
    fn resolve_sheet(&self, sheet: Option<SheetId>) -> u32 {
        sheet.map_or_else(|| self.current_sheet.get(), |SheetId(i)| i)
    }

    /// Find the single file whose region covers `(col,row)` on `sheet` (overlaps are rejected at
    /// load, so at most one does), returning its [`FileId`] and a borrow of it.
    fn covering(&self, sheet: u32, col: u32, row: u32) -> Option<(FileId, &LoadedFile)> {
        let tab = self.tabs.get(sheet as usize)?;
        tab.files
            .iter()
            .enumerate()
            .find(|(_, f)| f.region.contains(col, row))
            .map(|(i, f)| ((sheet, i), f))
    }

    /// Resolve one formula-backed cell to its value — the per-cell PULL, memoized and cycle-safe.
    ///
    /// `key` is the resolved `(sheet, col, row)`; `id` its covering file; `(dr, dc)` the cell's local
    /// offset into that file's grid. The value is the grid cell's parsed formula evaluated as-written
    /// (VAL1: no offset/drag-fill), collapsed to a scalar for its scalar cell position.
    ///
    /// Cycle-safe: `key` is inserted into [`Workbook::visiting`] for the duration of its own
    /// evaluation; re-entering it (a direct or transitive self-reference) is a located `#REF!`-class
    /// refusal returned immediately — the recursion always terminates because a cell is never
    /// re-entered while on the stack. **Memoized per cell**, so a diamond / deep DAG computes each cell
    /// once and can never re-evaluate exponentially (the anti-hang guarantee).
    ///
    /// Depth-safe: a finite but deep *acyclic* chain (`A1=A2`, `A2=A3`, ...) recurses one native
    /// frame-group per link with a fresh `EvalCtx`, so the engine's own depth bound never spans cells.
    /// [`Workbook::pull_depth`] bounds that native recursion: a chain deeper than [`MAX_PULL_DEPTH`]
    /// is a located `#NUM!`-class refusal returned before descending further, never a stack overflow.
    fn formula_value(&self, key: CellKey, id: FileId, dr: u32, dc: u32) -> Value {
        if let Some(hit) = self.memo.borrow().get(&key) {
            return hit.clone();
        }
        if self.visiting.borrow().contains(&key) {
            let name = self.file_name(id);
            let tab = self.tab_name(id.0);
            self.refuse(Diagnostic::new(
                Code::Cycle,
                Loc::tab_file(&tab, &name),
                format!(
                    "circular reference: evaluating {tab}/{name} re-entered it through a chain of \
                     cell references (a cross-sheet chain counts) -- refused as #REF!-class rather \
                     than looping"
                ),
            ));
            // Not memoized: this is the in-progress cell seen from inside its own evaluation; the
            // outer call memoizes the final (error-carrying) value.
            return Value::Error(ErrKind::Ref);
        }
        if self.pull_depth.get() >= MAX_PULL_DEPTH {
            let name = self.file_name(id);
            let tab = self.tab_name(id.0);
            // Count this refusal so every ANCESTOR cell on the chain sees its snapshot advance and
            // declines to memoize its own depth-tainted value (see the memo guard below).
            self.depth_refusals.set(self.depth_refusals.get() + 1);
            self.refuse(Diagnostic::new(
                Code::DepthLimit,
                Loc::tab_file(&tab, &name),
                format!(
                    "formula dependency chain exceeded the pull-depth bound of {MAX_PULL_DEPTH} at \
                     {tab}/{name} -- refused as #NUM!-class rather than overflowing the stack"
                ),
            ));
            // Not memoized: the refusal is a property of the DEPTH at which this cell was reached,
            // not of the cell itself. Memoizing it would poison a later shallower pull of the same
            // cell (which is legal and computes). Its callers up the chain also decline to memoize
            // their (depth-tainted) values — the memo guard below, keyed off `depth_refusals`.
            return Value::Error(ErrKind::Num);
        }

        self.visiting.borrow_mut().insert(key);
        self.pull_depth.set(self.pull_depth.get() + 1);
        let prev_sheet = self.current_sheet.replace(id.0);
        let prev_file = self.current_file.replace(Some(id));
        // Snapshot the depth-refusal count: if it advances while this cell's subtree evaluates, the
        // value is depth-tainted and must not be memoized (order-independence — see below).
        let depth_refusals_before = self.depth_refusals.get();

        let value = self.compute_formula(id, dr, dc);

        self.current_sheet.set(prev_sheet);
        self.current_file.set(prev_file);
        self.pull_depth.set(self.pull_depth.get() - 1);
        self.visiting.borrow_mut().remove(&key);
        // Memoize ONLY a depth-clean value. If a depth-limit refusal fired anywhere in this cell's
        // subtree, the value carries a propagated #NUM! that is a function of the depth this cell was
        // reached at, not of the cell — caching it would make a later, shallower (and legally
        // computable) pull of the same cell return the poisoned error, so the same cell would yield
        // different answers depending on call order. Declining to memoize keeps evaluation
        // order-independent; a depth-clean pull re-computes and caches the real value.
        if self.depth_refusals.get() == depth_refusals_before {
            self.memo.borrow_mut().insert(key, value.clone());
        }
        value
    }

    /// The value of the covering file's grid cell at offset `(dr, dc)`. The cell holds a parsed
    /// `Expr` (VAL1: evaluated exactly as written — no offset/drag-fill), collapsed to a scalar for
    /// its scalar cell position by [`scalar_cell`]. Runs *inside* the [`Workbook::formula_value`]
    /// guards (cycle / depth / memo), so it never manages them itself.
    fn compute_formula(&self, id: FileId, dr: u32, dc: u32) -> Value {
        // VAL1: a cell's value derives only from its own content. The grid cell holds a parsed `Expr`,
        // evaluated exactly as written — no offset/drag-fill. A formula in a scalar cell position that
        // evaluates to an array (a bare range like `=A1:A3`) is collapsed by `scalar_cell` (a 1×1
        // array to its single cell, a genuinely multi-cell array to `#VALUE!`).
        let file = &self.tabs[id.0 as usize].files[id.1];
        match file.grid.cell_at(dr, dc) {
            GridCell::Formula { expr, .. } => scalar_cell(eval(expr, self)),
            // `compute_formula` is only reached for a formula cell (see `Resolver::value`); a literal
            // cell never routes here, but is total-passed-through defensively rather than panicking.
            GridCell::Value(v) => v.clone(),
        }
    }

    /// The tab (sheet) name for a sheet index, for a sheet-qualified [`Loc::tab_file`] anchor.
    /// Falls back to the numeric index for an out-of-range sheet (never panics).
    fn tab_name(&self, sheet: u32) -> String {
        self.tabs
            .get(sheet as usize)
            .map_or_else(|| sheet.to_string(), |t| t.name.clone())
    }

    fn file_name(&self, id: FileId) -> String {
        self.tabs[id.0 as usize].files[id.1].name.clone()
    }

    /// The anchor for an eval-time refusal raised from *inside* a formula's evaluation (e.g. a
    /// range-too-large refusal in [`Resolver::range`]): the sheet-qualified formula file currently on
    /// the stack, or the current sheet's tab if none is active (defensive — `range` only runs mid-eval).
    fn eval_loc(&self) -> Loc {
        match self.current_file.get() {
            Some(id) => Loc::tab_file(&self.tab_name(id.0), &self.file_name(id)),
            None => Loc::tab(&self.tab_name(self.current_sheet.get())),
        }
    }
}

impl Resolver for Workbook {
    fn value(&self, cell: CellRef) -> Value {
        let sheet = self.resolve_sheet(cell.sheet);
        let Some((id, file)) = self.covering(sheet, cell.col, cell.row) else {
            // A gap (no file claims this cell) reads as Blank (the overlap policy: gaps are Blank,
            // not errors -- see `overlap`).
            return Value::Blank;
        };
        let dr = cell.row - file.region.min_row;
        let dc = cell.col - file.region.min_col;
        match file.grid.cell_at(dr, dc) {
            // A literal cell needs no evaluation — it is its own value.
            GridCell::Value(v) => v.clone(),
            GridCell::Formula { .. } => self.formula_value((sheet, cell.col, cell.row), id, dr, dc),
        }
    }

    fn range(&self, range: RangeRef) -> ArrayView<'_> {
        // Resolve `sheet: None` to the current context and key the arena by the qualified range, so a
        // memoized `A1:A3` on one sheet is never mistaken for `A1:A3` on another. Normalize the key's
        // corners to canonical min/max (top-left..bottom-right) — matching the materialization loop
        // below — so a reversed spelling (`B2:A1`) maps to the SAME arena entry
        // as `A1:B2` rather than materializing the identical rectangle twice under two keys.
        let eff = SheetId(self.resolve_sheet(range.start.sheet));
        let c0 = range.start.col.min(range.end.col);
        let c1 = range.start.col.max(range.end.col);
        let r0 = range.start.row.min(range.end.row);
        let r1 = range.start.row.max(range.end.row);
        let key = RangeRef {
            start: CellRef {
                col: c0,
                row: r0,
                sheet: Some(eff),
            },
            end: CellRef {
                col: c1,
                row: r1,
                sheet: Some(eff),
            },
        };
        if let Some(view) = self.arena.get(key) {
            return view;
        }

        let (rows, cols) = (r1 - r0 + 1, c1 - c0 + 1);
        let area = u64::from(rows) * u64::from(cols);
        if area > MAX_RANGE_CELLS {
            // A syntactically-valid but pathologically-large reference (`A2:ZZ100000`): refuse
            // (located) instead of materializing a `Value` per cell into an OOM abort. The range
            // resolves to a single #NUM! cell that the referencing aggregation propagates, so the
            // formula reports #NUM! cleanly. Cached under `key` like any other materialized range.
            self.refuse(Diagnostic::new(
                Code::RangeTooLarge,
                self.eval_loc(),
                format!(
                    "referenced range spans {area} cells ({rows} rows x {cols} cols), over the \
                     materialization bound of {MAX_RANGE_CELLS} -- refused as #NUM!-class rather \
                     than allocating every cell"
                ),
            ));
            return self.arena.insert(
                key,
                Shape { rows: 1, cols: 1 },
                vec![Value::Error(ErrKind::Num)],
            );
        }
        // Materialize the rectangle by PULLING each cell (demand-driven: a range reference demands
        // exactly its own cells). No arena borrow is held across these `value` calls, which may
        // recursively push more range buffers. Snapshot the depth-refusal count first: if it advances
        // while the rectangle materializes, some cell tripped the pull-depth guard and froze a
        // depth-tainted `#NUM!` into this buffer — a value that is a function of the DEPTH the range
        // was first demanded at, not of the range. Caching such a buffer would poison every later,
        // shallower (and legally computable) demand of the same range with that stale `#NUM!`, making
        // the range order-dependent. This mirrors the per-cell memo's depth guard in `formula_value`.
        let depth_refusals_before = self.depth_refusals.get();
        let mut buf = Vec::with_capacity((rows as usize) * (cols as usize));
        for r in r0..=r1 {
            for c in c0..=c1 {
                buf.push(self.value(CellRef {
                    col: c,
                    row: r,
                    sheet: Some(eff),
                }));
            }
        }
        let shape = Shape { rows, cols };
        if self.depth_refusals.get() == depth_refusals_before {
            // Depth-clean: commit to the keyed cache like any other materialized range.
            self.arena.insert(key, shape, buf)
        } else {
            // Depth-tainted: return a borrowed view (backed by a stable buffer) WITHOUT recording the
            // key, so a later shallower demand misses, re-materializes, and caches the real value.
            self.arena.insert_uncached(shape, buf)
        }
    }

    fn sheet_id(&self, name: &str) -> Option<SheetId> {
        self.tabs
            .iter()
            .position(|t| t.name == name)
            .map(|i| SheetId(i as u32))
    }

    fn now_serial(&self) -> f64 {
        self.now
    }
}

/// Collapse a formula's evaluated result to the single scalar a grid cell holds: a 1×1 array is its
/// lone cell, a genuinely multi-cell array (a bare range like `=A1:A3` written into one cell) is
/// `#VALUE!` (an array in a scalar cell position), and a scalar passes through. This mirrors the
/// engine's scalar-position rule; a scalar-valued formula always passes through unchanged.
fn scalar_cell(v: Value) -> Value {
    match v {
        Value::Array(shape, mut cells) if shape.rows == 1 && shape.cols == 1 => {
            cells.pop().unwrap_or(Value::Blank)
        }
        Value::Array(..) => Value::Error(ErrKind::Value),
        scalar => scalar,
    }
}

/// The wall-clock "now" as an Excel date-time serial. The [`Workbook`] must STORE `now` (so
/// [`Workbook::with_now`] can pin it) rather than read the clock lazily, so it cannot simply inherit
/// the [`Resolver`] trait's default `now_serial`; it instead composes the two shared single-homes —
/// the raw clock read ([`system_now_secs`], which also handles a pre-epoch clock) and the
/// epoch->serial mapping ([`unix_secs_to_serial`]) — so no clock/epoch boilerplate is re-derived.
fn system_now_serial() -> f64 {
    unix_secs_to_serial(system_now_secs())
}

/// An append-only arena that owns the cell buffers behind [`Resolver::range`]'s borrowed
/// [`ArrayView`]s, keyed by the (qualified) [`RangeRef`] so each distinct range materializes once.
///
/// The evaluator's `range()` must return `ArrayView<'a> = &'a [Value]` borrowing the resolver's
/// store, but the store is built lazily under `&self`. This arena resolves that: a materialized
/// buffer is boxed and **never moved or freed** while `&self` lives (entries are only appended, never
/// removed or mutated), so a reference into a boxed slice stays valid for the whole `&self` borrow.
#[derive(Default, Debug)]
struct Arena {
    /// The owned buffers. `Box<[Value]>` heap data is address-stable across `Vec` growth.
    bufs: RefCell<Vec<Box<[Value]>>>,
    /// Range -> (shape, index into `bufs`).
    index: RefCell<HashMap<RangeRef, (Shape, usize)>>,
}

impl Arena {
    /// A borrowed view of an already-materialized range, or `None` if it has not been materialized.
    fn get(&self, key: RangeRef) -> Option<ArrayView<'_>> {
        let (shape, i) = {
            let index = self.index.borrow();
            *index.get(&key)?
        };
        let ptr: *const [Value] = &*self.bufs.borrow()[i];
        // SAFETY: the arena is append-only — `bufs` entries are boxed slices that are never moved
        // (the box's heap data is independent of the `Vec`'s reallocations) and never freed or
        // mutated while `&self` lives. So the pointee outlives this `&self` borrow, and no `&mut`
        // to the same data is ever created. The returned view's lifetime is tied to `&self`.
        let cells: &[Value] = unsafe { &*ptr };
        Some(ArrayView { shape, cells })
    }

    /// Materialize a range: store its buffer (append-only) and return the borrowed view.
    fn insert(&self, key: RangeRef, shape: Shape, cells: Vec<Value>) -> ArrayView<'_> {
        {
            let mut bufs = self.bufs.borrow_mut();
            let i = bufs.len();
            bufs.push(cells.into_boxed_slice());
            self.index.borrow_mut().insert(key, (shape, i));
        }
        self.get(key).expect("just inserted the key")
    }

    /// Own a range buffer for the lifetime of `&self` (so its [`ArrayView`] can be returned) but do
    /// **not** record it in the key index — a later demand for the same key misses and re-materializes.
    /// Used for a DEPTH-TAINTED buffer (the depth guard fired during its materialization): its `#NUM!`
    /// is a function of the depth the range was first reached at, not the range, so committing it to
    /// the keyed cache would poison a later shallower (computable) demand. Mirrors the per-cell memo's
    /// depth guard in [`Workbook::formula_value`], keeping range evaluation order-independent.
    fn insert_uncached(&self, shape: Shape, cells: Vec<Value>) -> ArrayView<'_> {
        let ptr: *const [Value] = {
            let mut bufs = self.bufs.borrow_mut();
            bufs.push(cells.into_boxed_slice());
            &*bufs[bufs.len() - 1]
        };
        // SAFETY: same append-only invariant as `get` — the boxed slice's heap data is never moved
        // (independent of `Vec` reallocation), freed, or mutated while `&self` lives, so the pointee
        // outlives this `&self` borrow and no `&mut` to it is ever created.
        let cells: &[Value] = unsafe { &*ptr };
        ArrayView { shape, cells }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANN: &str = "# Concern: c | Non-concern: n | IO: input\n";

    /// Build a file's contents: the mandatory annotation line 1 plus a body.
    fn file(body: &str) -> String {
        format!("{ANN}{body}")
    }

    /// Load a single-tab workbook from `(filename, body)` pairs, asserting a clean load.
    fn load_one_tab(tab: &str, files: &[(&str, &str)]) -> Workbook {
        let owned: Vec<(String, String)> = files
            .iter()
            .map(|(n, b)| ((*n).to_string(), file(b)))
            .collect();
        let refs: Vec<(&str, &str)> = owned
            .iter()
            .map(|(n, c)| (n.as_str(), c.as_str()))
            .collect();
        Workbook::from_tabs(&[(tab, &refs)])
            .unwrap_or_else(|d| panic!("workbook should load clean: {d:?}"))
    }

    #[test]
    fn chain_a_to_b_to_c_pulls_through_the_model() {
        // A1 = 1 (literal); B1 = A1 + 1 (formula); C1 = B1 * 10 (formula). Requesting C1 pulls B1,
        // which pulls A1 — the demand-driven chain.
        let wb = load_one_tab("Sheet1", &[("A1", "1"), ("B1", "=A1+1"), ("C1", "=B1*10")]);
        assert_eq!(wb.value_at(0, 2, 0), Value::Number(20.0)); // C1
        assert_eq!(wb.value_at(0, 1, 0), Value::Number(2.0)); // B1
        assert!(wb.eval_diagnostics().is_empty());
    }

    #[test]
    fn a_direct_cycle_is_a_ref_refusal_not_a_hang() {
        // A1 = B1; B1 = A1 — a two-cell cycle. Must refuse with #REF!, never overflow the stack.
        let wb = load_one_tab("Sheet1", &[("A1", "=B1"), ("B1", "=A1")]);
        assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Ref)); // A1
        let diags = wb.eval_diagnostics();
        assert!(diags.iter().any(|d| d.code == Code::Cycle), "{diags:?}");
    }

    #[test]
    fn a_self_reference_is_a_cycle() {
        // A1 = A1 + 1 references its own cell.
        let wb = load_one_tab("Sheet1", &[("A1", "=A1+1")]);
        assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Ref));
        assert!(wb.eval_diagnostics().iter().any(|d| d.code == Code::Cycle));
    }

    #[test]
    fn cross_sheet_reference_resolves_the_named_tab() {
        // Inputs!A1 = 10; Summary!A1 = Inputs!A1 * 2 -> 20. Also proves an UNQUALIFIED ref inside a
        // Summary formula resolves against Summary, not tab 0.
        let wb = Workbook::from_tabs(&[
            ("Inputs", &[("A1", &file("10"))]),
            (
                "Summary",
                &[
                    ("A1", &file("=Inputs!A1*2")),
                    ("A2", &file("100")),
                    ("A3", &file("=A2+1")), // unqualified A2 must mean Summary!A2
                ],
            ),
        ])
        .expect("loads clean");
        assert_eq!(wb.value_at(1, 0, 0), Value::Number(20.0)); // Summary!A1
        assert_eq!(wb.value_at(1, 0, 2), Value::Number(101.0)); // Summary!A3 = Summary!A2 + 1
    }

    #[test]
    fn an_explicit_grid_gives_each_cell_its_own_formula() {
        // VAL1: a range file's content is the EXPLICIT grid — no drag-fill. A1:A3 is a literal column
        // vector 1,2,3. B1:B3 is a 3x1 grid of THREE explicit formulas `=A1`, `=A2`, `=A3` (one per
        // cell, written out), so B1=A1=1, B2=A2=2, B3=A3=3. D1 = SUM(A1:A3) pulls the whole range.
        let wb = load_one_tab(
            "Sheet1",
            &[
                ("A1:A3", "1\n2\n3"),
                ("D1", "=SUM(A1:A3)"),
                ("B1:B3", "=A1\n=A2\n=A3"),
            ],
        );
        assert_eq!(wb.value_at(0, 3, 0), Value::Number(6.0)); // D1
        assert_eq!(wb.value_at(0, 1, 0), Value::Number(1.0)); // B1 = A1
        assert_eq!(wb.value_at(0, 1, 1), Value::Number(2.0)); // B2 = A2
        assert_eq!(wb.value_at(0, 1, 2), Value::Number(3.0)); // B3 = A3
        assert!(wb.eval_diagnostics().is_empty());
    }

    #[test]
    fn an_explicit_grid_evaluates_absolute_and_relative_refs_as_written() {
        // The explicit-grid replacement for the old drag-fill: C2:C4 is a 3x1 grid whose three cells
        // are written out `=A2*B$1`, `=A3*B$1`, `=A4*B$1`. A is 1,2,3 down; B1 (the `$`-pinned row) is
        // 10. Each cell evaluates its OWN formula as written: C2=A2*B1=10, C3=A3*B1=20, C4=A4*B1=30.
        let wb = load_one_tab(
            "Sheet1",
            &[
                ("A2:A4", "1\n2\n3"),
                ("B1", "10"),
                ("C2:C4", "=A2*B$1\n=A3*B$1\n=A4*B$1"),
            ],
        );
        assert_eq!(wb.value_at(0, 2, 1), Value::Number(10.0)); // C2
        assert_eq!(wb.value_at(0, 2, 2), Value::Number(20.0)); // C3
        assert_eq!(wb.value_at(0, 2, 3), Value::Number(30.0)); // C4
        assert!(wb.eval_diagnostics().is_empty());
    }

    #[test]
    fn a_bare_range_formula_in_a_scalar_cell_is_a_value_error() {
        // A formula that evaluates to a genuinely multi-cell array (`=A1:A3`) written into a single
        // scalar cell has no scalar meaning — `scalar_cell` collapses it to `#VALUE!`.
        let wb = load_one_tab("Sheet1", &[("A1:A3", "1\n2\n3"), ("C1", "=A1:A3")]);
        assert_eq!(wb.value_at(0, 2, 0), Value::Error(ErrKind::Value)); // C1
    }

    #[test]
    fn a_diamond_dag_evaluates_each_cell_once_never_exponentially() {
        // A diamond that, WITHOUT per-cell memoization, re-evaluates the shared base exponentially:
        // each level references the one below TWICE, so a naive re-eval is 2^depth. With the per-cell
        // memo it is linear and returns instantly. A1=1; each A{n}=A{n+1}+A{n+1} down a long column,
        // so A1 = 2^(len-1). Reaching the assert at all proves no exponential hang.
        let len = 40usize; // 2^39 ~ 5.5e11 re-evals if exponential; instant if memoized
        let owned: Vec<(String, String)> = (0..len)
            .map(|i| {
                let name = format!("A{}", i + 1);
                let body = if i + 1 < len {
                    format!("=A{n}+A{n}", n = i + 2)
                } else {
                    "1".to_string()
                };
                (name, body)
            })
            .collect();
        let refs: Vec<(&str, &str)> = owned
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_str()))
            .collect();
        let wb = load_one_tab("Sheet1", &refs);
        assert_eq!(
            wb.value_at(0, 0, 0),
            Value::Number(2f64.powi((len - 1) as i32)) // A1 = 2^(len-1), computed linearly
        );
        assert!(wb.eval_diagnostics().is_empty());
    }

    #[test]
    fn memoization_gives_a_stable_answer_on_repeated_pulls() {
        // Re-requesting the same formula cell (and its dependents) yields the same value — the memo
        // does not corrupt state across pulls.
        let wb = load_one_tab("Sheet1", &[("A1", "5"), ("B1", "=A1*A1")]);
        assert_eq!(wb.value_at(0, 1, 0), Value::Number(25.0));
        assert_eq!(wb.value_at(0, 1, 0), Value::Number(25.0));
        assert!(wb.eval_diagnostics().is_empty());
    }

    #[test]
    fn a_gap_cell_reads_blank() {
        let wb = load_one_tab("Sheet1", &[("A1", "1")]);
        // Z9 is claimed by no file.
        assert_eq!(wb.value_at(0, 25, 8), Value::Blank);
    }

    #[test]
    fn load_surfaces_overlap_and_bad_files() {
        // Two files claiming intersecting cells -> a load-time overlap refusal. A1:C3 declares 3x3, so
        // its body is a full 3x3 grid; B2 is a single cell inside it.
        let err = Workbook::from_tabs(&[(
            "Sheet1",
            &[
                ("A1:C3", &file("1\t2\t3\n4\t5\t6\n7\t8\t9")),
                ("B2", &file("x")),
            ],
        )])
        .unwrap_err();
        assert!(err.iter().any(|d| d.code == Code::Overlap), "{err:?}");
    }

    #[test]
    fn an_unparseable_formula_is_a_load_time_refusal_not_a_panic() {
        // A formula is parsed at load (the grid holds a parsed `Expr`), so an unparseable formula is a
        // located load-time refusal, never a panic.
        let err = Workbook::from_tabs(&[("Sheet1", &[("A1", &file("=SUM("))])]).unwrap_err();
        assert!(err.iter().any(|d| d.code == Code::FormulaSyntax), "{err:?}");
    }

    #[test]
    fn load_dir_reads_folders_as_tabs() {
        // Round-trip through the filesystem loader: two tabs, a cross-sheet pull.
        let base = std::env::temp_dir().join(format!(
            "charlie-wb-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let inputs = base.join("Inputs");
        let summary = base.join("Summary");
        std::fs::create_dir_all(&inputs).unwrap();
        std::fs::create_dir_all(&summary).unwrap();
        std::fs::write(inputs.join("A1"), file("7")).unwrap();
        std::fs::write(summary.join("A1"), file("=Inputs!A1*6")).unwrap();

        let wb = Workbook::load_dir(&base)
            .expect("fs read ok")
            .expect("loads clean");
        assert_eq!(wb.sheet_names(), vec!["Inputs", "Summary"]);
        // Summary is tab index 1 (sorted: Inputs, Summary).
        assert_eq!(wb.value_at(1, 0, 0), Value::Number(42.0));

        std::fs::remove_dir_all(&base).ok();
    }

    /// Build a single-column chain `A1=A2(+1), A2=A3(+1), ..., A{len-1}=A{len}(+1)` with the bottom
    /// cell `A{len}` a literal `0`. Each `+1` makes the top cell's value the chain length minus one
    /// when it fully evaluates, so a computed answer proves the whole chain was walked.
    fn chain_files(len: usize) -> Vec<(String, String)> {
        (0..len)
            .map(|i| {
                let name = format!("A{}", i + 1);
                let body = if i + 1 < len {
                    format!("=A{}+1", i + 2)
                } else {
                    "0".to_string()
                };
                (name, body)
            })
            .collect()
    }

    #[test]
    fn a_legal_deep_chain_under_the_bound_computes_fully() {
        // A chain well within [`MAX_PULL_DEPTH`] evaluates end-to-end: the depth guard never fires on
        // a legal sheet, only on a pathologically deep one.
        let len = (MAX_PULL_DEPTH / 2) as usize; // comfortably under the bound
        let owned = chain_files(len);
        let refs: Vec<(&str, &str)> = owned
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_str()))
            .collect();
        let wb = load_one_tab("Sheet1", &refs);
        assert_eq!(wb.value_at(0, 0, 0), Value::Number((len - 1) as f64)); // A1
        assert!(
            wb.eval_diagnostics().is_empty(),
            "{:?}",
            wb.eval_diagnostics()
        );
    }

    #[test]
    fn a_deep_acyclic_chain_refuses_instead_of_overflowing_the_stack() {
        // A finite, entirely acyclic chain deeper than the bound. The `visiting` cycle set never trips
        // (nothing is re-entered), so ONLY the pull-depth guard stands between this and a native stack
        // overflow: reaching the assertions at all proves no SIGABRT. The deepest link is a located
        // #NUM!-class refusal that propagates up to the requested top cell.
        let len = (MAX_PULL_DEPTH as usize) + 64;
        let owned = chain_files(len);
        let refs: Vec<(&str, &str)> = owned
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_str()))
            .collect();
        let wb = load_one_tab("Sheet1", &refs);
        assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Num)); // A1
        let diags = wb.eval_diagnostics();
        assert!(
            diags.iter().any(|d| d.code == Code::DepthLimit),
            "{diags:?}"
        );
        // Never misclassified as a cycle: this chain has no cycle.
        assert!(!diags.iter().any(|d| d.code == Code::Cycle), "{diags:?}");
    }

    #[test]
    fn check_over_a_workbook_containing_a_deep_chain_does_not_crash() {
        // `lint` drives EVERY cell, so a workbook that merely CONTAINS an over-deep chain must lint to
        // a located refusal rather than aborting the process.
        let len = (MAX_PULL_DEPTH as usize) + 8;
        let owned = chain_files(len);
        let refs: Vec<(&str, &str)> = owned
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_str()))
            .collect();
        let wb = load_one_tab("Sheet1", &refs);
        let diags = wb.lint();
        assert!(
            diags.iter().any(|d| d.code == Code::DepthLimit),
            "{diags:?}"
        );
    }

    #[test]
    fn a_depth_refused_pull_does_not_poison_a_later_shallower_pull() {
        // Order-independence (never falsely reject a computable cell). One chain A1->A2->...->A320.
        // Pulling A1 FIRST refuses at depth 256 and propagates #NUM! up through A1..A256 -- but those
        // ancestor outcomes are depth-tainted and must NOT be memoized, so a LATER direct pull of A256
        // (whose own chain A256..A320 is only 65 links deep, legally computable) returns its real
        // value, not a cached #NUM!.
        let len = (MAX_PULL_DEPTH as usize) + 64; // 320
        let owned = chain_files(len);
        let refs: Vec<(&str, &str)> = owned
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_str()))
            .collect();
        let wb = load_one_tab("Sheet1", &refs);

        // Pull the deep top first: it refuses (its chain is 320 links).
        assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Num)); // A1

        // A256's own chain is short enough to compute: A256 = A257+1 = ... = A320(0) + 64 = 64.
        let a256 = wb.value_at(0, 0, 255); // A256 is column A (0), zero-based row 255
        assert_eq!(
            a256,
            Value::Number((len - 256) as f64),
            "call order poisoned a computable cell -- a depth-tainted outcome was memoized"
        );
    }

    #[test]
    fn a_depth_tainted_range_is_not_frozen_into_the_arena() {
        // Order-independence for RANGE materialization -- the arena analogue of the per-cell memo
        // depth guard. An H-chain H1->..->H99->0 forwards to 0 and is read by `SUM(H1:H1)`. That range
        // is FIRST demanded from the bottom of a 200-deep A-chain (A1->..->A200 = `=SUM(H1:H1)`):
        // pulling A1 descends ~200 links, so materializing H1:H1 there pushes past MAX_PULL_DEPTH (256)
        // and would freeze a depth-tainted #NUM! into the H1:H1 rectangle. A LATER shallow
        // `B1 = SUM(H1:H1)` (H1:H1 reached only 99 links deep, legally computable) must recompute to 0.
        let mut owned: Vec<(String, String)> = Vec::new();
        let h_len = 99usize; // H-chain: forwarding, bottom literal 0 => H1 == 0, reached 99 links deep.
        for i in 0..h_len {
            let name = format!("H{}", i + 1);
            let body = if i + 1 < h_len {
                format!("=H{}", i + 2)
            } else {
                "0".to_string()
            };
            owned.push((name, body));
        }
        let a_len = 200usize; // A-chain: forwarding, bottom cell reads SUM(H1:H1) at ~200 links deep.
        for i in 0..a_len {
            let name = format!("A{}", i + 1);
            let body = if i + 1 < a_len {
                format!("=A{}", i + 2)
            } else {
                "=SUM(H1:H1)".to_string()
            };
            owned.push((name, body));
        }
        owned.push(("B1".to_string(), "=SUM(H1:H1)".to_string())); // the later SHALLOW demand.
        let refs: Vec<(&str, &str)> = owned
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_str()))
            .collect();
        let wb = load_one_tab("Sheet1", &refs);

        // Pull the DEEP A-chain first: H1:H1 is reached past the depth bound, tainting the range.
        assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Num)); // A1 (deep -> #NUM!)

        // The later SHALLOW pull: H1:H1 is only 99 links deep here, so SUM(H1:H1) computes to 0.
        assert_eq!(
            wb.value_at(0, 1, 0), // B1 = col 1, row 0
            Value::Number(0.0),
            "a depth-tainted range buffer was frozen into the arena and poisoned a shallow demand"
        );
    }

    #[test]
    fn an_inverted_range_reuses_its_normalized_arena_entry() {
        // The arena key is normalized to canonical min/max corners (matching the materialization
        // loop), so a reversed spelling (`B2:A1`) maps to the SAME cache entry as `A1:B2` rather than
        // materializing the identical rectangle twice under two keys.
        let wb = load_one_tab("Sheet1", &[("A1", "1")]);
        let normalized = RangeRef {
            start: CellRef {
                col: 0,
                row: 0,
                sheet: None,
            },
            end: CellRef {
                col: 1,
                row: 1,
                sheet: None,
            },
        };
        let inverted = RangeRef {
            start: CellRef {
                col: 1,
                row: 1,
                sheet: None,
            },
            end: CellRef {
                col: 0,
                row: 0,
                sheet: None,
            },
        };
        let normalized_cells = wb.range(normalized).cells.to_vec();
        // Same rectangle regardless of corner order -- and, crucially, the same arena slot.
        assert_eq!(wb.range(inverted).cells, normalized_cells.as_slice());
        assert_eq!(
            wb.arena.index.borrow().len(),
            1,
            "an inverted range must reuse the normalized key's entry, not add a second"
        );
    }

    #[test]
    fn a_reference_to_a_pathologically_large_range_refuses_instead_of_oom() {
        // =SUM(A2:ZZ100000) references ~70M empty cells. Materializing a Value per cell would drive an
        // OOM abort; the model caps the range, so the reference resolves to a located #NUM! rather
        // than allocating.
        let wb = load_one_tab("Sheet1", &[("A1", "=SUM(A2:ZZ100000)")]);
        assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Num)); // A1
        let diags = wb.eval_diagnostics();
        assert!(
            diags.iter().any(|d| d.code == Code::RangeTooLarge),
            "{diags:?}"
        );
        // The refusal is sheet-qualified to the offending formula file.
        assert!(
            diags.iter().any(|d| matches!(
                &d.loc,
                Loc::TabFile { tab, name } if tab == "Sheet1" && name == "A1"
            )),
            "range-too-large refusal must anchor on Sheet1/A1: {diags:?}"
        );
    }

    #[test]
    fn a_range_at_the_materialization_bound_still_computes() {
        // A merely-large but valid range materializes: A1:A5 holds 1..5; =SUM(A1:A5) over 5 cells is
        // well under the bound -> 15.
        let wb = load_one_tab(
            "Sheet1",
            &[("A1:A5", "1\n2\n3\n4\n5"), ("C1", "=SUM(A1:A5)")],
        );
        assert_eq!(wb.value_at(0, 2, 0), Value::Number(15.0)); // C1
        assert!(wb.eval_diagnostics().is_empty());
    }

    #[test]
    fn a_cross_sheet_cycle_is_located_to_the_sheet_qualified_file() {
        // Sheet1!A1 = Sheet2!A1 and Sheet2!A1 = Sheet1!A1 -- a cross-sheet cycle. The refusal must
        // name the TAB, not a bare `A1` (which exists on BOTH sheets and is otherwise untraceable).
        let wb = Workbook::from_tabs(&[
            ("Sheet1", &[("A1", &file("=Sheet2!A1"))]),
            ("Sheet2", &[("A1", &file("=Sheet1!A1"))]),
        ])
        .expect("loads clean");
        assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Ref)); // Sheet1!A1
        let diags = wb.eval_diagnostics();
        let cyc = diags
            .iter()
            .find(|d| d.code == Code::Cycle)
            .expect("a cycle diagnostic must fire");
        match &cyc.loc {
            Loc::TabFile { tab, name } => {
                assert!(tab == "Sheet1" || tab == "Sheet2", "unexpected tab {tab:?}");
                assert_eq!(name, "A1");
            }
            other => panic!("cross-sheet cycle must be sheet-qualified, got {other:?}"),
        }
    }

    #[test]
    fn eval_formula_evaluates_an_ad_hoc_string_against_the_workbook() {
        // The `charlie-cli eval` entry: an ad-hoc formula string evaluates against the loaded workbook,
        // referencing stored cells. A clean value is `Value`; a spreadsheet error value is `Error`.
        let wb = load_one_tab("Sheet1", &[("A1", "6"), ("A2", "7")]);
        assert_eq!(
            wb.eval_formula(0, "A1*A2").unwrap(),
            FormulaOutcome::Value("42".to_string())
        );
        assert_eq!(
            wb.eval_formula(0, "1/0").unwrap(),
            FormulaOutcome::Error("#DIV/0!".to_string())
        );
        // A parse failure is a located refusal.
        assert!(wb.eval_formula(0, "SUM(").is_err());
    }
}
