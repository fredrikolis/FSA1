// Concern: the ENG7 PERSISTENT RESULT CACHE — the content-addressed on-disk store under `<workbook>/.cache/` (FS3) keyed by the WF1 COMPUTATION HASH, plus the engine-side glue that lets a demand SHORT-CIRCUIT recompute on a hit: [`ResultCache`] serializes/reads one six-type [`Value`] per hash-named file (atomic temp-write + rename, ~/.knowledge-base persistent-storage Part 8) and is best-effort (a corrupt/absent/failed entry is a MISS, never a value source, VAL2); [`CacheScan`] carries the per-demand shared hash + volatility memos; [`Workbook::cache_serve`] (called from the PLAN pass after the memo check) computes a demanded cell's hash from its CONTENT cone alone and, on a hit, injects the value into the memo so the cell's dependency cone is never planned or evaluated (the reuse win), and [`Workbook::cacheable_hash`] gates cacheability — only a non-array-region formula cell (regions recompute) with a computation hash (no cycle/depth-tainted cell, ENG7) whose cone contains NO volatile function (TODAY/NOW read the clock, which the hash does not fold, so caching them would be unsound) is written/served | Non-concern: computing the hash itself (the `hash` sibling owns `computation_hash_with`/`HashMemo`), building the dep graph or evaluating (the `plan`/`evaluate` siblings; this only decides to skip them), the `.cache/`-is-not-a-tab loader carve-out (mod.rs `load_dir` owns FS3 enumeration), and any authoritative or derived record beyond keyed result VALUES (VAL2 — no deps/consumers persisted) | IO: (a computation-hash hex key) <-> a serialized `Value` file under `.cache/`, via atomic write + plain read; the cache lives only where `load_dir` attached it (never for an in-memory `from_tabs` workbook, ENG5)
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use charlie_ast::{ErrKind, Expr, Shape, Value, func};

use crate::grid::Cell as GridCell;

use super::hash::{Fnv, HashMemo};
use super::{CellKey, Workbook};

/// The on-disk, content-addressed result store living under `<workbook>/.cache/` (FS3). One file per
/// computation hash holds that hash's serialized [`Value`]. The cache is a CONTAINED optimization
/// (ENG3/VAL2): it is never authoritative — deleting the directory changes only the work to recompute a
/// value, never the value — so every read/write failure degrades to a plain cache MISS.
///
/// GROWTH / GC: the store is append-only and UNBOUNDED — each content edit mints a NEW hash-named file
/// and orphans the prior one, and a crash between the temp-write and the rename can leave a stray
/// `.tmp-<hash>-<pid>` file that only the in-process failure path reclaims. There is deliberately no
/// eviction or GC yet (a future workbench); the FS3 escape hatch is that deleting `.cache/` is ALWAYS
/// safe and loses no value (VAL2), so an operator (or `--no-cache`) can reclaim the directory at any
/// time. `.cache/` size is bounded in practice by how much a workbook is edited between manual clears,
/// not by the engine.
#[derive(Debug)]
pub(super) struct ResultCache {
    /// The `.cache/` directory (created lazily on the first successful write).
    dir: PathBuf,
}

impl ResultCache {
    pub(super) fn new(dir: PathBuf) -> ResultCache {
        ResultCache { dir }
    }

    /// Read the value stored under `hash`, or `None` on a miss OR any read/decode failure. The cache
    /// is regenerable, so a truncated / corrupt / stale-scheme entry is simply a miss — it can never
    /// yield a wrong value: `decode_value` verifies the entry's PAYLOAD CHECKSUM and that the decode
    /// consumes the whole payload, so neither a torn file nor a same-length zero-/garbage-fill (which
    /// could otherwise decode cleanly to a valid `Value`) survives as a value.
    pub(super) fn get(&self, hash: &str) -> Option<Value> {
        let bytes = fs::read(self.dir.join(hash)).ok()?;
        decode_value(&bytes)
    }

    /// Persist `value` under `hash` via an ATOMIC temp-write + rename (persistent-storage Part 8): a
    /// reader sees either the old file or the whole new one, never a partial write. The temp name is
    /// per-process so two concurrent charlie runs writing the SAME (content-addressed) hash cannot
    /// clobber each other's temp file; the final rename is last-writer-wins over byte-identical
    /// content. Best-effort: every failure is swallowed (the cache is never load-bearing, VAL2).
    ///
    /// An explicit `fsync` before the rename is DELIBERATELY omitted (unlike Part 8's durability
    /// example): the cache is regenerable and non-authoritative (VAL2), so a crash-corrupted entry must
    /// be a clean MISS, never a wrong value — and durability buys nothing toward that. The integrity
    /// property that DOES matter (ENG7: a hit equals recomputation) is enforced by `decode_value`'s
    /// prepended PAYLOAD CHECKSUM, not by fsync: without it, an fsync-free rename can expose a
    /// same-length but zero-/garbage-filled entry that happens to decode cleanly (a 17-byte all-zero
    /// file decodes to `Number(0.0)`), silently serving `0.0` for a cell whose real value is not 0. The
    /// checksum makes ANY payload corruption (torn, zero-filled, or bit-rotted) fail verification and
    /// become a miss, so the rename (no reader ever sees a PARTIAL file) and the checksum (no reader
    /// ever trusts a corrupt WHOLE file) together restore the absolute "a corrupt entry is always a
    /// miss" guarantee — the cell simply recomputes.
    pub(super) fn put(&self, hash: &str, value: &Value) {
        if fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let tmp = self.dir.join(format!(".tmp-{hash}-{}", std::process::id()));
        if fs::write(&tmp, encode_value(value)).is_err() {
            let _ = fs::remove_file(&tmp);
            return;
        }
        if fs::rename(&tmp, self.dir.join(hash)).is_err() {
            let _ = fs::remove_file(&tmp);
        }
    }
}

/// The per-demand scratch memos the cache read/write share so a whole pass hashes each cell's cone at
/// most once: the WF1 content-hash memo and the cone-volatility memo. Fresh per demand (the demanded
/// cells' content cone is a pure function of their content, so nothing carries across demands).
pub(super) struct CacheScan {
    hashes: HashMemo,
    volatile: HashMap<CellKey, bool>,
}

impl CacheScan {
    pub(super) fn new() -> CacheScan {
        CacheScan {
            hashes: HashMemo::new(),
            volatile: HashMap::new(),
        }
    }
}

impl Workbook {
    /// The PLAN-pass cache short-circuit (ENG7). If caching is on and `key` is a cacheable cell whose
    /// computation hash HITS the cache, inject the cached value straight into the memo and return
    /// `true` — the caller then treats the cell as a resolved leaf, so its dependency cone is NEVER
    /// planned or evaluated (a cached subtree is not recomputed — the whole efficiency point). A miss,
    /// an uncacheable cell (a cycle/depth-tainted or volatile cone, or an array region), or caching-off
    /// returns `false` and the cell plans normally. The served value goes into the memo (not the pass
    /// results), so `finish_pass` never re-writes it.
    ///
    /// `depth` is the cell's PLAN depth (from [`Workbook::plan_visit`]). It gates soundness against the
    /// pull-depth bound (ENG7): a cached value is served ONLY when the cell's cone fits within the
    /// remaining depth budget from HERE, i.e. exactly when a cold descent from this same depth would
    /// compute it clean too. A cone that a cold descent would carry past [`MAX_PULL_DEPTH`] into a
    /// depth-tainted `#NUM!` gets no cache key at this depth (see [`Workbook::cacheable_hash`]) and
    /// plans normally, so the cell reaches the SAME depth refusal warm as cold — a warm run can never
    /// short-circuit a depth refusal a cache-deleted run would raise.
    pub(super) fn cache_serve(&self, key: CellKey, depth: u32, scan: &mut CacheScan) -> bool {
        let Some(cache) = &self.cache else {
            return false; // caching off (an in-memory workbook, or `--no-cache`): never touch disk
        };
        let Some(hash) = self.cacheable_hash(key, depth, scan) else {
            return false;
        };
        match cache.get(&hash) {
            Some(v) => {
                self.memo.borrow_mut().insert(key, v);
                true
            }
            None => false,
        }
    }

    /// Persist the just-computed CLEAN results of this pass to the cache (the ENG7 write, mirroring
    /// `finish_pass`'s clean-only memo rule). Only cacheable cells are written; a served cell is not in
    /// `clean` (it went straight to the memo), so it is never re-written. No-op when caching is off.
    pub(super) fn cache_store_clean(&self, clean: &[(CellKey, Value)]) {
        let Some(cache) = &self.cache else {
            return;
        };
        let mut scan = CacheScan::new();
        for (key, value) in clean {
            // The WRITE keys by the cell's ROOTED (depth-0) content hash — the canonical, depth-
            // independent key. A cell reached this pass is in `clean` only because it computed WITHOUT
            // a depth taint (`finish_pass` drops tainted results), so its cone fits from wherever it
            // was reached and therefore fits from depth 0 too: the rooted hash is a clean `Some`. The
            // serve side (`cache_serve`) re-derives this same value hash but with the plan depth, so it
            // only reads it back where a cold descent would also compute it clean.
            if let Some(hash) = self.cacheable_hash(*key, 0, &mut scan) {
                cache.put(&hash, value);
            }
        }
    }

    /// The cache key for `key` at plan `depth`, or `None` if the cell must NOT be cached at this depth.
    /// Cacheable iff it is a non-array-region FORMULA cell (a literal reads straight from the grid; an
    /// array region recomputes — its one hash covers many coordinates, so it is left uncached for
    /// simplicity) that HAS a computation hash whose content cone contains NO volatile function.
    ///
    /// The hash is derived STARTING at `depth` (the plan depth the cell sits at): a reference-cycle or
    /// depth-tainted cell has no hash (ENG7), and — the soundness gate for deep chains — a cell whose
    /// cone would be carried past [`MAX_PULL_DEPTH`] FROM THIS DEPTH is depth-tainted here and so gets
    /// NO key, even though its rooted (depth-0) cone would fit. The WRITE side passes `depth = 0` (the
    /// canonical content key of a cell that already computed clean); the SERVE side passes the plan
    /// depth, so a cached value is read back ONLY where a cold descent from that depth would recompute
    /// it clean — a warm run never short-circuits a depth refusal a cache-deleted run would raise.
    ///
    /// TODAY/NOW read the resolver clock, which the computation hash does not fold, so a cached volatile
    /// result could not equal a fresh recomputation (ENG7 soundness) — such cones are refused a key and
    /// always recompute.
    fn cacheable_hash(&self, key: CellKey, depth: u32, scan: &mut CacheScan) -> Option<String> {
        let (sheet, col, row) = key;
        let (_, file) = self.covering(sheet, col, row)?;
        if file.array_formula {
            return None; // an array-formula region recomputes (one hash spans many coordinates)
        }
        if !matches!(
            self.grid_cell_at(sheet, col, row)?,
            GridCell::Formula { .. }
        ) {
            return None; // a literal / gap is read from the grid, never cached
        }
        let hash = self.computation_hash_with(key, depth, &mut scan.hashes)?;
        if self.cone_volatile(key, &mut scan.volatile) {
            return None; // a volatile cone (TODAY/NOW) is unsound to cache
        }
        Some(hash)
    }

    /// Whether `key`'s content cone contains a VOLATILE function call (TODAY/NOW), memoized in `scan`.
    /// Cycle-safe (a back-edge is not volatile-by-itself; a cyclic cell has no hash and is uncacheable
    /// anyway) and array-region aware (a member folds through its anchor, matching the hash walk).
    fn cone_volatile(&self, key: CellKey, memo: &mut HashMap<CellKey, bool>) -> bool {
        let mut on_stack = HashSet::new();
        self.cone_volatile_walk(key, memo, &mut on_stack)
    }

    fn cone_volatile_walk(
        &self,
        key: CellKey,
        memo: &mut HashMap<CellKey, bool>,
        on_stack: &mut HashSet<CellKey>,
    ) -> bool {
        let key = self.array_region_anchor(key.0, key.1, key.2).unwrap_or(key);
        if let Some(&v) = memo.get(&key) {
            return v;
        }
        if on_stack.contains(&key) {
            return false; // a reference cycle: neither volatile nor cacheable
        }
        let (sheet, col, row) = key;
        let Some(GridCell::Formula { expr, .. }) = self.grid_cell_at(sheet, col, row) else {
            memo.insert(key, false); // a literal / gap holds no formula, so no volatile call
            return false;
        };
        if expr_has_volatile(expr) {
            memo.insert(key, true);
            return true;
        }
        on_stack.insert(key);
        let mut volatile = false;
        for d in self.expr_deps(expr, sheet) {
            if self.cone_volatile_walk(d, memo, on_stack) {
                volatile = true;
                break;
            }
        }
        on_stack.remove(&key);
        memo.insert(key, volatile);
        volatile
    }
}

/// Whether an expression tree calls a volatile built-in (TODAY/NOW) anywhere within it — the per-cell
/// half of [`Workbook::cone_volatile`], which then folds in the dependency cells' cones.
fn expr_has_volatile(expr: &Expr) -> bool {
    match expr {
        Expr::Call(id, args) => {
            // ENG7: a reference-forging call (`INDIRECT`/`OFFSET`) is VOLATILE — its resolved target
            // depends on runtime values the computation hash does not fold, so a forging cone can never
            // be cached (the same reason TODAY/NOW are volatile). Read here on the ORIGINAL grid expr
            // (the `cone_volatile_walk` caller passes the grid cell), so the forger is still seen even
            // after Pass 0 rewrote the effective form — the whole cone is correctly excluded.
            func::def(*id).is_some_and(|d| d.volatile)
                || super::forge::is_forger(*id)
                || args.iter().any(expr_has_volatile)
        }
        Expr::Unary(_, e) | Expr::ImplicitIntersect(e) | Expr::SpillRef(e) => expr_has_volatile(e),
        Expr::Binary(_, a, b) => expr_has_volatile(a) || expr_has_volatile(b),
        Expr::Lit(_) | Expr::Ref(_) | Expr::Range(_) => false,
    }
}

// ----------------------------------------------------------------------------------------------
// Value serialization — a compact, self-contained binary codec (no external serde dependency). Each
// on-disk file is `[8-byte FNV-1a checksum of the payload][payload]`; `decode_value` recomputes the
// checksum and rejects any mismatch, so a crash-/hardware-corrupted file that would otherwise be a
// valid same-length decode (e.g. an all-zero file that reads back as `Number(0.0)`) is a clean MISS
// rather than a wrong value (ENG7: a hit equals recomputation). The on-disk form need only round-trip
// within one build: the key is an OPAQUE change-detector (ENG7), so a scheme change simply orphans old
// files (they key differently and are ignored). A number is stored by its exact bit pattern so
// `-0.0`/`NaN` round-trip identically to `Value`'s bit-exact `Eq`.
// ----------------------------------------------------------------------------------------------

/// FNV-1a 64-bit over a byte slice — the cache file's PAYLOAD-INTEGRITY checksum (a corruption detector
/// prepended to each entry and verified on read). It reuses the engine's ONE FNV-1a fold — the `hash`
/// sibling's engine-private incremental [`Fnv`] (DRY: a single offset/prime and folding rule) — read out
/// as its raw 64-bit digest. The two USES stay distinct concerns even though they share the primitive:
/// the hash walk CONTENT-ADDRESSES a cell into an opaque [`CompHash`] (ENG7 keying), whereas this only
/// guards a stored file's bytes against corruption (cf. `err_code`, which — because it must INVERT —
/// keeps its own round-trippable mapping rather than sharing the hash sibling's one-way tag).
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = Fnv::new();
    h.write(bytes);
    h.digest()
}

/// Serialize one [`Value`] to a cache file: `[8-byte payload checksum][payload]` (the payload is the
/// tag-byte + payload form, arrays recursively). The checksum is verified on read so a corrupt WHOLE
/// file never decodes to a value.
fn encode_value(v: &Value) -> Vec<u8> {
    let mut payload = Vec::new();
    put_value(&mut payload, v);
    let checksum = fnv1a(&payload);
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&checksum.to_le_bytes());
    out.extend_from_slice(&payload);
    out
}

fn put_value(out: &mut Vec<u8>, v: &Value) {
    match v {
        Value::Number(n) => {
            out.push(0);
            out.extend_from_slice(&n.to_bits().to_le_bytes());
        }
        Value::Text(s) => {
            out.push(1);
            out.extend_from_slice(&(s.len() as u64).to_le_bytes());
            out.extend_from_slice(s.as_bytes());
        }
        Value::Bool(b) => {
            out.push(2);
            out.push(u8::from(*b));
        }
        Value::Error(k) => {
            out.push(3);
            out.push(err_code(*k));
        }
        Value::Blank => out.push(4),
        Value::Array(shape, cells) => {
            out.push(5);
            out.extend_from_slice(&shape.rows.to_le_bytes());
            out.extend_from_slice(&shape.cols.to_le_bytes());
            out.extend_from_slice(&(cells.len() as u64).to_le_bytes());
            for c in cells {
                put_value(out, c);
            }
        }
    }
}

/// Deserialize a [`Value`] from a whole cache file (`[8-byte checksum][payload]`). Returns `None` on
/// ANY malformation — a file shorter than the checksum header, a payload whose recomputed checksum does
/// not match the stored one (a torn, zero-filled, or bit-rotted entry, even one that would decode to a
/// valid same-length `Value`), an unknown tag, invalid UTF-8, a short read, or trailing bytes. A corrupt
/// entry is a clean cache miss, never a wrong value (VAL2 / ENG7). Total: never panics.
fn decode_value(bytes: &[u8]) -> Option<Value> {
    let stored = u64::from_le_bytes(bytes.get(..8)?.try_into().expect("took 8 bytes"));
    let payload = &bytes[8..];
    if fnv1a(payload) != stored {
        return None; // checksum mismatch: a corrupt file, even one that would otherwise decode cleanly
    }
    let mut cur = Cursor {
        bytes: payload,
        pos: 0,
    };
    let v = get_value(&mut cur)?;
    (cur.pos == payload.len()).then_some(v) // reject trailing bytes: a well-formed payload is exact
}

/// A bounds-checked read cursor over the cache file's bytes.
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let end = self.pos.checked_add(n)?;
        let slice = self.bytes.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|s| s[0])
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|s| u32::from_le_bytes(s.try_into().expect("took 4 bytes")))
    }

    fn u64(&mut self) -> Option<u64> {
        self.take(8)
            .map(|s| u64::from_le_bytes(s.try_into().expect("took 8 bytes")))
    }
}

fn get_value(cur: &mut Cursor) -> Option<Value> {
    match cur.u8()? {
        0 => Some(Value::Number(f64::from_bits(cur.u64()?))),
        1 => {
            let len = usize::try_from(cur.u64()?).ok()?;
            let s = std::str::from_utf8(cur.take(len)?).ok()?;
            Some(Value::Text(s.to_string()))
        }
        2 => Some(Value::Bool(cur.u8()? != 0)),
        3 => Some(Value::Error(err_from_code(cur.u8()?)?)),
        4 => Some(Value::Blank),
        5 => {
            let rows = cur.u32()?;
            let cols = cur.u32()?;
            let n = usize::try_from(cur.u64()?).ok()?;
            let mut cells = Vec::with_capacity(n.min(1 << 16));
            for _ in 0..n {
                cells.push(get_value(cur)?);
            }
            Some(Value::Array(Shape { rows, cols }, cells))
        }
        _ => None,
    }
}

/// A stable round-trippable byte per error class (distinct from the hash sibling's one-way fold tag:
/// this codec must invert, so it owns its own mapping).
fn err_code(k: ErrKind) -> u8 {
    match k {
        ErrKind::Ref => 0,
        ErrKind::Div0 => 1,
        ErrKind::Value => 2,
        ErrKind::Name => 3,
        ErrKind::Na => 4,
        ErrKind::Null => 5,
        ErrKind::Num => 6,
        ErrKind::Spill => 7,
        ErrKind::Calc => 8,
    }
}

fn err_from_code(b: u8) -> Option<ErrKind> {
    Some(match b {
        0 => ErrKind::Ref,
        1 => ErrKind::Div0,
        2 => ErrKind::Value,
        3 => ErrKind::Name,
        4 => ErrKind::Na,
        5 => ErrKind::Null,
        6 => ErrKind::Num,
        7 => ErrKind::Spill,
        8 => ErrKind::Calc,
        _ => return None,
    })
}
