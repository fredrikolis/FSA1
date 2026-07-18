// Concern: the EVALUATE pass of the two-pass engine (ENG2/ENG3/ENG4) — compute every [`DepGraph`] node exactly once in dependency order (`topo_order` gives an iterative, stack-safe post-order; `evaluate` walks it, turning terminal `Cycle`/`DepthRefused` nodes into their located error values and computing each `Formula` node via `compute_formula` through `charlie_ast::eval` with the resolver reading already-computed deps), then `finish_pass` promotes the clean (non-depth-tainted) results into the memo and clears the per-pass scratch | Non-concern: BUILDING the graph or detecting cycles/depth (the `plan` sibling owns the PLAN pass), reading cell/range values (the `resolver` sibling owns the `Resolver` impl + arena), and the graph types (defined in `plan`, consumed here as `pub(super)`) | IO: a populated `DepGraph` + the `Workbook`'s grids -> per-cell `Value`s written into the pass `results`, then promoted into the `memo`
use std::collections::HashSet;

use charlie_ast::{ErrKind, Value, eval, scalarize};

use crate::grid::Cell as GridCell;

use super::plan::{DepGraph, PlanNode};
use super::{CellKey, FileId, Workbook};

impl Workbook {
    // ------------------------------------------------------------------------------------------
    // EVALUATE pass — compute each graph node once, in dependency order.
    // ------------------------------------------------------------------------------------------

    /// Compute every node of `graph` exactly once (ENG2), each after its dependencies. A terminal
    /// (`Cycle`/`DepthRefused`) yields its located error; a `Formula` node evaluates its tree through
    /// [`charlie_ast::eval`] — during which the [`Resolver`](charlie_ast::Resolver) reads the already-
    /// computed dependency values from the pass results. A value that consumed a depth refusal is
    /// marked tainted so it is not memoized.
    pub(super) fn evaluate(&self, graph: &DepGraph) {
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
    pub(super) fn finish_pass(&self) {
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
    /// cell position by [`charlie_ast::scalarize`] (the AST owns the scalar-position rule). Called from
    /// the EVALUATE pass with `current_sheet`/`current_file` already set to this cell's context; the
    /// [`Resolver`](charlie_ast::Resolver) it evaluates against only READS already-computed dependency
    /// values (never recurses).
    fn compute_formula(&self, id: FileId, dr: u32, dc: u32) -> Value {
        let file = &self.tabs[id.0 as usize].files[id.1];
        match file.grid.cell_at(dr, dc) {
            GridCell::Formula { expr, .. } => scalarize(eval(expr, self)),
            // `compute_formula` is only reached for a formula node; a literal is total-passed-through
            // defensively rather than panicking.
            GridCell::Value(v) => v.clone(),
        }
    }
}
