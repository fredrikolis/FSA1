// Concern: projects the engine's dependency relation, either way, into a tree of value nodes | Non-concern: drawing that tree, computing the values | IO: (cell, Direction, depth) -> TraceNode

use std::collections::{HashMap, HashSet};

use fsa1_ast::{Value, format_cell};

use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::grid::Cell as GridCell;
use crate::render::display_value;

use super::hash::HashMemo;
use super::{CellKey, Workbook, sort_dedup};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Upstream,
    /// The SAME relation transposed, never a second parse of it.
    Downstream,
}

/// Reported per node, so a consumer need not re-inspect the value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceStatus {
    Ok,
    Literal,
    /// A gap or an empty literal cell.
    Blank,
    Cycle,
    Error,
}

impl TraceStatus {
    /// The stable spelling a consumer uses.
    pub fn as_str(self) -> &'static str {
        match self {
            TraceStatus::Ok => "ok",
            TraceStatus::Literal => "literal",
            TraceStatus::Blank => "blank",
            TraceStatus::Cycle => "cycle",
            TraceStatus::Error => "error",
        }
    }
}

/// A public VALUE projection: none of the engine's graph types appear here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceNode {
    /// Sheet-qualified: `Sheet1!C3`.
    pub cell: String,
    pub formula: Option<String>,
    /// Spelled exactly as `render` and `eval` spell it.
    pub value: String,
    pub status: TraceStatus,
    /// An opaque hex string, or `None` on a hashless terminal.
    pub hash: Option<String>,
    pub children: Vec<TraceNode>,
    /// A cell already emitted earlier in this trace is shown once with its value and hash but NOT
    /// re-descended, which is what stops a diamond blowing up. A cycle back-edge is `Cycle` instead.
    pub repeated: bool,
}

/// `children` makes [`TraceNode`] a LINKED structure, so derived drop glue would recurse once per
/// link and abort the process on a deep cone — the very crash the walk's explicit stack prevents.
/// Emptying each node into a heap worklist first keeps the native stack flat.
impl Drop for TraceNode {
    fn drop(&mut self) {
        let mut work = std::mem::take(&mut self.children);
        while let Some(mut node) = work.pop() {
            work.append(&mut node.children);
        }
    }
}

/// OWNED, so it holds no `&Workbook` borrow across the walk.
enum CellKind {
    Gap,
    Blank,
    Literal,
    /// Its verbatim source text.
    Formula(String),
    /// Its verbatim source text. It has no parseable dependencies, so the trace shows a terminal.
    LoadError(String),
}

/// A node whose own status is not yet decidable, because a descendant's back-edge may still mark it
/// a cycle member.
struct Pending {
    key: CellKey,
    cell: String,
    formula: Option<String>,
    value: String,
    /// All `classify` needs of the value.
    is_error: bool,
    kind: CellKind,
    depth: u32,
    kids: Vec<CellKey>,
    /// An index rather than a consuming iterator, so the frame stays a plain owned value.
    next: usize,
    children: Vec<TraceNode>,
}

impl Pending {
    fn next_kid(&mut self) -> Option<CellKey> {
        let k = self.kids.get(self.next).copied()?;
        self.next += 1;
        Some(k)
    }
}

enum Begun {
    Leaf(TraceNode),
    Descend(Pending),
}

struct Tracer<'w> {
    wb: &'w Workbook,
    dir: Direction,
    max_depth: Option<u32>,
    /// Built once, on the first downstream hop.
    reverse: Option<HashMap<CellKey, Vec<CellKey>>>,
    /// In ORDER, which a back-edge needs to mark every member from its target up to here.
    path: Vec<CellKey>,
    /// The same cells as a set. Membership is asked once per visited cell, so scanning `path` would be quadratic on the long chain a deep trace is; the O(depth) scan is paid only on a real back-edge.
    on_path: HashSet<CellKey>,
    /// For the visited-once (`repeated`) rule.
    visited: HashSet<CellKey>,
    cycle_members: HashSet<CellKey>,
    /// Shared across every node, so the whole trace hashes in O(cone).
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
            on_path: HashSet::new(),
            visited: HashSet::new(),
            cycle_members: HashSet::new(),
            hashes: HashMemo::new(),
        }
    }

    /// An EXPLICIT stack of half-built nodes, so BUILDING the tree is stack-safe on any cone. The
    /// tree it returns is a linked structure, so CONSUMING it must be iterative too or the overflow
    /// merely moves: this module owns [`TraceNode`]'s `Drop`, and a presenter owes the same of its
    /// own traversal. The walk has no depth bound of its own; `max_depth` is the caller's DISPLAY cap.
    fn walk(&mut self, root: CellKey) -> TraceNode {
        let mut stack: Vec<Pending> = Vec::new();
        match self.begin(root, 0) {
            Begun::Leaf(node) => return node,
            Begun::Descend(p) => stack.push(p),
        }
        loop {
            let next = stack
                .last_mut()
                .expect("the stack is non-empty here")
                .next_kid();
            match next {
                Some(k) => {
                    let depth = stack.last().expect("non-empty").depth + 1;
                    match self.begin(k, depth) {
                        Begun::Leaf(node) => {
                            stack.last_mut().expect("non-empty").children.push(node)
                        }
                        Begun::Descend(p) => stack.push(p),
                    }
                }
                None => {
                    let done = stack.pop().expect("non-empty");
                    if let Some(k) = self.path.pop() {
                        self.on_path.remove(&k);
                    }
                    let node = self.finish(done);
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(node),
                        None => return node,
                    }
                }
            }
        }
    }

    /// Terminates at a cycle back-edge, an already-emitted node, or the caller's depth cap.
    fn begin(&mut self, key: CellKey, depth: u32) -> Begun {
        // A region member is traced at its anchor: one node for the whole region.
        let key = self
            .wb
            .array_region_anchor(key.0, key.1, key.2)
            .unwrap_or(key);
        let cell = self.qualified(key);
        let val = self.wb.value_at(key.0, key.1, key.2);
        let value = display_value(&val);
        let is_error = matches!(val, Value::Error(_));
        let kind = self.cell_kind(key);
        let formula = match &kind {
            // A load-error cell shows its unparsed source, so a consumer sees the text that failed.
            CellKind::Formula(src) | CellKind::LoadError(src) => Some(src.clone()),
            _ => None,
        };

        if self.on_path.contains(&key) {
            let pos = self
                .path
                .iter()
                .position(|&k| k == key)
                .expect("`on_path` holds exactly the keys in `path`");
            for &m in &self.path[pos..] {
                self.cycle_members.insert(m);
            }
            return Begun::Leaf(TraceNode {
                cell,
                formula,
                value,
                status: TraceStatus::Cycle,
                hash: None,
                children: Vec::new(),
                repeated: false,
            });
        }

        if self.visited.contains(&key) {
            let hash = self.hashes_of(key);
            return Begun::Leaf(TraceNode {
                cell,
                formula,
                value,
                status: self.classify(key, &kind, is_error),
                hash,
                children: Vec::new(),
                repeated: true,
            });
        }

        self.visited.insert(key);
        self.path.push(key);
        self.on_path.insert(key);
        let kids = if self.max_depth.is_some_and(|m| depth >= m) {
            Vec::new()
        } else {
            self.neighbors(key, &kind)
        };
        Begun::Descend(Pending {
            key,
            cell,
            formula,
            value,
            is_error,
            kind,
            depth,
            kids,
            next: 0,
            children: Vec::new(),
        })
    }

    /// Classified only now, the sub-tree having by this point recorded any cycle membership.
    fn finish(&mut self, p: Pending) -> TraceNode {
        let status = self.classify(p.key, &p.kind, p.is_error);
        let hash = self.hashes_of(p.key);
        TraceNode {
            cell: p.cell,
            formula: p.formula,
            value: p.value,
            status,
            hash,
            children: p.children,
            repeated: false,
        }
    }

    /// Sorted and deduped, for a deterministic tree.
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

    /// A non-formula cell has no upstream dependencies.
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
        // The EFFECTIVE expr, so a traced forger shows the RESOLVED references the engine depends on. The forge pass has already run, `walk` having called `value_at` first.
        sort_dedup(self.wb.expr_deps(self.wb.effective_expr(key, expr), sheet))
    }

    fn classify(&self, key: CellKey, kind: &CellKind, is_error: bool) -> TraceStatus {
        match kind {
            CellKind::Gap | CellKind::Blank => TraceStatus::Blank,
            CellKind::Literal => TraceStatus::Literal,
            // A terminal: it resolved to an error value and has no dependencies to descend.
            CellKind::LoadError(_) => TraceStatus::Error,
            CellKind::Formula(_) => {
                if self.cycle_members.contains(&key) {
                    TraceStatus::Cycle
                } else if is_error {
                    TraceStatus::Error
                } else {
                    TraceStatus::Ok
                }
            }
        }
    }

    /// Each node's OWN rooted content identity, never one relative to the traced root.
    fn hashes_of(&mut self, key: CellKey) -> Option<String> {
        self.wb.computation_hash_with(key, &mut self.hashes)
    }

    fn qualified(&self, key: CellKey) -> String {
        format!("{}!{}", self.wb.tab_name(key.0), format_cell(key.1, key.2))
    }

    fn cell_kind(&self, key: CellKey) -> CellKind {
        let (sheet, col, row) = key;
        let Some(cell) = self.wb.grid_cell_at(sheet, col, row) else {
            return CellKind::Gap;
        };
        match cell {
            GridCell::Value {
                value: Value::Blank,
                ..
            } => CellKind::Blank,
            GridCell::Value { .. } => CellKind::Literal,
            GridCell::Formula { src, .. } => CellKind::Formula(src.clone()),
            GridCell::LoadError { src, .. } => CellKind::LoadError(src.clone()),
        }
    }
}

impl Workbook {
    /// Visited-once and cycle-safe. A node's `hash` is `None` on either hashless terminal: a cell on
    /// a cycle, or one whose formula — or anything upstream of it — references an over-bound range.
    /// `max_depth` caps the DISPLAYED tree; `None` is the whole cone, and the walk has no bound.
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
        Ok(tracer.walk((sheet, col, row)))
    }

    /// The engine's own forward dependency map, inverted in ONE pass — never a second parser. An
    /// array region contributes one source cell, its anchor, and is keyed by that anchor.
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
