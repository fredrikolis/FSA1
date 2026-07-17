// Concern: the TWO-PASS evaluation engine (ENG3) — load a sheet-directory (tabs=folders, files=closed ranges) into an in-memory `Workbook` via `parse_file`, then evaluate demanded cells in TWO passes: a PLAN pass builds one private dependency graph (`DepGraph`) of the cells a render demands — the viewport's cells plus their transitive dependencies, ranges expanded to their cells, a dependency shared by several demanded cells merged to a SINGLE node, accreted as more demanded cells are added — and an EVALUATE pass computes each node exactly once (ENG2 compute-once) in dependency order through `charlie_ast::eval` with THIS workbook as the `Resolver` (during evaluate the resolver only READS already-computed results), writing values; a cell's value derives only from its own content (VAL1), so there is no drag-fill/offset; EVERY prior behavior and diagnostic is preserved — a reference cycle is a located `#REF!` (CORE2 / ENG2 cycle-safe), the pull-depth guard is a located `#NUM!`, the range-materialization bound (`MAX_RANGE_CELLS`) is a located `#NUM!`, cross-sheet resolution, range materialization, and totality (never a panic); a clean result is memoized and reused while inputs are unchanged (ENG4) but a depth-tainted (root-relative `#NUM!`) result is not; an AD-HOC `=formula` string is evaluated against the loaded workbook via `eval_formula` (the `charlie-cli eval` entry) through the same two passes, returned as a `FormulaOutcome` (a clean value vs a spreadsheet error value, `display_value`-spelled) so the CLI sets its exit code without depending on `charlie-ast` | Non-concern: the formula LANGUAGE (charlie-ast owns lex/parse/eval), the filename/grid/overlap GRAMMAR (this reuses `parse_file`/`detect_overlaps`), xlsx serde, the CLI render surface, and the dependency-graph type's escape from this module — `DepGraph`/`PlanNode` are private and appear in no other module's surface | IO: (a sheet-directory or in-memory tabs) -> a `Workbook`; then (a demanded cell / a viewport of cells) -> `Value`s, or (a tab index + an ad-hoc `=formula` string) -> a `Result<FormulaOutcome, Diagnostic>`, plus the eval-time located `Diagnostic`s it accumulates
//! Two-pass evaluation (ENG3): [`Workbook`] loads a sheet-directory and implements [`Resolver`] over
//! it. A demand (one cell via [`Workbook::value_at`], a viewport via [`Workbook::values_at`], or an
//! ad-hoc formula via [`Workbook::eval_formula`]) runs a PLAN pass that builds one private dependency
//! graph of the demanded cells and their transitive dependencies (ranges expand to cells; a shared
//! dependency is one merged node), then an EVALUATE pass that computes each node once in dependency
//! order through `charlie_ast::eval`. The graph is a contained optimization — its type never leaves
//! this module and it EQUALS a naive per-cell evaluation (proven by the differential test below). The
//! engine stays memoized (ENG4), cycle-safe (a reference cycle is a located `#REF!`), and lazy (an
//! off-request cell never computes).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use charlie_ast::{
    ArrayView, CellRef, ErrKind, Expr, RangeRef, Resolver, Shape, SheetId, Value, eval, parse,
    system_now_secs, unix_secs_to_serial,
};

use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::grid::{Cell as GridCell, Grid};
use crate::overlap::{Rect, detect_overlaps};
use crate::{ParsedFile, parse_file};

/// The maximum cross-cell dependency depth the PLAN pass will descend before it refuses. Each link in
/// a `=formula` dependency chain (`A1=A2`, `A2=A3`, ...) is one level deeper in the plan's dependency
/// DFS, and the DFS recurses natively (`plan_visit -> plan_visit`), so a deep-but-acyclic chain would
/// otherwise grow the native stack one frame per link and abort the process on overflow while the
/// graph is being built.
///
/// A chain deeper than this is a located `#NUM!`-class refusal recorded at the offending cell, never a
/// crash. The value is deliberately conservative — chosen to be safe on the *smallest* stack any of
/// our entry points runs on (the test harness's ~2 MiB worker threads), not to match a spreadsheet
/// engine's much larger practical limit. A serial dependency chain this long is already pathological;
/// the contract is only that it refuses cleanly.
///
/// Past this bound the refusal is a property of the DEPTH the chain was reached at, not of the cell:
/// a `#NUM!` produced here (and every ancestor value that consumes it) is *depth-tainted* and is NOT
/// memoized, so a later, shallower — and legally computable — demand of the same cell recomputes its
/// real value rather than reading a poisoned cache entry (see
/// `a_depth_refused_pull_does_not_poison_a_later_shallower_pull`).
const MAX_PULL_DEPTH: u32 = 256;

/// The largest rectangular range (in cells) [`Resolver::range`] will MATERIALIZE before it refuses,
/// and the largest a range dependency the PLAN pass will EXPAND into its cells. A `=formula` may
/// reference a syntactically-valid but pathologically-large rectangle (`=SUM(A2:ZZ100000)` — ~70M
/// cells); expanding or materializing a cell for every one drives the process into an OOM abort.
/// Bounding the area turns that into a located `#NUM!`-class [`Code::RangeTooLarge`] refusal — the
/// plan leaves the range unexpanded and [`Resolver::range`] resolves it to a single error cell that
/// the referencing aggregation propagates — so no valid invocation can crash the process on
/// allocation. The bound is far above any real sheet's used range (a 1000×1000 block); only a
/// pathological reference reaches it.
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

/// The identity of one rendered cell: `(sheet index, zero-based col, zero-based row)`. Graph nodes,
/// the per-cell memo, and the per-pass results/taint sets are keyed by this — each grid cell is a
/// DISTINCT computation, so the graph and the caches are per cell, not per file.
type CellKey = (u32, u32, u32);

/// One node of the PLAN pass's dependency graph — how a demanded cell is computed by the EVALUATE
/// pass. PRIVATE to this module (ENG3 containment): it appears in no other module's surface and is
/// never re-exported.
///
/// A literal cell and a gap are NOT nodes — the EVALUATE pass reads them straight from the grid, so
/// only cells that need computation (formulas) or a pre-decided refusal (cycle / depth) are nodes.
enum PlanNode {
    /// A formula cell: its covering file, the cell's local offset into that file's grid, and the
    /// dependency cells whose values must be computed first. `deps` is the formula's static references
    /// (ranges expanded to their cells); a dep that is itself a graph node orders before this one.
    Formula {
        file: FileId,
        dr: u32,
        dc: u32,
        deps: Vec<CellKey>,
    },
    /// A cell on a reference cycle — a located `#REF!`. Terminal (no deps); its dependents propagate
    /// the `#REF!`.
    Cycle,
    /// A cell reached past [`MAX_PULL_DEPTH`] — a located `#NUM!`. Terminal and depth-tainted, so its
    /// value is never memoized.
    DepthRefused,
}

/// The PLAN pass's dependency graph: one merged node per demanded/depended cell (ENG3). PRIVATE to
/// this module and never re-exported — the render/check/eval surfaces consume only cell VALUES.
///
/// The graph is built up and MERGED across a render pass's demanded cells: [`Workbook::plan_visit`]
/// accretes each demanded cell (and its transitive dependencies) into the SAME graph, so a dependency
/// needed by more than one demanded cell becomes a single shared node — removing that sharing changes
/// performance, never results.
#[derive(Default)]
struct DepGraph {
    nodes: HashMap<CellKey, PlanNode>,
}

/// An in-memory charlie workbook that evaluates on demand in two passes (ENG3).
///
/// Load with [`Workbook::from_tabs`] (in-memory) or [`Workbook::load_dir`] (a filesystem tree), then
/// drive evaluation by requesting cells ([`Workbook::value_at`] / [`Workbook::values_at`]) or by
/// handing `&Workbook` to [`charlie_ast::eval`] as a [`Resolver`]. Evaluation is **demand-driven**
/// (only requested cells, transitively, compute), **two-pass** (a plan builds one dependency graph; an
/// evaluate pass computes each node once in dependency order — a diamond / deep DAG stays linear), and
/// **cycle-safe** (a reference cycle is a located `#REF!`-class refusal, never a hang). Each grid cell
/// derives its value only from its own content (VAL1) — a range file is an explicit grid, never a
/// drag-filled formula.
#[derive(Debug)]
pub struct Workbook {
    tabs: Vec<Tab>,
    /// The "now" instant [`Resolver::now_serial`] reports. Defaults to the wall clock at load; a test
    /// pins it with [`Workbook::with_now`]. (Production gets wall-clock time for free.)
    now: f64,
    /// The sheet an unqualified reference (`sheet: None`) resolves against during the EVALUATE pass —
    /// the home sheet of the formula node currently being computed. Set per node (evaluation is
    /// iterative, not nested), and around each ad-hoc [`Workbook::eval_formula`]. `Cell` because it is
    /// a plain copyable scalar.
    current_sheet: Cell<u32>,
    /// Per-CELL result cache (the memo): a cell computed once — and clean (see the pass results) — is
    /// stored here and reused while its content and everything upstream are unchanged (ENG4). Keyed by
    /// the resolved `(sheet, col, row)`. Per-cell (not per-file) makes an EXPLICIT GRID correct (each
    /// cell of a range file is a distinct computation, VAL1) AND makes a shared dependency compute
    /// once. Only DEPTH-CLEAN values reach here (a depth-tainted `#NUM!` is root-relative — see
    /// [`Workbook::finish_pass`]).
    memo: RefCell<HashMap<CellKey, Value>>,
    /// The current pass's computed values — the EVALUATE pass fills this in dependency order, and the
    /// [`Resolver`] reads it (then the memo, then the grid) so a formula's evaluation sees its already-
    /// computed dependencies without recomputing them. Promoted (clean entries only) into the memo and
    /// cleared at the end of each demand ([`Workbook::finish_pass`]).
    results: RefCell<HashMap<CellKey, Value>>,
    /// The current pass's DEPTH-TAINTED cells: a cell that IS a depth refusal, or that (transitively)
    /// consumed one. Its `#NUM!` is a function of the DEPTH the chain was reached at, not of the cell,
    /// so it must not be memoized (a later shallower, computable demand would read a poisoned entry).
    /// [`Resolver::range`] also consults it so a range spanning a tainted cell is not frozen into the
    /// arena. Cleared at the end of each demand.
    pass_tainted: RefCell<HashSet<CellKey>>,
    /// The formula file currently being evaluated, if any — the anchor an eval-time refusal raised
    /// from *inside* eval (e.g. a range-too-large refusal in [`Resolver::range`]) points at. Set per
    /// node in the EVALUATE pass, like [`Workbook::current_sheet`].
    current_file: Cell<Option<FileId>>,
    /// An append-only arena backing the borrowed [`ArrayView`]s that [`Resolver::range`] returns.
    arena: Arena,
    /// Located refusals surfaced during evaluation (cycles, depth limits, over-large ranges, spills).
    /// Load-time refusals are returned by the loader; these accumulate as cells are planned/pulled.
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
                results: RefCell::new(HashMap::new()),
                pass_tainted: RefCell::new(HashSet::new()),
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
    /// tab index; `col`/`row` are zero-based. Runs the two passes over a graph rooted at this one cell
    /// (unless its value is already memoized), then reads the result.
    pub fn value_at(&self, sheet: u32, col: u32, row: u32) -> Value {
        let key = (sheet, col, row);
        if let Some(hit) = self.memo.borrow().get(&key) {
            return hit.clone();
        }
        self.demand(&[key]);
        let v = self.value(CellRef {
            col,
            row,
            sheet: Some(SheetId(sheet)),
        });
        self.finish_pass();
        v
    }

    /// Resolve a batch of cells to their values in ONE plan+evaluate pass — the render entry (ENG3).
    /// All the (uncached) demanded cells accrete into a SINGLE dependency graph, so a dependency shared
    /// by several of them is computed exactly once. Returns the values in the requested order.
    pub fn values_at(&self, cells: &[CellKey]) -> Vec<Value> {
        let uncached: Vec<CellKey> = {
            let memo = self.memo.borrow();
            cells
                .iter()
                .copied()
                .filter(|k| !memo.contains_key(k))
                .collect()
        };
        if !uncached.is_empty() {
            self.demand(&uncached);
        }
        let out = cells
            .iter()
            .map(|&(s, c, r)| {
                self.value(CellRef {
                    col: c,
                    row: r,
                    sheet: Some(SheetId(s)),
                })
            })
            .collect();
        self.finish_pass();
        out
    }

    /// The located refusals accumulated during evaluation so far (cycles, depth limits, over-large
    /// ranges, spills, unparseable bodies). Snapshot — call after driving the cells of interest.
    pub fn eval_diagnostics(&self) -> Vec<Diagnostic> {
        self.diagnostics.borrow().clone()
    }

    /// Evaluate an AD-HOC formula string against this loaded workbook — the `charlie-cli eval` entry.
    /// Parses `formula` through `charlie_ast`, PLANS + EVALUATES its dependency cone (so the resolver
    /// reads already-computed values), then evaluates the formula tree with THIS workbook as the
    /// [`Resolver`]. Unqualified references (`A1`, `A1:A5`) resolve against `sheet` (the tab index).
    /// Read-only: no file writes, no cell mutation.
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
        // PLAN the ad-hoc formula's dependency cells (rooted at `sheet`, the ad-hoc home) into a graph,
        // then EVALUATE it — exactly the two passes a stored formula rides.
        let deps = self.expr_deps(&expr, sheet);
        let mut graph = DepGraph::default();
        for &d in &deps {
            let mut on_stack = HashSet::new();
            self.plan_visit(d, 0, &mut graph, &mut on_stack);
        }
        self.evaluate(&graph);
        // The EVALUATE pass left `current_sheet`/`current_file` at the last node's context; set the
        // ad-hoc home so this formula's unqualified refs resolve against `sheet` AND a top-level
        // eval-time refusal (e.g. an over-large range) anchors on the ad-hoc `Loc::tab(sheet)` rather
        // than the stale last-node file. The ad-hoc formula has no covering file, so `current_file`
        // is cleared for its evaluation.
        let prev_sheet = self.current_sheet.replace(sheet);
        let prev_file = self.current_file.replace(None);
        let value = eval(&expr, self);
        self.current_sheet.set(prev_sheet);
        self.current_file.set(prev_file);
        self.finish_pass();
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

    /// Lint the whole workbook: drive every cell of every file (so every formula evaluates, memoized)
    /// and return the eval-time located refusals — cycles, over-deep chains (`#NUM!`-class), over-large
    /// ranges, formula-result dimension mismatches (`#SPILL!`-class), and unparseable formula bodies.
    /// Load-time refusals (overlap, literal dimension mismatch, bad filenames) surface from the loader,
    /// not here.
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
        // Each formula file records its refusal at most once during a full drive (memoization returns
        // the cached outcome before re-planning on a repeat pull), so consecutive-duplicate removal
        // only guards the rare case where two adjacent drives surface the identical located refusal.
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

    // ------------------------------------------------------------------------------------------
    // PLAN pass — build one dependency graph of the demanded cells and their transitive deps.
    // ------------------------------------------------------------------------------------------

    /// Build (and merge into) the dependency graph for a set of demanded cells, then run the EVALUATE
    /// pass over it. Each demanded cell accretes into the SAME [`DepGraph`] (a shared dependency
    /// becomes one node); an already-memoized cell is a resolved leaf and is not re-planned (ENG4).
    fn demand(&self, roots: &[CellKey]) {
        let mut graph = DepGraph::default();
        for &r in roots {
            let mut on_stack = HashSet::new();
            self.plan_visit(r, 0, &mut graph, &mut on_stack);
        }
        self.evaluate(&graph);
    }

    /// Plan one cell into `graph` (recursively planning its dependencies), the PLAN-pass DFS.
    ///
    /// `depth` is the number of formula ancestors above this cell (the demanded root is depth 0);
    /// `on_stack` is the current DFS path, for cycle detection. A cell already in the graph is a shared
    /// node (returned at once — the merge); a cell already in the memo is a resolved leaf (ENG4). A
    /// re-entered cell (on the stack) is a located `#REF!` cycle; a cell past [`MAX_PULL_DEPTH`] is a
    /// located `#NUM!`. Otherwise a formula's static references (ranges expanded to cells) become its
    /// dependencies and are planned before it becomes a `Formula` node.
    fn plan_visit(
        &self,
        key: CellKey,
        depth: u32,
        graph: &mut DepGraph,
        on_stack: &mut HashSet<CellKey>,
    ) {
        if graph.nodes.contains_key(&key) {
            return; // already planned this pass — the shared/merged node
        }
        if self.memo.borrow().contains_key(&key) {
            return; // a clean, memoized value — a resolved leaf (ENG4 reuse)
        }
        if on_stack.contains(&key) {
            // A reference cycle: the re-entered cell is a located `#REF!`. Its dependents propagate it.
            graph.nodes.insert(key, PlanNode::Cycle);
            self.refuse(self.cycle_diag(key));
            return;
        }
        let (sheet, col, row) = key;
        let Some((id, file)) = self.covering(sheet, col, row) else {
            return; // a gap reads Blank — not a node; the resolver reads it directly
        };
        let dr = row - file.region.min_row;
        let dc = col - file.region.min_col;
        // Only a formula cell needs a node; a literal is read straight from the grid at evaluate.
        let GridCell::Formula { expr, .. } = file.grid.cell_at(dr, dc) else {
            return;
        };
        if depth >= MAX_PULL_DEPTH {
            // Reached past the pull-depth bound: a located `#NUM!` recorded before descending further,
            // so the plan DFS cannot overflow the native stack.
            graph.nodes.insert(key, PlanNode::DepthRefused);
            self.refuse(self.depth_diag(id));
            return;
        }
        on_stack.insert(key);
        let deps = self.expr_deps(expr, sheet);
        for &d in &deps {
            self.plan_visit(d, depth + 1, graph, on_stack);
        }
        on_stack.remove(&key);
        // A dependency descent may have re-entered THIS cell (a cycle back-edge) and already marked it
        // a `Cycle` — or the depth guard may have marked it — so do not overwrite a terminal verdict.
        if !matches!(
            graph.nodes.get(&key),
            Some(PlanNode::Cycle | PlanNode::DepthRefused)
        ) {
            graph.nodes.insert(
                key,
                PlanNode::Formula {
                    file: id,
                    dr,
                    dc,
                    deps,
                },
            );
        }
    }

    /// The dependency cells of a formula's parsed tree, resolved to `(sheet, col, row)` keys against
    /// `home` (the sheet an unqualified reference binds to). Every reference is static in v1 (there are
    /// no reference-forging functions — `INDIRECT`/`OFFSET` are reserved refusals), so this is the
    /// formula's complete dependency set. A range expands to its cells; a range over
    /// [`MAX_RANGE_CELLS`] is left unexpanded (the resolver refuses it as `#NUM!` at evaluate rather
    /// than allocating a key per cell); an unknown sheet name resolves to no cell (the evaluator maps
    /// it to `#REF!`).
    fn expr_deps(&self, expr: &Expr, home: u32) -> Vec<CellKey> {
        let mut out = Vec::new();
        self.collect_deps(expr, home, &mut out);
        out
    }

    fn collect_deps(&self, expr: &Expr, home: u32, out: &mut Vec<CellKey>) {
        match expr {
            Expr::Lit(_) => {}
            Expr::Ref(r) => {
                if let Some(cr) = r.resolve(|name| self.sheet_id(name)) {
                    let s = cr.sheet.map_or(home, |SheetId(i)| i);
                    out.push((s, cr.col, cr.row));
                }
            }
            Expr::Range(rn) => {
                if let Some(rr) = rn.resolve(|name| self.sheet_id(name)) {
                    let s = rr.start.sheet.map_or(home, |SheetId(i)| i);
                    let c0 = rr.start.col.min(rr.end.col);
                    let c1 = rr.start.col.max(rr.end.col);
                    let r0 = rr.start.row.min(rr.end.row);
                    let r1 = rr.start.row.max(rr.end.row);
                    let area = (u64::from(r1 - r0) + 1) * (u64::from(c1 - c0) + 1);
                    if area <= MAX_RANGE_CELLS {
                        for row in r0..=r1 {
                            for col in c0..=c1 {
                                out.push((s, col, row));
                            }
                        }
                    }
                    // Over the bound: left unexpanded; `Resolver::range` refuses it at evaluate.
                }
            }
            Expr::Unary(_, e) => self.collect_deps(e, home, out),
            Expr::Binary(_, a, b) => {
                self.collect_deps(a, home, out);
                self.collect_deps(b, home, out);
            }
            Expr::Call(_, args) => {
                for a in args {
                    self.collect_deps(a, home, out);
                }
            }
            Expr::ImplicitIntersect(e) => self.collect_deps(e, home, out),
            Expr::SpillRef(e) => self.collect_deps(e, home, out),
        }
    }

    // ------------------------------------------------------------------------------------------
    // EVALUATE pass — compute each graph node once, in dependency order.
    // ------------------------------------------------------------------------------------------

    /// Compute every node of `graph` exactly once (ENG2), each after its dependencies. A terminal
    /// (`Cycle`/`DepthRefused`) yields its located error; a `Formula` node evaluates its tree through
    /// [`charlie_ast::eval`] — during which the [`Resolver`] reads the already-computed dependency
    /// values from the pass results. A value that consumed a depth refusal is marked tainted so it is
    /// not memoized.
    fn evaluate(&self, graph: &DepGraph) {
        for key in self.topo_order(graph) {
            match graph.nodes.get(&key) {
                Some(PlanNode::Cycle) => {
                    // A cycle is a permanent, content-deterministic `#REF!` — clean, so it memoizes.
                    self.results
                        .borrow_mut()
                        .insert(key, Value::Error(ErrKind::Ref));
                }
                Some(PlanNode::DepthRefused) => {
                    self.results
                        .borrow_mut()
                        .insert(key, Value::Error(ErrKind::Num));
                    self.pass_tainted.borrow_mut().insert(key);
                }
                Some(PlanNode::Formula { file, dr, dc, deps }) => {
                    let tainted = {
                        let t = self.pass_tainted.borrow();
                        deps.iter().any(|d| t.contains(d))
                    };
                    self.current_sheet.set(file.0);
                    self.current_file.set(Some(*file));
                    let v = self.compute_formula(*file, *dr, *dc);
                    self.results.borrow_mut().insert(key, v);
                    if tainted {
                        self.pass_tainted.borrow_mut().insert(key);
                    }
                }
                None => {}
            }
        }
    }

    /// A dependency order of `graph`'s nodes (each node after every dependency that is itself a node).
    /// Iterative post-order DFS — deliberately not recursive, so evaluation order is stack-safe for any
    /// DAG the plan built (and independent nodes could later be evaluated in parallel). Cycles were
    /// already broken in the plan (the re-entered cell is a terminal), so the graph handed here is a
    /// DAG.
    fn topo_order(&self, graph: &DepGraph) -> Vec<CellKey> {
        let mut order = Vec::with_capacity(graph.nodes.len());
        let mut seen: HashSet<CellKey> = HashSet::new();
        // Seed the DFS from a deterministic (sorted) root order so diagnostics pushed during the
        // evaluate pass (e.g. RangeTooLarge) have a stable order run-to-run. Values are unaffected
        // either way — topo order is respected within each cone — and independent nodes could still be
        // evaluated in parallel later.
        let mut roots: Vec<CellKey> = graph.nodes.keys().copied().collect();
        roots.sort_unstable();
        for root in roots {
            if seen.contains(&root) {
                continue;
            }
            let mut stack = vec![(root, false)];
            while let Some((k, emit)) = stack.pop() {
                if emit {
                    order.push(k);
                    continue;
                }
                if !seen.insert(k) {
                    continue;
                }
                stack.push((k, true));
                if let Some(PlanNode::Formula { deps, .. }) = graph.nodes.get(&k) {
                    for &d in deps {
                        if graph.nodes.contains_key(&d) && !seen.contains(&d) {
                            stack.push((d, false));
                        }
                    }
                }
            }
        }
        order
    }

    /// Promote the pass's clean (non-tainted) results into the memo (ENG4 reuse) and clear the pass
    /// scratch. A depth-tainted value is dropped, not memoized — its `#NUM!` is root-relative, so a
    /// later shallower demand must recompute it.
    fn finish_pass(&self) {
        {
            let results = self.results.borrow();
            let tainted = self.pass_tainted.borrow();
            let mut memo = self.memo.borrow_mut();
            for (k, v) in results.iter() {
                if !tainted.contains(k) {
                    memo.insert(*k, v.clone());
                }
            }
        }
        self.results.borrow_mut().clear();
        self.pass_tainted.borrow_mut().clear();
    }

    /// The value of the covering file's grid cell at offset `(dr, dc)`. The cell holds a parsed `Expr`
    /// (VAL1: evaluated exactly as written — no offset/drag-fill), collapsed to a scalar for its scalar
    /// cell position by [`scalar_cell`]. Called from the EVALUATE pass with `current_sheet`/
    /// `current_file` already set to this cell's context; the [`Resolver`] it evaluates against only
    /// READS already-computed dependency values (never recurses).
    fn compute_formula(&self, id: FileId, dr: u32, dc: u32) -> Value {
        let file = &self.tabs[id.0 as usize].files[id.1];
        match file.grid.cell_at(dr, dc) {
            GridCell::Formula { expr, .. } => scalar_cell(eval(expr, self)),
            // `compute_formula` is only reached for a formula node; a literal is total-passed-through
            // defensively rather than panicking.
            GridCell::Value(v) => v.clone(),
        }
    }

    /// The located `#REF!`-class cycle refusal for a re-entered cell, anchored on its sheet-qualified
    /// file.
    fn cycle_diag(&self, key: CellKey) -> Diagnostic {
        let (id, _) = self
            .covering(key.0, key.1, key.2)
            .expect("a cycle cell is a formula cell and is therefore covered");
        let name = self.file_name(id);
        let tab = self.tab_name(id.0);
        Diagnostic::new(
            Code::Cycle,
            Loc::tab_file(&tab, &name),
            format!(
                "circular reference: evaluating {tab}/{name} re-entered it through a chain of \
                 cell references (a cross-sheet chain counts) -- refused as #REF!-class rather \
                 than looping"
            ),
        )
    }

    /// The located `#NUM!`-class depth refusal for a cell reached past [`MAX_PULL_DEPTH`], anchored on
    /// its sheet-qualified file.
    fn depth_diag(&self, id: FileId) -> Diagnostic {
        let name = self.file_name(id);
        let tab = self.tab_name(id.0);
        Diagnostic::new(
            Code::DepthLimit,
            Loc::tab_file(&tab, &name),
            format!(
                "formula dependency chain exceeded the pull-depth bound of {MAX_PULL_DEPTH} at \
                 {tab}/{name} -- refused as #NUM!-class rather than overflowing the stack"
            ),
        )
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
    /// READ a cell's value — during the EVALUATE pass the value has already been computed (a formula
    /// node) or is read straight from the grid (a literal or a gap). The pass results win, then the
    /// memo, then the grid; a formula cell is never recomputed here (the plan guaranteed it computes
    /// first, in dependency order).
    fn value(&self, cell: CellRef) -> Value {
        let sheet = self.resolve_sheet(cell.sheet);
        let key = (sheet, cell.col, cell.row);
        if let Some(v) = self.results.borrow().get(&key) {
            return v.clone();
        }
        if let Some(v) = self.memo.borrow().get(&key) {
            return v.clone();
        }
        let Some((_, file)) = self.covering(sheet, cell.col, cell.row) else {
            // A gap (no file claims this cell) reads as Blank (the overlap policy: gaps are Blank).
            return Value::Blank;
        };
        let dr = cell.row - file.region.min_row;
        let dc = cell.col - file.region.min_col;
        match file.grid.cell_at(dr, dc) {
            GridCell::Value(v) => v.clone(),
            // A formula cell that is neither in the results nor the memo was not planned — unreachable
            // in a proper demand (the plan is a superset of what eval reads). The debug_assert fails
            // loud in tests if a future planning change under-approximates deps (fail-fast); release
            // stays total and never panics (CORE2).
            GridCell::Formula { .. } => {
                debug_assert!(
                    false,
                    "unplanned formula cell read at ({sheet}, {}, {})",
                    cell.col, cell.row
                );
                Value::Blank
            }
        }
    }

    fn range(&self, range: RangeRef) -> ArrayView<'_> {
        // Resolve `sheet: None` to the current context and key the arena by the qualified range, so a
        // memoized `A1:A3` on one sheet is never mistaken for `A1:A3` on another. Normalize the key's
        // corners to canonical min/max so a reversed spelling (`B2:A1`) maps to the SAME arena entry
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
            // resolves to a single #NUM! cell that the referencing aggregation propagates. The
            // refusal is deterministic (a function of the range size, not of order), so it caches.
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
        // Materialize the rectangle by READING each cell (its value was computed earlier in the pass,
        // as a planned dependency). If any cell is DEPTH-TAINTED, the buffer's `#NUM!` is a function of
        // the DEPTH the range was first demanded at, not of the range — caching it would poison a later
        // shallower (computable) demand — so return a borrowed view over a stable buffer WITHOUT
        // recording the key (mirrors the per-cell memo's depth guard). No arena borrow is held across
        // these `value` calls, which may recursively push more range buffers.
        let mut buf = Vec::with_capacity((rows as usize) * (cols as usize));
        let mut tainted = false;
        for r in r0..=r1 {
            for c in c0..=c1 {
                if self.pass_tainted.borrow().contains(&(eff.0, c, r)) {
                    tainted = true;
                }
                buf.push(self.value(CellRef {
                    col: c,
                    row: r,
                    sheet: Some(eff),
                }));
            }
        }
        let shape = Shape { rows, cols };
        if tainted {
            self.arena.insert_uncached(shape, buf)
        } else {
            self.arena.insert(key, shape, buf)
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
    /// Used for a DEPTH-TAINTED buffer (a cell of the range consumed a depth refusal): its `#NUM!` is a
    /// function of the depth the range was first reached at, not the range, so committing it to the
    /// keyed cache would poison a later shallower (computable) demand. Keeps range evaluation
    /// order-independent, mirroring the per-cell memo's depth guard in [`Workbook::finish_pass`].
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
        // A diamond that, WITHOUT single-node sharing, re-evaluates the shared base exponentially:
        // each level references the one below TWICE, so a naive re-eval is 2^depth. The two-pass graph
        // merges the shared node so it is linear and returns instantly. A1=1; each A{n}=A{n+1}+A{n+1}
        // down a long column, so A1 = 2^(len-1). Reaching the assert at all proves no exponential hang.
        let len = 40usize; // 2^39 ~ 5.5e11 re-evals if exponential; instant if shared
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
        // A finite, entirely acyclic chain deeper than the bound. The cycle detector never trips
        // (nothing is re-entered), so ONLY the pull-depth guard stands between the plan DFS and a
        // native stack overflow: reaching the assertions at all proves no SIGABRT. The deepest link is
        // a located #NUM!-class refusal that propagates up to the requested top cell.
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

    #[test]
    fn a_shared_dependency_computes_once_across_a_batch_render() {
        // ENG3 sharing: a viewport demanded via `values_at` builds ONE merged graph, so a dependency
        // referenced by several viewport cells is computed once. A1=2 (shared base); B1=A1+1, C1=A1+1
        // (both read A1); the batch returns all three from one pass.
        let wb = load_one_tab("Sheet1", &[("A1", "2"), ("B1", "=A1+1"), ("C1", "=A1+1")]);
        let vals = wb.values_at(&[(0, 0, 0), (0, 1, 0), (0, 2, 0)]);
        assert_eq!(
            vals,
            vec![Value::Number(2.0), Value::Number(3.0), Value::Number(3.0)]
        );
        assert!(wb.eval_diagnostics().is_empty());
    }

    // ------------------------------------------------------------------------------------------
    // NAIVE reference oracle + the ENG3 differential test (the graph EQUALS a naive per-cell eval).
    // ------------------------------------------------------------------------------------------

    /// An independent, dead-simple per-cell evaluator — the TEST-ONLY reference the differential test
    /// grades the two-pass engine against. It evaluates a demanded cell straight from the grids by
    /// native recursion through `charlie_ast::eval`, with a `visiting` set for basic cycle protection
    /// and a tiny memo so a shared/diamond ancestor does not re-evaluate exponentially. It shares NONE
    /// of the two-pass algorithm (no `DepGraph`, no plan, no `topo_order`) — only the workbook's
    /// structural cell-location plumbing (`covering`) and the `Arena`/`scalar_cell` resolver helpers,
    /// which are not evaluation logic. Its verdict is VALUES; the differential test asserts nothing
    /// about how either side reaches them.
    ///
    /// NAMED COVERAGE BOUNDARY: this oracle has no pull-depth guard and unconditionally memoizes every
    /// result, so it structurally cannot model the two-pass path's depth-tainted `#NUM!` — that value is
    /// deliberately NOT memoized and is root-relative/order-dependent (see `MAX_PULL_DEPTH`,
    /// `finish_pass`, and the range-materialization `#NUM!` bound). The differential cases here therefore
    /// exercise only CLEAN-value shapes (diamond, deep-but-under-bound chain, cross-tab, shared range).
    /// The memo/taint interactions the oracle can't represent are frozen separately by single-path
    /// `assert_eq` tests (`a_legal_deep_chain…`, the `#NUM!` depth tests, the range-too-large test) —
    /// they are not graded against this oracle.
    struct NaiveOracle<'w> {
        wb: &'w Workbook,
        cur: Cell<u32>,
        memo: RefCell<HashMap<CellKey, Value>>,
        visiting: RefCell<HashSet<CellKey>>,
        arena: Arena,
    }

    impl<'w> NaiveOracle<'w> {
        fn new(wb: &'w Workbook) -> NaiveOracle<'w> {
            NaiveOracle {
                wb,
                cur: Cell::new(0),
                memo: RefCell::new(HashMap::new()),
                visiting: RefCell::new(HashSet::new()),
                arena: Arena::default(),
            }
        }

        fn eval_cell(&self, sheet: u32, col: u32, row: u32) -> Value {
            self.value(CellRef {
                col,
                row,
                sheet: Some(SheetId(sheet)),
            })
        }
    }

    impl Resolver for NaiveOracle<'_> {
        fn value(&self, cell: CellRef) -> Value {
            let sheet = cell.sheet.map_or_else(|| self.cur.get(), |SheetId(i)| i);
            let key = (sheet, cell.col, cell.row);
            if let Some(v) = self.memo.borrow().get(&key) {
                return v.clone();
            }
            if self.visiting.borrow().contains(&key) {
                return Value::Error(ErrKind::Ref); // basic cycle protection
            }
            let Some((id, file)) = self.wb.covering(sheet, cell.col, cell.row) else {
                return Value::Blank;
            };
            let dr = cell.row - file.region.min_row;
            let dc = cell.col - file.region.min_col;
            let v = match file.grid.cell_at(dr, dc) {
                GridCell::Value(v) => v.clone(),
                GridCell::Formula { expr, .. } => {
                    self.visiting.borrow_mut().insert(key);
                    let prev = self.cur.replace(id.0);
                    let r = scalar_cell(eval(expr, self));
                    self.cur.set(prev);
                    self.visiting.borrow_mut().remove(&key);
                    r
                }
            };
            self.memo.borrow_mut().insert(key, v.clone());
            v
        }

        fn range(&self, range: RangeRef) -> ArrayView<'_> {
            let eff = SheetId(cell_sheet(range.start.sheet, self.cur.get()));
            let c0 = range.start.col.min(range.end.col);
            let c1 = range.start.col.max(range.end.col);
            let r0 = range.start.row.min(range.end.row);
            let r1 = range.start.row.max(range.end.row);
            let (rows, cols) = (r1 - r0 + 1, c1 - c0 + 1);
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
            self.arena.insert(key, Shape { rows, cols }, buf)
        }

        fn sheet_id(&self, name: &str) -> Option<SheetId> {
            self.wb.sheet_id(name)
        }

        fn now_serial(&self) -> f64 {
            self.wb.now
        }
    }

    fn cell_sheet(sheet: Option<SheetId>, home: u32) -> u32 {
        sheet.map_or(home, |SheetId(i)| i)
    }

    /// Assert the naive oracle and the two-pass engine agree on the VALUE of every demanded cell —
    /// values only, never the graph's shape/node-count/traversal order (asserting internals would
    /// freeze "how" and block a future parallel-execution refactor).
    fn assert_agrees(wb: &Workbook, cells: &[(u32, u32, u32)]) {
        let oracle = NaiveOracle::new(wb);
        // Interleave demands so the two-pass memo/arena is exercised across cells, and evaluate the
        // batch through the merged-graph path too.
        let batch = wb.values_at(cells);
        for (&(s, c, r), two_pass) in cells.iter().zip(batch) {
            let naive = oracle.eval_cell(s, c, r);
            assert_eq!(
                naive, two_pass,
                "naive vs two-pass diverge at (sheet {s}, col {c}, row {r}): \
                 naive={naive:?} two_pass={two_pass:?}"
            );
        }
    }

    #[test]
    fn differential_diamond_shared_ancestor() {
        // A diamond: one shared base A1 reached by many cells and by a two-path top. B1,C1 both read
        // A1; D1 reads B1 and C1 (A1 via two paths); wide fan-out E1,F1 also read A1.
        let wb = load_one_tab(
            "Sheet1",
            &[
                ("A1", "3"),
                ("B1", "=A1+1"),
                ("C1", "=A1*2"),
                ("D1", "=B1+C1"),
                ("E1", "=A1*10"),
                ("F1", "=A1-1+E1"),
            ],
        );
        assert_agrees(
            &wb,
            &[
                (0, 0, 0),
                (0, 1, 0),
                (0, 2, 0),
                (0, 3, 0),
                (0, 4, 0),
                (0, 5, 0),
            ],
        );
    }

    #[test]
    fn differential_deep_linear_chain() {
        // A deep (but under-bound) linear chain shared by a top demand and interior demands.
        let len = 60usize;
        let owned = chain_files(len); // A1=A2+1, ..., A60=0
        let refs: Vec<(&str, &str)> = owned
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_str()))
            .collect();
        let wb = load_one_tab("Sheet1", &refs);
        assert_agrees(&wb, &[(0, 0, 0), (0, 0, 29), (0, 0, 59), (0, 0, 10)]);
    }

    #[test]
    fn differential_cross_tab_shared_ancestor() {
        // A cross-tab shared ancestor: Base!A1 feeds several formulas on two other tabs, and a cell
        // that combines both tabs (a cross-tab diamond bottoming out at Base!A1).
        let wb = Workbook::from_tabs(&[
            ("Base", &[("A1", &file("100"))]),
            (
                "R1",
                &[("A1", &file("=Base!A1*2")), ("A2", &file("=Base!A1+A1"))],
            ),
            (
                "R2",
                &[
                    ("A1", &file("=Base!A1-10")),
                    ("A2", &file("=R1!A1+R2!A1+Base!A1")),
                ],
            ),
        ])
        .expect("loads clean");
        assert_agrees(
            &wb,
            &[
                (0, 0, 0), // Base!A1
                (1, 0, 0), // R1!A1
                (1, 0, 1), // R1!A2
                (2, 0, 0), // R2!A1
                (2, 0, 1), // R2!A2 (combines both tabs + Base)
            ],
        );
    }

    #[test]
    fn differential_one_large_range_aggregated_by_several_cells() {
        // One large shared range (a 100-cell column) aggregated by several cells: SUM, an offset SUM,
        // and a formula that reads two aggregates. Every aggregate shares the SAME 100 ancestor cells.
        let column: String = (1..=100)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let wb = load_one_tab(
            "Sheet1",
            &[
                ("A1:A100", column.as_str()),
                ("C1", "=SUM(A1:A100)"),
                ("C2", "=SUM(A1:A100)+1"),
                ("C3", "=C1+C2"),
                ("C4", "=AVERAGE(A1:A100)"),
            ],
        );
        assert_agrees(
            &wb,
            &[(0, 2, 0), (0, 2, 1), (0, 2, 2), (0, 2, 3), (0, 0, 49)],
        );
    }
}
