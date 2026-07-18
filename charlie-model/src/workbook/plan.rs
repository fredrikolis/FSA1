// Concern: the PLAN pass of the two-pass engine (ENG3) — the engine-private dependency-graph types [`DepGraph`]/[`PlanNode`] and the DFS that BUILDS the graph for a set of demanded cells (`demand` orchestrates a fresh pass; `plan_visit` accretes each demanded cell and its transitive dependencies into ONE merged graph, expanding ranges to cells via `expr_deps`/`collect_deps`, detecting reference cycles and the pull-depth bound as terminal nodes with their located `cycle_diag`/`depth_diag` refusals) | Non-concern: computing any VALUE (the `evaluate` sibling owns the EVALUATE pass), reading cells (the `resolver` sibling), and the graph types' escape from this module — they are `pub(super)` at most, re-exported by no one (ENG3 containment) | IO: (a set of demanded `CellKey`s + the `Workbook`'s grids) -> a populated `DepGraph`, plus the located cycle/depth `Diagnostic`s pushed during planning
use std::collections::{HashMap, HashSet};

use charlie_ast::{Expr, RangeRef, Resolver, SheetId};

use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::grid::Cell as GridCell;

use super::cache::CacheScan;
use super::{CellKey, FileId, MAX_PULL_DEPTH, MAX_RANGE_CELLS, Workbook};

/// One node of the PLAN pass's dependency graph — how a demanded cell is computed by the EVALUATE
/// pass. PRIVATE to this module (ENG3 containment): it appears in no other module's surface and is
/// never re-exported.
///
/// A literal cell and a gap are NOT nodes — the EVALUATE pass reads them straight from the grid, so
/// only cells that need computation (formulas) or a pre-decided refusal (cycle / depth) are nodes.
pub(super) enum PlanNode {
    /// A formula cell: its covering file, the cell's local offset into that file's grid, and the
    /// dependency cells whose values must be computed first. `deps` is the formula's static references
    /// (ranges expanded to their cells); a dep that is itself a graph node orders before this one.
    Formula {
        file: FileId,
        dr: u32,
        dc: u32,
        deps: Vec<CellKey>,
    },
    /// A GRID5 ARRAY-FORMULA REGION, keyed at its TOP-LEFT (anchor) cell: the single `=formula` (the
    /// covering file's lone grid cell) whose array value fills the WHOLE region. Computed exactly once
    /// (ENG2/ENG3); the EVALUATE pass writes each array element into its coordinate's result, so every
    /// coordinate reference into the region resolves to that element. `deps` is the formula's
    /// dependency cells (a reference into ANOTHER region is already redirected to that region's anchor
    /// by `collect_deps`, so the graph orders one region's compute before a dependent region's).
    ArrayRegion { file: FileId, deps: Vec<CellKey> },
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
pub(super) struct DepGraph {
    pub(super) nodes: HashMap<CellKey, PlanNode>,
}

impl Workbook {
    // ------------------------------------------------------------------------------------------
    // PLAN pass — build one dependency graph of the demanded cells and their transitive deps.
    // ------------------------------------------------------------------------------------------

    /// Build (and merge into) the dependency graph for a set of demanded cells, then run the EVALUATE
    /// pass over it. Each demanded cell accretes into the SAME [`DepGraph`] (a shared dependency
    /// becomes one node); an already-memoized cell is a resolved leaf and is not re-planned (ENG4).
    pub(super) fn demand(&self, roots: &[CellKey]) {
        let mut graph = DepGraph::default();
        // One cache scan shared across the pass so each cell's content cone hashes at most once (ENG7).
        let mut scan = CacheScan::new();
        for &r in roots {
            let mut on_stack = HashSet::new();
            self.plan_visit(r, 0, &mut graph, &mut on_stack, &mut scan);
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
    pub(super) fn plan_visit(
        &self,
        key: CellKey,
        depth: u32,
        graph: &mut DepGraph,
        on_stack: &mut HashSet<CellKey>,
        scan: &mut CacheScan,
    ) {
        // GRID5: a coordinate inside an array-formula region is planned/computed at the region's single
        // ANCHOR (top-left) cell — one node for the whole region (VAL1/ENG3, compute once). Redirect
        // here so a demand of any region coordinate plans the shared anchor node; the EVALUATE pass
        // then fills every coordinate's result from the one computed array.
        let key = self.array_region_anchor(key.0, key.1, key.2).unwrap_or(key);
        if graph.nodes.contains_key(&key) {
            return; // already planned this pass — the shared/merged node
        }
        if self.memo.borrow().contains_key(&key) {
            return; // a clean, memoized value — a resolved leaf (ENG4 reuse)
        }
        // ENG7: a persistent cache HIT serves this cell's value into the memo, so it becomes a resolved
        // leaf here and its whole dependency cone is never planned or evaluated (a cached subtree is
        // not recomputed). A miss / uncacheable cell / caching-off falls through to plan normally. The
        // plan `depth` gates the serve against the pull-depth bound: a cell whose cone a cold descent
        // would carry past `MAX_PULL_DEPTH` from here is not served (it would suppress the depth refusal
        // a cache-deleted run raises), so it plans on and reaches the SAME refusal warm as cold (ENG7).
        if self.cache_serve(key, depth, scan) {
            return;
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
            self.plan_visit(d, depth + 1, graph, on_stack, scan);
        }
        on_stack.remove(&key);
        // A dependency descent may have re-entered THIS cell (a cycle back-edge) and already marked it
        // a `Cycle` — or the depth guard may have marked it — so do not overwrite a terminal verdict.
        if !matches!(
            graph.nodes.get(&key),
            Some(PlanNode::Cycle | PlanNode::DepthRefused)
        ) {
            // A GRID5 region anchor becomes an `ArrayRegion` node (computed once, filling the whole
            // region at eval); every other formula cell is a per-cell `Formula` node.
            let node = if file.array_formula {
                PlanNode::ArrayRegion { file: id, deps }
            } else {
                PlanNode::Formula {
                    file: id,
                    dr,
                    dc,
                    deps,
                }
            };
            graph.nodes.insert(key, node);
        }
    }

    /// The dependency cells of a formula's parsed tree, resolved to `(sheet, col, row)` keys against
    /// `home` (the sheet an unqualified reference binds to). Every reference is static in v1 (there are
    /// no reference-forging functions — `INDIRECT`/`OFFSET` are reserved refusals), so this is the
    /// formula's complete dependency set. A range expands to its cells; a range over
    /// [`MAX_RANGE_CELLS`] is left unexpanded (the resolver refuses it as `#NUM!` at evaluate rather
    /// than allocating a key per cell); an unknown sheet name resolves to no cell (the evaluator maps
    /// it to `#REF!`).
    pub(super) fn expr_deps(&self, expr: &Expr, home: u32) -> Vec<CellKey> {
        let mut out = Vec::new();
        self.collect_deps(expr, home, &mut out);
        out
    }

    /// The graph-ordering dependency key for a referenced coordinate: its GRID5 region ANCHOR when it
    /// lands inside an array-formula region, else the coordinate itself. Redirecting a dependency onto
    /// the region's single node makes the plan order that region's ONE compute before this dependent
    /// (the region's EVALUATE pass fills the referenced coordinate's result, which the dependent then
    /// reads via the resolver). Free when the workbook has no array regions.
    fn dep_key(&self, sheet: u32, col: u32, row: u32) -> CellKey {
        self.array_region_anchor(sheet, col, row)
            .unwrap_or((sheet, col, row))
    }

    fn collect_deps(&self, expr: &Expr, home: u32, out: &mut Vec<CellKey>) {
        match expr {
            Expr::Lit(_) => {}
            Expr::Ref(r) => {
                if let Some(cr) = r.resolve(|name| self.sheet_id(name)) {
                    let s = cr.sheet.map_or(home, |SheetId(i)| i);
                    out.push(self.dep_key(s, cr.col, cr.row));
                }
            }
            Expr::Range(rn) => {
                if let Some(rr) = rn
                    .resolve(|name| self.sheet_id(name))
                    .map(RangeRef::normalized)
                {
                    let s = rr.start.sheet.map_or(home, |SheetId(i)| i);
                    let (c0, c1) = (rr.start.col, rr.end.col);
                    let (r0, r1) = (rr.start.row, rr.end.row);
                    let area = (u64::from(r1 - r0) + 1) * (u64::from(c1 - c0) + 1);
                    if area <= MAX_RANGE_CELLS {
                        for row in r0..=r1 {
                            for col in c0..=c1 {
                                out.push(self.dep_key(s, col, row));
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
}
