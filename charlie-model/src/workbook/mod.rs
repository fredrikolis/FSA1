// Concern: the TWO-PASS evaluation engine's ROOT (ENG3) — the `Workbook` struct + its PUBLIC surface (load via `from_tabs`/`load_dir`; demand-driven reads `value_at`/`values_at`; the render surface `sheet_names`/`used_region`/`source_at`; `lint`; the ad-hoc `eval_formula` returning a `FormulaOutcome`; `CellSource`) and the demand ORCHESTRATION that sequences the two passes, plus the shared structural helpers (`covering`/`tab_name`/`file_name`/`refuse`) and engine invariants (`MAX_PULL_DEPTH`/`MAX_RANGE_CELLS`) the passes reuse; the LOADER also separates FS4 NAME entries from cell/grid files (reading a symlink's target or a ref-file's content off disk in `load_dir`, since the pure `names` module is filesystem-blind), builds the `NameTable`, and RESOLVES each formula's name tokens to A1/expr at load (a source rewrite, keeping the engine A1-only, ENG1); the passes themselves live in the private sibling submodules — `plan` (the `DepGraph`/`PlanNode` graph + PLAN pass), `evaluate` (the EVALUATE pass), `resolver` (the `Resolver` impl + range-materialization `Arena`) — none of whose engine types escape this module (ENG3 containment: `DepGraph`/`PlanNode`/`Arena` are `pub(super)` at most, re-exported by no one) | Non-concern: the formula LANGUAGE (charlie-ast owns lex/parse/eval), the filename/grid/overlap GRAMMAR (this reuses `parse_file`/`detect_overlaps`), the name-table LOGIC + rewrite (the `names` module owns it), xlsx serde, the CLI render surface, and the PLAN/EVALUATE/resolver pass bodies (the submodules own them) | IO: (a sheet-directory or in-memory tabs) -> a `Workbook`; then (a demanded cell / a viewport of cells) -> `Value`s, or (a tab index + an ad-hoc `=formula` string) -> a `Result<FormulaOutcome, Diagnostic>`, plus the eval-time located `Diagnostic`s it accumulates
//! Two-pass evaluation (ENG3): [`Workbook`] loads a sheet-directory and implements [`Resolver`] over
//! it. A demand (one cell via [`Workbook::value_at`], a viewport via [`Workbook::values_at`], or an
//! ad-hoc formula via [`Workbook::eval_formula`]) runs a PLAN pass that builds one private dependency
//! graph of the demanded cells and their transitive dependencies (ranges expand to cells; a shared
//! dependency is one merged node), then an EVALUATE pass that computes each node once in dependency
//! order through `charlie_ast::eval`. The graph is a contained optimization — its type never leaves
//! this module and it EQUALS a naive per-cell evaluation (proven by the differential test below). The
//! engine stays memoized (ENG4), cycle-safe (a reference cycle is a located `#REF!`), and lazy (an
//! off-request cell never computes).

mod cache;
mod evaluate;
mod forge;
mod hash;
mod plan;
mod resolver;
#[cfg(test)]
mod tests;
mod trace;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use charlie_ast::{
    CellRef, Expr, Resolver, SheetId, Value, eval, parse, system_now_secs, unix_secs_to_serial,
};

use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::grid::{Cell as GridCell, Grid};
use crate::names::{NameRepr, NameScope, NameTable, RawNameEntry, is_cell_filename};
use crate::overlap::{Rect, detect_overlaps};
use crate::{ParsedFile, parse_file};

use cache::{CacheScan, ResultCache};
use forge::ForgeStore;
use plan::DepGraph;
use resolver::Arena;

pub use trace::{Direction, TraceNode, TraceStatus};

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
///
/// `array_formula` marks a GRID5 ARRAY-FORMULA REGION: the whole file is one `=formula` (the lone
/// `1x1` `grid` cell) whose declared `region` spans more than one coordinate. The engine evaluates it
/// ONCE at the region's top-left and fills every coordinate with the matching array element (VAL1: one
/// array-formula cell spanning its range, not many cells).
#[derive(Clone, Debug)]
struct LoadedFile {
    name: String,
    region: Rect,
    grid: Grid,
    array_formula: bool,
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

/// One tab folder's contents as read from disk: its CELL/GRID `(filename, content)` files and its
/// sheet-scoped FS4 name entries (in either representation).
type TabParts = (Vec<(String, String)>, Vec<RawNameEntry>);

/// The identity of one rendered cell: `(sheet index, zero-based col, zero-based row)`. Graph nodes,
/// the per-cell memo, and the per-pass results/taint sets are keyed by this — each grid cell is a
/// DISTINCT computation, so the graph and the caches are per cell, not per file.
type CellKey = (u32, u32, u32);

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
    /// The workbook's FS4 name table (built at load). A stored formula's name tokens are already
    /// resolved to A1 in the grid, but the AD-HOC [`Workbook::eval_formula`] parses a fresh formula, so
    /// it consults this to resolve any name the caller writes (`=SUM(Days)`) — keeping ad-hoc eval and a
    /// stored formula on the same name semantics. Empty when the workbook has no names.
    names: NameTable,
    /// Whether ANY loaded file is a GRID5 array-formula region. The plan/dep passes only pay the
    /// per-coordinate "is this an array region?" redirect cost when this is `true`, so a workbook with
    /// no array regions (the overwhelming common case) plans exactly as it did before GRID5.
    has_array_regions: bool,
    /// Whether ANY loaded formula contains a reference-forging call (`INDIRECT`/`OFFSET`, ENG6), set at
    /// load exactly as `has_array_regions` is. The ZERO-OVERHEAD gate: when `false`, [`Workbook::demand`]
    /// skips the forge Pass 0 on a bool branch and [`Workbook::effective_expr`] returns the grid expr on
    /// a bool check — so a workbook with no forgers (the overwhelming common case) plans and evaluates
    /// byte-for-byte as it did before forging (a single plan->evaluate), the forge module untouched.
    has_forgers: bool,
    /// The forge REWRITE store (ENG6): a demanded forger's `Call` subtree source-rewritten to a static
    /// `Expr::Ref`/`Expr::Range`, keyed by its cell and handed to the [`Workbook::effective_expr`] seam.
    /// Address-stable under `&self` (the append-only `Arena` idiom) so the seam returns `&Expr`. Empty
    /// and never touched when `has_forgers` is `false`. Filled lazily by the demand-driven Pass 0, it
    /// persists across demands for the immutable workbook's lifetime (a forge target is stable, ENG4).
    forge: ForgeStore,
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
    /// An append-only arena backing the borrowed [`ArrayView`](charlie_ast::ArrayView)s that
    /// [`Resolver::range`] returns.
    arena: Arena,
    /// Located refusals surfaced during evaluation (cycles, depth limits, over-large ranges, spills).
    /// Load-time refusals are returned by the loader; these accumulate as cells are planned/pulled.
    diagnostics: RefCell<Vec<Diagnostic>>,
    /// The ENG4 PERSISTENT result cache under `<workbook>/.cache/`, or `None` when caching is off — an
    /// in-memory [`Workbook::from_tabs`] has no filesystem (ENG5), and `--no-cache` clears it
    /// ([`Workbook::disable_cache`]). Attached only by [`Workbook::load_dir`], which knows the root
    /// path. A contained optimization (ENG3/VAL2): deleting `.cache/` changes performance, never
    /// values (the `cache` sibling owns the read short-circuit + the atomic write).
    cache: Option<ResultCache>,
    /// A per-Workbook (i.e. per-invocation) count of formula EVALUATIONS actually performed — the
    /// test-visible instrument proving ENG4 reuse (a warm-cache re-run performs materially fewer evals
    /// because a cached subtree is served without evaluating). Incremented once per computed formula
    /// cell / array region; a cache hit adds nothing (the cell is never evaluated). `#[cfg(test)]`: the
    /// field, its increments, and its `eval_count()` reader compile ONLY under test, so a production
    /// build carries and mutates no test-only instrument in the hot per-formula path.
    #[cfg(test)]
    eval_count: Cell<u64>,
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
/// render surface shows in `--functions` mode. Borrows the workbook, so it cannot outlive it.
#[derive(Clone, Copy, Debug)]
pub struct CellSource<'a> {
    /// The covering file's name (`A1`, `A3:G8`).
    pub file_name: &'a str,
    /// The declared region the file claims.
    pub region: Rect,
    /// The specific grid cell at the requested coordinate — a parsed `=formula` (with its source
    /// text) or a literal value (un-evaluated). For a GRID5 array-formula region, this is always the
    /// region's single `=formula` (the file's lone grid cell), whatever coordinate was requested.
    pub cell: &'a GridCell,
    /// `true` iff the requested coordinate is a CONTINUATION cell of a GRID5 array-formula region — a
    /// cell filled by the array formula anchored at the region's TOP-LEFT, but not the anchor itself.
    /// The `--functions` render marks it rather than re-printing the formula at every coordinate (the
    /// formula lives once, at the anchor — VAL1). `false` for the anchor and for every non-region cell.
    pub array_continuation: bool,
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
        // FS4 name entries, in EITHER representation (symlink / ref-file), read here where the fs is
        // present (the pure `names` module never touches the filesystem): workbook-scoped from the root,
        // sheet-scoped from each tab folder.
        let mut raw_names: Vec<RawNameEntry> = Vec::new();
        let mut entries: Vec<_> = std::fs::read_dir(root)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let ft = entry.file_type()?;
            let entry_name = entry.file_name().to_string_lossy().into_owned();
            if ft.is_dir() {
                // FS3: the reserved `.cache/` sub-folder is NOT a tab — it holds the regenerable,
                // non-authoritative ENG4 result cache. Every OTHER sub-folder is a tab (FS1).
                if entry_name == ".cache" {
                    continue;
                }
                let (files, names) = read_tab_dir(root, &entry_name, &entry.path())?;
                tabs.push((entry_name, files));
                raw_names.extend(names);
            } else if let Some(name) =
                read_name_entry(root, NameScope::Workbook, &entry_name, &entry.path(), ft)?
            {
                // A root-level symlink or non-A1 regular file is a WORKBOOK-scoped name (FS4). A
                // root-level A1-shaped regular file is neither a tab nor a name — ignored.
                raw_names.push(name);
            }
        }
        // Attach the persistent ENG4 cache at `<root>/.cache/` (FS3). A load-time refusal keeps its
        // `Err`; only a workbook that loads gets a cache.
        Ok(Workbook::from_dir_parts(tabs, raw_names)
            .map(|wb| wb.with_cache_dir(root.join(".cache"))))
    }

    /// Attach the persistent result cache rooted at `dir` (`<workbook>/.cache/`). Consuming builder so
    /// [`Workbook::load_dir`] can wire the cache the in-memory loader knows nothing about (ENG5).
    fn with_cache_dir(mut self, dir: std::path::PathBuf) -> Workbook {
        self.cache = Some(ResultCache::new(dir));
        self
    }

    /// Turn the persistent cache OFF for this workbook — the ENG4 testing bypass behind the
    /// `--no-cache` CLI flag. Bypasses BOTH the read short-circuit and the write (the cache field is
    /// the single gate both consult), so a `--no-cache` run neither reads nor writes `.cache/` and
    /// yields identical values (VAL2: no value ever derived from the cache).
    pub fn disable_cache(&mut self) {
        self.cache = None;
    }

    /// The in-memory loader over owned strings: partitions each tab's files into cell/grid files and
    /// sheet-scoped ref-file NAME entries (FS4 — an in-memory workbook has no symlinks, so only the
    /// ref-file representation), builds the name table, and assembles. `load_dir` uses
    /// [`Workbook::from_dir_parts`] instead (it has already separated names and read symlinks).
    fn from_owned(tabs: Vec<(String, Vec<(String, String)>)>) -> Result<Workbook, Vec<Diagnostic>> {
        let mut cell_tabs = Vec::with_capacity(tabs.len());
        let mut raw_names = Vec::new();
        for (tab_name, files) in tabs {
            let mut cells = Vec::new();
            for (fname, contents) in files {
                if is_cell_filename(&fname) {
                    cells.push((fname, contents));
                } else {
                    // A non-A1 filename is a NAME entry in the ref-file representation (FS4).
                    raw_names.push(RawNameEntry {
                        scope: NameScope::Sheet(tab_name.clone()),
                        entry_name: fname,
                        form: NameRepr::RefFile { content: contents },
                    });
                }
            }
            cell_tabs.push((tab_name, cells));
        }
        Workbook::from_dir_parts(cell_tabs, raw_names)
    }

    /// Assemble a workbook from CELL/GRID files (already separated from names) plus the raw FS4 name
    /// entries. Builds the name table (collecting its located refusals), then loads each cell file with
    /// its formula name tokens RESOLVED to A1/expr against the table (the engine stays A1-only, ENG1) —
    /// the resolution is a source rewrite at load, analogous to a deserializer normalization (GRID3).
    fn from_dir_parts(
        tabs: Vec<(String, Vec<(String, String)>)>,
        raw_names: Vec<RawNameEntry>,
    ) -> Result<Workbook, Vec<Diagnostic>> {
        let (name_table, mut diags) = NameTable::build(raw_names);
        let mut out_tabs = Vec::with_capacity(tabs.len());
        for (tab_name, files) in tabs {
            let mut loaded = Vec::new();
            let mut regions: Vec<(String, Rect)> = Vec::new();
            for (fname, contents) in files {
                // Resolve name tokens in every `=formula` field to their A1/expr BEFORE deserializing,
                // so the grid the engine sees carries only A1 (an unresolvable name stays verbatim and
                // loads as a located `#NAME?`, GRID6/VAL3).
                let resolved = name_table.rewrite_tsv(&contents, &tab_name);
                match parse_file(&fname, &resolved) {
                    Ok(ParsedFile {
                        region,
                        declared_shape: _,
                        grid,
                        array_formula,
                    }) => {
                        regions.push((fname.clone(), region));
                        loaded.push(LoadedFile {
                            name: fname,
                            region,
                            grid,
                            array_formula,
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
            let has_array_regions = out_tabs
                .iter()
                .any(|t| t.files.iter().any(|f| f.array_formula));
            // The ENG6 forging gate, computed exactly like `has_array_regions`: scan every loaded
            // formula's parsed expr for an INDIRECT/OFFSET call. `false` short-circuits the forge pass.
            let has_forgers = out_tabs.iter().any(|t| {
                t.files.iter().any(|f| {
                    f.grid.cells.iter().any(|c| {
                        matches!(c, GridCell::Formula { expr, .. } if forge::expr_has_forger(expr))
                    })
                })
            });
            Ok(Workbook {
                tabs: out_tabs,
                names: name_table,
                has_array_regions,
                has_forgers,
                forge: ForgeStore::default(),
                now: system_now_serial(),
                current_sheet: Cell::new(0),
                memo: RefCell::new(HashMap::new()),
                results: RefCell::new(HashMap::new()),
                pass_tainted: RefCell::new(HashSet::new()),
                current_file: Cell::new(None),
                arena: Arena::default(),
                diagnostics: RefCell::new(Vec::new()),
                // In-memory (`from_tabs`) workbooks have no filesystem, so no cache (ENG5);
                // `load_dir` attaches one afterward via `with_cache_dir`.
                cache: None,
                #[cfg(test)]
                eval_count: Cell::new(0),
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
    /// ranges, spills). A stored formula's UNPARSEABLE body is not here — GRID6 makes it a load-time
    /// per-cell error surfaced by [`Workbook::lint`]'s load-error scan. Snapshot — call after driving cells.
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
        // Resolve any FS4 name the caller wrote (`=SUM(Days)`) to A1/expr against the ad-hoc home sheet,
        // so ad-hoc eval and a stored formula share the same name semantics (the engine stays A1-only).
        let resolved = self.names.rewrite_tsv(formula, &self.tab_name(sheet));
        let expr = parse(&resolved).map_err(|diag| {
            // The parser's byte span indexes the RESOLVED text. When a name expanded, that text is not
            // what the caller typed, so a precise column would point into the expansion — instead anchor
            // on the whole original formula and name the resolved form in the message. When no name
            // expanded (`resolved == formula`, the common ad-hoc case) the span maps 1:1 onto the input.
            let (loc, message) = if resolved == formula {
                (
                    Loc::body_span(
                        formula,
                        1,
                        (diag.span.start as u32) + 1,
                        1,
                        (diag.span.end as u32) + 1,
                    ),
                    format!("cannot parse formula {formula:?}: {}", diag.message),
                )
            } else {
                (
                    Loc::body_span(formula, 1, 1, 1, formula.chars().count() as u32 + 1),
                    format!(
                        "cannot parse formula {formula:?} (names resolved to {resolved:?}): {}",
                        diag.message
                    ),
                )
            };
            Diagnostic::new(Code::FormulaSyntax, loc, message)
        })?;
        // PLAN + EVALUATE the ad-hoc formula's dependency cone, then evaluate it — exactly the two
        // passes a stored formula rides (the shared [`Workbook::eval_root_expr`] core, which also runs
        // the forge Pass 0 so a dependency that is itself a forger cell resolves to its static form
        // before it is read). Unqualified refs resolve against `sheet`; a top-level eval-time refusal
        // anchors on `sheet`'s tab (the ad-hoc formula has no covering file). NOTE (bounded scope): a
        // forger written DIRECTLY in the ad-hoc formula (`eval "=OFFSET(...)"`) is not itself rewritten
        // — the forge pass is keyed by a stored cell — so it hits the `#REF!` backstop; a stored forger
        // cell it REFERENCES does forge. Rendering/reading workbook cells is the supported forge path.
        let value = self.eval_root_expr(&expr, sheet);
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
    /// and (un-evaluated) body — for the render `--functions` surface. `None` for a gap (no file
    /// claims the cell). Overlaps are rejected at load, so at most one file covers a cell.
    pub fn source_at(&self, sheet: u32, col: u32, row: u32) -> Option<CellSource<'_>> {
        let (_, file) = self.covering(sheet, col, row)?;
        // A GRID5 array-formula region has one grid cell (its `=formula`, at grid (0,0)); every
        // coordinate maps to that cell. The anchor (top-left) renders the formula; a continuation cell
        // is flagged so `--functions` marks it instead of re-printing the shared formula.
        let (dr, dc, array_continuation) = if file.array_formula {
            let is_anchor = row == file.region.min_row && col == file.region.min_col;
            (0, 0, !is_anchor)
        } else {
            (row - file.region.min_row, col - file.region.min_col, false)
        };
        Some(CellSource {
            file_name: &file.name,
            region: file.region,
            cell: file.grid.cell_at(dr, dc),
            array_continuation,
        })
    }

    /// Lint the whole workbook: report every located fault a loaded workbook can still carry, in file
    /// order first then eval order. Two sources:
    /// * GRID6 LOAD-ERROR cells — an unparseable/unsupported formula deserialized to a located error
    ///   value (VAL3) rather than aborting the load; the load-error scan in
    ///   [`lint_located`](Workbook::lint_located) collects them so `check` reports each with its
    ///   location and a non-zero exit (never a silent drop).
    /// * EVAL-time refusals — cycles, over-deep chains (`#NUM!`-class), over-large ranges, and
    ///   formula-result dimension mismatches (`#SPILL!`-class) — surfaced by driving every cell.
    ///
    /// Structural load-time refusals (overlap, literal dimension mismatch, bad filenames) abort the
    /// load itself and surface from the loader's `Err`, not here.
    pub fn lint(&self) -> Vec<Diagnostic> {
        self.lint_located().into_iter().map(|(_, d)| d).collect()
    }

    /// Lint, but keep only the diagnostics whose location falls within `scope` (CLI1) — the
    /// `charlie-cli check --tab/--range/--cell` surface. An unscoped [`Scope`] returns the full
    /// [`lint`](Workbook::lint) verbatim (whole-workbook check, unchanged). A scoped one filters on each
    /// diagnostic's TRUE tab (resolved here, since a bare-filename GRID6 loc is ambiguous across tabs,
    /// [`Loc::Body`]) and its cell region (from its loc); a file-level diagnostic with no cell region is
    /// kept iff its tab is in scope (see [`Scope::includes`]). Read-only, exactly like `lint`.
    pub fn lint_scoped(&self, scope: &crate::scope::Scope) -> Vec<Diagnostic> {
        let located = self.lint_located();
        if !scope.is_scoped() {
            return located.into_iter().map(|(_, d)| d).collect();
        }
        located
            .into_iter()
            .filter(|(sheet, d)| {
                // The loc supplies the region; the TRUE tab is the file's enclosing tab (the loc alone
                // cannot disambiguate a bare filename across tabs), so pass the resolved tab name.
                let (_loc_tab, region) = crate::scope::loc_target(&d.loc);
                scope.includes(Some(&self.tab_name(*sheet)), region)
            })
            .map(|(_, d)| d)
            .collect()
    }

    /// [`lint`](Workbook::lint), but each diagnostic paired with the 0-based index of the tab it belongs
    /// to. The pairing resolves the ambiguity a bare-filename loc cannot express: a GRID6 load error is
    /// located as `Body{file}` (no tab), yet the same address can exist on two tabs — here the enclosing
    /// tab is known from the iteration, so [`lint_scoped`](Workbook::lint_scoped) can filter on the true
    /// tab. Order matches `lint`: GRID6 load-error cells first (tab -> file -> row-major), then the
    /// deduped eval-time refusals.
    fn lint_located(&self) -> Vec<(u32, Diagnostic)> {
        // GRID6 load-error cells first — a per-cell located refusal that did not abort the load. The
        // enclosing tab index is known here (the loc's `Body{file}` is not tab-qualified).
        let mut out: Vec<(u32, Diagnostic)> = Vec::new();
        for (s, tab) in self.tabs.iter().enumerate() {
            for file in &tab.files {
                for cell in &file.grid.cells {
                    if let GridCell::LoadError { diag, .. } = cell {
                        out.push((s as u32, diag.clone()));
                    }
                }
            }
        }
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
        let mut eval = self.eval_diagnostics();
        eval.dedup();
        // An eval-time refusal is anchored on a `TabFile`/`Tab` loc that names its tab (plan/resolver),
        // so resolve the sheet index from that name (a fallback of 0 for the rare unnamed anchor).
        for d in eval {
            let sheet = match &d.loc {
                Loc::TabFile { tab, .. } | Loc::Tab { tab } => self.tab_index(tab).unwrap_or(0),
                _ => 0,
            };
            out.push((sheet, d));
        }
        out
    }

    /// Record one eval-time refusal.
    fn refuse(&self, diag: Diagnostic) {
        self.diagnostics.borrow_mut().push(diag);
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

    /// The single grid [`Cell`](GridCell) covering `(sheet,col,row)`, or `None` for a gap. For a GRID5
    /// array-formula region the covering file holds ONE grid cell (its `=formula` at grid `(0,0)`) that
    /// every coordinate maps to; a plain file's cell is at the region-relative offset. Single-homes the
    /// "read the covering cell, branching on `array_formula`" rule the hash (`comp_hash`) and trace
    /// (`upstream_deps`/`cell_kind`) surfaces share, so the GRID5 branch has one place to stay correct.
    fn grid_cell_at(&self, sheet: u32, col: u32, row: u32) -> Option<&GridCell> {
        let (_, file) = self.covering(sheet, col, row)?;
        Some(if file.array_formula {
            file.grid.cell_at(0, 0)
        } else {
            file.grid
                .cell_at(row - file.region.min_row, col - file.region.min_col)
        })
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

    /// If `(sheet,col,row)` sits inside a GRID5 array-formula region, the region's TOP-LEFT (anchor)
    /// [`CellKey`] — the single cell the region's ONE formula is planned and computed at (VAL1/ENG3:
    /// the region is one array-formula cell, computed once). `None` for a coordinate that is not in an
    /// array region (a literal cell, a per-cell formula, or a gap). Free when the workbook has no
    /// array regions at all (the `has_array_regions` short-circuit), so non-GRID5 workbooks are
    /// unaffected.
    fn array_region_anchor(&self, sheet: u32, col: u32, row: u32) -> Option<CellKey> {
        if !self.has_array_regions {
            return None;
        }
        let (_, file) = self.covering(sheet, col, row)?;
        file.array_formula
            .then_some((sheet, file.region.min_col, file.region.min_row))
    }

    /// The EFFECTIVE expr for a cell (ENG6): the forge REWRITE if the forge pass produced one, else the
    /// `grid` expr the caller already read. The single seam through which the plan pass's dependency
    /// collection and the evaluate pass's `compute_formula` read a cell's references — so a demanded
    /// forger's `SUM(OFFSET(...))` plans and evaluates as the static `SUM($A$1:$A$3)` its Pass 0
    /// rewrote. ZERO-OVERHEAD: when `has_forgers` is `false` this is a single bool branch returning the
    /// grid expr (the store is never consulted), so the non-forger path is byte-for-byte as before. The
    /// computation hash + volatility deliberately do NOT route through here — they read the ORIGINAL
    /// grid expr (ENG3 split: content-addressing is over the written source, and a forging cone is
    /// volatile/uncached regardless, so the hash's dep shape never gates a cache serve).
    fn effective_expr<'a>(&'a self, key: CellKey, grid: &'a Expr) -> &'a Expr {
        if self.has_forgers
            && let Some(rewritten) = self.forge.get(key)
        {
            return rewritten;
        }
        grid
    }

    /// Plan + evaluate a root expression's dependency cone, then evaluate the expression itself against
    /// this workbook — the shared core of the ad-hoc [`Workbook::eval_formula`] and the forge pass's
    /// argument-cone evaluation. Runs the forge Pass 0 first (gated) so a dependency that is itself a
    /// forger cell is resolved to its static form before it is planned/read; the expression's
    /// unqualified references resolve against `sheet` (its home), and a top-level eval-time refusal
    /// anchors on `sheet`'s tab (no covering file). Ends the pass (`finish_pass`) so its clean results
    /// promote to the memo, exactly as a stored-formula demand does.
    fn eval_root_expr(&self, expr: &Expr, sheet: u32) -> Value {
        let deps = self.expr_deps(expr, sheet);
        if self.has_forgers {
            self.resolve_forgers(&deps);
        }
        let mut graph = DepGraph::default();
        let mut scan = CacheScan::new();
        for &d in &deps {
            let mut on_stack = HashSet::new();
            self.plan_visit(d, 0, &mut graph, &mut on_stack, &mut scan);
        }
        self.evaluate(&graph);
        let prev_sheet = self.current_sheet.replace(sheet);
        let prev_file = self.current_file.replace(None);
        let value = eval(expr, self);
        self.current_sheet.set(prev_sheet);
        self.current_file.set(prev_file);
        self.finish_pass();
        value
    }
}

/// Read one tab folder into its CELL/GRID files and its sheet-scoped FS4 name entries (in either
/// representation). A nested sub-folder is reserved (skipped); a symlink or a non-A1-named regular file
/// is a name entry, an A1-named regular file is a cell file (entries sorted for deterministic order).
fn read_tab_dir(root: &Path, tab_name: &str, dir: &Path) -> std::io::Result<TabParts> {
    let mut files = Vec::new();
    let mut names = Vec::new();
    let mut file_entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    file_entries.sort_by_key(|e| e.file_name());
    for f in file_entries {
        let ft = f.file_type()?;
        if ft.is_dir() {
            continue; // nested folders are reserved (not sub-sheets)
        }
        let name = f.file_name().to_string_lossy().into_owned();
        if ft.is_file() && is_cell_filename(&name) {
            files.push((name, std::fs::read_to_string(f.path())?));
        } else if let Some(entry) = read_name_entry(
            root,
            NameScope::Sheet(tab_name.to_string()),
            &name,
            &f.path(),
            ft,
        )? {
            names.push(entry);
        }
    }
    Ok((files, names))
}

/// Classify one filesystem entry as an FS4 name entry, or `None` when it is not a name (an A1-shaped
/// regular file — a cell — or an unreadable kind). A symlink is the symlink representation (its target
/// resolved here to `(sheet, cell)`); a non-A1 regular file is the ref-file representation (its content
/// read — a degraded symlink lands here too).
fn read_name_entry(
    root: &Path,
    scope: NameScope,
    entry_name: &str,
    path: &Path,
    ft: std::fs::FileType,
) -> std::io::Result<Option<RawNameEntry>> {
    if ft.is_symlink() {
        let (target_sheet, target_cell) = resolve_symlink_target(root, path)?;
        return Ok(Some(RawNameEntry {
            scope,
            entry_name: entry_name.to_string(),
            form: NameRepr::Symlink {
                target_sheet,
                target_cell,
            },
        }));
    }
    if ft.is_file() && !is_cell_filename(entry_name) {
        return Ok(Some(RawNameEntry {
            scope,
            entry_name: entry_name.to_string(),
            form: NameRepr::RefFile {
                content: std::fs::read_to_string(path)?,
            },
        }));
    }
    Ok(None)
}

/// Resolve a name symlink to the `(sheet, cell-A1)` its target names — LEXICALLY (the target cell file
/// need not exist yet for the reader), taking the target's filename as the cell and its parent folder's
/// name as the sheet. A relative link is joined onto the link's own directory first.
fn resolve_symlink_target(root: &Path, link_path: &Path) -> std::io::Result<(String, String)> {
    let target = std::fs::read_link(link_path)?;
    let joined = if target.is_absolute() {
        target
    } else {
        link_path.parent().unwrap_or(root).join(target)
    };
    let cell = joined
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let sheet = joined
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok((sheet, cell))
}

/// The wall-clock "now" as an Excel date-time serial. The [`Workbook`] must STORE `now` (so
/// [`Workbook::with_now`] can pin it) rather than read the clock lazily, so it cannot simply inherit
/// the [`Resolver`] trait's default `now_serial`; it instead composes the two shared single-homes —
/// the raw clock read ([`system_now_secs`], which also handles a pre-epoch clock) and the
/// epoch->serial mapping ([`unix_secs_to_serial`]) — so no clock/epoch boilerplate is re-derived.
fn system_now_serial() -> f64 {
    unix_secs_to_serial(system_now_secs())
}

/// Sort + dedup a list of dependency [`CellKey`]s into the deterministic dependency-key order the
/// computation-hash fold (`comp_hash`) and the trace walk (`upstream_deps`/downstream `neighbors`)
/// both rely on. Single-homes that ordering guarantee so a hash and a trace over the same cell agree
/// on the shape of its dependency set.
fn sort_dedup(mut keys: Vec<CellKey>) -> Vec<CellKey> {
    keys.sort_unstable();
    keys.dedup();
    keys
}
