// Concern: the COMPUTATION HASH primitive (the ENG7 digest, no persistence) — a per-cell deterministic digest of a cell's OWN content (a literal's value bytes, a formula's verbatim text, a blank/gap a fixed blank tag) folded with its DEPENDENCIES' computation hashes in a DETERMINISTIC dependency-key order (so it is traversal-independent, VAL1: never the cell's own address), computed in ONE memoized walk over the dependency relation via `expr_deps`; a cell on a reference CYCLE and a depth-tainted cell have NO hash (`None`), mirroring the plan's `Cycle`/`DepthRefused` terminals, and a `None` dependency propagates upward; the raw digest type [`CompHash`] and its [`HashMemo`] stay engine-private (ENG3 containment), the only PUBLIC surface being [`Workbook::computation_hash`] which returns an OPAQUE hex `String` or `None` | Non-concern: computing any VALUE (the `evaluate` sibling owns that; this hashes CONTENT, never the computed value), building the dependency graph (the `plan` sibling owns `DepGraph`/`PlanNode`; this reuses only `expr_deps`), persistence / the on-disk `.cache/` (that is a later workbench), and a stable cross-version identity (the hash is an opaque change-detector, ENG7 — a fast non-cryptographic FNV-1a digest) | IO: (a `(sheet,col,row)` cell + the `Workbook`'s grids) -> an `Option<String>` opaque hex digest (`None` on a cycle/depth-tainted cell)
use std::collections::{HashMap, HashSet};

use charlie_ast::{ErrKind, Value};

use crate::grid::Cell as GridCell;

use super::{CellKey, MAX_PULL_DEPTH, Workbook, sort_dedup};

/// FNV-1a 64-bit offset basis and prime — a fast, deterministic, NON-cryptographic hash. The digest
/// is an opaque change-detector (ENG7), so a stable-within-a-run-and-across-runs value is all that is
/// required; cross-version stability is explicitly NOT a contract (changing the scheme only
/// invalidates a later cache).
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Content-kind tag bytes, folded first so a literal, a formula, and a blank can never collide even
/// with otherwise-identical trailing bytes.
const TAG_BLANK: u8 = 0;
const TAG_LITERAL: u8 = 1;
const TAG_FORMULA: u8 = 2;
/// GRID6 load-error cell content tag — folded first so an unparseable-formula cell can never collide
/// with a `#NAME?` literal or a parsed formula sharing its bytes.
const TAG_LOAD_ERROR: u8 = 3;

/// A per-cell computation digest — the ENG7 hash. PRIVATE to the engine (ENG3 containment): it appears
/// in no other module's surface and is never re-exported. The public accessor
/// ([`Workbook::computation_hash`]) hands out only its opaque hex spelling.
#[derive(Clone, Copy, PartialEq, Eq)]
struct CompHash(u64);

impl CompHash {
    /// The opaque, fixed-width hex spelling handed across the engine boundary.
    fn to_hex(self) -> String {
        format!("{:016x}", self.0)
    }
}

/// A tiny incremental FNV-1a hasher — enough to fold tag bytes, a formula's text, a literal's value,
/// and dependency keys + digests in a deterministic order. This is the engine's SINGLE FNV-1a fold
/// (DRY): the computation-hash walk here folds content into a [`CompHash`] via [`Fnv::finish`], while
/// the cache sibling's payload-integrity checksum folds a byte slice through the very same primitive
/// and reads out the raw digest via [`Fnv::digest`]. `pub(super)` keeps it engine-private (ENG3): it
/// is visible only within the `workbook` module tree, never re-exported to `charlie-cli`/`charlie-ast`.
pub(super) struct Fnv(u64);

impl Fnv {
    pub(super) fn new() -> Fnv {
        Fnv(FNV_OFFSET)
    }

    pub(super) fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(FNV_PRIME);
        }
    }

    /// The raw 64-bit digest — the cache sibling's checksum codec reads its fold out here (the
    /// computation-hash walk instead wraps the same bits in an opaque [`CompHash`] via [`finish`]).
    pub(super) fn digest(self) -> u64 {
        self.0
    }

    fn finish(self) -> CompHash {
        CompHash(self.0)
    }
}

/// The fixed digest of a blank cell / a gap — content-free, so every blank hashes identically (VAL1: a
/// gap and an empty literal cell are the same content, and both read `Blank`).
fn blank_hash() -> CompHash {
    let mut h = Fnv::new();
    h.write(&[TAG_BLANK]);
    h.finish()
}

/// The digest of a LITERAL cell's value — over its bytes, not its address (VAL1). A number canonicalizes
/// `-0.0` to `0.0` (Excel never distinguishes them); a `Blank` routes to the shared [`blank_hash`] tag so
/// an empty literal and a gap agree. An `Array` never occurs as literal cell content but is folded
/// defensively (element-wise) so this is total.
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

/// The digest of a GRID6 load-error cell — over its verbatim source bytes (VAL1: content, not address),
/// tagged distinctly so it never collides with a parsed formula or a literal of the same bytes.
fn load_error_hash(src: &str) -> CompHash {
    let mut h = Fnv::new();
    h.write(&[TAG_LOAD_ERROR]);
    h.write(src.as_bytes());
    h.finish()
}

/// Fold one [`Value`]'s content into `h` deterministically: a discriminant byte then the payload bytes.
fn write_value(h: &mut Fnv, v: &Value) {
    match v {
        Value::Number(n) => {
            h.write(&[1]);
            // Canonicalize -0.0 to 0.0 so `0` and `-0` (identical to Excel) hash identically. A literal
            // is always finite (the lexer rejects inf/nan), so `to_bits` is stable.
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

/// A stable byte per error class (the digest need not match the spelled `#REF!` text — it is opaque).
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

/// The engine-private memo backing a batch of [`Workbook::computation_hash_with`] calls (the `trace`
/// surface reuses ONE across every node so a whole trace hashes in O(cone)). PRIVATE to the engine
/// (ENG3 containment): its element type [`CompHash`] never escapes, and only opaque hex `String`s cross
/// the boundary. A clean result (a literal, a blank, a cycle `None`) is memoized; a depth-tainted
/// `None` is root-relative and deliberately NOT memoized, mirroring the value engine's `finish_pass`.
pub(super) struct HashMemo {
    map: HashMap<CellKey, Option<CompHash>>,
}

impl HashMemo {
    pub(super) fn new() -> HashMemo {
        HashMemo {
            map: HashMap::new(),
        }
    }
}

impl Workbook {
    /// The PUBLIC computation-hash accessor (the ENG7 primitive): a deterministic, opaque hex digest of
    /// the cell's own content folded with its dependencies' digests, or `None` when the cell lies on a
    /// reference cycle or is depth-tainted. A fresh one-shot memo per call, so the digest is a pure
    /// function of the cell's content cone (VAL1) and is stable run-to-run.
    pub fn computation_hash(&self, sheet: u32, col: u32, row: u32) -> Option<String> {
        let mut memo = HashMemo::new();
        self.computation_hash_with((sheet, col, row), 0, &mut memo)
    }

    /// The same digest, reusing a caller-owned [`HashMemo`] across many cells (the `trace` surface) and
    /// STARTING the depth count at `at_depth` — the plan depth the cell sits at (the trace/public
    /// callers pass `0`, the cell's own rooted identity). The digest VALUE is content-only and so is
    /// independent of `at_depth`; `at_depth` only shifts WHEN the walk hits the pull-depth bound, i.e.
    /// whether the digest is a clean `Some` or a depth-tainted `None`. The ENG7 cache serve threads the
    /// plan depth here so a cached value is served for a cell EXACTLY when a cold descent from that same
    /// depth would compute it clean (see [`Workbook::cacheable_hash`]); a cone that would refuse from
    /// this depth returns `None` and is not served. The raw [`CompHash`] stays inside; only the opaque
    /// hex spelling is returned.
    pub(super) fn computation_hash_with(
        &self,
        key: CellKey,
        at_depth: u32,
        memo: &mut HashMemo,
    ) -> Option<String> {
        let mut on_stack = HashSet::new();
        self.comp_hash(key, at_depth, &mut memo.map, &mut on_stack)
            .0
            .map(CompHash::to_hex)
    }

    /// One memoized step of the computation-hash walk. Returns `(digest, depth_tainted)`:
    /// * a cell already in `memo` -> its clean cached digest;
    /// * a cell on the DFS stack (`on_stack`) -> `None` (a reference cycle; clean, not memoized here as
    ///   it is still in flight — the enclosing frames fold it and memoize their own `None`);
    /// * a cell past [`MAX_PULL_DEPTH`] -> a depth-tainted `None` (root-relative, never memoized, so a
    ///   later shallower demand recomputes — mirrors the value engine's depth guard);
    /// * a gap / a blank literal -> the fixed [`blank_hash`]; a non-blank literal -> its [`literal_hash`];
    /// * a formula -> the digest of its verbatim text folded with each dependency's `(key, digest)` in
    ///   sorted dependency-key order (traversal-independent). Any dependency with no digest makes this
    ///   cell's digest `None` too (a `None` propagates upward); a depth-tainted dependency taints this
    ///   cell so its (possibly `None`) result is not memoized.
    ///
    /// GRID5: the cell is first redirected to its array-formula region ANCHOR, so every region member
    /// shares the anchor's one digest (VAL1/ENG3: the region is one computation).
    fn comp_hash(
        &self,
        key: CellKey,
        depth: u32,
        memo: &mut HashMap<CellKey, Option<CompHash>>,
        on_stack: &mut HashSet<CellKey>,
    ) -> (Option<CompHash>, bool) {
        let key = self.array_region_anchor(key.0, key.1, key.2).unwrap_or(key);
        if let Some(cached) = memo.get(&key) {
            return (*cached, false);
        }
        if on_stack.contains(&key) {
            return (None, false); // a reference cycle: clean `None`, in flight (not memoized here)
        }
        if depth >= MAX_PULL_DEPTH {
            return (None, true); // depth-tainted `None`: root-relative, never memoized
        }
        let (sheet, col, row) = key;
        // The lone covering grid cell (the anchor redirect above already mapped a region member to
        // `(0,0)`); a gap reads `Blank` -> the fixed blank digest.
        let Some(cell) = self.grid_cell_at(sheet, col, row) else {
            let h = Some(blank_hash());
            memo.insert(key, h);
            return (h, false);
        };
        let GridCell::Formula { src, expr } = cell else {
            let h = match cell {
                GridCell::Value(v) => Some(literal_hash(v)),
                // GRID6: a load-error cell has fixed, deterministic content (its verbatim source) and
                // no dependencies — hash it like a leaf over that source text, so editing the file
                // between a load error and a valid formula (or a `#NAME?` literal) mints a new hash.
                GridCell::LoadError { src, .. } => Some(load_error_hash(src)),
                GridCell::Formula { .. } => unreachable!("matched a formula in the else arm"),
            };
            memo.insert(key, h);
            return (h, false);
        };
        // A formula: fold its verbatim text with its dependencies' digests in sorted key order.
        let deps = sort_dedup(self.expr_deps(expr, sheet));
        on_stack.insert(key);
        let mut child: Vec<(CellKey, Option<CompHash>)> = Vec::with_capacity(deps.len());
        let mut tainted = false;
        for d in deps {
            let (h, t) = self.comp_hash(d, depth + 1, memo, on_stack);
            tainted |= t;
            child.push((d, h));
        }
        on_stack.remove(&key);
        let result = if child.iter().any(|(_, h)| h.is_none()) {
            None // a dependency with no digest -> this cell has none either
        } else {
            let mut f = Fnv::new();
            f.write(&[TAG_FORMULA]);
            f.write(src.as_bytes());
            for (k, h) in &child {
                f.write(&k.0.to_le_bytes());
                f.write(&k.1.to_le_bytes());
                f.write(&k.2.to_le_bytes());
                f.write(&h.expect("checked all-some above").0.to_le_bytes());
            }
            Some(f.finish())
        };
        // A depth-tainted result is root-relative — never memoize it (a later shallower demand must be
        // free to recompute), exactly as the value engine drops depth-tainted values in `finish_pass`.
        if !tainted {
            memo.insert(key, result);
        }
        (result, tainted)
    }
}
