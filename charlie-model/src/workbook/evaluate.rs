// Concern: the EVALUATE pass of the two-pass engine (ENG2/ENG3/ENG4) — compute every [`DepGraph`] node exactly once in dependency order (`topo_order` gives an iterative, stack-safe post-order; `evaluate` walks it, turning terminal `Cycle`/`DepthRefused` nodes into their located error values and computing each `Formula` node via `compute_formula` through `charlie_ast::eval` with the resolver reading already-computed deps), then `finish_pass` promotes the clean (non-depth-tainted) results into the memo and clears the per-pass scratch | Non-concern: BUILDING the graph or detecting cycles/depth (the `plan` sibling owns the PLAN pass), reading cell/range values (the `resolver` sibling owns the `Resolver` impl + arena), and the graph types (defined in `plan`, consumed here as `pub(super)`) | IO: a populated `DepGraph` + the `Workbook`'s grids -> per-cell `Value`s written into the pass `results`, then promoted into the `memo`
use std::collections::HashSet;

use charlie_ast::{ErrKind, Shape, Value, eval};

use crate::diagnostic::{Code, Diagnostic, Loc};
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
                    // For a GRID5 region anchor this fills EVERY region coordinate: the region's ONE
                    // formula never runs, so without this the continuation coordinates would be absent
                    // from the results and the resolver's grid fall-through would index the region's
                    // 1x1 grid out of bounds (CORE2 — a cyclic region is a located `#REF!` at every
                    // coordinate, ENG2, never a panic).
                    self.fill_terminal(key, Value::Error(ErrKind::Ref), false);
                }
                Some(PlanNode::DepthRefused) => {
                    // Same region fan-out as the cycle case, but depth-tainted (root-relative `#NUM!`,
                    // never memoized): a region reached past the pull-depth bound is a located `#NUM!`
                    // at every coordinate, never an out-of-bounds grid read.
                    self.fill_terminal(key, Value::Error(ErrKind::Num), true);
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
                Some(PlanNode::ArrayRegion { file, deps }) => {
                    // A GRID5 region: evaluate its ONE formula, then write each array element into its
                    // coordinate's result (compute once, ENG2/ENG3). A tainted dep taints every filled
                    // coordinate so none is memoized (mirrors the per-cell depth guard).
                    let tainted = {
                        let t = self.pass_tainted.borrow();
                        deps.iter().any(|d| t.contains(d))
                    };
                    self.current_sheet.set(file.0);
                    self.current_file.set(Some(*file));
                    self.fill_array_region(*file, tainted);
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
    /// (VAL1: evaluated exactly as written — no offset/drag-fill), collapsed to its single-cell scalar
    /// by [`cell_scalar`] — a genuinely multi-cell array result keeps only its TOP-LEFT element (the
    /// GRID5 implicit-intersection rule for a formula in ONE cell, never `#VALUE!`).
    /// [`charlie_ast::scalarize`] still governs the DISTINCT in-expression scalar-position rule (a
    /// scalar-only argument slot demotes a multi-cell array to `#VALUE!`); this cell-position collapse
    /// keeps the top-left instead. Called from the EVALUATE pass with `current_sheet`/`current_file`
    /// already set to this cell's context; the [`Resolver`](charlie_ast::Resolver) it evaluates against
    /// only READS already-computed dependency values (never recurses).
    fn compute_formula(&self, id: FileId, dr: u32, dc: u32) -> Value {
        let file = &self.tabs[id.0 as usize].files[id.1];
        match file.grid.cell_at(dr, dc) {
            GridCell::Formula { expr, .. } => cell_scalar(eval(expr, self)),
            // `compute_formula` is only reached for a formula node; a literal is total-passed-through
            // defensively rather than panicking.
            GridCell::Value(v) => v.clone(),
        }
    }

    /// Evaluate a GRID5 array-formula region's ONE formula and WRITE each result into its coordinate
    /// (the region anchored at `id`'s top-left). Three outcomes, all TOTAL (CORE2 — never a panic):
    /// * an array whose shape AND orientation match the region -> element `(r,c)` fills coordinate
    ///   `(r,c)` (row-major);
    /// * any other value (a scalar, or a wrong-shaped/wrong-oriented array) -> a LOCATED dimension
    ///   error (GRID4's code), recorded once, and every coordinate filled with `#SPILL!` (detected AT
    ///   EVALUATION, GRID5);
    /// * an error value from the formula itself -> that error fills every coordinate (ordinary error
    ///   propagation, not a structural refusal).
    fn fill_array_region(&self, id: FileId, tainted: bool) {
        let file = &self.tabs[id.0 as usize].files[id.1];
        let region = file.region;
        let rows = region.max_row - region.min_row + 1;
        let cols = region.max_col - region.min_col + 1;
        let value = match file.grid.cell_at(0, 0) {
            GridCell::Formula { expr, .. } => eval(expr, self),
            // Defensive: an array region is always a formula file (`parse_file` guarantees it).
            GridCell::Value(v) => v.clone(),
        };
        let region_shape = Shape { rows, cols };
        let fill: Value = match &value {
            Value::Array(shape, cells) if *shape == region_shape => {
                self.scatter(id.0, region.min_col, region.min_row, cols, cells, tainted);
                return;
            }
            Value::Error(k) => Value::Error(*k),
            _ => {
                // Single-source the surfaced cell's error class from the diagnostic registry: the same
                // `Code` that locates the refusal also names its spreadsheet-error class (`err_class`),
                // so the `#SPILL!` cell value can never drift from the code's cited class.
                let diag = self.region_mismatch_diag(id, region_shape, &value);
                let kind = diag
                    .code
                    .err_class()
                    .expect("a region dimension mismatch cites the #SPILL! error class");
                self.refuse(diag);
                Value::Error(kind)
            }
        };
        // A propagated error / a mismatch #SPILL! fills every coordinate uniformly.
        let uniform: Vec<Value> = vec![fill; (rows as usize) * (cols as usize)];
        self.scatter(
            id.0,
            region.min_col,
            region.min_row,
            cols,
            &uniform,
            tainted,
        );
    }

    /// Record a TERMINAL node's error value (a cycle `#REF!` or a depth-refused `#NUM!`) into the
    /// pass results. An ordinary formula cell writes its ONE coordinate; a GRID5 array-formula region
    /// ANCHOR fans the error out to EVERY region coordinate — the region's single formula never runs,
    /// so the continuation coordinates it would have filled must still get a located value here. Without
    /// this the resolver's `value()` would miss the continuation coordinate in the results/memo and fall
    /// through to a region-ABSOLUTE grid read against the region's 1x1 grid, indexing out of bounds
    /// (CORE2 totality — a cyclic / over-deep region is a located error at every coordinate, never a
    /// panic). `tainted` mirrors the terminal's memoization policy (a `#REF!` cycle is clean; a
    /// depth-refused `#NUM!` is root-relative and never memoized).
    fn fill_terminal(&self, anchor: CellKey, err: Value, tainted: bool) {
        let (sheet, col, row) = anchor;
        // A region anchor's coordinate covers an array-formula file; snapshot its region (Copy) so no
        // `&self.tabs` borrow is held across the `scatter` write.
        let region = self
            .covering(sheet, col, row)
            .filter(|(_, f)| f.array_formula)
            .map(|(_, f)| f.region);
        match region {
            Some(region) => {
                let rows = region.max_row - region.min_row + 1;
                let cols = region.max_col - region.min_col + 1;
                let uniform = vec![err; (rows as usize) * (cols as usize)];
                self.scatter(
                    sheet,
                    region.min_col,
                    region.min_row,
                    cols,
                    &uniform,
                    tainted,
                );
            }
            None => {
                self.results.borrow_mut().insert(anchor, err);
                if tainted {
                    self.pass_tainted.borrow_mut().insert(anchor);
                }
            }
        }
    }

    /// Write a row-major `cells` block (of width `cols`) into the results, coordinate `(min_col+c,
    /// min_row+r)` on `sheet` taking element `(r,c)`. Tainted cells are also recorded so `finish_pass`
    /// drops them from the memo (their value is root-relative, ENG4).
    fn scatter(
        &self,
        sheet: u32,
        min_col: u32,
        min_row: u32,
        cols: u32,
        cells: &[Value],
        tainted: bool,
    ) {
        let mut results = self.results.borrow_mut();
        for (i, v) in cells.iter().enumerate() {
            let r = (i as u32) / cols;
            let c = (i as u32) % cols;
            let key = (sheet, min_col + c, min_row + r);
            results.insert(key, v.clone());
            if tainted {
                self.pass_tainted.borrow_mut().insert(key);
            }
        }
    }

    /// The located `#SPILL!`-class dimension-mismatch refusal for a GRID5 region whose formula value
    /// does not fill the range (a scalar, or a wrong-shaped/wrong-oriented array). Reuses GRID4's
    /// [`Code::DimensionMismatch`] (a located dimension error), anchored on the sheet-qualified file.
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

/// Collapse a stored SINGLE-CELL formula result to the cell's scalar value: a scalar passes through;
/// an ARRAY keeps only its TOP-LEFT element (GRID5/ENG6 carve-out: no dynamic spill beyond a declared
/// range, so a formula in ONE cell holds the array's implicit-intersection top-left, never `#VALUE!`).
/// A 1x1 array yields its single cell; an empty array is `Blank`. A GRID5 region (>1 coordinate) never
/// routes through here — it distributes elements in [`Workbook::fill_array_region`] instead.
fn cell_scalar(v: Value) -> Value {
    match v {
        Value::Array(_, cells) => cells.into_iter().next().unwrap_or(Value::Blank),
        other => other,
    }
}
