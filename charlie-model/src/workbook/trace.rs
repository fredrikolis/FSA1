// Concern: the TRACE dependency-inspection surface (CLI2) — a PUBLIC value projection [`TraceNode`] of a cell's UPSTREAM dependencies (recurse `expr_deps`) or DOWNSTREAM consumers ([`Direction::Downstream`], the SAME engine dependency relation TRANSPOSED via a one-pass inversion of the forward-dep map, O(cells) — never a second dependency parser) rooted at a demanded cell, each node carrying its sheet-qualified A1, its formula text, its value (via `value_at`), a [`TraceStatus`], and its computation hash (`None` on a cycle); the walk is visited-once (a shared dependency appears once, `repeated=true`, not re-descended — ENG3 sharing) and cycle-safe (a back-edge is reported `status=Cycle`, never looped — ENG2), and every bad input is a located refusal, never a panic (CORE2) | Non-concern: computing VALUES (the `evaluate` sibling / `value_at`), the computation-hash digest itself (the `hash` sibling owns it; this only projects the opaque `Option<String>`), the engine's private `DepGraph`/`PlanNode`/`CompHash` types (ENG3 containment — none appear in this public projection), and argv parsing / rendering the tree (charlie-cli owns the `trace` subcommand + its text/JSON forms) | IO: (a `(sheet,col,row)` cell + a [`Direction`] + an optional depth cap) -> `Result<TraceNode, Diagnostic>` (a located refusal for an out-of-range tab)
use std::collections::{HashMap, HashSet};

use charlie_ast::{Value, format_cell};

use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::grid::Cell as GridCell;
use crate::render::display_value;

use super::hash::HashMemo;
use super::{CellKey, MAX_PULL_DEPTH, Workbook, sort_dedup};

/// Which way to walk the engine's dependency relation from the traced cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// The cells this cell READS, transitively (its upstream dependencies).
    Upstream,
    /// The cells that READ this cell, transitively (its downstream consumers) — the same relation
    /// TRANSPOSED (CLI2: dependents are the dependency relation transposed, not a separate parse).
    Downstream,
}

/// The classification of a traced cell — mirrors the plan's terminals plus the literal/blank/ok/error
/// cases. Reported per node so a consumer need not re-inspect the value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceStatus {
    /// A formula cell that computed to a non-error value.
    Ok,
    /// A literal (non-blank) cell.
    Literal,
    /// A gap (no file) or an empty literal cell.
    Blank,
    /// A cell on a reference cycle (ENG2) — reported, never looped; it carries no computation hash.
    Cycle,
    /// A formula cell that computed to a spreadsheet error value (e.g. `#DIV/0!`, `#REF!`).
    Error,
    /// A formula branch the trace stopped descending at the engine's pull-depth bound (never a stack
    /// overflow — CORE2).
    DepthLimit,
}

impl TraceStatus {
    /// The stable lowercase name a consumer (the CLI's text/JSON forms) spells this status as.
    pub fn as_str(self) -> &'static str {
        match self {
            TraceStatus::Ok => "ok",
            TraceStatus::Literal => "literal",
            TraceStatus::Blank => "blank",
            TraceStatus::Cycle => "cycle",
            TraceStatus::Error => "error",
            TraceStatus::DepthLimit => "depth-limit",
        }
    }
}

/// One node of a dependency trace — a PUBLIC value projection (the engine's graph types never appear
/// here, ENG3 containment). Its `children` are the next hop in the requested [`Direction`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceNode {
    /// The sheet-qualified A1 address (`Sheet1!C3`).
    pub cell: String,
    /// The cell's formula source text (`=SUM(C1:C2)`), or `None` for a literal / blank / gap.
    pub formula: Option<String>,
    /// The cell's computed value, spelled the same way `render`/`eval` spell it.
    pub value: String,
    /// The cell's classification.
    pub status: TraceStatus,
    /// The cell's computation hash (ENG7) as an opaque hex string, or `None` on a cycle / depth-tainted
    /// cell.
    pub hash: Option<String>,
    /// The next hop of the trace (upstream dependencies or downstream consumers). Empty at a leaf, at a
    /// `repeated` node, at a cycle back-edge, or where the depth cap stopped the walk.
    pub children: Vec<TraceNode>,
    /// `true` iff this cell was already emitted earlier in the trace (a shared dependency / consumer):
    /// it is shown once with its value + hash and NOT re-descended (ENG3 sharing; avoids diamond
    /// blow-up). A cycle back-edge is `status=Cycle` instead, never `repeated`.
    pub repeated: bool,
}

/// The kind of the cell covering a coordinate (owned, so it does not hold a `&Workbook` borrow across
/// the recursive walk).
enum CellKind {
    /// No file covers the coordinate.
    Gap,
    /// A literal `Blank` cell.
    Blank,
    /// A literal, non-blank value.
    Literal,
    /// A formula cell (its verbatim source text).
    Formula(String),
}

/// The mutable state of one trace walk.
struct Tracer<'w> {
    wb: &'w Workbook,
    dir: Direction,
    max_depth: Option<u32>,
    /// The reverse (consumer) index, built once on the first downstream hop (O(cells)).
    reverse: Option<HashMap<CellKey, Vec<CellKey>>>,
    /// The cells currently on the DFS path — for cycle (back-edge) detection.
    path: Vec<CellKey>,
    /// Every cell already emitted this trace — for the visited-once (`repeated`) rule.
    visited: HashSet<CellKey>,
    /// Cells discovered to lie on a reference cycle (the members between a back-edge and its target).
    cycle_members: HashSet<CellKey>,
    /// One shared computation-hash memo across every node, so the whole trace hashes in O(cone).
    hashes: HashMemo,
}

impl<'w> Tracer<'w> {
    fn new(wb: &'w Workbook, dir: Direction, max_depth: Option<u32>) -> Tracer<'w> {
        Tracer {
            wb,
            dir,
            max_depth,
            reverse: None,
            path: Vec::new(),
            visited: HashSet::new(),
            cycle_members: HashSet::new(),
            hashes: HashMemo::new(),
        }
    }

    /// Recursively build the trace node for `key` at DFS `depth`.
    fn walk(&mut self, key: CellKey, depth: u32) -> TraceNode {
        // GRID5: a region member is traced at its anchor — one node for the whole region.
        let key = self
            .wb
            .array_region_anchor(key.0, key.1, key.2)
            .unwrap_or(key);
        let cell = self.qualified(key);
        let val = self.wb.value_at(key.0, key.1, key.2);
        let value = display_value(&val);
        let kind = self.cell_kind(key);
        let formula = match &kind {
            CellKind::Formula(src) => Some(src.clone()),
            _ => None,
        };

        // A back-edge to a cell already on the path: a reference cycle. Mark every member from the
        // back-edge target up to here, and report it as a Cycle terminal (never re-descended).
        if let Some(pos) = self.path.iter().position(|&k| k == key) {
            for &m in &self.path[pos..] {
                self.cycle_members.insert(m);
            }
            return TraceNode {
                cell,
                formula,
                value,
                status: TraceStatus::Cycle,
                hash: None,
                children: Vec::new(),
                repeated: false,
            };
        }

        // Already emitted (a shared dependency / consumer): show it once, do not re-descend.
        if self.visited.contains(&key) {
            let hash = self.hashes_of(key);
            return TraceNode {
                cell,
                formula,
                value,
                status: self.classify(key, &kind, &val, false),
                hash,
                children: Vec::new(),
                repeated: true,
            };
        }

        self.visited.insert(key);
        self.path.push(key);
        let user_capped = self.max_depth.is_some_and(|m| depth >= m);
        let hard_capped = depth >= MAX_PULL_DEPTH;
        let children: Vec<TraceNode> = if user_capped || hard_capped {
            Vec::new()
        } else {
            self.neighbors(key, &kind)
                .into_iter()
                .map(|n| self.walk(n, depth + 1))
                .collect()
        };
        self.path.pop();

        let status = self.classify(key, &kind, &val, hard_capped);
        let hash = self.hashes_of(key);
        TraceNode {
            cell,
            formula,
            value,
            status,
            hash,
            children,
            repeated: false,
        }
    }

    /// The next hop from `key`: upstream dependencies (the formula's `expr_deps`) or downstream
    /// consumers (the transposed relation). Sorted + deduped for a deterministic tree.
    fn neighbors(&mut self, key: CellKey, kind: &CellKind) -> Vec<CellKey> {
        match self.dir {
            Direction::Upstream => self.upstream_deps(key, kind),
            Direction::Downstream => {
                let wb = self.wb;
                let rev = self.reverse.get_or_insert_with(|| wb.build_reverse_deps());
                sort_dedup(rev.get(&key).cloned().unwrap_or_default())
            }
        }
    }

    /// The upstream dependency cells of a formula (already `dep_key`-redirected to region anchors by
    /// `expr_deps`), sorted + deduped. A non-formula cell has no upstream dependencies.
    fn upstream_deps(&self, key: CellKey, kind: &CellKind) -> Vec<CellKey> {
        if !matches!(kind, CellKind::Formula(_)) {
            return Vec::new();
        }
        let (sheet, col, row) = key;
        let Some(cell) = self.wb.grid_cell_at(sheet, col, row) else {
            return Vec::new();
        };
        let GridCell::Formula { expr, .. } = cell else {
            return Vec::new();
        };
        sort_dedup(self.wb.expr_deps(expr, sheet))
    }

    /// Classify a cell: literal/blank from its grid kind; a formula is `Cycle` (a discovered cycle
    /// member), `DepthLimit` (the walk hit the pull-depth bound here), `Error` (an error value), else
    /// `Ok`.
    fn classify(
        &self,
        key: CellKey,
        kind: &CellKind,
        val: &Value,
        hard_capped: bool,
    ) -> TraceStatus {
        match kind {
            CellKind::Gap | CellKind::Blank => TraceStatus::Blank,
            CellKind::Literal => TraceStatus::Literal,
            CellKind::Formula(_) => {
                if self.cycle_members.contains(&key) {
                    TraceStatus::Cycle
                } else if hard_capped {
                    TraceStatus::DepthLimit
                } else if matches!(val, Value::Error(_)) {
                    TraceStatus::Error
                } else {
                    TraceStatus::Ok
                }
            }
        }
    }

    /// The opaque computation hash of a cell, using the shared memo.
    fn hashes_of(&mut self, key: CellKey) -> Option<String> {
        self.wb.computation_hash_with(key, &mut self.hashes)
    }

    /// The sheet-qualified A1 address of a cell (`Sheet1!C3`).
    fn qualified(&self, key: CellKey) -> String {
        format!("{}!{}", self.wb.tab_name(key.0), format_cell(key.1, key.2))
    }

    /// The kind of the cell covering `key` (owned, so it holds no `&Workbook` borrow).
    fn cell_kind(&self, key: CellKey) -> CellKind {
        let (sheet, col, row) = key;
        let Some(cell) = self.wb.grid_cell_at(sheet, col, row) else {
            return CellKind::Gap;
        };
        match cell {
            GridCell::Value(Value::Blank) => CellKind::Blank,
            GridCell::Value(_) => CellKind::Literal,
            GridCell::Formula { src, .. } => CellKind::Formula(src.clone()),
        }
    }
}

impl Workbook {
    /// Trace a cell's dependencies (CLI2). [`Direction::Upstream`] lists the cells it reads
    /// transitively; [`Direction::Downstream`] lists the cells that read it (the same engine dependency
    /// relation transposed, ENG3). The walk is visited-once (a shared cell appears once with
    /// `repeated=true`) and cycle-safe (a back-edge is `status=Cycle`, never looped, ENG2). Each node
    /// carries its value and, unless on a cycle, its computation hash (ENG7). `max_depth` caps the walk
    /// (`None` = unbounded, still bounded by the engine's pull-depth guard). An out-of-range tab is a
    /// located refusal (CORE2), never a panic.
    pub fn trace(
        &self,
        sheet: u32,
        col: u32,
        row: u32,
        dir: Direction,
        max_depth: Option<u32>,
    ) -> Result<TraceNode, Diagnostic> {
        if sheet as usize >= self.tabs.len() {
            return Err(Diagnostic::new(
                Code::CellOutOfRange,
                Loc::tab(&sheet.to_string()),
                format!(
                    "cannot trace tab index {sheet}: the workbook has {} tab(s)",
                    self.tabs.len()
                ),
            ));
        }
        let mut tracer = Tracer::new(self, dir, max_depth);
        Ok(tracer.walk((sheet, col, row), 0))
    }

    /// Build the DOWNSTREAM (consumer) index in ONE pass over every cell — the forward dependency map
    /// the engine already computes (`expr_deps`), inverted: for each formula cell `c` with dependency
    /// `d`, record `c` under `d`. O(cells); reuses the engine's ONE dependency parser (never a second).
    /// A GRID5 region contributes ONE source cell (its anchor formula); references INTO a region are
    /// already `dep_key`-redirected to the anchor, so a region's consumers are keyed by the anchor.
    fn build_reverse_deps(&self) -> HashMap<CellKey, Vec<CellKey>> {
        let mut rev: HashMap<CellKey, Vec<CellKey>> = HashMap::new();
        for (s, tab) in self.tabs.iter().enumerate() {
            let sheet = s as u32;
            for file in &tab.files {
                if file.array_formula {
                    let anchor = (sheet, file.region.min_col, file.region.min_row);
                    if let GridCell::Formula { expr, .. } = file.grid.cell_at(0, 0) {
                        for d in self.expr_deps(expr, sheet) {
                            rev.entry(d).or_default().push(anchor);
                        }
                    }
                    continue;
                }
                let rows = file.region.max_row - file.region.min_row + 1;
                let cols = file.region.max_col - file.region.min_col + 1;
                for dr in 0..rows {
                    for dc in 0..cols {
                        if let GridCell::Formula { expr, .. } = file.grid.cell_at(dr, dc) {
                            let src = (sheet, file.region.min_col + dc, file.region.min_row + dr);
                            for d in self.expr_deps(expr, sheet) {
                                rev.entry(d).or_default().push(src);
                            }
                        }
                    }
                }
            }
        }
        rev
    }
}
