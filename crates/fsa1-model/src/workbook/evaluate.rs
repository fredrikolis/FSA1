// Concern: the EVALUATE pass, computing each planned node once in dependency order | Non-concern: building the graph, reading a computed value | IO: (&DepGraph) -> per-cell results

use std::collections::HashSet;

use fsa1_ast::{ErrKind, Shape, Value, eval_at};

use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::grid::Cell as GridCell;

use super::plan::{DepGraph, PlanNode};
use super::{CellKey, FileId, Workbook};

impl Workbook {
    /// The [`Resolver`](fsa1_ast::Resolver) each node evaluates against only READS values its
    /// dependencies already wrote into the pass results; nothing here recurses.
    pub(super) fn evaluate(&self, graph: &DepGraph) {
        for key in self.topo_order(graph) {
            match graph.nodes.get(&key) {
                Some(PlanNode::Cycle) => {
                    // A cycle is a permanent, content-deterministic `#REF!`, so it memoizes.
                    self.fill_terminal(key, Value::Error(ErrKind::Ref));
                }
                Some(PlanNode::Formula { file, dr, dc, .. }) => {
                    self.current_sheet.set(file.0);
                    self.current_file.set(Some(*file));
                    #[cfg(test)]
                    self.eval_count.set(self.eval_count.get() + 1);
                    let v = self.compute_formula(*file, *dr, *dc);
                    self.results.borrow_mut().insert(key, v);
                }
                Some(PlanNode::ArrayRegion { file, .. }) => {
                    self.current_sheet.set(file.0);
                    self.current_file.set(Some(*file));
                    #[cfg(test)]
                    self.eval_count.set(self.eval_count.get() + 1);
                    self.fill_array_region(*file);
                }
                None => {}
            }
        }
    }

    /// Iterative rather than recursive, so the order is stack-safe for any DAG the plan built. The
    /// graph IS a DAG: the plan already broke every cycle into a terminal.
    fn topo_order(&self, graph: &DepGraph) -> Vec<CellKey> {
        let mut order = Vec::with_capacity(graph.nodes.len());
        let mut seen: HashSet<CellKey> = HashSet::new();
        // Sorted, so diagnostics pushed during the pass have a stable order run-to-run. Values are unaffected: topo order is respected within each cone either way.
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
                let deps = match graph.nodes.get(&k) {
                    Some(PlanNode::Formula { deps, .. } | PlanNode::ArrayRegion { deps, .. }) => {
                        Some(deps)
                    }
                    _ => None,
                };
                if let Some(deps) = deps {
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

    /// EVERY result promotes: a value derives from its content cone alone, so it is the same value
    /// any other demand would compute and reusing it is sound by construction.
    pub(super) fn finish_pass(&self) {
        {
            let results = self.results.borrow();
            let mut memo = self.memo.borrow_mut();
            for (k, v) in results.iter() {
                memo.insert(*k, v.clone());
            }
        }
        self.results.borrow_mut().clear();
    }

    /// A repeat demand of an already-memoized cone must not increment this. Matching values alone
    /// would not prove the memo REUSES rather than recomputes; this does.
    #[cfg(test)]
    pub(crate) fn eval_count(&self) -> u64 {
        self.eval_count.get()
    }

    #[cfg(test)]
    pub(crate) fn pass_count(&self) -> u64 {
        self.pass_count.get()
    }

    /// Candidate regions examined in `covering`'s multi-cell fallback; a single-cell lookup resolves
    /// through the `single` index and adds nothing here.
    #[cfg(test)]
    pub(crate) fn covering_scan_steps(&self) -> u64 {
        self.covering_scan_steps.get()
    }

    /// Requires `current_sheet`/`current_file` already set to this cell's context. A multi-cell array
    /// result collapses to its TOP-LEFT element rather than `#VALUE!` — implicit intersection for a
    /// formula occupying ONE cell, distinct from [`fsa1_ast::scalarize`]'s in-expression rule. The
    /// last two arms are unreachable, only a formula node routing here, but stay total.
    fn compute_formula(&self, id: FileId, dr: u32, dc: u32) -> Value {
        let file = &self.tabs[id.0 as usize].files[id.1];
        match file.grid.cell_at(dr, dc) {
            GridCell::Formula { expr, .. } => {
                let effective = if self.has_forgers {
                    let key = (id.0, file.region.min_col + dc, file.region.min_row + dr);
                    self.effective_expr(key, expr)
                } else {
                    expr
                };
                // The anchor for the no-argument ROW()/COLUMN() forms; every other formula ignores it.
                let row = file.region.min_row + dr;
                let col = file.region.min_col + dc;
                cell_scalar(eval_at(effective, self, row, col))
            }
            GridCell::Value { value, .. } => value.clone(),
            GridCell::LoadError { diag, .. } => crate::grid::load_error_value(diag),
        }
    }

    /// Three outcomes, all TOTAL: an array matching the region's shape AND orientation scatters
    /// element-wise; the formula's own error value fills every coordinate by ordinary propagation;
    /// anything else is one located dimension refusal plus a `#SPILL!` at every coordinate.
    fn fill_array_region(&self, id: FileId) {
        let file = &self.tabs[id.0 as usize].files[id.1];
        let region = file.region;
        let rows = region.max_row - region.min_row + 1;
        let cols = region.max_col - region.min_col + 1;
        let value = match file.grid.cell_at(0, 0) {
            // v1 keeps ROW()/COLUMN() scalar, so the region's one formula reports its anchor coordinate rather than an array of them. The two arms below are unreachable — `parse_file` guarantees a region is a lone `=formula` — but stay total.
            GridCell::Formula { expr, .. } => eval_at(expr, self, region.min_row, region.min_col),
            GridCell::Value { value, .. } => value.clone(),
            GridCell::LoadError { diag, .. } => crate::grid::load_error_value(diag),
        };
        let region_shape = Shape { rows, cols };
        let fill: Value = match &value {
            Value::Array(shape, cells) if *shape == region_shape => {
                self.scatter(id.0, region.min_col, region.min_row, cols, cells);
                return;
            }
            Value::Error(k) => Value::Error(*k),
            _ => {
                // The `Code` that locates the refusal also names its class, so the surfaced cell value cannot drift from it.
                let diag = self.region_mismatch_diag(id, region_shape, &value);
                let kind = diag
                    .code
                    .err_class()
                    .expect("a region dimension mismatch cites the #SPILL! error class");
                self.refuse(diag);
                Value::Error(kind)
            }
        };
        let uniform: Vec<Value> = vec![fill; (rows as usize) * (cols as usize)];
        self.scatter(id.0, region.min_col, region.min_row, cols, &uniform);
    }

    /// A region ANCHOR fans the error out to EVERY coordinate, because the region's single formula
    /// never runs: without that, `value()` would miss a continuation coordinate in the results and
    /// fall through to a region-ABSOLUTE read against the region's 1x1 grid, indexing out of bounds.
    fn fill_terminal(&self, anchor: CellKey, err: Value) {
        let (sheet, col, row) = anchor;
        // Snapshotted, so no `&self.tabs` borrow is held across the `scatter` write.
        let region = self
            .covering(sheet, col, row)
            .filter(|(_, f)| f.array_formula)
            .map(|(_, f)| f.region);
        match region {
            Some(region) => {
                let rows = region.max_row - region.min_row + 1;
                let cols = region.max_col - region.min_col + 1;
                let uniform = vec![err; (rows as usize) * (cols as usize)];
                self.scatter(sheet, region.min_col, region.min_row, cols, &uniform);
            }
            None => {
                self.results.borrow_mut().insert(anchor, err);
            }
        }
    }

    /// `cells` is row-major of width `cols`; element `(r,c)` lands at `(min_col+c, min_row+r)`.
    fn scatter(&self, sheet: u32, min_col: u32, min_row: u32, cols: u32, cells: &[Value]) {
        let mut results = self.results.borrow_mut();
        for (i, v) in cells.iter().enumerate() {
            let r = (i as u32) / cols;
            let c = (i as u32) % cols;
            results.insert((sheet, min_col + c, min_row + r), v.clone());
        }
    }

    fn region_mismatch_diag(&self, id: FileId, region_shape: Shape, got: &Value) -> Diagnostic {
        let name = self.file_name(id);
        let tab = self.tab_name(id.0);
        let got_desc = match got {
            Value::Array(s, _) => format!("a {}x{} array", s.rows, s.cols),
            Value::Blank => "a blank scalar".to_string(),
            _ => "a scalar".to_string(),
        };
        Diagnostic::new(
            Code::DimensionMismatch,
            Loc::tab_file(&tab, &name),
            format!(
                "the array formula in {tab}/{name} produced {got_desc}, which does not fill its \
                 {}x{} range with the exact shape and orientation -- refused as a located dimension \
                 error (#SPILL!-class, GRID5)",
                region_shape.rows, region_shape.cols,
            ),
        )
    }
}

/// There is no dynamic spill beyond a declared range, so a formula in ONE cell holds the array's
/// implicit-intersection top-left rather than `#VALUE!`. A multi-coordinate region never routes here.
fn cell_scalar(v: Value) -> Value {
    match v {
        Value::Array(_, cells) => cells.into_iter().next().unwrap_or(Value::Blank),
        other => other,
    }
}
