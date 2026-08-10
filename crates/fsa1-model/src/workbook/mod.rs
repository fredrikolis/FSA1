// Concern: the loaded workbook, what a skeleton or demand load reads into it and which faults it grades | Non-concern: presentation (overlay.rs), evaluation | IO: (dir [+ demand], or tabs) -> Workbook
//! Every demand runs a PLAN pass then an EVALUATE pass. The dependency graph between them is a
//! CONTAINED optimization: its type never leaves this module, and it equals a naive per-cell
//! evaluation, which the differential test below proves.

mod evaluate;
mod forge;
mod hash;
mod plan;
mod ref_rects;
mod resolver;
#[cfg(test)]
mod tests;
mod trace;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fsa1_ast::{
    CellRef, Expr, RangeNode, Resolver, SheetId, Value, eval, eval_at, parse, system_now_secs,
    unix_secs_to_serial,
};

use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::figure::Binding;
use crate::figures::Figures;
use crate::filename::parse_filename;
use crate::grid::{Cell as GridCell, Grid};
use crate::names::{
    NameRepr, NameScope, NameTable, RawNameEntry, figure_occupancy, is_cell_filename,
    is_figure_entry, is_presentation_entry,
};
use crate::overlap::{Rect, detect_overlaps};
use crate::scope::Scope;
use crate::{ParsedFile, figure_in_root, parse_file, presentation_in_grid};

use forge::ForgeStore;
use plan::DepGraph;
use resolver::Arena;

pub use trace::{Direction, TraceNode, TraceStatus};

/// A `=formula` may reference a syntactically-valid but pathological rectangle (`=SUM(A2:ZZ100000)`
/// is ~70M cells), and expanding or materializing one per cell would drive the process into an OOM
/// abort; bounding the area makes that a located refusal instead. Far above any real sheet's used
/// range, so only a pathological reference reaches it.
const MAX_RANGE_CELLS: u64 = 1_000_000;

/// What [`Workbook::clamped_rect`] and [`Workbook::clamped_range`] return, so the plan pass, the
/// resolver, and the computation hash all measure a range EXACTLY alike before each derives its own
/// verdict against [`MAX_RANGE_CELLS`].
struct ClampedRange {
    sheet: u32,
    /// The near corner, never open and so never clamped.
    c0: u32,
    r0: u32,
    /// The far corner AFTER the open-axis clamp.
    c1: u32,
    r1: u32,
    rows: u32,
    cols: u32,
    /// Widened, so a pathological rectangle cannot wrap.
    area: u64,
}

/// `array_formula` marks a region: the whole file is one `=formula`, held as the lone `1x1` grid
/// cell, whose declared `region` spans more than one coordinate.
#[derive(Clone, Debug)]
struct LoadedFile {
    name: String,
    region: Rect,
    grid: Grid,
    array_formula: bool,
    /// `true` where the demand named this file, `false` where the closure pulled it in through a
    /// reference. A pulled file is read for its VALUES and is not graded.
    demanded: bool,
}

/// The coordinate index is SPLIT because a tab post-import is overwhelmingly single-cell files:
/// `single` maps a 1x1 file's coordinate straight to its index, while `spans` holds only the
/// genuinely multi-cell ones and is scanned. Overlaps are rejected at load, so at most one file
/// covers a coordinate and the two lookups cannot disagree.
#[derive(Clone, Debug)]
struct Tab {
    name: String,
    files: Vec<LoadedFile>,
    /// Whether the demand reached the TAB, which is what a tab-located refusal is graded by. A tab
    /// holding nothing but pulled files was never asked about; one holding no files was excluded by
    /// nothing.
    demanded: bool,
    /// How far the tab's declared cells reach, off the FULL listing rather than the loaded subset:
    /// deriving it from `files` under a demand would let `=SUM(A:A)` clamp short and answer a
    /// silently wrong number.
    content: Option<Rect>,
    single: HashMap<(u32, u32), usize>,
    spans: Vec<(Rect, usize)>,
    by_name: HashMap<String, usize>,
}

impl Tab {
    /// File order is PRESERVED: a [`FileId`]'s second component indexes `files`.
    fn new(name: String, files: Vec<LoadedFile>, content: Option<Rect>) -> Tab {
        let mut single = HashMap::new();
        let mut spans = Vec::new();
        let mut by_name = HashMap::new();
        for (i, f) in files.iter().enumerate() {
            let r = f.region;
            if r.min_col == r.max_col && r.min_row == r.max_row {
                single.insert((r.min_col, r.min_row), i);
            } else {
                spans.push((r, i));
            }
            by_name.insert(f.name.clone(), i);
        }
        let demanded = files.is_empty() || files.iter().any(|f| f.demanded);
        Tab {
            name,
            files,
            demanded,
            content,
            single,
            spans,
            by_name,
        }
    }
}

/// `(sheet index, file index within the tab)` — what an eval-time refusal's file anchor is keyed by.
type FileId = (u32, usize);

/// One tab's tree as the reader classified it: its range files as `(name, text)`. A sidecar is
/// classified and then dropped, presentation reaching no [`Workbook`] (VAL1); a figure is dropped
/// too, but a RANGE-form one leaves its `(entry name, rectangle)` in `occupied`, read from the name
/// alone, so the range it fills still collides.
struct TabInput {
    name: String,
    files: Vec<(String, String)>,
    occupied: Vec<(String, Rect)>,
}

/// One tab's FULL listing: every entry classified by NAME, with no range file's content read. The
/// listing is complete whatever the demand, which is what keeps `content` and the overlap verdict
/// total at no read cost.
struct TabListing {
    name: String,
    entries: Vec<TabEntry>,
    occupied: Vec<(String, Rect)>,
    content: Option<Rect>,
}

/// A range file as its NAME alone describes it. `region` is `None` where the name states no extent —
/// which is why no demand can exclude such an entry.
struct TabEntry {
    name: String,
    path: PathBuf,
    region: Option<Rect>,
}

/// One tab's listing as [`list_tab_dir`] classifies it: its range-file entries, the defined names it
/// holds, and the rectangles its range-form figures fill.
type TabParts = (Vec<TabEntry>, Vec<RawNameEntry>, Vec<(String, Rect)>);

/// A listed entry's slot in the closure: `None` while unread, else what reading and parsing it gave.
type ReadSlot = Option<Result<LoadedFile, Vec<Diagnostic>>>;

/// What the root walk yields before a single cell is read: the tabs, and every defined-name entry
/// the whole tree holds.
struct Listing {
    tabs: Vec<TabListing>,
    raw_names: Vec<RawNameEntry>,
}

/// A tab as assembly takes it: what parsed, what refused, the regions the overlap check grades, and
/// the content rect an open axis clamps against.
struct TabLoad {
    name: String,
    files: Vec<LoadedFile>,
    file_diags: Vec<Diagnostic>,
    /// A pulled file's own parse fault, which is NOT fatal and is reported by
    /// [`Workbook::lint_located`] instead — the one grade a pulled file earns.
    pulled_faults: Vec<Diagnostic>,
    regions: Vec<(String, Rect)>,
    content: Option<Rect>,
}

/// `(sheet index, zero-based col, zero-based row)`. Every grid cell is a DISTINCT computation, so
/// the graph and the caches are keyed per cell, never per file.
type CellKey = (u32, u32, u32);

/// Evaluation is demand-driven, so only requested cells compute, and cycle-safe, so a reference
/// cycle is a located `#REF!` rather than a hang. Load with [`Workbook::from_tabs`] or
/// [`Workbook::load_dir`], then request cells or hand `&Workbook` to `fsa1_ast::eval` itself.
#[derive(Debug)]
pub struct Workbook {
    tabs: Vec<Tab>,
    /// The load faults of the PULLED files, kept with the sheet each sits on. They did not abort the
    /// load — the demanded values that reference them read a load error rather than a blank — and
    /// [`Workbook::lint_located`] emits them whatever the demand.
    pulled_faults: Vec<(u32, Diagnostic)>,
    /// A stored formula's name tokens are already resolved to A1 in the grid, but the AD-HOC
    /// [`Workbook::eval_formula`] parses fresh text, so it resolves names through this.
    names: NameTable,
    /// The per-coordinate array-region redirect is paid only when this is `true`.
    has_array_regions: bool,
    /// The ZERO-OVERHEAD gate: when `false`, [`Workbook::demand`] skips the forge pass and
    /// [`Workbook::effective_expr`] returns the grid expr, both on a bool check.
    has_forgers: bool,
    /// Address-stable under `&self`, so [`Workbook::effective_expr`] can return `&Expr`. Filled
    /// lazily by the demand-driven pass, it persists for the immutable workbook's lifetime.
    forge: ForgeStore,
    /// What [`Resolver::now_serial`] reports; a test pins it with [`Workbook::with_now`].
    now: f64,
    /// The home sheet of the formula node currently being computed, which an unqualified reference
    /// resolves against. Set per node, evaluation being iterative rather than nested.
    current_sheet: Cell<u32>,
    /// EVERY computed value reaches this cache: a cell's value is a function of its content cone
    /// alone, never of the demand that reached it, so nothing is pass-local.
    memo: RefCell<HashMap<CellKey, Value>>,
    /// The current pass's values, which the [`Resolver`] reads before the memo and the grid, so a
    /// formula sees its already-computed dependencies. Promoted and cleared at the end of a demand.
    results: RefCell<HashMap<CellKey, Value>>,
    /// The anchor for a refusal raised from INSIDE eval. Set per node, like `current_sheet`.
    current_file: Cell<Option<FileId>>,
    arena: Arena,
    /// Refusals surfaced during EVALUATION; the loader returns load-time ones itself.
    diagnostics: RefCell<Vec<Diagnostic>>,
    /// A repeat demand of an already-memoized cone must perform no further evaluations. The field,
    /// its increments, and its reader all compile only under test, so a production build carries no
    /// instrument in the hot per-formula path.
    #[cfg(test)]
    eval_count: Cell<u64>,
    /// Pins the BATCHED drive: a whole-workbook `lint` must accrete every coordinate into ONE pass.
    #[cfg(test)]
    pass_count: Cell<u64>,
    /// Pins the coordinate index: a single-cell-file lookup increments this ZERO, so a drive over N
    /// such files stays O(1) per lookup rather than scanning.
    #[cfg(test)]
    covering_scan_steps: Cell<u64>,
}

/// The variant lets `fsa1-cli eval` set its exit code without re-inspecting a `fsa1_ast::Value`,
/// which would breach the firewall. Both strings carry the render surface's spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormulaOutcome {
    Value(String),
    Error(String),
}

/// Carries NO content: a consumer fetches each coordinate's text through the render surface, which
/// also names the coordinate, so the grid never leaks and the spelling stays single-sourced.
#[derive(Clone, Copy, Debug)]
pub struct FileEntry<'a> {
    pub name: &'a str,
    pub region: Rect,
    /// A whole file that is one `=formula` filling a multi-coordinate range, so a `functions` view
    /// shows ONE node while a `values` view expands the computed cells like any range.
    pub array_formula: bool,
}

/// The UN-EVALUATED source at a coordinate. Borrows the workbook, so it cannot outlive it.
#[derive(Clone, Copy, Debug)]
pub struct CellSource<'a> {
    pub file_name: &'a str,
    pub region: Rect,
    /// For an array-formula region this is always the region's single `=formula`, whatever
    /// coordinate was requested.
    pub cell: &'a GridCell,
    /// A coordinate an array formula fills but does not anchor. The `functions` render marks it
    /// rather than re-printing the formula, which lives once, at the anchor.
    pub array_continuation: bool,
}

impl Workbook {
    /// `tabs` is `(tab name, [(filename, contents)])`. Every load-time refusal is returned together.
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

    /// A NAMED set, deliberately, not a blanket "ignore dotfiles" rule: an unexpected dot-prefixed
    /// entry stays a located refusal instead of being swallowed. `.cache` keeps a stale directory an
    /// older build left behind INERT rather than parsed as a tab; the rest are git's, an FSA1
    /// workbook being designed to BE a git repository.
    const RESERVED_ENTRIES: &'static [&'static str] = &[
        ".cache",
        ".git",
        ".gitignore",
        ".gitattributes",
        ".gitmodules",
        ".github",
        ".githooks",
    ];

    /// The clamp rule, single-homed here because the PLAN and EVALUATE passes must agree exactly:
    /// clamping in only one of them lets an open range's area read as `u32::MAX`, exceed the bound,
    /// go unexpanded, and be read as blank at eval — a silent wrong value. Call it through
    /// [`Workbook::clamped_rect`]. An empty tab clamps to the near corner, a one-cell region.
    fn clamp_open(&self, sheet: u32, c0: u32, c1: u32, r0: u32, r1: u32) -> (u32, u32) {
        if c1 != RangeNode::OPEN && r1 != RangeNode::OPEN {
            return (c1, r1);
        }
        let used = self.content_region(sheet);
        (
            if c1 == RangeNode::OPEN {
                used.map(|u| u.max_col).unwrap_or(c0)
            } else {
                c1
            },
            if r1 == RangeNode::OPEN {
                used.map(|u| u.max_row).unwrap_or(r0)
            } else {
                r1
            },
        )
    }

    /// THE single home for how big a range really is; its three callers all read it here, so none
    /// can measure differently. It returns the rectangle rather than a `bool` verdict because each
    /// caller needs a different part of it, and a predicate would make the resolver clamp twice.
    fn clamped_rect(&self, sheet: u32, c0: u32, c1: u32, r0: u32, r1: u32) -> ClampedRange {
        let (c1, r1) = self.clamp_open(sheet, c0, c1, r0, r1);
        let (rows, cols) = (r1 - r0 + 1, c1 - c0 + 1);
        ClampedRange {
            sheet,
            c0,
            r0,
            c1,
            r1,
            rows,
            cols,
            area: u64::from(rows) * u64::from(cols),
        }
    }

    /// The same measurement from a SYNTACTIC node, resolving and normalizing the corners first, so
    /// the plan pass and the computation hash share one entry point. `None` on an unknown sheet,
    /// which the evaluator maps to `#REF!` and which therefore has no rectangle.
    fn clamped_range(&self, rn: &RangeNode, home: u32) -> Option<ClampedRange> {
        let rr = rn.resolve(|name| self.sheet_id(name))?.normalized();
        let sheet = rr.start.sheet.map_or(home, |SheetId(i)| i);
        Some(self.clamped_rect(sheet, rr.start.col, rr.end.col, rr.start.row, rr.end.row))
    }

    pub fn is_reserved_entry(name: &str) -> bool {
        Self::RESERVED_ENTRIES.contains(&name)
    }

    /// The outer `io::Result` reports a filesystem failure and the inner one the workbook's own
    /// refusals — kept apart, an unreadable directory not being a spreadsheet diagnostic.
    pub fn load_dir(root: &Path) -> std::io::Result<Result<Workbook, Vec<Diagnostic>>> {
        Workbook::load_dir_scoped(root, &Scope::unscoped())
    }

    /// The listing and name phases and NOTHING else: `sheet_names`, `content_region`, `tab_index`,
    /// the overlap verdict and `name_table` are all complete, and every tab holds zero cells. It is
    /// what a probe deciding a workbook root — and resolving a trailing defined name — needs before
    /// the demand exists, so no path pays for a second content load.
    pub fn load_skeleton(root: &Path) -> std::io::Result<Result<Workbook, Vec<Diagnostic>>> {
        let listing = match list_dir(root)? {
            Ok(listing) => listing,
            Err(faults) => return Ok(Err(faults)),
        };
        let (names, diags) = NameTable::build(listing.raw_names);
        let loads = listing
            .tabs
            .into_iter()
            .map(|tab| {
                let TabListing {
                    name,
                    entries,
                    occupied,
                    content,
                } = tab;
                TabLoad {
                    name,
                    files: Vec::new(),
                    file_diags: Vec::new(),
                    pulled_faults: Vec::new(),
                    regions: listed_regions(&entries, occupied),
                    content,
                }
            })
            .collect();
        Ok(assemble(loads, names, diags))
    }

    /// [`Workbook::load_dir`] under a demand. The listing, the names and the overlap verdict stay
    /// total; the CELLS read are the demand's own files plus the transitive closure of the
    /// rectangles their formulas — and the in-scope figures' bindings — reference. A file the
    /// closure never frames is never opened, so it neither computes nor refuses.
    pub fn load_dir_scoped(
        root: &Path,
        demand: &Scope,
    ) -> std::io::Result<Result<Workbook, Vec<Diagnostic>>> {
        let Listing { tabs, raw_names } = match list_dir(root)? {
            Ok(listing) => listing,
            Err(faults) => return Ok(Err(faults)),
        };
        let (names, name_diags) = NameTable::build(raw_names);
        let sheets: Vec<String> = tabs.iter().map(|t| t.name.clone()).collect();
        let content: Vec<Option<Rect>> = tabs.iter().map(|t| t.content).collect();
        let mut read: Vec<Vec<ReadSlot>> = tabs
            .iter()
            .map(|t| t.entries.iter().map(|_| None).collect())
            .collect();

        let mut frontier = seed(&tabs, demand);
        // Unscoped, the seed already admits every entry of every tab, so re-reading the figures to frame rectangles already framed buys nothing.
        if demand.is_scoped() {
            frame_rects(
                &tabs,
                &figure_rects(root, demand, &sheets)?,
                &mut frontier,
                false,
            );
        }
        let mut scratch = Vec::new();
        let mut forged = false;
        let mut pulled_faults: Vec<Vec<Diagnostic>> = tabs.iter().map(|_| Vec::new()).collect();
        while !frontier.is_empty() {
            let mut rects: Vec<(u32, Rect)> = Vec::new();
            for (ti, ei, pulled) in std::mem::take(&mut frontier) {
                if read[ti][ei].is_some() {
                    continue;
                }
                let entry = &tabs[ti].entries[ei];
                let text = read_file_to_string(&entry.path, &mut scratch)?;
                let parsed = match parse_one(&names, &tabs[ti].name, &entry.name, &text, !pulled) {
                    // A pulled file's fault must abort nothing — nobody asked about it — yet answering the demanded cell as blank would be worse, so it loads as its declared region of load errors and the fault is carried for the lint instead.
                    Err(d) if pulled => match load_error_file(&tabs[ti].name, &entry.name, d) {
                        Ok((file, fault)) => {
                            pulled_faults[ti].push(fault);
                            Ok(file)
                        }
                        Err(d) => Err(d),
                    },
                    other => other,
                };
                if let Ok(file) = &parsed {
                    for cell in &file.grid.cells {
                        if let GridCell::Formula { expr, .. } = cell {
                            rects.extend(ref_rects::ref_rects(expr, ti as u32, &sheets, &content));
                            forged |= forge::expr_has_forger(expr);
                        }
                    }
                }
                read[ti][ei] = Some(parsed);
            }
            if forged {
                // A forger builds its reference at eval time, so no static closure covers it: every remaining file joins the frontier, and the round that reads them adds none.
                for (ti, tab) in tabs.iter().enumerate() {
                    let unread = (0..tab.entries.len()).filter(|&ei| read[ti][ei].is_none());
                    frontier.extend(unread.map(|ei| (ti, ei, true)));
                }
            } else {
                frame_rects(&tabs, &rects, &mut frontier, true);
            }
        }

        let mut loads = Vec::with_capacity(tabs.len());
        for (ti, tab) in tabs.into_iter().enumerate() {
            let TabListing {
                name,
                entries,
                occupied,
                content,
            } = tab;
            let mut files = Vec::new();
            let mut file_diags = Vec::new();
            for slot in read[ti].drain(..).flatten() {
                match slot {
                    Ok(file) => files.push(file),
                    Err(d) => file_diags.extend(d),
                }
            }
            loads.push(TabLoad {
                name,
                files,
                file_diags,
                pulled_faults: std::mem::take(&mut pulled_faults[ti]),
                regions: listed_regions(&entries, occupied),
                content,
            });
        }
        Ok(assemble(loads, names, name_diags))
    }

    /// An in-memory workbook has no symlinks, so a name here is always the ref-file representation.
    fn from_owned(tabs: Vec<(String, Vec<(String, String)>)>) -> Result<Workbook, Vec<Diagnostic>> {
        let mut cell_tabs = Vec::with_capacity(tabs.len());
        let mut raw_names = Vec::new();
        for (tab_name, files) in tabs {
            let mut cells = Vec::new();
            let mut occupied = Vec::new();
            for (fname, contents) in files {
                // A sidecar is classified FIRST, exactly as on disk: its stem holds a range separator, so the cell arm would otherwise take it and its name would die as malformed. A range-form figure leaves the rectangle its NAME states, `contents` untouched.
                if is_presentation_entry(&fname) || is_figure_entry(&fname) {
                    if let Some(rect) = figure_occupancy(&fname) {
                        occupied.push((fname, rect));
                    }
                    continue;
                }
                if is_cell_filename(&fname) {
                    cells.push((fname, contents));
                } else {
                    raw_names.push(RawNameEntry {
                        scope: NameScope::Sheet(tab_name.clone()),
                        entry_name: fname,
                        form: NameRepr::RefFile { content: contents },
                    });
                }
            }
            cell_tabs.push(TabInput {
                name: tab_name,
                files: cells,
                occupied,
            });
        }
        Workbook::from_dir_parts(cell_tabs, raw_names)
    }

    /// The in-memory path: every file handed in is read, so all of them are seeded and the tab's
    /// content rect is the union of their names' regions, exactly as a full listing would give it.
    fn from_dir_parts(
        tabs: Vec<TabInput>,
        raw_names: Vec<RawNameEntry>,
    ) -> Result<Workbook, Vec<Diagnostic>> {
        let (name_table, diags) = NameTable::build(raw_names);
        let mut loads = Vec::with_capacity(tabs.len());
        for tab in tabs {
            let TabInput {
                name: tab_name,
                files,
                occupied,
            } = tab;
            let mut loaded = Vec::new();
            let mut file_diags = Vec::new();
            let mut regions: Vec<(String, Rect)> = Vec::new();
            let mut content = None;
            for (fname, contents) in files {
                if let Some(region) = filename_region(&fname) {
                    regions.push((fname.clone(), region));
                    content = Rect::union(content, Some(region));
                }
                match parse_one(&name_table, &tab_name, &fname, &contents, true) {
                    Ok(file) => loaded.push(file),
                    Err(d) => file_diags.extend(d),
                }
            }
            // After the cell files, so a colliding pair reads `<cell>  and  <figure>.json`.
            regions.extend(occupied);
            loads.push(TabLoad {
                name: tab_name,
                files: loaded,
                file_diags,
                pulled_faults: Vec::new(),
                regions,
                content,
            });
        }
        assemble(loads, name_table, diags)
    }

    pub fn with_now(mut self, serial: f64) -> Workbook {
        self.now = serial;
        self
    }

    /// The name a [`SheetId`] addresses, for a caller keyed by name rather than by index.
    pub fn sheet_name(&self, sheet: u32) -> Option<&str> {
        self.tabs.get(sheet as usize).map(|t| t.name.as_str())
    }

    /// In tab order, so an index IS a [`SheetId`].
    pub fn sheet_names(&self) -> Vec<&str> {
        self.tabs.iter().map(|t| t.name.as_str()).collect()
    }

    /// `col` and `row` are zero-based.
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

    /// ONE plan+evaluate pass over every cell, so a dependency several of them share computes once.
    /// Values come back in the requested order.
    pub fn values_at(&self, cells: &[CellKey]) -> Vec<Value> {
        // Without the `needs_compute` test a viewport of literals would start a pass that plans nothing, and a caller who had already demanded a region could not tell that it was done.
        let uncached: Vec<CellKey> = {
            let memo = self.memo.borrow();
            cells
                .iter()
                .copied()
                .filter(|k| !memo.contains_key(k) && self.needs_compute(*k))
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

    /// A snapshot, so call it AFTER driving cells. An unparseable formula body is not here: that is a
    /// load-time per-cell error, which [`Workbook::lint`] surfaces instead.
    pub fn eval_diagnostics(&self) -> Vec<Diagnostic> {
        self.diagnostics.borrow().clone()
    }

    /// Read-only, and spelled by the render surface's own formatter. Unqualified references resolve
    /// against `sheet`. An evaluation yielding a spreadsheet ERROR VALUE is [`FormulaOutcome::Error`]
    /// so a caller can exit non-zero; a parse failure is a located [`Diagnostic`] instead.
    pub fn eval_formula(&self, sheet: u32, formula: &str) -> Result<FormulaOutcome, Diagnostic> {
        // So an ad-hoc formula and a stored one share name semantics, the engine staying A1-only.
        let resolved = self.names.rewrite_tsv(formula, &self.tab_name(sheet));
        let expr = parse(&resolved).map_err(|diag| {
            // The parser's span indexes the RESOLVED text, so where a name expanded a precise column would point into the expansion; anchor on the whole original instead and name the resolved form.
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
        // A forger written DIRECTLY here is not rewritten — the forge pass is keyed by a stored cell — so it hits the `#REF!` backstop; a stored forger cell it REFERENCES does forge.
        let value = self.eval_root_expr(&expr, sheet, None);
        let shown = crate::render::display_value(&value);
        Ok(match value {
            Value::Error(_) => FormulaOutcome::Error(shown),
            _ => FormulaOutcome::Value(shown),
        })
    }

    /// A literal, a load-error cell, and a gap all read straight from the grid, so no pass produces
    /// them. A region's continuation coordinate does count, its one formula filling it.
    fn needs_compute(&self, key: CellKey) -> bool {
        matches!(
            self.grid_cell_at(key.0, key.1, key.2),
            Some(GridCell::Formula { .. })
        )
    }

    /// The seam a caller batching a view uses to accrete a defined name's cone into the SAME demand
    /// as the grid's, so a name never starts a pass of its own. Empty if the formula will not parse.
    pub fn formula_deps(&self, sheet: u32, formula: &str) -> Vec<CellKey> {
        let resolved = self.names.rewrite_tsv(formula, &self.tab_name(sheet));
        match parse(&resolved) {
            Ok(expr) => self.expr_deps(&expr, sheet),
            Err(_) => Vec::new(),
        }
    }

    /// In load order, which is sorted on the filesystem path. `None` for an out-of-range sheet.
    pub fn tab_files(&self, sheet: u32) -> Option<Vec<FileEntry<'_>>> {
        let tab = self.tabs.get(sheet as usize)?;
        Some(
            tab.files
                .iter()
                .map(|f| FileEntry {
                    name: &f.name,
                    region: f.region,
                    array_formula: f.array_formula,
                })
                .collect(),
        )
    }

    /// Read-only: the classification authority stays in the `names` module.
    pub fn name_table(&self) -> &NameTable {
        &self.names
    }

    /// The index IS the [`SheetId`] the resolver uses.
    pub fn tab_index(&self, name: &str) -> Option<u32> {
        self.tabs
            .iter()
            .position(|t| t.name == name)
            .map(|i| i as u32)
    }

    /// O(1) through the tab's name index: a workbook whose files each carry a refusal must not
    /// re-scan the tab once per diagnostic.
    fn file_index(&self, tab: &str, name: &str) -> Option<usize> {
        let idx = self.tab_index(tab)? as usize;
        self.tabs[idx].by_name.get(name).copied()
    }

    /// How far the tab's CONTENT reaches — range-file names only. A sidecar states no value, so a
    /// block cannot move the bound an open-axis reference resolves against (VAL1).
    pub fn content_region(&self, sheet: u32) -> Option<Rect> {
        self.tabs.get(sheet as usize)?.content
    }

    /// `None` for a gap. Overlaps are rejected at load, so at most one file covers a cell.
    pub fn source_at(&self, sheet: u32, col: u32, row: u32) -> Option<CellSource<'_>> {
        let (_, file) = self.covering(sheet, col, row)?;
        // A region holds ONE grid cell, at (0,0), that every coordinate maps to.
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

    /// Every fault a LOADED workbook can still carry, in file order throughout: the per-cell load
    /// errors first, then the eval-time refusals, then the pulled files' own parse faults —
    /// independent of the schedule the pass evaluated them in. A structural load-time refusal aborts
    /// the load itself and surfaces from the loader.
    pub fn lint(&self) -> Vec<Diagnostic> {
        self.lint_located().into_iter().map(|(_, d)| d).collect()
    }

    /// The tab index resolves what a bare-filename loc cannot express: a load error is located as
    /// `Body{file}`, yet the same address can exist on two tabs, and the enclosing tab is known here.
    /// Only the DEMANDED work is graded: a file the closure pulled in through a reference is read for
    /// its values alone, its own parse fault (which the demanded value depends on) excepted.
    fn lint_located(&self) -> Vec<(u32, Diagnostic)> {
        let mut out: Vec<(u32, Diagnostic)> = Vec::new();
        for (s, tab) in self.tabs.iter().enumerate() {
            for file in tab.files.iter().filter(|f| f.demanded) {
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
        let mut keys: Vec<CellKey> = Vec::new();
        for (sheet, region) in regions {
            for row in region.min_row..=region.max_row {
                for col in region.min_col..=region.max_col {
                    keys.push((sheet, col, row));
                }
            }
        }
        let _ = self.values_at(&keys);
        // Memoization already records each file's refusal at most once per drive, so this only guards two adjacent drives surfacing the identical refusal.
        let mut eval = self.eval_diagnostics();
        eval.dedup();
        // The pass pushes these in topo order, so sort to a deterministic FILE order. Two refusals from one multi-cell file share a key, and the stable sort leaves those in topo order.
        let mut located: Vec<(u32, usize, Diagnostic)> = eval
            .into_iter()
            .filter(|d| self.demand_grades(&d.loc))
            .map(|d| {
                let (sheet, file) = match &d.loc {
                    Loc::TabFile { tab, name } => (
                        self.tab_index(tab).unwrap_or(0),
                        self.file_index(tab, name).unwrap_or(usize::MAX),
                    ),
                    Loc::Tab { tab } => (self.tab_index(tab).unwrap_or(0), usize::MAX),
                    _ => (0, usize::MAX),
                };
                (sheet, file, d)
            })
            .collect();
        located.sort_by_key(|(sheet, file, _)| (*sheet, *file));
        out.extend(located.into_iter().map(|(sheet, _, d)| (sheet, d)));
        out.extend(self.pulled_faults.iter().cloned());
        out
    }

    /// Whether an eval-time refusal falls on the demanded work: a file anchor is graded where THAT
    /// file was demanded, a tab anchor where the tab was, and an anchor naming neither is kept.
    fn demand_grades(&self, loc: &Loc) -> bool {
        match loc {
            Loc::TabFile { tab, name } => match (self.tab_index(tab), self.file_index(tab, name)) {
                (Some(s), Some(i)) => self.tabs[s as usize].files[i].demanded,
                _ => true,
            },
            Loc::Tab { tab } => self
                .tab_index(tab)
                .is_none_or(|s| self.tabs[s as usize].demanded),
            _ => true,
        }
    }

    fn refuse(&self, diag: Diagnostic) {
        self.diagnostics.borrow_mut().push(diag);
    }

    /// Whether a range file states the coordinate at all — the gap test an [`crate::overlay::Overlay`]
    /// resolver asks before it calls a coordinate under no block an EMPTY style rather than nothing.
    pub(crate) fn covers(&self, sheet: u32, col: u32, row: u32) -> bool {
        self.covering(sheet, col, row).is_some()
    }

    /// Overlaps are rejected at load, so at most one file covers a coordinate.
    fn covering(&self, sheet: u32, col: u32, row: u32) -> Option<(FileId, &LoadedFile)> {
        let tab = self.tabs.get(sheet as usize)?;
        let idx = match tab.single.get(&(col, row)) {
            Some(&i) => i,
            None => {
                // A SHORT scan: its length is the tab's multi-cell FILE count, not its cell count.
                let mut hit = None;
                for (r, i) in &tab.spans {
                    #[cfg(test)]
                    self.covering_scan_steps
                        .set(self.covering_scan_steps.get() + 1);
                    if r.contains(col, row) {
                        hit = Some(*i);
                        break;
                    }
                }
                hit?
            }
        };
        Some(((sheet, idx), &tab.files[idx]))
    }

    /// Single-homes the "read the covering cell, branching on `array_formula`" rule the hash and
    /// trace surfaces share, so that branch has one place to stay correct. `None` for a gap.
    fn grid_cell_at(&self, sheet: u32, col: u32, row: u32) -> Option<&GridCell> {
        let (_, file) = self.covering(sheet, col, row)?;
        Some(if file.array_formula {
            file.grid.cell_at(0, 0)
        } else {
            file.grid
                .cell_at(row - file.region.min_row, col - file.region.min_col)
        })
    }

    /// Falls back to the numeric index for an out-of-range sheet, so it never panics.
    fn tab_name(&self, sheet: u32) -> String {
        self.tabs
            .get(sheet as usize)
            .map_or_else(|| sheet.to_string(), |t| t.name.clone())
    }

    fn file_name(&self, id: FileId) -> String {
        self.tabs[id.0 as usize].files[id.1].name.clone()
    }

    /// The single cell a region's ONE formula is planned and computed at, or `None` for a coordinate
    /// in no region. Free where `has_array_regions` is false.
    fn array_region_anchor(&self, sheet: u32, col: u32, row: u32) -> Option<CellKey> {
        if !self.has_array_regions {
            return None;
        }
        let (_, file) = self.covering(sheet, col, row)?;
        file.array_formula
            .then_some((sheet, file.region.min_col, file.region.min_row))
    }

    /// The ONE seam through which the plan and evaluate passes read a cell's references, so a
    /// demanded forger plans and evaluates as the static form Pass 0 rewrote it to. The computation
    /// hash deliberately does NOT route through here: content-addressing is over the WRITTEN source.
    fn effective_expr<'a>(&'a self, key: CellKey, grid: &'a Expr) -> &'a Expr {
        if self.has_forgers
            && let Some(rewritten) = self.forge.get(key)
        {
            return rewritten;
        }
        grid
    }

    /// The shared core of ad-hoc evaluation and the forge pass's argument-cone evaluation. `anchor`
    /// is the 0-based `(row, col)` of the home cell owning this expression — the no-argument
    /// `ROW()`/`COLUMN()` seam. `None` anchors those to A1, an ad-hoc formula having no home cell.
    fn eval_root_expr(&self, expr: &Expr, sheet: u32, anchor: Option<(u32, u32)>) -> Value {
        let deps = self.expr_deps(expr, sheet);
        if self.has_forgers {
            self.resolve_forgers(&deps);
        }
        let mut graph = DepGraph::default();
        for &d in &deps {
            self.plan_visit(d, &mut graph);
        }
        self.evaluate(&graph);
        let prev_sheet = self.current_sheet.replace(sheet);
        let prev_file = self.current_file.replace(None);
        let value = match anchor {
            Some((row, col)) => eval_at(expr, self, row, col),
            None => eval(expr, self),
        };
        self.current_sheet.set(prev_sheet);
        self.current_file.set(prev_file);
        self.finish_pass();
        value
    }
}

/// The root walk, listing only: every tab in full, every root-level entry classified. A ROOT-level
/// fault sits outside every tab, so no demand excludes it and it short-circuits the load.
fn list_dir(root: &Path) -> std::io::Result<Result<Listing, Vec<Diagnostic>>> {
    let mut tabs: Vec<TabListing> = Vec::new();
    // Read here, where the filesystem is present: the pure `names` module never touches it.
    let mut raw_names: Vec<RawNameEntry> = Vec::new();
    let mut root_faults: Vec<Diagnostic> = Vec::new();
    let mut scratch = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(root)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let ft = entry.file_type()?;
        let entry_name = entry.file_name().to_string_lossy().into_owned();
        // Checked before the dir/file split, so a reserved name is excluded whichever kind it is on disk: `.git` is a directory and `.gitignore` a file, and either would otherwise be read.
        if Workbook::is_reserved_entry(&entry_name) {
            continue;
        }
        if ft.is_dir() {
            let (entries, names, occupied) = list_tab_dir(root, &entry_name, &entry.path())?;
            let content = entries
                .iter()
                .fold(None, |acc, e| Rect::union(acc, e.region));
            tabs.push(TabListing {
                name: entry_name,
                entries,
                occupied,
                content,
            });
            raw_names.extend(names);
        } else if is_presentation_entry(&entry_name) {
            // A sidecar styles COORDINATES, and a coordinate is a tab's; at the root it names none.
            root_faults.push(presentation_in_grid(Loc::file(&entry_name)));
        } else if is_figure_entry(&entry_name) {
            // Without this arm it falls into `read_name_entry`'s `RefFile` arm and is claimed as a defined name.
            root_faults.push(figure_in_root(Loc::file(&entry_name)));
        } else if let Some(name) = read_name_entry(
            root,
            NameScope::Workbook,
            &entry_name,
            &entry.path(),
            ft,
            &mut scratch,
        )? {
            // A root-level A1-shaped regular file is neither a tab nor a name, so it is ignored.
            raw_names.push(name);
        }
    }
    Ok(if root_faults.is_empty() {
        Ok(Listing { tabs, raw_names })
    } else {
        Err(root_faults)
    })
}

/// Entries are sorted, for a deterministic load order. A sidecar is tested for first: its stem holds
/// a range separator, so the cell arm below would take `A1:C3.css` and refuse it as a malformed
/// range name. A range file's CONTENT is left for the closure to ask for; a NAME entry's is read
/// whatever the demand, `rewrite_tsv` having to resolve what a demanded formula spells to parse it.
fn list_tab_dir(root: &Path, tab_name: &str, dir: &Path) -> std::io::Result<TabParts> {
    let mut files = Vec::new();
    let mut names = Vec::new();
    let mut occupied = Vec::new();
    let mut scratch = Vec::new();
    let mut file_entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    file_entries.sort_by_key(|e| e.file_name());
    for f in file_entries {
        let ft = f.file_type()?;
        if ft.is_dir() {
            continue; // nested folders are reserved (not sub-sheets)
        }
        let name = f.file_name().to_string_lossy().into_owned();
        // Reserved inside a tab folder too: a nested `.gitignore` is ordinary, and is not a cell.
        if Workbook::is_reserved_entry(&name) {
            continue;
        }
        // A figure is skipped with the sidecars: it states no cell, and the name arm below would otherwise claim its stem and let the `.json` suffix die as part of that name. A range-form one still leaves the rectangle its name fills, for `detect_overlaps`.
        if ft.is_file() && (is_presentation_entry(&name) || is_figure_entry(&name)) {
            if let Some(rect) = figure_occupancy(&name) {
                occupied.push((name, rect));
            }
            continue;
        }
        if ft.is_file() && is_cell_filename(&name) {
            let region = filename_region(&name);
            files.push(TabEntry {
                name,
                path: f.path(),
                region,
            });
        } else if let Some(entry) = read_name_entry(
            root,
            NameScope::Sheet(tab_name.to_string()),
            &name,
            &f.path(),
            ft,
            &mut scratch,
        )? {
            names.push(entry);
        }
    }
    Ok((files, names, occupied))
}

/// The A1 rectangle a range file's NAME declares, or `None` where the name declares nothing
/// parseable — an entry a demand can therefore never exclude.
fn filename_region(name: &str) -> Option<Rect> {
    parse_filename(name).ok().map(|f| f.region)
}

/// The overlap check's input, off the FULL listing rather than the parsed subset: every range
/// filename that states a rectangle, then the range-form figures, so a colliding pair reads
/// `<cell>  and  <figure>.json`.
fn listed_regions(entries: &[TabEntry], occupied: Vec<(String, Rect)>) -> Vec<(String, Rect)> {
    let mut out: Vec<(String, Rect)> = entries
        .iter()
        .filter_map(|e| e.region.map(|r| (e.name.clone(), r)))
        .collect();
    out.extend(occupied);
    out
}

/// The closure's seed: in each wanted tab, every range file whose filename region meets the demanded
/// rect, plus every entry whose filename states no extent. An unscoped or tab-only demand seeds
/// every file in every wanted tab.
fn seed(tabs: &[TabListing], demand: &Scope) -> Vec<(usize, usize, bool)> {
    let mut out = Vec::new();
    for (ti, tab) in tabs.iter().enumerate() {
        if !demand.wants(Some(&tab.name), None) {
            continue;
        }
        for (ei, entry) in tab.entries.iter().enumerate() {
            if entry
                .region
                .is_none_or(|r| demand.wants(Some(&tab.name), Some(r)))
            {
                out.push((ti, ei, false));
            }
        }
    }
    out
}

/// The frontier step: every entry whose FILENAME region meets one of `rects` on that rect's own
/// sheet. A rect carries its sheet, so a cross-tab reference is an ordinary member, and admission is
/// never widened beyond a file's own name.
fn frame_rects(
    tabs: &[TabListing],
    rects: &[(u32, Rect)],
    out: &mut Vec<(usize, usize, bool)>,
    pulled: bool,
) {
    for &(sheet, rect) in rects {
        let Some(tab) = tabs.get(sheet as usize) else {
            continue;
        };
        for (ei, entry) in tab.entries.iter().enumerate() {
            if entry.region.is_some_and(|r| r.intersect(&rect).is_some()) {
                out.push((sheet as usize, ei, pulled));
            }
        }
    }
}

/// The figure half of the seed: a figure's data range is a dependency no formula spells, so an
/// in-scope figure's bindings are rectangles the closure must frame or it draws an empty dataset.
/// A figure that will not parse seeds nothing — the workbook load does not grade one, and `check`
/// reports it off its own read. Cost accepted: an in-scope figure is opened twice.
fn figure_rects(
    root: &Path,
    demand: &Scope,
    sheets: &[String],
) -> std::io::Result<Vec<(u32, Rect)>> {
    let Ok(figures) = Figures::load_dir_scoped(root, demand)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (tab, figure) in figures.all() {
        let Some(home) = sheet_index(sheets, tab) else {
            continue;
        };
        for text in figure.bindings() {
            let Ok(binding) = Binding::parse(&text) else {
                continue;
            };
            let sheet = match &binding.tab {
                None => Some(home),
                Some(name) => sheet_index(sheets, name),
            };
            if let Some(sheet) = sheet {
                out.push((sheet, binding.rect));
            }
        }
    }
    Ok(out)
}

fn sheet_index(sheets: &[String], name: &str) -> Option<u32> {
    sheets.iter().position(|s| s == name).map(|i| i as u32)
}

/// A range file's content, name-resolved BEFORE deserializing so the grid the engine sees carries
/// only A1. An unresolvable name stays verbatim and loads as a located `#NAME?`.
fn parse_one(
    names: &NameTable,
    tab: &str,
    fname: &str,
    contents: &str,
    demanded: bool,
) -> Result<LoadedFile, Vec<Diagnostic>> {
    let resolved = names.rewrite_tsv(contents, tab);
    parse_file(fname, &resolved).map(
        |ParsedFile {
             region,
             declared_shape: _,
             grid,
             array_formula,
         }| LoadedFile {
            name: fname.to_string(),
            region,
            grid,
            array_formula,
            demanded,
        },
    )
}

/// A pulled file that would not parse, as the region its NAME declares filled with the fault, plus
/// the tab-qualified diagnostic the lint reports it by. `Err` keeps today's fatal path for a name
/// stating no region (seeded, so demanded), one declaring over [`MAX_RANGE_CELLS`] — the grid is a
/// cell per DECLARED coordinate, no longer bounded by the file's bytes — and any but one fault.
fn load_error_file(
    tab: &str,
    fname: &str,
    mut diags: Vec<Diagnostic>,
) -> Result<(LoadedFile, Diagnostic), Vec<Diagnostic>> {
    let Ok(declared) = parse_filename(fname) else {
        return Err(diags);
    };
    let shape = declared.declared_shape;
    let area = u64::from(shape.rows) * u64::from(shape.cols);
    if area > MAX_RANGE_CELLS || diags.len() != 1 {
        return Err(diags);
    }
    let mut fault = diags.remove(0);
    fault.loc = Loc::tab_file(tab, fname);
    let cell = GridCell::LoadError {
        src: fault.message.clone(),
        diag: fault.clone(),
    };
    let file = LoadedFile {
        name: fname.to_string(),
        region: declared.region,
        grid: Grid {
            shape,
            cells: vec![cell; (shape.rows as usize) * (shape.cols as usize)],
        },
        array_formula: false,
        demanded: false,
    };
    Ok((file, fault))
}

/// The phase every entry point ends in: the overlap verdict off each tab's full listing — fatal
/// whatever the demand, because the coordinate index it guards admits at most one file per
/// coordinate — then the caches a demand-driven evaluation reads.
fn assemble(
    tabs: Vec<TabLoad>,
    names: NameTable,
    mut diags: Vec<Diagnostic>,
) -> Result<Workbook, Vec<Diagnostic>> {
    let mut out_tabs = Vec::with_capacity(tabs.len());
    let mut pulled_faults: Vec<(u32, Diagnostic)> = Vec::new();
    for tab in tabs {
        let TabLoad {
            name,
            files,
            file_diags,
            pulled_faults: pulled,
            regions,
            content,
        } = tab;
        diags.extend(file_diags);
        diags.extend(detect_overlaps(&name, &regions));
        pulled_faults.extend(pulled.into_iter().map(|d| (out_tabs.len() as u32, d)));
        out_tabs.push(Tab::new(name, files, content));
    }
    if !diags.is_empty() {
        return Err(diags);
    }
    let has_array_regions = out_tabs
        .iter()
        .any(|t| t.files.iter().any(|f| f.array_formula));
    let has_forgers = out_tabs.iter().any(|t| {
        t.files.iter().any(|f| {
            f.grid.cells.iter().any(
                |c| matches!(c, GridCell::Formula { expr, .. } if forge::expr_has_forger(expr)),
            )
        })
    });
    Ok(Workbook {
        tabs: out_tabs,
        pulled_faults,
        names,
        has_array_regions,
        has_forgers,
        forge: ForgeStore::default(),
        now: system_now_serial(),
        current_sheet: Cell::new(0),
        memo: RefCell::new(HashMap::new()),
        results: RefCell::new(HashMap::new()),
        current_file: Cell::new(None),
        arena: Arena::default(),
        diagnostics: RefCell::new(Vec::new()),
        #[cfg(test)]
        eval_count: Cell::new(0),
        #[cfg(test)]
        pass_count: Cell::new(0),
        #[cfg(test)]
        covering_scan_steps: Cell::new(0),
    })
}

/// `None` for an A1-shaped regular file, which is a cell, for a presentation sidecar, or for an
/// unreadable kind. A degraded symlink lands in the ref-file arm.
fn read_name_entry(
    root: &Path,
    scope: NameScope,
    entry_name: &str,
    path: &Path,
    ft: std::fs::FileType,
    scratch: &mut Vec<u8>,
) -> std::io::Result<Option<RawNameEntry>> {
    // Before the symlink arm: a figure is one however the filesystem stores it, and a name branch claiming its stem is the one thing this entry kind may never do.
    if is_figure_entry(entry_name) {
        return Ok(None);
    }
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
    if ft.is_file()
        && !is_cell_filename(entry_name)
        && !is_presentation_entry(entry_name)
        && !is_figure_entry(entry_name)
    {
        return Ok(Some(RawNameEntry {
            scope,
            entry_name: entry_name.to_string(),
            form: NameRepr::RefFile {
                content: read_file_to_string(path, scratch)?,
            },
        }));
    }
    Ok(None)
}

/// Every cell file in a real workbook is a few dozen bytes, so one `read` of this size is the file.
const READ_BUF: usize = 64 * 1024;

/// ONE read is the whole file: POSIX's causes of a short read are end-of-file, a signal, and a
/// special source, and every caller has gated on `is_file()`. The `EINTR` retry covers only half the
/// signal case — the other half rests on Linux's regular-file path not testing for a deliverable
/// signal mid-copy. `scratch` is the BUFFER, never the content: only the `filled` prefix is read.
fn read_file_to_string(path: &Path, scratch: &mut Vec<u8>) -> std::io::Result<String> {
    use std::io::Read;

    if scratch.len() < READ_BUF {
        scratch.resize(READ_BUF, 0);
    }
    let mut file = std::fs::File::open(path)?;
    let mut filled = loop {
        match file.read(&mut scratch[..READ_BUF]) {
            Ok(n) => break n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    };
    if filled == READ_BUF {
        // The one ambiguous case: the read may have stopped at the buffer's edge, not the file's.
        scratch.truncate(READ_BUF);
        filled += file.read_to_end(scratch)?;
    }
    match std::str::from_utf8(&scratch[..filled]) {
        Ok(text) => Ok(text.to_owned()),
        // `read_to_string`'s own wording, hand-copied because std does not export the string, and a committed observable: `a_cell_file_that_is_not_utf8_is_a_located_refusal` pins it verbatim.
        Err(_) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        )),
    }
}

/// LEXICAL: the target cell file need not exist for the reader.
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

/// A [`Workbook`] must STORE `now` so [`Workbook::with_now`] can pin it, which is why it cannot
/// inherit the [`Resolver`] trait's own `now_serial`; this composes the two shared single-homes
/// rather than re-deriving any clock or epoch arithmetic.
fn system_now_serial() -> f64 {
    unix_secs_to_serial(system_now_secs())
}

/// Single-homes the ordering the hash fold and the trace walk both rely on, so a hash and a trace
/// over one cell agree on the shape of its dependency set.
fn sort_dedup(mut keys: Vec<CellKey>) -> Vec<CellKey> {
    keys.sort_unstable();
    keys.dedup();
    keys
}
