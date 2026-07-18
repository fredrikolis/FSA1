// Concern: the LOOKUP & REFERENCE worksheet functions (XLOOKUP INDEX MATCH VLOOKUP HLOOKUP LOOKUP XMATCH CHOOSE ROW COLUMN ROWS COLUMNS, + the reserved INDIRECT/OFFSET refusal) — the search family agreeing on ONE cross-type ordering (`eval::value_cmp`) and ONE wildcard engine (`criteria::wildcard_match`), with a guaranteed-terminating approximate `binary_search_approx`, XLOOKUP/XMATCH's modern exact-by-default, and the shape queries ROWS/COLUMNS | Non-concern: the registry table + dispatch (func/mod.rs), the ordering/wildcard primitives (eval.rs owns `value_cmp`, criteria.rs owns `wildcard_match`), and the shared `block` helper (func/helpers.rs) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
use super::*;

// Lookup & reference batch v1: XLOOKUP INDEX MATCH VLOOKUP CHOOSE ROW COLUMN (+ reserved INDIRECT /
// OFFSET). These are the "hard semantics" scope.md item 4 concentrates: a family that must agree on
// ONE notion of "does this cell equal / order against this needle", and get the classic approximate-
// match bug RIGHT.
//
// CROSS-TYPE ORDERING (the shared spine): every APPROXIMATE search reuses the engine's own
// `eval::value_cmp` — the identical total order the comparison operators read (numbers numerically,
// text case-insensitively, cross-type ranked Number < Text < Bool, a lone Blank against the other
// side's zero). So a lookup's ordering can NEVER drift from `A1<B1`'s ordering. Every EXACT search
// with a text needle reuses the ONE wildcard engine (`criteria::wildcard_match`: `*`/`?`/`~`,
// case-folded) — MATCH mode 0 and XLOOKUP match_mode 2 do wildcards; XLOOKUP match_mode 0 does not.
//
// APPROXIMATE MATCH — the classic must-be-SORTED bug, gotten right (VLOOKUP/MATCH):
//   * VLOOKUP defaults to APPROXIMATE (4th arg omitted or TRUE) and MATCH defaults to match_type 1;
//     both ASSUME the search vector is sorted ASCENDING and return the position of the LARGEST value
//     that is <= the needle (the "next-smaller" rule — a needle between two keys lands on the lower
//     key, not #N/A). MATCH match_type -1 assumes DESCENDING and returns the smallest value >= needle.
//   * The sorted-range assumption is honored via a `binary_search_approx` that is GUARANTEED to
//     terminate: each step strictly shrinks `[lo, hi]` (either `lo` rises past `mid` or `hi` falls
//     below it, with the `mid == 0` underflow guarded), so it halts in <= ceil(log2 n)+1 steps for
//     ANY data — sorted or not (unsorted merely yields Excel's documented "undefined" position, never
//     a hang or a panic). Exact match (VLOOKUP FALSE / MATCH 0) is a linear FIRST-hit scan instead.
//
// XLOOKUP: exact BY DEFAULT (match_mode 0 — the modern fix to VLOOKUP's dangerous approximate
// default), plus an explicit `if_not_found` value (returned in place of #N/A, evaluated lazily only
// on a miss), match_mode -1/0/1/2 (exact / exact-or-next-smaller / exact-or-next-larger / wildcard),
// and a forward/reverse `search_mode` (>= 0 first-to-last, < 0 last-to-first — the binary-search
// modes 2/-2 collapse to the equivalent directional linear scan, identical on the sorted data they
// assume). The approximate modes scan LINEARLY for the closest key (XLOOKUP, unlike VLOOKUP, does not
// require sorted data), first in the search direction on a tie.
//
// INDEX(array, r, c): 1-based; `r == 0` selects the WHOLE column `c`, `c == 0` the WHOLE row `r`
// (both 0 → the whole array); a single-cell pick of a blank yields 0 (Excel). A row/col index past
// the array bound is #REF!; a negative index is #VALUE!. The 2-arg form indexes the sole dimension of
// a 1-D array, or (on a 2-D array) selects the whole `r`-th row.
//
// ROW/COLUMN read the STATIC coordinate of a reference NODE (never its value) — 1-based, top-left of
// a range. The no-argument current-cell form is deferred (the evaluator has no current-cell seam), so
// v1 requires the reference argument (arity 1); a non-reference argument is #VALUE!.
//
// INDIRECT / OFFSET are reference-returning + dynamic-dependency-forging: they have no v1 node, so
// they are REGISTERED (recognized names) but their `validate` seam ALWAYS refuses at parse with the
// located `reserved-ref-function` DiagCode — never a wrong guess, never the generic unknown-function
// path. `reserved_ref_eval` exists only to keep the row total (eval is unreachable via a parsed tree).
/// Evaluate a lookup argument to a rectangular block `(rows, cols, cells)` — the one materialization
/// the lookup family shares (a range/array as itself, a scalar as a `1×1` block, an error propagated).
/// Identical in spirit to the criteria family's [`block`]; kept a separate call so a future divergence
/// (e.g. lookup accepting a lone scalar Excel would reject) stays local.
fn lookup_block(ctx: &mut EvalCtx, e: &Expr) -> Result<(u32, u32, Vec<Value>), ErrKind> {
    block(ctx, e)
}

/// Coerce a lookup index argument (INDEX's row/col, CHOOSE's selector) to a non-negative 1-based (or
/// 0-sentinel) integer, truncating toward zero like Excel. An error propagates; a non-coercible value
/// is `#VALUE!`; a NEGATIVE index is `#VALUE!` (Excel). Returns the truncated magnitude as `u32`.
fn index_arg(ctx: &mut EvalCtx, e: &Expr) -> Result<u32, ErrKind> {
    let n = coerce_num(&scalarize(ctx.eval(e)))?;
    let t = n.trunc();
    if t < 0.0 {
        return Err(ErrKind::Value);
    }
    // `t` is finite (coerce_num guarantees it) and >= 0; clamp defensively so an absurd magnitude
    // becomes an out-of-bounds #REF! at the caller rather than overflowing the cast.
    Ok(t.min(u32::MAX as f64) as u32)
}

/// The value INDEX/VLOOKUP/XLOOKUP hand back for a single matched cell: a blank cell reads as `0`
/// (Excel returns 0, not empty, for an INDEX/lookup hit on a blank), everything else passes through.
fn matched_scalar(v: &Value) -> Value {
    match v {
        Value::Blank => Value::Number(0.0),
        other => other.clone(),
    }
}

/// Whether `cell` is an EXACT match for `needle`. With `wildcard`, a TEXT needle matches a TEXT cell
/// by the shared Excel wildcard grammar (`*`/`?`/`~`, case-folded); otherwise (and for any non-text
/// needle) equality is the engine's own `value_cmp == Equal` (numbers numerically, text case-
/// insensitively, cross-type never equal) — so exact match agrees with `=`.
fn cell_matches_exact(needle: &Value, cell: &Value, wildcard: bool) -> bool {
    if wildcard && let Value::Text(pat) = needle {
        return matches!(cell, Value::Text(s) if wildcard_match(pat, s));
    }
    value_cmp(needle, cell) == std::cmp::Ordering::Equal
}

/// Binary search for an APPROXIMATE match over a vector ASSUMED sorted. `ascending` picks the rule:
/// ascending → the position of the LARGEST cell `<= needle`; descending → the SMALLEST cell `>= needle`.
/// Returns `None` when no cell satisfies the rule (needle below every ascending key / above every
/// descending key → the caller's `#N/A`). GUARANTEED to terminate: `[lo, hi]` strictly shrinks every
/// iteration (the `mid == 0` case breaks rather than underflowing `usize`), so it halts for ANY input.
fn binary_search_approx(col: &[Value], needle: &Value, ascending: bool) -> Option<usize> {
    use std::cmp::Ordering;
    if col.is_empty() {
        return None;
    }
    let mut lo = 0usize;
    let mut hi = col.len() - 1;
    let mut found: Option<usize> = None;
    loop {
        let mid = lo + (hi - lo) / 2;
        let ord = value_cmp(&col[mid], needle);
        // "Acceptable" = on the correct side of (or equal to) the needle for this direction.
        let acceptable = if ascending {
            ord != Ordering::Greater // col[mid] <= needle
        } else {
            ord != Ordering::Less // col[mid] >= needle
        };
        if acceptable {
            // A candidate; the best one lies at or after `mid` (ascending) — keep searching right.
            found = Some(mid);
            lo = mid + 1;
        } else if mid == 0 {
            break;
        } else {
            hi = mid - 1;
        }
        if lo > hi {
            break;
        }
    }
    found
}

/// Linear FIRST-hit exact search (VLOOKUP FALSE / MATCH 0): the lowest index whose cell exactly
/// matches `needle` (wildcards honored per `wildcard`), or `None`.
fn linear_exact(col: &[Value], needle: &Value, wildcard: bool) -> Option<usize> {
    col.iter()
        .position(|c| cell_matches_exact(needle, c, wildcard))
}

/// `MATCH(lookup_value, lookup_array, [match_type])` — the position (1-based) of `lookup_value` in a
/// 1-D `lookup_array`. `match_type` 1 (default) approximates ASCENDING (largest value `<=` needle),
/// -1 approximates DESCENDING (smallest `>=` needle), 0 is an exact first-hit with wildcards on a text
/// needle. A 2-D array, or no match, is `#N/A`; an error needle/array propagates.
pub(crate) fn match_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let needle = scalarize(ctx.eval(&args[0]));
    if let Value::Error(k) = needle {
        return Value::Error(k);
    }
    let (rows, cols, cells) = match lookup_block(ctx, &args[1]) {
        Err(k) => return Value::Error(k),
        Ok(t) => t,
    };
    // MATCH wants a vector: exactly one of the dimensions must be 1 (a lone scalar 1×1 qualifies).
    if rows > 1 && cols > 1 {
        return Value::Error(ErrKind::Na);
    }
    let match_type = match args.get(2) {
        None => 1,
        Some(e) => match coerce_num(&scalarize(ctx.eval(e))) {
            Err(k) => return Value::Error(k),
            Ok(n) => n.trunc() as i64,
        },
    };
    let pos = if match_type == 0 {
        linear_exact(&cells, &needle, true)
    } else {
        binary_search_approx(&cells, &needle, match_type > 0)
    };
    match pos {
        Some(i) => Value::Number((i + 1) as f64),
        None => Value::Error(ErrKind::Na),
    }
}

/// `INDEX(array, row_num, [col_num])` — the value at a 1-based `(row_num, col_num)` of `array`. A `0`
/// row (or col) selects the WHOLE column (or row); both `0` → the whole array. An out-of-bounds index
/// is `#REF!`; a negative index is `#VALUE!` (via [`index_arg`]); a blank single cell reads as `0`.
pub(crate) fn index_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (rows, cols, cells) = match lookup_block(ctx, &args[0]) {
        Err(k) => return Value::Error(k),
        Ok(t) => t,
    };
    let r = match index_arg(ctx, &args[1]) {
        Err(k) => return Value::Error(k),
        Ok(v) => v,
    };
    // Resolve which (row-selector, col-selector) the argument shape implies. A selector of `None`
    // means "the whole span" of that dimension (an explicit 0, or an omitted col on a 2-D array).
    let (row_sel, col_sel): (Option<u32>, Option<u32>) = if let Some(ce) = args.get(2) {
        let c = match index_arg(ctx, ce) {
            Err(k) => return Value::Error(k),
            Ok(v) => v,
        };
        (sel(r), sel(c))
    } else if rows == 1 {
        // A single row: the sole index is the COLUMN.
        (Some(1), sel(r))
    } else if cols == 1 {
        // A single column: the sole index is the ROW.
        (sel(r), Some(1))
    } else {
        // A 2-D array with the column omitted: select the whole `r`-th row.
        (sel(r), None)
    };
    // Bounds: a specific index beyond the array is #REF!.
    if row_sel.is_some_and(|ri| ri > rows) || col_sel.is_some_and(|ci| ci > cols) {
        return Value::Error(ErrKind::Ref);
    }
    let at = |ri: u32, ci: u32| -> Value { cells[((ri - 1) * cols + (ci - 1)) as usize].clone() };
    match (row_sel, col_sel) {
        (Some(ri), Some(ci)) => matched_scalar(&at(ri, ci)),
        (Some(ri), None) => {
            // The whole ri-th row → a 1×cols array.
            let cell_row: Vec<Value> = (1..=cols).map(|ci| at(ri, ci)).collect();
            Value::Array(Shape { rows: 1, cols }, cell_row)
        }
        (None, Some(ci)) => {
            // The whole ci-th column → a rows×1 array.
            let col: Vec<Value> = (1..=rows).map(|ri| at(ri, ci)).collect();
            Value::Array(Shape { rows, cols: 1 }, col)
        }
        (None, None) => Value::Array(Shape { rows, cols }, cells),
    }
}

/// Map a raw INDEX index to a selector: `0` → `None` (the whole span), `n` → `Some(n)`.
fn sel(n: u32) -> Option<u32> {
    (n != 0).then_some(n)
}

/// `VLOOKUP(lookup_value, table_array, col_index, [range_lookup])` — find `lookup_value` in the FIRST
/// column of `table_array`, return the `col_index`-th column's cell of that row. Approximate BY
/// DEFAULT (the classic must-be-sorted bug: omitted/`TRUE` 4th arg → the largest first-column value
/// `<= lookup_value` on a vector ASSUMED sorted ascending); `FALSE` → an exact first-hit (wildcards on
/// a text needle). `col_index < 1` is `#VALUE!`, `> width` is `#REF!`; no match is `#N/A`.
pub(crate) fn vlookup(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let needle = scalarize(ctx.eval(&args[0]));
    if let Value::Error(k) = needle {
        return Value::Error(k);
    }
    let (rows, cols, cells) = match lookup_block(ctx, &args[1]) {
        Err(k) => return Value::Error(k),
        Ok(t) => t,
    };
    let col_index = match coerce_num(&scalarize(ctx.eval(&args[2]))) {
        Err(k) => return Value::Error(k),
        Ok(n) => n.trunc(),
    };
    if col_index < 1.0 {
        return Value::Error(ErrKind::Value);
    }
    if col_index > cols as f64 {
        return Value::Error(ErrKind::Ref);
    }
    let approximate = match args.get(3) {
        None => true,
        Some(e) => match coerce_bool(&ctx.eval(e)) {
            Err(k) => return Value::Error(k),
            Ok(b) => b,
        },
    };
    // The first column, materialized as a vector for the search.
    let first_col: Vec<Value> = (0..rows)
        .map(|r| cells[(r * cols) as usize].clone())
        .collect();
    let hit = if approximate {
        binary_search_approx(&first_col, &needle, true)
    } else {
        linear_exact(&first_col, &needle, true)
    };
    match hit {
        None => Value::Error(ErrKind::Na),
        Some(r) => matched_scalar(&cells[(r as u32 * cols + (col_index as u32 - 1)) as usize]),
    }
}

/// `XLOOKUP(lookup, lookup_array, return_array, [if_not_found], [match_mode], [search_mode])` — exact
/// BY DEFAULT (`match_mode 0`). `match_mode` -1/1 accept the next-smaller/next-larger key, `2` does
/// wildcards; `search_mode` 1/2 scan first-to-last, -1/-2 last-to-first. A `match_mode` outside
/// {-1,0,1,2} or a `search_mode` outside {1,-1,2,-2} is `#VALUE!` (Excel rejects an out-of-domain
/// mode, never a lenient fallback). On a miss it returns `if_not_found` (evaluated only then) or
/// `#N/A`. `lookup_array` and `return_array` must be the same length (else `#VALUE!`).
pub(crate) fn xlookup(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let needle = scalarize(ctx.eval(&args[0]));
    if let Value::Error(k) = needle {
        return Value::Error(k);
    }
    let (_lr, _lc, lookup) = match lookup_block(ctx, &args[1]) {
        Err(k) => return Value::Error(k),
        Ok(t) => t,
    };
    let (_rr, _rc, ret) = match lookup_block(ctx, &args[2]) {
        Err(k) => return Value::Error(k),
        Ok(t) => t,
    };
    if lookup.len() != ret.len() || lookup.is_empty() {
        return Value::Error(ErrKind::Value);
    }
    let match_mode = match args.get(4) {
        None => 0,
        Some(e) => match coerce_num(&scalarize(ctx.eval(e))) {
            Err(k) => return Value::Error(k),
            Ok(n) => n.trunc() as i64,
        },
    };
    // Excel rejects a match_mode outside {-1, 0, 1, 2} with #VALUE! — NOT a lenient exact-only
    // fallback for an out-of-domain mode.
    if !matches!(match_mode, -1..=2) {
        return Value::Error(ErrKind::Value);
    }
    let search_mode = match args.get(5) {
        None => 1,
        Some(e) => match coerce_num(&scalarize(ctx.eval(e))) {
            Err(k) => return Value::Error(k),
            Ok(n) => n.trunc() as i64,
        },
    };
    // Excel rejects a search_mode outside {1, -1, 2, -2} with #VALUE!. The binary modes (±2) assume
    // sorted data and collapse to the equivalent directional linear scan here (identical on the
    // sorted input they assume); only the sign — forward vs reverse — drives the linear search.
    if !matches!(search_mode, 1 | -1 | 2 | -2) {
        return Value::Error(ErrKind::Value);
    }
    let reverse = search_mode < 0;
    match xlookup_find(&needle, &lookup, match_mode, reverse) {
        Some(i) => matched_scalar(&ret[i]),
        // On a miss: the explicit `if_not_found` (evaluated lazily, only now) or #N/A.
        None => match args.get(3) {
            Some(e) => ctx.eval(e),
            None => Value::Error(ErrKind::Na),
        },
    }
}

/// The XLOOKUP index search: an exact hit always wins (first in the search direction); failing that,
/// `match_mode 1` takes the smallest cell strictly GREATER than the needle and `-1` the largest cell
/// strictly LESS (ties broken by search direction), while `0`/`2` give no fallback. LINEAR (XLOOKUP
/// does not require sorted data); `match_mode 2` enables wildcards on a text needle.
fn xlookup_find(needle: &Value, cells: &[Value], match_mode: i64, reverse: bool) -> Option<usize> {
    use std::cmp::Ordering;
    let wildcard = match_mode == 2;
    let order: Vec<usize> = if reverse {
        (0..cells.len()).rev().collect()
    } else {
        (0..cells.len()).collect()
    };
    // 1) Exact match, first in the search direction.
    for &i in &order {
        if cell_matches_exact(needle, &cells[i], wildcard) {
            return Some(i);
        }
    }
    // 2) Approximate fallback for the ±1 modes.
    if match_mode != 1 && match_mode != -1 {
        return None;
    }
    let want_greater = match_mode == 1;
    let mut best: Option<usize> = None;
    for &i in &order {
        let c = &cells[i];
        let side = value_cmp(c, needle);
        let on_side = if want_greater {
            side == Ordering::Greater
        } else {
            side == Ordering::Less
        };
        if !on_side {
            continue;
        }
        best = match best {
            None => Some(i),
            Some(b) => {
                // Keep the CLOSEST to the needle: smallest of the greaters / largest of the lessers.
                let better = if want_greater {
                    value_cmp(c, &cells[b]) == Ordering::Less
                } else {
                    value_cmp(c, &cells[b]) == Ordering::Greater
                };
                // A strictly-closer candidate replaces; an equal-value candidate keeps the first seen
                // in the search direction (so `order` already encodes the tie-break).
                if better { Some(i) } else { Some(b) }
            }
        };
    }
    best
}

/// `CHOOSE(index, value1, value2, …)` — return the `index`-th value (1-based). Only the selected
/// argument is evaluated (lazy selection, as `IF` is in this engine); an out-of-range or non-positive
/// index is `#VALUE!`, and an error/non-coercible index propagates.
pub(crate) fn choose(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let idx = match coerce_num(&scalarize(ctx.eval(&args[0]))) {
        Err(k) => return Value::Error(k),
        Ok(n) => n.trunc(),
    };
    // Values are args[1..]; a valid index is 1..=(count).
    let count = args.len() - 1;
    if idx < 1.0 || idx > count as f64 {
        return Value::Error(ErrKind::Value);
    }
    ctx.eval(&args[idx as usize])
}

/// `ROW(reference)` — the 1-based row number of a reference (the top row of a range). Reads the
/// STATIC coordinate of the reference NODE, never its value, so it does not consult the resolver; a
/// non-reference argument is `#VALUE!`. (The no-arg current-cell form is deferred — the evaluator has
/// no current-cell seam — so v1's arity requires the reference.)
pub(crate) fn row_fn(_ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match &args[0] {
        Expr::Ref(r) => Value::Number((r.row + 1) as f64),
        Expr::Range(rn) => Value::Number((rn.start_row.min(rn.end_row) + 1) as f64),
        _ => Value::Error(ErrKind::Value),
    }
}

/// `COLUMN(reference)` — the 1-based column number of a reference (the left column of a range). The
/// column dual of [`row_fn`]; reads the reference NODE's static coordinate, never its value.
pub(crate) fn column_fn(_ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match &args[0] {
        Expr::Ref(r) => Value::Number((r.col + 1) as f64),
        Expr::Range(rn) => Value::Number((rn.start_col.min(rn.end_col) + 1) as f64),
        _ => Value::Error(ErrKind::Value),
    }
}

/// `HLOOKUP(lookup_value, table_array, row_index, [range_lookup])` — the HORIZONTAL dual of
/// [`vlookup`]: find `lookup_value` in the FIRST ROW of `table_array`, return the `row_index`-th row's
/// cell of that column. Approximate BY DEFAULT (omitted/`TRUE` → the largest first-row value `<=`
/// `lookup_value`, first row ASSUMED sorted ascending); `FALSE` → an exact first-hit (wildcards on a
/// text needle). `row_index < 1` is `#VALUE!`, `> height` is `#REF!`; no match is `#N/A`.
pub(crate) fn hlookup(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let needle = scalarize(ctx.eval(&args[0]));
    if let Value::Error(k) = needle {
        return Value::Error(k);
    }
    let (rows, cols, cells) = match lookup_block(ctx, &args[1]) {
        Err(k) => return Value::Error(k),
        Ok(t) => t,
    };
    let row_index = match coerce_num(&scalarize(ctx.eval(&args[2]))) {
        Err(k) => return Value::Error(k),
        Ok(n) => n.trunc(),
    };
    if row_index < 1.0 {
        return Value::Error(ErrKind::Value);
    }
    if row_index > rows as f64 {
        return Value::Error(ErrKind::Ref);
    }
    let approximate = match args.get(3) {
        None => true,
        Some(e) => match coerce_bool(&ctx.eval(e)) {
            Err(k) => return Value::Error(k),
            Ok(b) => b,
        },
    };
    // The first row, materialized as a vector for the search.
    let first_row: Vec<Value> = (0..cols).map(|c| cells[c as usize].clone()).collect();
    let hit = if approximate {
        binary_search_approx(&first_row, &needle, true)
    } else {
        linear_exact(&first_row, &needle, true)
    };
    match hit {
        None => Value::Error(ErrKind::Na),
        Some(c) => matched_scalar(&cells[((row_index as u32 - 1) * cols + c as u32) as usize]),
    }
}

/// `LOOKUP(lookup_value, lookup_vector, [result_vector])` — has TWO Excel syntaxes on one arity, and
/// this implements BOTH so a migrated `=LOOKUP(x, A1:C10)` never grades as a silent Diverge:
///   * VECTOR form (3-arg, OR 2-arg on a genuine 1-row/1-col vector): approximate-match `lookup_value`
///     in `lookup_vector` (ASSUMED sorted ascending — the largest value `<=` needle), then return the
///     value at the SAME position of `result_vector` (or of `lookup_vector` itself when omitted).
///   * ARRAY form (2-arg on a true 2-D array): Excel's aspect-ratio rule — a WIDER-than-tall array
///     searches its FIRST ROW and returns the aligned cell of the LAST ROW; a square-or-taller array
///     searches its FIRST COLUMN and returns the aligned cell of the LAST COLUMN. A 1×n / n×1 vector
///     reduces to exactly the vector form (its search line and result line coincide).
///
/// A needle below every key, or a position past a shorter `result_vector`, is `#N/A`.
pub(crate) fn lookup_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let needle = scalarize(ctx.eval(&args[0]));
    if let Value::Error(k) = needle {
        return Value::Error(k);
    }
    let (rows, cols, cells) = match lookup_block(ctx, &args[1]) {
        Err(k) => return Value::Error(k),
        Ok(t) => t,
    };
    // 2-arg → Excel's ARRAY form (which reduces to the vector form for a vector). 3-arg → the classic
    // vector form with an explicit result vector, searched on the flattened lookup vector.
    let Some(result_e) = args.get(2) else {
        return lookup_array_form(&needle, rows, cols, &cells);
    };
    let pos = match binary_search_approx(&cells, &needle, true) {
        None => return Value::Error(ErrKind::Na),
        Some(p) => p,
    };
    let (_rr, _rc, result_vec) = match lookup_block(ctx, result_e) {
        Err(k) => return Value::Error(k),
        Ok(t) => t,
    };
    match result_vec.get(pos) {
        Some(v) => matched_scalar(v),
        None => Value::Error(ErrKind::Na),
    }
}

/// The 2-arg ARRAY form of `LOOKUP` (Excel's aspect-ratio rule): search the FIRST ROW when the array
/// is wider than tall (more columns than rows) and return the aligned cell of the LAST ROW; otherwise
/// (square or taller) search the FIRST COLUMN and return the aligned cell of the LAST COLUMN. A 1-row
/// or 1-col vector reduces exactly to the classic vector form — its search line IS its result line, so
/// a matched key returns itself. A needle below every key is `#N/A`.
fn lookup_array_form(needle: &Value, rows: u32, cols: u32, cells: &[Value]) -> Value {
    let at = |r: u32, c: u32| cells[(r * cols + c) as usize].clone();
    let (search, result): (Vec<Value>, Vec<Value>) = if cols > rows {
        // Wider than tall: first row is the search line, last row the aligned result line (by column).
        let last = rows - 1;
        (
            (0..cols).map(|c| at(0, c)).collect(),
            (0..cols).map(|c| at(last, c)).collect(),
        )
    } else {
        // Square or taller: first column is the search line, last column the result line (by row).
        let last = cols - 1;
        (
            (0..rows).map(|r| at(r, 0)).collect(),
            (0..rows).map(|r| at(r, last)).collect(),
        )
    };
    match binary_search_approx(&search, needle, true) {
        None => Value::Error(ErrKind::Na),
        Some(pos) => matched_scalar(&result[pos]),
    }
}

/// `ROWS(range_or_array)` — the ROW COUNT of a reference or array (a bare scalar is `1`). Reads the
/// shape via [`lookup_block`]; an error argument propagates. (This is a shape-only query, but the
/// shared [`lookup_block`] materializes every cell to learn `(rows, cols)` — a minor cost on a large
/// range; a value-free shape seam on `EvalCtx` is a later optimization, not a v1 concern.)
pub(crate) fn rows_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match lookup_block(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok((rows, _, _)) => Value::Number(rows as f64),
    }
}

/// `COLUMNS(range_or_array)` — the COLUMN COUNT of a reference or array (the column dual of
/// [`rows_fn`]; a bare scalar is `1`). An error argument propagates.
pub(crate) fn columns_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match lookup_block(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok((_, cols, _)) => Value::Number(cols as f64),
    }
}

/// `XMATCH(lookup_value, lookup_array, [match_mode])` — the modern [`match_fn`]: return the 1-based
/// position of `lookup_value` in a 1-D `lookup_array`. `match_mode` `0` (default) is exact, `-1`
/// exact-or-next-SMALLER, `1` exact-or-next-LARGER, `2` exact with wildcards; any other mode is
/// `#VALUE!`. Unlike MATCH the approximate modes do NOT assume sorted data (a linear closest-scan,
/// shared with XLOOKUP via [`xlookup_find`]). A 2-D array, or no match, is `#N/A`.
///
/// DEFERRED (v1 gap): Excel's full signature has a 4th `[search_mode]` argument
/// (`XMATCH(lookup_value, lookup_array, [match_mode], [search_mode])`) for forward/reverse and binary
/// search order. v1 registers `max_args: Some(3)` and always searches first-to-last, so a
/// last-to-first tie-break (`search_mode -1`) is not yet expressible — it lands as a `BadArity`
/// refusal, never a silently-wrong position. XLOOKUP already carries the `search_mode` seam; XMATCH
/// gains it in the same batch that widens this row.
pub(crate) fn xmatch(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let needle = scalarize(ctx.eval(&args[0]));
    if let Value::Error(k) = needle {
        return Value::Error(k);
    }
    let (rows, cols, cells) = match lookup_block(ctx, &args[1]) {
        Err(k) => return Value::Error(k),
        Ok(t) => t,
    };
    // XMATCH wants a vector: exactly one dimension must be 1 (a lone scalar 1×1 qualifies).
    if rows > 1 && cols > 1 {
        return Value::Error(ErrKind::Na);
    }
    let match_mode = match args.get(2) {
        None => 0,
        Some(e) => match coerce_num(&scalarize(ctx.eval(e))) {
            Err(k) => return Value::Error(k),
            Ok(n) => n.trunc() as i64,
        },
    };
    // Excel rejects a match_mode outside {-1, 0, 1, 2} with #VALUE! (mirrors XLOOKUP).
    if !matches!(match_mode, -1..=2) {
        return Value::Error(ErrKind::Value);
    }
    match xlookup_find(&needle, &cells, match_mode, false) {
        Some(i) => Value::Number((i + 1) as f64),
        None => Value::Error(ErrKind::Na),
    }
}

/// The eval stub for the RESERVED reference-returning functions (`INDIRECT`/`OFFSET`). Unreachable via
/// a parsed tree — [`refuse_reserved_ref_function`] refuses the call at parse — but present so the
/// registry row is total and eval stays panic-free for a hand-synthesized `Call`. Returns `#REF!`.
pub(crate) fn reserved_ref_eval(_ctx: &mut EvalCtx, _args: &[Expr]) -> Value {
    Value::Error(ErrKind::Ref)
}

/// The always-refuse `validate` seam for the reserved reference-returning functions: every call is a
/// located `reserved-ref-function` refusal on the call name, whatever its arguments. This is the
/// documented, NAMED refusal path (not a wrong value at eval, not the generic `unknown-function`
/// path — the name is recognized-but-reserved), kept as registry data rather than a parser hand-fork.
pub(crate) fn refuse_reserved_ref_function(_args: &[Expr], span: Span) -> Result<(), Diag> {
    Err(Diag::new(
        DiagCode::ReservedRefFunction,
        span,
        "this reference-returning function (INDIRECT/OFFSET) is reserved for a later phase: it \
         returns a reference and forges a dynamic dependency the v1 scalar engine has no node for \
         (scope.md)",
    ))
}
