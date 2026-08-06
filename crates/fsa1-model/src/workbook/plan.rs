// Concern: the PLAN pass, building one merged dependency graph over the demanded cells and their cone | Non-concern: computing a value, reading one | IO: (&[CellKey]) -> DepGraph

use std::collections::{HashMap, HashSet};

use fsa1_ast::{Expr, Resolver, SheetId};

use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::grid::Cell as GridCell;

use super::{CellKey, FileId, MAX_RANGE_CELLS, Workbook};

/// A literal cell and a gap are NOT nodes — the EVALUATE pass reads them straight from the grid — so
/// only a cell needing computation, or a pre-decided refusal, is one.
pub(super) enum PlanNode {
    /// `deps` is the formula's static references, ranges expanded to their cells; a dep that is
    /// itself a node orders before this one.
    Formula {
        file: FileId,
        dr: u32,
        dc: u32,
        deps: Vec<CellKey>,
    },
    /// Keyed at the region's TOP-LEFT anchor and computed exactly once, the EVALUATE pass writing
    /// each array element into its own coordinate's result.
    ArrayRegion { file: FileId, deps: Vec<CellKey> },
    /// The one terminal the plan itself decides: a cycle has no content-derived value, so refusing
    /// it IS the value, and that refusal is a property of the cell rather than of the walk.
    Cycle,
}

/// Built up and MERGED across a pass's demanded cells, so a dependency several of them need becomes
/// one shared node. Removing that sharing would change performance, never results.
#[derive(Default)]
pub(super) struct DepGraph {
    pub(super) nodes: HashMap<CellKey, PlanNode>,
}

/// What native recursion kept implicitly. `Exit` is pushed BENEATH a cell's dependencies, so it pops
/// last and takes the cell back off the DFS path.
enum Step {
    Enter(CellKey),
    Exit(CellKey),
}

impl Workbook {
    /// An already-memoized cell is a resolved leaf and is not re-planned.
    pub(super) fn demand(&self, roots: &[CellKey]) {
        #[cfg(test)]
        self.pass_count.set(self.pass_count.get() + 1);
        // Pass 0: rewrite every forger in the cone to a static reference BEFORE planning, so both later passes operate on the fully-static effective form.
        if self.has_forgers {
            self.resolve_forgers(roots);
        }
        let mut graph = DepGraph::default();
        for &r in roots {
            self.plan_visit(r, &mut graph);
        }
        self.evaluate(&graph);
    }

    /// The DFS is driven by an EXPLICIT stack, so its depth is bounded by the heap rather than the
    /// native one: a cell's value is a function of its content cone alone, so a dependency chain has
    /// no length limit and can never depend on how deep the walk that reached it happened to be. A
    /// cell re-entered while still on the path REPLACES the node its own entry recorded.
    pub(super) fn plan_visit(&self, root: CellKey, graph: &mut DepGraph) {
        // The DFS path, for cycle detection.
        let mut on_stack: HashSet<CellKey> = HashSet::new();
        let mut stack = vec![Step::Enter(root)];
        while let Some(step) = stack.pop() {
            let key = match step {
                Step::Exit(key) => {
                    on_stack.remove(&key);
                    continue;
                }
                Step::Enter(key) => key,
            };
            // Redirected so a demand of ANY region coordinate plans the one shared anchor node.
            let key = self.array_region_anchor(key.0, key.1, key.2).unwrap_or(key);
            if on_stack.contains(&key) {
                // Recorded once: a second back-edge finds the terminal already there.
                if !matches!(graph.nodes.get(&key), Some(PlanNode::Cycle)) {
                    graph.nodes.insert(key, PlanNode::Cycle);
                    self.refuse(self.cycle_diag(key));
                }
                continue;
            }
            if graph.nodes.contains_key(&key) {
                continue; // the shared node
            }
            if self.memo.borrow().contains_key(&key) {
                continue; // a resolved leaf
            }
            let (sheet, col, row) = key;
            let Some((id, file)) = self.covering(sheet, col, row) else {
                continue; // a gap; the resolver reads Blank directly
            };
            let dr = row - file.region.min_row;
            let dc = col - file.region.min_col;
            let GridCell::Formula { expr, .. } = file.grid.cell_at(dr, dc) else {
                continue;
            };
            // The EFFECTIVE expr, so a demanded `SUM(OFFSET(...))` plans the static form it was rewritten to.
            let deps = self.expr_deps(self.effective_expr(key, expr), sheet);
            on_stack.insert(key);
            // Exit first, then the deps in reverse, so popping reproduces the recursive walk's pre-order.
            stack.push(Step::Exit(key));
            for &d in deps.iter().rev() {
                stack.push(Step::Enter(d));
            }
            // Recorded BEFORE the descent, since nothing borrows `deps` afterwards.
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

    /// `home` is the sheet an unqualified reference binds to. Callers must pass the EFFECTIVE expr,
    /// on which every reference is static and this is therefore the complete dependency set. A range
    /// over [`MAX_RANGE_CELLS`] is left unexpanded rather than allocating a key per cell.
    pub(super) fn expr_deps(&self, expr: &Expr, home: u32) -> Vec<CellKey> {
        let mut out = Vec::new();
        self.collect_deps(expr, home, &mut out);
        out
    }

    /// Redirecting a dependency onto its region's single node is what makes the plan order that
    /// region's one compute before this dependent.
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
                // Measured through the ONE shared helper, so the plan stays a superset of what eval reads and the computation hash grades the same rectangle.
                if let Some(r) = self.clamped_range(rn, home)
                    && r.area <= MAX_RANGE_CELLS
                {
                    for row in r.r0..=r.r1 {
                        for col in r.c0..=r.c1 {
                            out.push(self.dep_key(r.sheet, col, row));
                        }
                    }
                }
                // Over the bound the dependency set is not enumerable here, so the cell also gets no hash digest.
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
}
