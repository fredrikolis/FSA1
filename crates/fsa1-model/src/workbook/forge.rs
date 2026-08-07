// Concern: rewrites every INDIRECT/OFFSET in a demanded cone to a static reference before planning | Non-concern: planning, evaluating the rewritten form | IO: (&[CellKey]) -> ForgeStore entries

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use fsa1_ast::{ErrKind, Expr, FuncId, RangeNode, RefNode, Value, func, parse};

use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::grid::Cell as GridCell;

use super::{CellKey, Workbook};

/// The SINGLE forger predicate, consulted by both the load gate and the forge pass. Reading the name
/// from the registry keeps the two rows' registry position from becoming a magic number here.
pub(super) fn is_forger(id: FuncId) -> bool {
    func::def(id).is_some_and(|d| d.name == "INDIRECT" || d.name == "OFFSET")
}

pub(super) fn expr_has_forger(expr: &Expr) -> bool {
    match expr {
        Expr::Call(id, args) => is_forger(*id) || args.iter().any(expr_has_forger),
        Expr::Unary(_, e) | Expr::ImplicitIntersect(e) | Expr::SpillRef(e) => expr_has_forger(e),
        Expr::Binary(_, a, b) => expr_has_forger(a) || expr_has_forger(b),
        Expr::Lit(_) | Expr::Ref(_) | Expr::Range(_) => false,
    }
}

/// Append-only for the same reason `resolver::Arena` is: [`Workbook::effective_expr`] must return an
/// `&Expr` borrowing this store, which the forge pass fills lazily under `&self`. The zero-overhead
/// gate is `Workbook::has_forgers`, not this store, which a non-forging workbook never touches.
#[derive(Default, Debug)]
pub(super) struct ForgeStore {
    /// The `Box` is LOAD-BEARING, not the usual redundant `Vec<Box<_>>`: a bare `Vec<Expr>` would
    /// MOVE its elements on realloc and dangle the reference `get` returns.
    #[allow(clippy::vec_box)]
    exprs: RefCell<Vec<Box<Expr>>>,
    index: RefCell<HashMap<CellKey, usize>>,
}

impl ForgeStore {
    /// `None` when the cell is not a forger, or is not yet resolved.
    pub(super) fn get(&self, key: CellKey) -> Option<&Expr> {
        let i = {
            let index = self.index.borrow();
            *index.get(&key)?
        };
        let ptr: *const Expr = &*self.exprs.borrow()[i];
        // SAFETY: `exprs` entries are boxed `Expr`s whose heap data is independent of the `Vec`'s reallocations and are never freed or mutated while `&self` lives, so the pointee outlives this borrow and no `&mut` to it is ever created.
        let e: &Expr = unsafe { &*ptr };
        Some(e)
    }

    /// The store persists across demands, like the memo.
    fn contains(&self, key: CellKey) -> bool {
        self.index.borrow().contains_key(&key)
    }

    fn insert(&self, key: CellKey, expr: Expr) {
        let mut exprs = self.exprs.borrow_mut();
        let i = exprs.len();
        exprs.push(Box::new(expr));
        self.index.borrow_mut().insert(key, i);
    }

    /// A non-forging workbook must never record one.
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.index.borrow().len()
    }
}

/// An EMPTY argument slot, which the parser spells `Expr::Lit(Value::Blank)` — `OFFSET(A1,0,0,,3)`.
/// The grammar has no blank literal, so a blank arriving in an argument position can only be the
/// parser's empty slot; a blank that arrives through a REFERENCE is a value and still coerces to 0.
fn omitted(a: &Expr) -> bool {
    matches!(a, Expr::Lit(Value::Blank))
}

impl Workbook {
    /// A fixpoint in dependency order: a forger whose argument cone still holds an unresolved forger
    /// waits a round, and when a round makes no progress the remainder are mutually blocked and become
    /// located `#REF!`. Idempotent, since a rewrite is stable for the immutable workbook's lifetime.
    /// Terminating, since the forger set is bounded by the finite cone and each round resolves one.
    pub(super) fn resolve_forgers(&self, roots: &[CellKey]) {
        let mut forgers: Vec<CellKey> = Vec::new();
        let mut in_set: HashSet<CellKey> = HashSet::new();
        self.collect_forgers_into(roots, &mut forgers, &mut in_set);
        if forgers.is_empty() {
            return;
        }
        let mut resolved: HashSet<CellKey> = HashSet::new();
        for &f in &forgers {
            if self.forge.contains(f) {
                resolved.insert(f); // already rewritten by an earlier demand
            }
        }
        while resolved.len() < forgers.len() {
            let mut progressed = false;
            // Re-collection only appends AFTER the round, so a newly found forger waits for the next one.
            for &x in &forgers {
                if resolved.contains(&x) || !self.forger_ready(x, &resolved) {
                    continue;
                }
                self.resolve_one_forger(x);
                resolved.insert(x);
                progressed = true;
            }
            if progressed {
                // A just-rewritten range may reach a forger the ORIGINAL grid cone never did, and without re-collecting it would evaluate its un-rewritten `Call` to a silent, memoized `#REF!`.
                self.collect_forgers_into(roots, &mut forgers, &mut in_set);
                continue;
            }
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

    /// `in_set` PERSISTS across rounds, so a forger is never listed twice, while a FRESH per-call
    /// `seen` lets a round re-walk a cone whose effective deps grew after a rewrite. Iterative and
    /// visited-once, so it terminates on any cone, cyclic or arbitrarily deep.
    fn collect_forgers_into(
        &self,
        roots: &[CellKey],
        out: &mut Vec<CellKey>,
        in_set: &mut HashSet<CellKey>,
    ) {
        let mut seen = HashSet::new();
        let mut stack: Vec<CellKey> = roots.iter().rev().copied().collect();
        while let Some(key) = stack.pop() {
            let key = self.array_region_anchor(key.0, key.1, key.2).unwrap_or(key);
            if !seen.insert(key) {
                continue;
            }
            let Some(expr) = self.cloned_grid_formula(key) else {
                continue; // a literal or gap holds no forger and no dependencies
            };
            if expr_has_forger(&expr) && in_set.insert(key) {
                out.push(key);
            }
            let eff = self.effective_expr(key, &expr);
            for d in self.expr_deps(eff, key.0).into_iter().rev() {
                stack.push(d);
            }
        }
    }

    /// Ready iff no unresolved forger is reachable from `x` — including `x` itself, through a cycle
    /// back to its own cell, in which case both fall to the no-progress `#REF!` branch.
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
                return false;
            }
            // A resolved forger contributes its rewrite's static deps; every other cell its grid deps.
            let eff = self.effective_expr(k, &kexpr);
            for d in self.expr_deps(eff, k.0) {
                stack.push(d);
            }
        }
        true
    }

    fn resolve_one_forger(&self, key: CellKey) {
        let Some(expr) = self.cloned_grid_formula(key) else {
            return;
        };
        let rewritten = self.rewrite_forgers(&expr, key.0, key);
        self.forge.insert(key, rewritten);
    }

    /// Every non-forger node is rewritten structurally, so a forger nested inside a reducer becomes
    /// `SUM($A$1:$A$3)`, which the existing SUM handles unchanged.
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

    /// Dispatched on the registry name: the two forgers have distinct target math. The fallthrough
    /// is unreachable, [`is_forger`] having gated us here, but a `#REF!` keeps it total.
    fn resolve_forger_call(&self, id: FuncId, args: &[Expr], home: u32, key: CellKey) -> Expr {
        match func::def(id).map(|d| d.name) {
            Some("INDIRECT") => self.forge_indirect(args, home, key),
            Some("OFFSET") => self.forge_offset(args, home, key),
            _ => Expr::Lit(Value::Error(ErrKind::Ref)),
        }
    }

    /// `INDIRECT(ref_text, [a1])`. Restricted v1 is A1 style only, so an explicit `a1=FALSE` (R1C1)
    /// is a located `#REF!`.
    fn forge_indirect(&self, args: &[Expr], home: u32, key: CellKey) -> Expr {
        if let Some(a1_arg) = args.get(1).filter(|a| !omitted(a)) {
            // Coerced BEFORE the check, so `INDIRECT(text, 0)` is read as R1C1 rather than as A1.
            match coerce_logical(self.eval_root_expr(a1_arg, home, Some((key.2, key.1)))) {
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
        let text = match self.eval_root_expr(&args[0], home, Some((key.2, key.1))) {
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
        // The formula parser owns the A1 grammar; only a LONE reference or range is a valid target.
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

    /// `OFFSET(reference, rows, cols, [height], [width])`, height and width defaulting to the base's
    /// own extent. An over-large but ON-GRID area still produces its `Expr::Range` node, left for
    /// `Resolver::range` to refuse at eval.
    fn forge_offset(&self, args: &[Expr], home: u32, key: CellKey) -> Expr {
        let (top, left, base_h, base_w, sheet) = match &args[0] {
            Expr::Ref(r) => (r.row, r.col, 1u32, 1u32, r.sheet.clone()),
            // An open axis has no extent at forge time, and `abs_diff` on its `u32::MAX` sentinel would overflow the `+ 1` into a wrapped extent.
            Expr::Range(rn) if rn.is_open_rows() || rn.is_open_cols() => {
                self.refuse(self.forge_diag(
                    key,
                    "OFFSET over a whole-column/row base is not supported (give a bounded base)",
                ));
                return Expr::Lit(Value::Error(ErrKind::Ref));
            }
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
        let rows =
            match coerce_offset_int(self.eval_root_expr(&args[1], home, Some((key.2, key.1)))) {
                Ok(n) => n,
                Err(k) => return Expr::Lit(Value::Error(k)),
            };
        let cols =
            match coerce_offset_int(self.eval_root_expr(&args[2], home, Some((key.2, key.1)))) {
                Ok(n) => n,
                Err(k) => return Expr::Lit(Value::Error(k)),
            };
        let height = match args.get(3).filter(|a| !omitted(a)) {
            Some(a) => {
                match coerce_offset_int(self.eval_root_expr(a, home, Some((key.2, key.1)))) {
                    Ok(n) => n,
                    Err(k) => return Expr::Lit(Value::Error(k)),
                }
            }
            None => i64::from(base_h),
        };
        let width = match args.get(4).filter(|a| !omitted(a)) {
            Some(a) => {
                match coerce_offset_int(self.eval_root_expr(a, home, Some((key.2, key.1)))) {
                    Ok(n) => n,
                    Err(k) => return Expr::Lit(Value::Error(k)),
                }
            }
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

    /// CLONED, so no `&self.tabs` borrow is held while the pass evaluates arguments through the
    /// engine, which borrows the results and memo.
    fn cloned_grid_formula(&self, key: CellKey) -> Option<Expr> {
        match self.grid_cell_at(key.0, key.1, key.2)? {
            GridCell::Formula { expr, .. } => Some(expr.clone()),
            _ => None,
        }
    }

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

/// Excel's logical coercion. An array collapses to its top-left element first.
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

/// Excel's numeric coercion, truncating toward zero. An array collapses to its top-left first.
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

/// The no-progress branch only, where a target cannot be computed at all. Recurses structurally, so
/// a forger nested in a reducer still becomes a `#REF!` the reducer propagates.
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
