// Concern: a per-cell digest of the authored content cone, for change detection | Non-concern: the cell's VALUE, which the digest never covers | IO: (sheet, col, row) -> Option<hex digest>

use std::collections::{HashMap, HashSet};

use fsa1_ast::{ErrKind, Expr, Value};

use crate::grid::Cell as GridCell;

use super::{CellKey, MAX_RANGE_CELLS, Workbook, sort_dedup};

/// FNV-1a, deliberately NON-cryptographic. Nothing persists a digest, so cross-version stability is
/// not a contract and changing the scheme invalidates nothing.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Folded FIRST, so two content kinds sharing their trailing bytes can never collide.
const TAG_BLANK: u8 = 0;
const TAG_LITERAL: u8 = 1;
const TAG_FORMULA: u8 = 2;
const TAG_LOAD_ERROR: u8 = 3;

/// Never crosses the engine boundary: [`Workbook::computation_hash`] hands out only the hex.
#[derive(Clone, Copy, PartialEq, Eq)]
struct CompHash(u64);

impl CompHash {
    fn to_hex(self) -> String {
        format!("{:016x}", self.0)
    }
}

struct Fnv(u64);

impl Fnv {
    fn new() -> Fnv {
        Fnv(FNV_OFFSET)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(FNV_PRIME);
        }
    }

    fn finish(self) -> CompHash {
        CompHash(self.0)
    }
}

/// A gap and an empty literal cell are the same content, so both hash here.
fn blank_hash() -> CompHash {
    let mut h = Fnv::new();
    h.write(&[TAG_BLANK]);
    h.finish()
}

/// Over the value's bytes, never its address.
fn literal_hash(v: &Value) -> CompHash {
    match v {
        Value::Blank => blank_hash(),
        _ => {
            let mut h = Fnv::new();
            h.write(&[TAG_LITERAL]);
            write_value(&mut h, v);
            h.finish()
        }
    }
}

/// Over the VERBATIM source bytes, since a load-error cell has no parsed form to fold.
fn load_error_hash(src: &str) -> CompHash {
    let mut h = Fnv::new();
    h.write(&[TAG_LOAD_ERROR]);
    h.write(src.as_bytes());
    h.finish()
}

/// A discriminant byte, then the payload bytes.
fn write_value(h: &mut Fnv, v: &Value) {
    match v {
        Value::Number(n) => {
            h.write(&[1]);
            // `0` and `-0` are identical to Excel, and a literal is always finite, so `to_bits` is stable.
            let bits = if *n == 0.0 { 0.0f64 } else { *n }.to_bits();
            h.write(&bits.to_le_bytes());
        }
        Value::Text(s) => {
            h.write(&[2]);
            h.write(&(s.len() as u64).to_le_bytes());
            h.write(s.as_bytes());
        }
        Value::Bool(b) => h.write(&[3, u8::from(*b)]),
        Value::Error(k) => h.write(&[4, err_tag(*k)]),
        Value::Blank => h.write(&[5]),
        Value::Array(shape, cells) => {
            h.write(&[6]);
            h.write(&shape.rows.to_le_bytes());
            h.write(&shape.cols.to_le_bytes());
            for c in cells {
                write_value(h, c);
            }
        }
    }
}

/// A stable byte per class; the digest is opaque, so it need not match the spelled `#REF!` text.
fn err_tag(k: ErrKind) -> u8 {
    match k {
        ErrKind::Ref => 1,
        ErrKind::Div0 => 2,
        ErrKind::Value => 3,
        ErrKind::Name => 4,
        ErrKind::Na => 5,
        ErrKind::Null => 6,
        ErrKind::Num => 7,
        ErrKind::Spill => 8,
        ErrKind::Calc => 9,
    }
}

/// The `trace` surface reuses ONE across every node, so a whole trace hashes in O(cone). Every
/// completed verdict memoizes: a digest is a function of the content cone alone, never of the walk.
pub(super) struct HashMemo {
    map: HashMap<CellKey, Option<CompHash>>,
    /// A plain `bool`, unlike the digest map, because this verdict reads only the cell's OWN
    /// expression and never its dependencies.
    over_bound: HashMap<CellKey, bool>,
}

/// `Fold` is pushed BENEATH a formula's dependencies, so it pops only once every one has settled.
enum HashStep {
    Visit(CellKey),
    Fold(CellKey, Vec<CellKey>),
}

impl HashMemo {
    pub(super) fn new() -> HashMemo {
        HashMemo {
            map: HashMap::new(),
            over_bound: HashMap::new(),
        }
    }
}

impl Workbook {
    /// The WRITTEN content cone only: neither the computing cell's own coordinate nor any runtime
    /// input is folded, so `=ROW()` at A2 and at A9 share a digest and a `TODAY()` cell's never
    /// changes though its value does. A MATCH means the authored cone is unchanged, never the value.
    /// `None` where the cell lies on a reference cycle.
    pub fn computation_hash(&self, sheet: u32, col: u32, row: u32) -> Option<String> {
        let mut memo = HashMemo::new();
        self.computation_hash_with((sheet, col, row), &mut memo)
    }

    /// The same digest against a caller-owned memo, so a cell's identity is still its own content
    /// and never relative to the walk that reached it.
    pub(super) fn computation_hash_with(
        &self,
        key: CellKey,
        memo: &mut HashMemo,
    ) -> Option<String> {
        self.comp_hash(key, memo).map(CompHash::to_hex)
    }

    /// A formula folds its verbatim text with each dependency's `(key, digest)` in SORTED
    /// dependency-key order, so the result is traversal-independent, and a dependency with no digest
    /// makes this cell's `None` too. A dependency absent from `memo` at fold time is exactly the
    /// cycle case: every other one memoized before this `Fold` step was popped.
    fn comp_hash(&self, key: CellKey, memo: &mut HashMemo) -> Option<CompHash> {
        let root = self.array_region_anchor(key.0, key.1, key.2).unwrap_or(key);
        let mut on_stack: HashSet<CellKey> = HashSet::new();
        let mut stack = vec![HashStep::Visit(root)];
        while let Some(step) = stack.pop() {
            let key = match step {
                HashStep::Fold(key, deps) => {
                    on_stack.remove(&key);
                    let digest = self.fold_formula(key, &deps, memo);
                    memo.map.insert(key, digest);
                    continue;
                }
                HashStep::Visit(key) => {
                    self.array_region_anchor(key.0, key.1, key.2).unwrap_or(key)
                }
            };
            if memo.map.contains_key(&key) || on_stack.contains(&key) {
                continue; // settled, or a cycle back-edge the enclosing fold reads as `None`
            }
            let (sheet, col, row) = key;
            let Some(cell) = self.grid_cell_at(sheet, col, row) else {
                memo.map.insert(key, Some(blank_hash()));
                continue;
            };
            let GridCell::Formula { expr, .. } = cell else {
                let h = match cell {
                    GridCell::Value { value, .. } => Some(literal_hash(value)),
                    // A leaf: it has fixed content and no dependencies, so editing between a load error and a valid formula still mints a new digest.
                    GridCell::LoadError { src, .. } => Some(load_error_hash(src)),
                    GridCell::Formula { .. } => unreachable!("matched a formula in the else arm"),
                };
                memo.map.insert(key, h);
                continue;
            };
            // The plan leaves an over-bound range unexpanded, so such a cell would otherwise hash as if the range were absent — a `Some` must mean a digest over the WHOLE cone, never a truncated one.
            if self.references_over_bound_range(key, expr, sheet, memo) {
                memo.map.insert(key, None);
                continue;
            }
            let deps = sort_dedup(self.expr_deps(expr, sheet));
            on_stack.insert(key);
            stack.push(HashStep::Fold(key, deps.clone()));
            for &d in deps.iter().rev() {
                stack.push(HashStep::Visit(d));
            }
        }
        memo.map.get(&root).copied().flatten()
    }

    fn fold_formula(&self, key: CellKey, deps: &[CellKey], memo: &HashMemo) -> Option<CompHash> {
        let Some(GridCell::Formula { src, .. }) = self.grid_cell_at(key.0, key.1, key.2) else {
            // Failing fast rather than returning `None`, which is indistinguishable from the cycle terminal and would answer a wrong digest silently.
            unreachable!(
                "a fold step is only ever pushed for a formula cell, but {key:?} is not one"
            );
        };
        let mut f = Fnv::new();
        f.write(&[TAG_FORMULA]);
        f.write(src.as_bytes());
        for &d in deps {
            let h = (*memo.map.get(&d)?)?;
            f.write(&d.0.to_le_bytes());
            f.write(&d.1.to_le_bytes());
            f.write(&d.2.to_le_bytes());
            f.write(&h.0.to_le_bytes());
        }
        Some(f.finish())
    }

    /// NOT content-intrinsic: it reads the CLAMPED rectangle, so a distant cell widening the used
    /// extent of any tab the expression names can flip the verdict. Memoizing it is still sound —
    /// every such tab is immutable for a `Workbook`'s lifetime. A range FORGED at runtime is
    /// deliberately invisible here: the digest is over what the author WROTE.
    fn references_over_bound_range(
        &self,
        key: CellKey,
        expr: &Expr,
        home: u32,
        memo: &mut HashMemo,
    ) -> bool {
        if let Some(&v) = memo.over_bound.get(&key) {
            return v;
        }
        let v = self.expr_over_bound_range(expr, home);
        memo.over_bound.insert(key, v);
        v
    }

    /// The `match` is EXHAUSTIVE deliberately, so a new [`Expr`] variant fails to compile here rather
    /// than being silently ignored. A compile-time floor only: the arm still has to be written right.
    fn expr_over_bound_range(&self, expr: &Expr, home: u32) -> bool {
        match expr {
            Expr::Range(rn) => self
                .clamped_range(rn, home)
                .is_some_and(|r| r.area > MAX_RANGE_CELLS),
            Expr::Unary(_, e) | Expr::ImplicitIntersect(e) | Expr::SpillRef(e) => {
                self.expr_over_bound_range(e, home)
            }
            Expr::Binary(_, a, b) => {
                self.expr_over_bound_range(a, home) || self.expr_over_bound_range(b, home)
            }
            Expr::Call(_, args) => args.iter().any(|a| self.expr_over_bound_range(a, home)),
            Expr::Lit(_) | Expr::Ref(_) => false,
        }
    }
}
