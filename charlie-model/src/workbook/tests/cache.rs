// Concern: the ENG4 persistent-cache + FS3 FITNESS pins — over a REAL temp-dir workbook loaded through `Workbook::load_dir` (the only path that attaches a `.cache/`): (a) SOUNDNESS, a `.cache`-deleted run yields byte-identical values to a warm-cache run across a multi-formula workbook incl. a GRID5 region; (b) REUSE, a warm re-run performs materially FEWER formula evals (the test-visible `eval_count` instrument, not just matching values); (c) INVALIDATION, editing an upstream cell makes the dependent reflect the edit (new hash) while an unrelated cached cell still HITS; (d) CYCLE, a cyclic cell is never written to `.cache/` (only the cacheable sibling is); (e) `--no-cache` (`disable_cache`) neither reads nor writes `.cache/`, with identical values; (f) FS3, a workbook with `.cache/` present loads with the correct tab set; (g) DEPTH SOUNDNESS, a warm run of a deep chain cannot short-circuit a pull-depth `#NUM!` refusal a cache-deleted run raises (a mid-chain cell cached clean when demanded shallowly is NOT served into a deeper demand — warm equals cold at the depth boundary); (h) VOLATILE, a TODAY/NOW cone is never written to `.cache/` and recomputes against the pinned clock (never served a stale instant) while a non-volatile sibling still HITS; (i) SERVE-AT-VARYING-DEPTH, a diamond/multi-root batch where a mid cell cached clean when demanded SHALLOW is cache-served at depth 0 and then reached DEEP through a long chain (the deep root dedups onto the served leaf) yields warm==cold values AND eval counts — locking the ENG4 soundness invariant (a served cell's cone is wholly in-bound, so serving prunes no `#NUM!` terminal) against a future reordering of the graph-dedup vs cache_serve steps | Non-concern: the cache codec internals (`workbook::cache` owns encode/decode + atomicity) and the in-memory behavioral pins (the parent `tests` module owns those) | IO: temp-dir workbook trees on disk -> asserted `Value`s / eval counts / on-disk `.cache/` state
use std::fs;
use std::path::{Path, PathBuf};

use charlie_ast::{ErrKind, Value};

use crate::workbook::Workbook;

/// A fresh, unique temp directory for one test's workbook (tests run in parallel, so each owns its
/// own root and thus its own `.cache/`).
fn temp_base(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "charlie-eng7-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ))
}

/// Lay out `(tab, filename, body)` triples as a workbook directory tree under `base`.
fn write_files(base: &Path, files: &[(&str, &str, &str)]) {
    for (tab, name, body) in files {
        let dir = base.join(tab);
        fs::create_dir_all(&dir).expect("create tab dir");
        fs::write(dir.join(name), body).expect("write cell file");
    }
}

/// Load a workbook from disk (cache ON — `load_dir` attaches `<base>/.cache/`).
fn load(base: &Path) -> Workbook {
    Workbook::load_dir(base)
        .expect("fs read ok")
        .expect("loads clean")
}

/// Every value of a tab's used region, row-major — one demand-driven pass (the render surface).
fn tab_values(wb: &Workbook, sheet: u32) -> Vec<Value> {
    let r = wb.used_region(sheet).expect("a non-empty tab");
    let coords: Vec<(u32, u32, u32)> = (r.min_row..=r.max_row)
        .flat_map(|row| (r.min_col..=r.max_col).map(move |col| (sheet, col, row)))
        .collect();
    wb.values_at(&coords)
}

/// Count the hash-named entries in `<base>/.cache/` (the persisted result files), ignoring any
/// in-flight `.tmp-*` temp files. `0` when the directory does not exist.
fn cache_entry_count(base: &Path) -> usize {
    let dir = base.join(".cache");
    match fs::read_dir(&dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|e| !e.file_name().to_string_lossy().starts_with(".tmp-"))
            .count(),
        Err(_) => 0,
    }
}

/// A multi-formula workbook including a GRID5 array-formula region (`D1:D5 = A1:A5`) and cross-file
/// aggregations — the soundness fixture.
fn multi_formula_files() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("Grid", "A1:A5", "10\n20\n30\n40\n50"),
        ("Grid", "B1:B5", "=A1*2\n=A2*2\n=A3*2\n=A4*2\n=A5*2"),
        ("Grid", "C1", "=SUM(A1:A5)"),
        ("Grid", "C2", "=SUM(B1:B5)"),
        ("Grid", "D1:D5", "=A1:A5"), // GRID5 region: the range value fills the 5x1 range
        ("Grid", "E1", "=SUM(D1:D5)"),
    ]
}

#[test]
fn a_soundness_cache_deleted_equals_warm_cache_across_a_grid5_workbook() {
    let base = temp_base("sound");
    write_files(&base, &multi_formula_files());

    // Run 1 (cold): populates the cache. Run 2 (warm): reads it. Run 3: after DELETING `.cache/`.
    let cold = tab_values(&load(&base), 0);
    let warm = tab_values(&load(&base), 0);
    assert!(
        cache_entry_count(&base) > 0,
        "the cold run must populate .cache/"
    );
    fs::remove_dir_all(base.join(".cache")).expect("delete .cache");
    let recomputed = tab_values(&load(&base), 0);

    // VAL2/ENG4: deleting `.cache/` changes only performance, never values — byte-identical (Value
    // equality is bit-exact) across the cold, warm, and cache-deleted runs.
    assert_eq!(cold, warm, "warm-cache values must equal the cold run");
    assert_eq!(
        cold, recomputed,
        "a cache-deleted run must reproduce the values exactly"
    );
    // Spot-check a couple of concrete values so the vectors are not vacuously equal.
    assert_eq!(load(&base).value_at(0, 4, 0), Value::Number(150.0)); // E1 = SUM(D1:D5)
    assert_eq!(load(&base).value_at(0, 2, 1), Value::Number(300.0)); // C2 = SUM(B1:B5)

    fs::remove_dir_all(&base).ok();
}

#[test]
fn b_reuse_a_warm_run_performs_materially_fewer_evals() {
    let base = temp_base("reuse");
    write_files(
        &base,
        &[
            ("S", "A1:A4", "1\n2\n3\n4"),
            ("S", "B1:B4", "=A1+1\n=A2+1\n=A3+1\n=A4+1"),
            ("S", "C1", "=SUM(B1:B4)"),
        ],
    );

    // Cold run: every formula cell (B1..B4, C1 = 5) is actually evaluated.
    let wb1 = load(&base);
    let cold_vals = tab_values(&wb1, 0);
    let cold_evals = wb1.eval_count();
    assert_eq!(cold_evals, 5, "cold run evaluates all five formula cells");

    // Warm run (a FRESH workbook reading the on-disk cache): a cached subtree is served without
    // evaluating — ZERO formula evals, proving reuse actually happens (values matching is not proof).
    let wb2 = load(&base);
    let warm_vals = tab_values(&wb2, 0);
    let warm_evals = wb2.eval_count();
    assert_eq!(warm_vals, cold_vals, "warm values must match cold values");
    assert!(
        warm_evals < cold_evals,
        "warm run must perform materially fewer evals ({warm_evals} vs {cold_evals})"
    );
    assert_eq!(warm_evals, 0, "a fully cached render evaluates no formula");

    fs::remove_dir_all(&base).ok();
}

#[test]
fn c_invalidation_edit_reflects_downstream_while_unrelated_stays_cached() {
    let base = temp_base("inval");
    // B1 depends on A1; X1 is independent of A1.
    write_files(
        &base,
        &[("S", "A1", "5"), ("S", "B1", "=A1*10"), ("S", "X1", "=7+7")],
    );

    // Warm the cache.
    let wb1 = load(&base);
    assert_eq!(wb1.value_at(0, 1, 0), Value::Number(50.0)); // B1 = A1*10
    assert_eq!(wb1.value_at(0, 23, 0), Value::Number(14.0)); // X1 = 7+7

    // Edit the upstream literal A1 (5 -> 6) on disk.
    fs::write(base.join("S").join("A1"), "6").expect("edit A1");

    // A FRESH load: the unrelated X1's content cone is unchanged, so its hash HITS the cache — served
    // with no eval.
    let wb2 = load(&base);
    assert_eq!(wb2.value_at(0, 23, 0), Value::Number(14.0)); // X1 still correct
    assert_eq!(
        wb2.eval_count(),
        0,
        "the unrelated cached cell is served, not recomputed"
    );

    // B1's upstream content changed -> new computation hash -> the stale `50` entry is never looked up;
    // B1 recomputes and reflects the edit.
    assert_eq!(wb2.value_at(0, 1, 0), Value::Number(60.0)); // B1 = 6*10, the edit is reflected
    assert_eq!(
        wb2.eval_count(),
        1,
        "only B1 recomputed; X1 came from the cache"
    );

    fs::remove_dir_all(&base).ok();
}

#[test]
fn d_a_cyclic_cell_is_never_written_to_the_cache() {
    let base = temp_base("cycle");
    // A1<->B1 is a reference cycle; C1 is an ordinary cacheable formula in the same tab.
    write_files(
        &base,
        &[("S", "A1", "=B1"), ("S", "B1", "=A1"), ("S", "C1", "=1+1")],
    );

    let wb = load(&base);
    // The cycle is a located #REF! at every cyclic cell (ENG2), never a hang.
    assert_eq!(wb.value_at(0, 0, 0), Value::Error(ErrKind::Ref)); // A1
    assert_eq!(wb.value_at(0, 1, 0), Value::Error(ErrKind::Ref)); // B1
    assert_eq!(wb.value_at(0, 2, 0), Value::Number(2.0)); // C1

    // ENG4: a cell with no computation hash (a reference cycle) is NEVER cached. Exactly ONE entry is
    // written — C1's — so both cyclic cells were skipped.
    assert_eq!(
        cache_entry_count(&base),
        1,
        "only the acyclic C1 is cached; the cyclic A1/B1 are never written"
    );

    fs::remove_dir_all(&base).ok();
}

#[test]
fn e_no_cache_neither_reads_nor_writes() {
    let base = temp_base("nocache");
    write_files(
        &base,
        &[("S", "A1:A3", "1\n2\n3"), ("S", "B1", "=SUM(A1:A3)")],
    );

    // A `--no-cache` run on a cold workbook: identical values, and NOTHING written to `.cache/`.
    let mut wb_off = load(&base);
    wb_off.disable_cache();
    let off_vals = tab_values(&wb_off, 0);
    let off_evals = wb_off.eval_count();
    assert!(
        !base.join(".cache").exists(),
        "--no-cache must not write .cache/"
    );

    // Now warm the cache with a NORMAL run, then a fresh `--no-cache` run: it must NOT read the cache,
    // so it fully recomputes (same eval count as the cold run) and yields identical values.
    let warm = load(&base);
    let warm_vals = tab_values(&warm, 0);
    assert!(
        cache_entry_count(&base) > 0,
        "the normal run populates the cache"
    );

    let mut wb_off2 = load(&base);
    wb_off2.disable_cache();
    let off2_vals = tab_values(&wb_off2, 0);
    assert_eq!(off_vals, warm_vals);
    assert_eq!(off2_vals, warm_vals, "--no-cache values are identical");
    assert_eq!(
        wb_off2.eval_count(),
        off_evals,
        "--no-cache ignores the warm cache and recomputes everything"
    );

    fs::remove_dir_all(&base).ok();
}

#[test]
fn f_a_workbook_with_a_cache_dir_loads_with_the_correct_tab_set() {
    let base = temp_base("fs3");
    write_files(&base, &[("Data", "A1", "=1+1"), ("More", "A1", "42")]);
    // A pre-existing `.cache/` with a junk entry, as a real cache would have.
    let cache_dir = base.join(".cache");
    fs::create_dir_all(&cache_dir).unwrap();
    fs::write(cache_dir.join("deadbeefcafef00d"), b"\x00junk").unwrap();

    let wb = load(&base);
    // FS3: `.cache/` is NOT counted as a tab; every other sub-folder is (FS1). The tab set is exactly
    // the two real tabs, and the junk cache file is never mistaken for a range file.
    assert_eq!(wb.sheet_names(), vec!["Data", "More"]);
    assert_eq!(wb.value_at(0, 0, 0), Value::Number(2.0)); // Data!A1
    assert_eq!(wb.value_at(1, 0, 0), Value::Number(42.0)); // More!A1

    fs::remove_dir_all(&base).ok();
}

/// A single-column dependency chain `A1=A2+1, A2=A3+1, ..., A(n-1)=A(n)+1, A(n)=0` as ONE range file
/// (row-major body). `A_k` computes to `n-k` when demanded within the pull-depth budget; demanded past
/// [`super::super::MAX_PULL_DEPTH`] links deep it is a depth-tainted `#NUM!`. `n` is chosen well above
/// the bound so `A1`'s cone overruns it while a mid-chain cell's own cone fits.
fn chain_body(n: usize) -> String {
    let mut lines: Vec<String> = (1..n).map(|k| format!("=A{}+1", k + 1)).collect();
    lines.push("0".to_string()); // the last cell is a literal, bottoming the chain out (VAL2)
    lines.join("\n")
}

#[test]
fn g_a_warm_run_cannot_short_circuit_a_pull_depth_refusal() {
    // A 400-cell chain: A1's cone (400 deep) overruns the 256 pull-depth bound, but A200's own cone
    // (201 deep) fits, so A200 is cacheable-clean when demanded as a shallow root.
    let base = temp_base("depth");
    let body = chain_body(400);
    write_files(&base, &[("S", "A1:A400", &body)]);

    // Run 1: demand the MID cell A200 (row index 199) as a shallow root — its cone fits the depth
    // budget, so it computes clean (= 400 - 200) and is written to `.cache/`.
    let run1 = load(&base);
    assert_eq!(run1.value_at(0, 0, 199), Value::Number(200.0));
    assert!(
        cache_entry_count(&base) > 0,
        "the mid-chain cell must populate .cache/"
    );

    // Run 2 (WARM, fresh workbook): demand A1. Its cold cone runs A1..A257, hitting the pull-depth
    // bound at A257 -> a located `#NUM!`. The warm cache holds A200's clean `200`; the fix must NOT
    // serve it into A1's deeper demand (A200 sits 199 links down, and A200's own cone then adds 201
    // more -> well past the bound), which would have made A1 = `200 + 199 = 399` instead of `#NUM!`.
    let warm = load(&base).value_at(0, 0, 0);

    // Run 3 (COLD): delete `.cache/` and demand A1 from scratch.
    fs::remove_dir_all(base.join(".cache")).expect("delete .cache");
    let cold = load(&base).value_at(0, 0, 0);

    // ENG4 fitness: a reused result equals recomputation from scratch. Warm MUST equal cold.
    assert_eq!(
        warm, cold,
        "a warm run must not short-circuit the depth refusal a cache-deleted run raises"
    );
    // Pin the concrete depth-boundary semantics both runs share (documents the bound, not just parity).
    assert_eq!(
        cold,
        Value::Error(ErrKind::Num),
        "A1's cone overruns the pull-depth bound -> a located #NUM! (warm and cold alike)"
    );

    fs::remove_dir_all(&base).ok();
}

#[test]
fn h_a_volatile_cone_recomputes_against_the_clock_and_is_never_cached() {
    // A1 = NOW() (a volatile cone); B1 = 1+1 (an ordinary cacheable formula, no volatile).
    let base = temp_base("volatile");
    write_files(&base, &[("S", "A1", "=NOW()"), ("S", "B1", "=1+1")]);

    let t1 = 45000.25_f64; // the pinned "now" for the warming run
    let t2 = 45123.75_f64; // a LATER instant for the second run

    // Warm the cache at clock t1. A1 reads the clock; B1 does not.
    let wb1 = load(&base).with_now(t1);
    assert_eq!(wb1.value_at(0, 0, 0), Value::Number(t1)); // A1 = NOW() = t1
    assert_eq!(wb1.value_at(0, 1, 0), Value::Number(2.0)); // B1 = 1+1
    // ENG4 soundness: a volatile cone is NEVER written to `.cache/` (the hash does not fold the clock,
    // so a cached instant could not equal a fresh recomputation). Exactly ONE entry exists — B1's.
    assert_eq!(
        cache_entry_count(&base),
        1,
        "only the non-volatile B1 is cached; the volatile A1 is never written"
    );

    // A fresh run at the LATER clock t2. B1's content cone is unchanged -> it HITS the cache (no eval).
    // A1 is volatile -> it must RECOMPUTE against t2, never serve the stale t1.
    let wb2 = load(&base).with_now(t2);
    assert_eq!(wb2.value_at(0, 1, 0), Value::Number(2.0)); // B1 served from cache
    assert_eq!(
        wb2.eval_count(),
        0,
        "the non-volatile B1 is served, not recomputed"
    );
    assert_eq!(
        wb2.value_at(0, 0, 0),
        Value::Number(t2),
        "the volatile A1 recomputes against the new clock, never a stale cached instant"
    );
    assert_eq!(wb2.eval_count(), 1, "only the volatile A1 recomputed");
    // Still no volatile entry written on the second run either.
    assert_eq!(
        cache_entry_count(&base),
        1,
        "A1 is never written across runs"
    );

    fs::remove_dir_all(&base).ok();
}

#[test]
fn a_forging_cone_is_never_cached_but_its_non_forger_sibling_is() {
    // ENG4 (ENG6 forging): a forger cone (INDIRECT/OFFSET) is VOLATILE — its resolved target depends on
    // runtime values the computation hash does not fold — so it is never written to `.cache/`, while an
    // ordinary sibling still caches. B1 = SUM(OFFSET($A$1,0,0,3,1)) forges; C1 = 1+1 does not.
    let base = temp_base("forge");
    write_files(
        &base,
        &[
            ("S", "A1", "1"),
            ("S", "A2", "2"),
            ("S", "A3", "3"),
            ("S", "B1", "=SUM(OFFSET($A$1,0,0,3,1))"),
            ("S", "C1", "=1+1"),
        ],
    );

    // Warm run: B1 forges to SUM($A$1:$A$3) = 6; C1 = 2. Only C1 (the non-forger) is written.
    let wb1 = load(&base);
    assert_eq!(wb1.value_at(0, 1, 0), Value::Number(6.0)); // B1
    assert_eq!(wb1.value_at(0, 2, 0), Value::Number(2.0)); // C1
    assert_eq!(
        cache_entry_count(&base),
        1,
        "only the non-forger C1 is cached; the forging B1 cone is never written"
    );

    // Second run: C1 is served (no eval), B1 recomputes (its cone is uncacheable) to the same value.
    let wb2 = load(&base);
    assert_eq!(wb2.value_at(0, 2, 0), Value::Number(2.0)); // C1 served
    assert_eq!(
        wb2.eval_count(),
        0,
        "the non-forger C1 is served, not recomputed"
    );
    assert_eq!(wb2.value_at(0, 1, 0), Value::Number(6.0)); // B1 recomputes to the same value
    assert!(
        wb2.eval_count() > 0,
        "the forging B1 recomputes (its cone is never served)"
    );
    assert_eq!(
        cache_entry_count(&base),
        1,
        "B1 is never written across runs"
    );

    fs::remove_dir_all(&base).ok();
}

#[test]
fn i_a_served_mid_cell_is_reused_at_varying_depth_in_one_batch() {
    // Test (g) uses a pure linear chain in which EVERY cell is depth-tainted, so no cell is ever
    // cache-served across the depth boundary — it exercises only the fresh-walk refusal. This locks the
    // subtler DIAMOND interaction: a mid cell cached clean when demanded SHALLOW enters the shared
    // memo (served at depth 0), then the SAME cell is reached DEEP through the long chain in the same
    // batch, where the deep root dedups onto the served leaf. The soundness invariant (ENG4/ENG3): a
    // served cell's cone is wholly in-bound, so serving it prunes no `#NUM!` terminal a cold run would
    // raise — warm equals cold whatever the batch composition, and the batch's SHARED-node value (the
    // deep root is computable BECAUSE the shallow root makes the mid cell a clean shared node) is
    // identical warm and cold.
    let base = temp_base("diamond");
    let body = chain_body(400);
    write_files(&base, &[("S", "A1:A400", &body)]);

    // Run 1: demand the MID cell A200 (row 199) ALONE — its cone (201 deep) fits the pull-depth budget
    // from depth 0, so A200..A399 compute clean and A200's ROOTED (depth-0) hash is written to `.cache/`.
    let run1 = load(&base);
    assert_eq!(run1.value_at(0, 0, 199), Value::Number(200.0));
    assert!(
        cache_entry_count(&base) > 0,
        "the mid cell must populate .cache/"
    );

    // The batch: the shallow root A200 (row 199) THEN the deep root A1 (row 0), in ONE `values_at` pass.
    let batch = [(0u32, 0u32, 199u32), (0, 0, 0)];

    // Run 2 (WARM, fresh wb): A200 is cache-SERVED at depth 0 (the shallow root) into the shared memo,
    // then A1's deep descent reaches A200 at depth 199 and dedups onto that served leaf — so A1's cone
    // stops at A200 and A1 = 200 + 199 = 399 (never a depth `#NUM!`, because the shallow demand made
    // A200 a clean shared node). Only A1..A199 (199 formulas) evaluate; A200's whole cone was served.
    let wb2 = load(&base);
    let warm = wb2.values_at(&batch);
    let warm_evals = wb2.eval_count();

    // Run 3 (COLD, fresh wb, `.cache/` deleted): the SAME batch. A200 (shallow root) computes clean and
    // becomes a shared node; A1's deep descent dedups onto it exactly as warm did — so the values match
    // and A1 is 399 here too (the depth refusal a LONE deep root sees in (g) does not arise once a
    // shallow sibling makes the mid cell a clean shared node). All 399 chain formulas evaluate.
    fs::remove_dir_all(base.join(".cache")).expect("delete .cache");
    let wb3 = load(&base);
    let cold = wb3.values_at(&batch);
    let cold_evals = wb3.eval_count();

    // ENG4 soundness: a reused result equals recomputation from scratch — warm equals cold for BOTH the
    // served mid cell and the deep root that reaches it, across the varying serve depth.
    assert_eq!(warm, cold, "warm batch must equal the cache-deleted batch");
    assert_eq!(
        cold,
        vec![Value::Number(200.0), Value::Number(399.0)],
        "A200 = 200 (shallow), and A1 = A200 + 199 = 399 (the shared-node value, not a depth #NUM!)"
    );

    // REUSE actually happened: the warm batch served A200's entire cone (A200..A399, 200 formulas) and
    // evaluated only A1..A199, materially fewer than the cold batch's full 399 chain evals.
    assert!(
        warm_evals < cold_evals,
        "warm must eval materially fewer ({warm_evals} vs {cold_evals})"
    );
    assert_eq!(
        warm_evals, 199,
        "warm evaluates only A1..A199; A200's cone is served"
    );
    assert_eq!(
        cold_evals, 399,
        "cold evaluates every chain formula A1..A399"
    );

    fs::remove_dir_all(&base).ok();
}
