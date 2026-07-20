// Concern: reference FORGING (ENG6) as a GATED Expr SOURCE-REWRITE (restricted v1) — Pass 0 of a demand, run only when `has_forgers`: for each `INDIRECT`/`OFFSET` `Call` in a demanded cell's cone, EVALUATE its argument cone through the normal engine (reusing the memo), COMPUTE the concrete target (INDIRECT: parse the text arg as an A1 ref/range; OFFSET: base ref + row/col/height/width arithmetic), and REWRITE the forger subtree in place with a static `Expr::Ref`/`Expr::Range` node stored ADDRESS-STABLE under `&self` in the append-only [`ForgeStore`] (the resolver `Arena` idiom); a fixpoint resolves forgers in dependency order (the forger set is RE-COLLECTED against the EFFECTIVE cone after every progressing round, so a forger reachable only through another forger's just-rewritten range is discovered and resolved rather than silently left as an un-rewritten backstop `#REF!`; a forger whose arg cone still holds an unresolved forger waits; a forger-arg cycle -> the no-progress branch -> located `#REF!`), and a forger whose own arguments forge (nested forging, out of restricted v1) or whose target is off-grid / not a valid reference is a located `#REF!` (Code::ForgeRefusal); the shared detection predicates [`is_forger`]/[`expr_has_forger`] drive the `has_forgers` load gate and the `cache::cone_volatile` exclusion | Non-concern: the Value model + charlie-ast eval (UNTOUCHED — forging never eval-returns a reference; the forger's static rewrite is what the two-pass engine evaluates), building the dep graph / computing values (the `plan`/`evaluate` siblings own the passes — this only rewrites the source they consume via `effective_expr`), the computation hash + cache keying (the `hash`/`cache` siblings read the ORIGINAL grid expr, ENG3 split), and the `ForgeStore`'s escape from this module (it is `pub(super)`, re-exported by no one, ENG3 containment) | IO: (a set of demanded `CellKey`s + the `Workbook`'s grids, via the normal engine) -> forger rewrites recorded in the `ForgeStore` + the located `ForgeRefusal` `Diagnostic`s pushed during resolution
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use charlie_ast::{ErrKind, Expr, FuncId, RangeNode, RefNode, Value, func, parse};

use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::grid::Cell as GridCell;

use super::{CellKey, MAX_PULL_DEPTH, Workbook};

/// Whether a [`FuncId`] names a reference-forging function (`INDIRECT`/`OFFSET`, ENG6). Reads the name
/// from the registry rather than hard-coding a `FuncId` value, so the two rows' registry position is
/// not a magic number here. This is the SINGLE forger predicate — the `has_forgers` load gate, the
/// forge pass, and the `cache::cone_volatile` exclusion all consult it (never a second name list).
pub(super) fn is_forger(id: FuncId) -> bool {
    func::def(id).is_some_and(|d| d.name == "INDIRECT" || d.name == "OFFSET")
}

/// Whether an expression tree contains a forging `Call` anywhere within it — the per-cell half the
/// `has_forgers` load gate folds over every parsed formula (mirroring `has_array_regions`), and the
/// in-cell NESTED-FORGING check (`INDIRECT(INDIRECT(...))`) reuses it over a forger's arguments.
pub(super) fn expr_has_forger(expr: &Expr) -> bool {
    match expr {
        Expr::Call(id, args) => is_forger(*id) || args.iter().any(expr_has_forger),
        Expr::Unary(_, e) | Expr::ImplicitIntersect(e) | Expr::SpillRef(e) => expr_has_forger(e),
        Expr::Binary(_, a, b) => expr_has_forger(a) || expr_has_forger(b),
        Expr::Lit(_) | Expr::Ref(_) | Expr::Range(_) => false,
    }
}

/// An append-only store of a cell's forge-REWRITTEN `Expr`, keyed by [`CellKey`], handed out as a
/// borrowed `&Expr` for the [`Workbook::effective_expr`] seam. It resolves the same lifetime problem the
/// resolver's `Arena` does: the seam must return `&Expr` borrowing the store, but the store is filled
/// lazily under `&self` (the forge pass). A rewritten expr is boxed and **never moved or freed** while
/// `&self` lives (entries are only appended, never removed or mutated), so a reference into a boxed
/// `Expr` stays valid for the whole `&self` borrow. Empty (and never touched) when the workbook has no
/// forgers — the zero-overhead gate is `Workbook::has_forgers`, not this store.
#[derive(Default, Debug)]
pub(super) struct ForgeStore {
    /// The owned rewritten exprs. The `Box` is LOAD-BEARING (not the usual redundant `Vec<Box<_>>`): the
    /// boxed `Expr`'s heap data is address-stable across `Vec` growth, which is exactly what lets `get`
    /// hand out a `&Expr` that outlives the transient `bufs` borrow (the resolver `Arena` idiom). A bare
    /// `Vec<Expr>` would MOVE its elements on realloc and dangle the returned reference.
    #[allow(clippy::vec_box)]
    exprs: RefCell<Vec<Box<Expr>>>,
    /// Cell -> index into `exprs`.
    index: RefCell<HashMap<CellKey, usize>>,
}

impl ForgeStore {
    /// The forge rewrite for `key`, or `None` if the cell has none (not a forger, or not yet resolved).
    pub(super) fn get(&self, key: CellKey) -> Option<&Expr> {
        let i = {
            let index = self.index.borrow();
            *index.get(&key)?
        };
        let ptr: *const Expr = &*self.exprs.borrow()[i];
        // SAFETY: the store is append-only — `exprs` entries are boxed `Expr`s that are never moved
        // (the box's heap data is independent of the `Vec`'s reallocations) and never freed or mutated
        // while `&self` lives. So the pointee outlives this `&self` borrow, and no `&mut` to the same
        // data is ever created. The returned reference's lifetime is tied to `&self`. Mirrors
        // `resolver::Arena::get`.
        let e: &Expr = unsafe { &*ptr };
        Some(e)
    }

    /// Whether `key` already has a forge rewrite (an earlier demand resolved it; the store persists
    /// across demands like the memo, ENG4).
    fn contains(&self, key: CellKey) -> bool {
        self.index.borrow().contains_key(&key)
    }

    /// Record `expr` as `key`'s forge rewrite (append-only).
    fn insert(&self, key: CellKey, expr: Expr) {
        let mut exprs = self.exprs.borrow_mut();
        let i = exprs.len();
        exprs.push(Box::new(expr));
        self.index.borrow_mut().insert(key, i);
    }

    /// The number of recorded rewrites — a test-visible instrument proving the zero-overhead gate (a
    /// non-forging workbook never records one).
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.index.borrow().len()
    }
}

impl Workbook {
    // ------------------------------------------------------------------------------------------
    // Pass 0 — resolve every forger in a demanded cone to a static reference (gated on has_forgers).
    // ------------------------------------------------------------------------------------------

    /// Rewrite every reference-forging call in `roots`' dependency cone to a static reference, BEFORE
    /// the two-pass plan/evaluate runs on the effective (rewritten) form. Called once at the top of
    /// [`Workbook::demand`] (and [`Workbook::eval_root_expr`]) under the `has_forgers` gate. Idempotent:
    /// a forger already in the [`ForgeStore`] from an earlier demand is skipped (the rewrite is stable
    /// for the immutable workbook's lifetime). The fixpoint resolves forgers in dependency order — a
    /// forger whose argument cone still holds an unresolved forger waits a round; when a round makes no
    /// progress the remaining forgers are mutually blocked (a forger-arg cycle) and become located
    /// `#REF!`. After every progressing round the forger set is RE-COLLECTED against the effective cone,
    /// so a forger reachable only through another forger's just-rewritten range is discovered and
    /// resolved (rather than evaluating its un-rewritten `Call` to a silent, memoized backstop `#REF!`).
    /// Terminating: the set grows monotonically and is bounded by the (finite) cone, and rewrites are
    /// monotone — each progressing round resolves at least one forger.
    pub(super) fn resolve_forgers(&self, roots: &[CellKey]) {
        let mut forgers: Vec<CellKey> = Vec::new();
        let mut in_set: HashSet<CellKey> = HashSet::new();
        // Collect the forger set from the demanded cone. The first pass (nothing resolved yet) walks the
        // grid deps exactly as before; each progressing round below RE-COLLECTS against the now-richer
        // effective cone, so a forger reachable only through a just-rewritten forged range enters the set.
        self.collect_forgers_into(roots, &mut forgers, &mut in_set);
        if forgers.is_empty() {
            return;
        }
        let mut resolved: HashSet<CellKey> = HashSet::new();
        for &f in &forgers {
            if self.forge.contains(f) {
                resolved.insert(f); // already rewritten by an earlier demand (ENG4 persistence)
            }
        }
        while resolved.len() < forgers.len() {
            let mut progressed = false;
            // Iterate the CURRENT forger set; a re-collection only appends AFTER the round completes, so
            // this borrow is stable for the round (a newly discovered forger is processed next round).
            for &x in &forgers {
                if resolved.contains(&x) || !self.forger_ready(x, &resolved) {
                    continue;
                }
                self.resolve_one_forger(x);
                resolved.insert(x);
                progressed = true;
            }
            if progressed {
                // A just-resolved forger's rewritten range may now reach a forger the ORIGINAL grid cone
                // never did (a forger INSIDE a forged range). Re-collect against the richer effective
                // cone so it enters the set and resolves next round — the "no false-reject" completeness
                // fix that keeps such a chained forger from evaluating its un-rewritten `Call` to a silent
                // (and memoized) backstop `#REF!`. Terminating: the set grows monotonically and is bounded
                // by the (finite) demanded cone, and each progressing round resolves at least one forger.
                self.collect_forgers_into(roots, &mut forgers, &mut in_set);
                continue;
            }
            // No forger became ready: the rest are mutually blocked (a forger-arg cycle). Each is a
            // located `#REF!` — its forger subtrees become `#REF!` values.
            for &x in &forgers {
                if resolved.insert(x) {
                    self.refuse(self.forge_diag(
                        x,
                        "a reference-forging call depends (through its arguments) on its own \
                         output -- a forger-arg cycle refused as #REF!",
                    ));
                    let rewritten = replace_forgers_with_ref(
                        &self
                            .cloned_grid_formula(x)
                            .unwrap_or(Expr::Lit(Value::Blank)),
                    );
                    self.forge.insert(x, rewritten);
                }
            }
            break;
        }
    }

    /// Append every forger reachable from `roots` through the EFFECTIVE dependency cone into `out`,
    /// de-duplicated by `in_set` (which PERSISTS across re-collection rounds so a forger is never listed
    /// twice). A FRESH per-call `seen` guards the traversal, so a round re-walks a cone whose effective
    /// deps grew after a rewrite and finds the forgers that range now reaches.
    fn collect_forgers_into(
        &self,
        roots: &[CellKey],
        out: &mut Vec<CellKey>,
        in_set: &mut HashSet<CellKey>,
    ) {
        let mut seen = HashSet::new();
        for &r in roots {
            self.collect_forgers(r, 0, &mut seen, out, in_set);
        }
    }

    /// Walk `key`'s dependency cone following the EFFECTIVE expr (a resolved forger contributes its
    /// rewrite's static deps; an unresolved cell its grid deps — on the first pass nothing is rewritten,
    /// so this is the grid cone) collecting every cell that contains a forging call. Bounded by
    /// [`MAX_PULL_DEPTH`] (native recursion, like `plan_visit`) and visited-once (`seen`), so it
    /// terminates on any cone including a cyclic one.
    fn collect_forgers(
        &self,
        key: CellKey,
        depth: u32,
        seen: &mut HashSet<CellKey>,
        out: &mut Vec<CellKey>,
        in_set: &mut HashSet<CellKey>,
    ) {
        let key = self.array_region_anchor(key.0, key.1, key.2).unwrap_or(key);
        if depth >= MAX_PULL_DEPTH || !seen.insert(key) {
            return;
        }
        let Some(expr) = self.cloned_grid_formula(key) else {
            return; // a literal / gap holds no forger and has no dependencies
        };
        if expr_has_forger(&expr) && in_set.insert(key) {
            out.push(key);
        }
        let eff = self.effective_expr(key, &expr);
        for d in self.expr_deps(eff, key.0) {
            self.collect_forgers(d, depth + 1, seen, out, in_set);
        }
    }

    /// Whether forger cell `x` can be resolved this round: its dependency cone (following the EFFECTIVE
    /// expr of already-resolved cells, and the grid expr of unresolved ones) contains no unresolved
    /// forger. A forger reachable from `x` that is not yet `resolved` — including `x` itself via a cycle
    /// back to its own cell — blocks `x` (it must resolve first, or, if it never does, both fall to the
    /// no-progress `#REF!` branch). Iterative (a stack, not native recursion), so it is stack-safe.
    fn forger_ready(&self, x: CellKey, resolved: &HashSet<CellKey>) -> bool {
        let Some(expr) = self.cloned_grid_formula(x) else {
            return true;
        };
        let mut stack: Vec<CellKey> = self.expr_deps(&expr, x.0);
        let mut seen = HashSet::new();
        while let Some(k) = stack.pop() {
            let k = self.array_region_anchor(k.0, k.1, k.2).unwrap_or(k);
            if !seen.insert(k) {
                continue;
            }
            let Some(kexpr) = self.cloned_grid_formula(k) else {
                continue;
            };
            if expr_has_forger(&kexpr) && !resolved.contains(&k) {
                return false; // an unresolved forger in the cone (or `x` itself via a cycle): not ready
            }
            // A resolved forger contributes its rewrite's (static) deps; every other cell its grid deps.
            let eff = self.effective_expr(k, &kexpr);
            for d in self.expr_deps(eff, k.0) {
                stack.push(d);
            }
        }
        true
    }

    /// Resolve a single ready forger cell: rewrite every forger subtree of its grid expr to a static
    /// reference (or a located `#REF!` for a nested / off-grid / invalid forger) and record the whole
    /// rewritten expr in the [`ForgeStore`].
    fn resolve_one_forger(&self, key: CellKey) {
        let Some(expr) = self.cloned_grid_formula(key) else {
            return;
        };
        let rewritten = self.rewrite_forgers(&expr, key.0, key);
        self.forge.insert(key, rewritten);
    }

    /// Rewrite `expr`, replacing each forging `Call` subtree with its resolved static node. A forger
    /// whose own arguments forge (nested forging) is out of restricted v1 -> a located `#REF!`. Every
    /// non-forger node is rewritten structurally (its children recursed) so a forger nested inside a
    /// reducer (`SUM(OFFSET(...))`) rewrites to `SUM($A$1:$A$3)` the existing SUM handles unchanged.
    fn rewrite_forgers(&self, expr: &Expr, home: u32, key: CellKey) -> Expr {
        match expr {
            Expr::Call(id, args) if is_forger(*id) => {
                if args.iter().any(expr_has_forger) {
                    self.refuse(self.forge_diag(
                        key,
                        "a reference-forging call whose own arguments forge (nested forging) is out \
                         of restricted v1 -- refused as #REF!",
                    ));
                    return Expr::Lit(Value::Error(ErrKind::Ref));
                }
                self.resolve_forger_call(*id, args, home, key)
            }
            Expr::Call(id, args) => Expr::Call(
                *id,
                args.iter()
                    .map(|a| self.rewrite_forgers(a, home, key))
                    .collect(),
            ),
            Expr::Binary(op, a, b) => Expr::Binary(
                *op,
                Box::new(self.rewrite_forgers(a, home, key)),
                Box::new(self.rewrite_forgers(b, home, key)),
            ),
            Expr::Unary(op, e) => Expr::Unary(*op, Box::new(self.rewrite_forgers(e, home, key))),
            Expr::ImplicitIntersect(e) => {
                Expr::ImplicitIntersect(Box::new(self.rewrite_forgers(e, home, key)))
            }
            Expr::SpillRef(e) => Expr::SpillRef(Box::new(self.rewrite_forgers(e, home, key))),
            other => other.clone(),
        }
    }

    /// Compute one forger's concrete target as a static `Expr::Ref`/`Expr::Range` (or a located `#REF!`
    /// error literal). Dispatched on the registry name — the two forgers have distinct target math.
    fn resolve_forger_call(&self, id: FuncId, args: &[Expr], home: u32, key: CellKey) -> Expr {
        match func::def(id).map(|d| d.name) {
            Some("INDIRECT") => self.forge_indirect(args, home, key),
            Some("OFFSET") => self.forge_offset(args, home, key),
            // Defensive: `is_forger` gated us here, so this is unreachable — a `#REF!` keeps it total.
            _ => Expr::Lit(Value::Error(ErrKind::Ref)),
        }
    }

    /// `INDIRECT(ref_text, [a1])` — evaluate the text argument and parse it as an A1 reference or range.
    /// Restricted v1 supports A1 style only (`a1` defaulting to / equal to TRUE); an explicit `a1=FALSE`
    /// (R1C1) is a located `#REF!`. A sheet-qualified target (`Sheet1!B2`) is parsed by the formula
    /// parser, so a cross-sheet forge resolves through the normal `Resolver`. Invalid text, or text that
    /// parses to anything other than a lone reference/range, is a located `#REF!`.
    fn forge_indirect(&self, args: &[Expr], home: u32, key: CellKey) -> Expr {
        if let Some(a1_arg) = args.get(1) {
            // Excel coerces the `a1` flag to a LOGICAL: FALSE — and equally a numeric 0 or the text
            // "FALSE" — selects R1C1 style (refused in restricted v1). Coerce BEFORE the check so
            // `INDIRECT(text, 0)` is treated as R1C1, not mistaken for A1.
            match coerce_logical(self.eval_root_expr(a1_arg, home)) {
                Ok(false) => {
                    self.refuse(self.forge_diag(
                        key,
                        "INDIRECT with a1=FALSE (R1C1 style) is not supported in restricted v1 -- \
                         refused as #REF!",
                    ));
                    return Expr::Lit(Value::Error(ErrKind::Ref));
                }
                Ok(true) => {}                               // A1 style
                Err(k) => return Expr::Lit(Value::Error(k)), // error / non-logical text
            }
        }
        let text = match self.eval_root_expr(&args[0], home) {
            Value::Text(s) => s,
            Value::Error(k) => return Expr::Lit(Value::Error(k)),
            _ => {
                self.refuse(self.forge_diag(
                    key,
                    "INDIRECT's reference text did not evaluate to text -- refused as #REF!",
                ));
                return Expr::Lit(Value::Error(ErrKind::Ref));
            }
        };
        // Reuse the formula parser for the A1 grammar (`A1`, `$A$1`, `A1:B2`, `Sheet1!B2`), then accept
        // ONLY a lone reference/range — any other parse (an expression, a name) is not a valid target.
        match parse(&format!("={text}")) {
            Ok(e @ (Expr::Ref(_) | Expr::Range(_))) => e,
            _ => {
                self.refuse(self.forge_diag(
                    key,
                    &format!(
                        "INDIRECT text {text:?} is not a valid A1 reference -- refused as #REF!"
                    ),
                ));
                Expr::Lit(Value::Error(ErrKind::Ref))
            }
        }
    }

    /// `OFFSET(reference, rows, cols, [height], [width])` — a static base reference shifted by evaluated
    /// row/col offsets and resized to an evaluated height/width (defaulting to the base's own extent). A
    /// non-static base, an off-grid target (negative or beyond the addressable grid), or a non-positive
    /// size is a located `#REF!`; a non-numeric offset propagates its error/`#VALUE!`. An over-large but
    /// on-grid AREA is left for `Resolver::range` to refuse as `#NUM!` at eval (the shared
    /// `MAX_RANGE_CELLS` bound) — the rewrite still produces the `Expr::Range` node.
    fn forge_offset(&self, args: &[Expr], home: u32, key: CellKey) -> Expr {
        let (top, left, base_h, base_w, sheet) = match &args[0] {
            Expr::Ref(r) => (r.row, r.col, 1u32, 1u32, r.sheet.clone()),
            Expr::Range(rn) => (
                rn.start_row.min(rn.end_row),
                rn.start_col.min(rn.end_col),
                rn.start_row.abs_diff(rn.end_row) + 1,
                rn.start_col.abs_diff(rn.end_col) + 1,
                rn.sheet.clone(),
            ),
            _ => {
                self.refuse(self.forge_diag(
                    key,
                    "OFFSET's base argument is not a static reference -- refused as #REF!",
                ));
                return Expr::Lit(Value::Error(ErrKind::Ref));
            }
        };
        let rows = match coerce_offset_int(self.eval_root_expr(&args[1], home)) {
            Ok(n) => n,
            Err(k) => return Expr::Lit(Value::Error(k)),
        };
        let cols = match coerce_offset_int(self.eval_root_expr(&args[2], home)) {
            Ok(n) => n,
            Err(k) => return Expr::Lit(Value::Error(k)),
        };
        let height = match args.get(3) {
            Some(a) => match coerce_offset_int(self.eval_root_expr(a, home)) {
                Ok(n) => n,
                Err(k) => return Expr::Lit(Value::Error(k)),
            },
            None => i64::from(base_h),
        };
        let width = match args.get(4) {
            Some(a) => match coerce_offset_int(self.eval_root_expr(a, home)) {
                Ok(n) => n,
                Err(k) => return Expr::Lit(Value::Error(k)),
            },
            None => i64::from(base_w),
        };
        let new_top = i64::from(top) + rows;
        let new_left = i64::from(left) + cols;
        if new_top < 0 || new_left < 0 || height <= 0 || width <= 0 {
            self.refuse(self.forge_diag(
                key,
                "OFFSET target is off-grid or has non-positive size -- refused as #REF!",
            ));
            return Expr::Lit(Value::Error(ErrKind::Ref));
        }
        let (Ok(t), Ok(l), Ok(b), Ok(r)) = (
            u32::try_from(new_top),
            u32::try_from(new_left),
            u32::try_from(new_top + height - 1),
            u32::try_from(new_left + width - 1),
        ) else {
            self.refuse(self.forge_diag(
                key,
                "OFFSET target coordinate is beyond the addressable grid -- refused as #REF!",
            ));
            return Expr::Lit(Value::Error(ErrKind::Ref));
        };
        if height == 1 && width == 1 {
            Expr::Ref(RefNode {
                col: l,
                row: t,
                col_abs: false,
                row_abs: false,
                sheet,
            })
        } else {
            Expr::Range(RangeNode {
                start_col: l,
                start_row: t,
                end_col: r,
                end_row: b,
                start_col_abs: false,
                start_row_abs: false,
                end_col_abs: false,
                end_row_abs: false,
                sheet,
            })
        }
    }

    /// The grid formula expr at `key`, CLONED (so no `&self.tabs` borrow is held while the forge pass
    /// evaluates arguments through the engine, which borrows the results/memo). `None` for a literal,
    /// a gap, or a GRID6 load-error cell — none of which forge.
    fn cloned_grid_formula(&self, key: CellKey) -> Option<Expr> {
        match self.grid_cell_at(key.0, key.1, key.2)? {
            GridCell::Formula { expr, .. } => Some(expr.clone()),
            _ => None,
        }
    }

    /// The located `ForgeRefusal` diagnostic for a forger cell, anchored on its sheet-qualified file.
    fn forge_diag(&self, key: CellKey, msg: &str) -> Diagnostic {
        let (id, _) = self
            .covering(key.0, key.1, key.2)
            .expect("a forger cell is a formula cell and is therefore covered");
        Diagnostic::new(
            Code::ForgeRefusal,
            Loc::tab_file(&self.tab_name(id.0), &self.file_name(id)),
            msg.to_string(),
        )
    }
}

/// Coerce an evaluated `INDIRECT` `a1` flag to a LOGICAL (Excel): a number is FALSE iff zero, a boolean
/// passes through, a blank (an omitted-but-present slot) is FALSE, an error propagates, and text is the
/// case-insensitive `"TRUE"`/`"FALSE"` literal else `#VALUE!`. An array collapses to its top-left element
/// first (implicit intersection), mirroring [`coerce_offset_int`].
fn coerce_logical(v: Value) -> Result<bool, ErrKind> {
    match v {
        Value::Bool(b) => Ok(b),
        Value::Number(n) => Ok(n != 0.0),
        Value::Blank => Ok(false),
        Value::Error(k) => Err(k),
        Value::Text(s) => match s.trim().to_ascii_uppercase().as_str() {
            "TRUE" => Ok(true),
            "FALSE" => Ok(false),
            _ => Err(ErrKind::Value),
        },
        Value::Array(_, cells) => coerce_logical(cells.into_iter().next().unwrap_or(Value::Blank)),
    }
}

/// Coerce an evaluated OFFSET offset/size argument to an integer: a number truncates toward zero, a
/// boolean is 0/1, a blank (an omitted-but-present slot) is 0, an error propagates, and text is
/// `#VALUE!` (Excel). An array collapses to its top-left element first (implicit intersection).
fn coerce_offset_int(v: Value) -> Result<i64, ErrKind> {
    match v {
        Value::Number(n) => Ok(n.trunc() as i64),
        Value::Bool(b) => Ok(i64::from(b)),
        Value::Blank => Ok(0),
        Value::Error(k) => Err(k),
        Value::Text(_) => Err(ErrKind::Value),
        Value::Array(_, cells) => {
            coerce_offset_int(cells.into_iter().next().unwrap_or(Value::Blank))
        }
    }
}

/// Replace EVERY forging `Call` subtree with a `#REF!` error literal (the forger-arg-cycle refusal),
/// recursing structurally through the rest so a forger nested in a reducer still becomes a `#REF!` the
/// reducer propagates. Used only for the no-progress (cycle) branch, where the target cannot be
/// computed at all.
fn replace_forgers_with_ref(expr: &Expr) -> Expr {
    match expr {
        Expr::Call(id, _) if is_forger(*id) => Expr::Lit(Value::Error(ErrKind::Ref)),
        Expr::Call(id, args) => {
            Expr::Call(*id, args.iter().map(replace_forgers_with_ref).collect())
        }
        Expr::Binary(op, a, b) => Expr::Binary(
            *op,
            Box::new(replace_forgers_with_ref(a)),
            Box::new(replace_forgers_with_ref(b)),
        ),
        Expr::Unary(op, e) => Expr::Unary(*op, Box::new(replace_forgers_with_ref(e))),
        Expr::ImplicitIntersect(e) => {
            Expr::ImplicitIntersect(Box::new(replace_forgers_with_ref(e)))
        }
        Expr::SpillRef(e) => Expr::SpillRef(Box::new(replace_forgers_with_ref(e))),
        other => other.clone(),
    }
}
