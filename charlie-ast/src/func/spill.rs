// Concern: the DYNAMIC-ARRAY (spill) function bodies — the built-ins whose value is itself an `array`: `SORT`/`SORTBY` (order a block's rows/cols by a key column/row or by parallel key vectors), `UNIQUE` (first-occurrence-distinct rows/cols, optionally exactly-once), `FILTER` (keep the rows/cols a boolean vector selects, or an `if_empty` fallback), `SEQUENCE` (a generated `rows`x`cols` counter from `start` by `step`), `TRANSPOSE` (swap a block's axes), the STACKERS `VSTACK`/`HSTACK` (concatenate blocks, padding short lines with `#N/A`), and the RESHAPERS `TAKE`/`DROP`/`CHOOSEROWS`/`CHOOSECOLS` (select a sub-grid by count or by index); each is Excel-compatible in arg order and error semantics and returns a `Value::Array` (or a LOCATED first-class error value — a bad index/shape/empty result is `#VALUE!`/`#CALC!`, never a panic, CORE2) | Non-concern: the IMPLICIT-array broadcaster that maps a SCALAR-arg function element-wise (func::array owns that — these functions consume arrays WHOLE, so their registry rows carry no `broadcast` positions), the registry table + dispatch (func/mod.rs), and where a returned array is PLACED into a range file (charlie-model's GRID5 region owns the shape/orientation match) | IO: (`EvalCtx`, the call's arg `Expr`s) -> a `Value` (an `array`, or a located error value)
use super::*;

/// The largest array [`sequence_fn`] will generate before refusing — a `SEQUENCE(1e6, 1e6)` would
/// otherwise allocate ~1e12 cells into an OOM abort. Over the bound is a located `#NUM!` (CORE2:
/// a valid call never crashes the process). Far above any range a file could declare.
const SEQUENCE_MAX_CELLS: u64 = 1_000_000;

/// `TRANSPOSE(array)` — swap the block's axes: element `(r,c)` of an `R`x`C` input becomes element
/// `(c,r)` of the `C`x`R` result. A scalar is a `1x1` block (unchanged); an error argument
/// propagates (Excel). Cells are moved verbatim (a blank stays blank).
pub(crate) fn transpose_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (rows, cols, cells) = match block(ctx, &args[0]) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let mut out = Vec::with_capacity(cells.len());
    for c in 0..cols {
        for r in 0..rows {
            out.push(cells[(r * cols + c) as usize].clone());
        }
    }
    Value::Array(
        Shape {
            rows: cols,
            cols: rows,
        },
        out,
    )
}

/// `SEQUENCE(rows, [cols], [start], [step])` — a generated `rows`x`cols` array counting row-major from
/// `start` (default `1`) by `step` (default `1`); `cols` defaults to `1`. A NEGATIVE `rows`/`cols` is
/// a located `#VALUE!`; an empty (zero) `rows`/`cols` is a located `#CALC!` (Excel's empty-array
/// result); an over-[`SEQUENCE_MAX_CELLS`] area is a located `#NUM!` (the area is a SATURATING
/// multiply, so an overflowing `rows`×`cols` still trips the cap rather than wrapping past it or
/// panicking under overflow-checks — CORE2); a non-finite generated value folds to `#NUM!`
/// ([`finite_or_num`]).
pub(crate) fn sequence_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let rows = match one_num(ctx, &args[0]) {
        Ok(n) => n.trunc() as i64,
        Err(k) => return Value::Error(k),
    };
    let cols = match opt_num(ctx, args, 1, 1.0) {
        Ok(n) => n.trunc() as i64,
        Err(k) => return Value::Error(k),
    };
    let start = match opt_num(ctx, args, 2, 1.0) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let step = match opt_num(ctx, args, 3, 1.0) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    // Excel splits the non-positive cases: a NEGATIVE dimension is `#VALUE!`, an empty (zero)
    // dimension is `#CALC!` (its empty-array result). CORE2: a located error value, never a panic.
    if rows < 0 || cols < 0 {
        return Value::Error(ErrKind::Value);
    }
    if rows < 1 || cols < 1 {
        return Value::Error(ErrKind::Calc);
    }
    // `rows`/`cols` are here in `1..=i64::MAX` (a `trunc` of an arbitrary numeric arg), so a naive
    // `rows * cols` can overflow `u64` — a PANIC under overflow-checks (the config `cargo test`
    // runs) and a silent WRAP in release that could slip past the cap. A saturating multiply pins
    // the product at `u64::MAX` so an absurd area still trips the guard below. CORE2: no crash.
    let area = (rows as u64).saturating_mul(cols as u64);
    if area > SEQUENCE_MAX_CELLS {
        return Value::Error(ErrKind::Num);
    }
    let mut out = Vec::with_capacity(area as usize);
    for i in 0..area {
        out.push(finite_or_num(start + (i as f64) * step));
    }
    Value::Array(
        Shape {
            rows: rows as u32,
            cols: cols as u32,
        },
        out,
    )
}

/// `SORT(array, [sort_index], [sort_order], [by_col])` — order a block's rows (or, when `by_col` is
/// TRUE, its columns) by the `sort_index`-th (1-based) column (or row) key. `sort_order` `1`
/// (default) is ascending, `-1` descending — any OTHER value is a located `#VALUE!` (Excel accepts
/// only ±1); `by_col` defaults to FALSE. A `sort_index` outside the key axis is a located `#VALUE!`. Keys rank by the engine's [`value_cmp`] total order (numbers
/// numerically, text case-insensitively, cross-type Number<Text<Bool) for present values — except a
/// `Blank` key always sorts LAST in either direction (Excel), so it never intermixes with a literal
/// `0`. The sort is STABLE (equal keys keep input order).
pub(crate) fn sort_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (rows, cols, cells) = match block(ctx, &args[0]) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let sort_index = match opt_num(ctx, args, 1, 1.0) {
        Ok(n) => n.trunc() as i64,
        Err(k) => return Value::Error(k),
    };
    let sort_order = match opt_num(ctx, args, 2, 1.0) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let by_col = match opt_bool(ctx, args, 3, false) {
        Ok(b) => b,
        Err(k) => return Value::Error(k),
    };
    // Excel accepts ONLY 1 (ascending) or -1 (descending) for `sort_order`; any other value is a
    // located `#VALUE!` (never a silent ascending fall-back for 0 / 5 / ...). CORE2: no panic.
    let desc = if sort_order == 1.0 {
        false
    } else if sort_order == -1.0 {
        true
    } else {
        return Value::Error(ErrKind::Value);
    };
    // The key axis length: sorting rows keys on a COLUMN (index in 1..=cols); sorting columns keys on
    // a ROW (index in 1..=rows).
    let key_len = if by_col { rows } else { cols };
    // Compare in `i64` (not `sort_index as u32`, which TRUNCATES): an absurd index like `2^32+2`
    // must be a located `#VALUE!`, never truncate into the valid range and silently key the wrong
    // axis. CORE2: no panic, no wrong-column result.
    if sort_index < 1 || sort_index > i64::from(key_len) {
        return Value::Error(ErrKind::Value);
    }
    let k = (sort_index - 1) as u32;
    let at = |r: u32, c: u32| cells[(r * cols + c) as usize].clone();
    if by_col {
        // Sort the COLUMNS; key of column j is cell (k, j).
        let mut order: Vec<u32> = (0..cols).collect();
        order.sort_by(|&a, &b| cmp_dir(&at(k, a), &at(k, b), desc));
        let mut out = Vec::with_capacity(cells.len());
        for r in 0..rows {
            for &j in &order {
                out.push(at(r, j));
            }
        }
        Value::Array(Shape { rows, cols }, out)
    } else {
        // Sort the ROWS; key of row i is cell (i, k).
        let mut order: Vec<u32> = (0..rows).collect();
        order.sort_by(|&a, &b| cmp_dir(&at(a, k), &at(b, k), desc));
        let mut out = Vec::with_capacity(cells.len());
        for &i in &order {
            for c in 0..cols {
                out.push(at(i, c));
            }
        }
        Value::Array(Shape { rows, cols }, out)
    }
}

/// `UNIQUE(array, [by_col], [exactly_once])` — the FIRST-occurrence-distinct rows (or columns, when
/// `by_col` is TRUE) of a block, preserving input order. With `exactly_once` TRUE, keep only the
/// rows/cols that occur exactly once. Two rows/cols are equal iff every paired cell ranks `Equal`
/// under [`value_cmp`] (so text folds case-insensitively, matching the operators). An empty result
/// (every row filtered out under `exactly_once`) is a located `#CALC!`.
pub(crate) fn unique_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (rows, cols, cells) = match block(ctx, &args[0]) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let by_col = match opt_bool(ctx, args, 1, false) {
        Ok(b) => b,
        Err(k) => return Value::Error(k),
    };
    let exactly_once = match opt_bool(ctx, args, 2, false) {
        Ok(b) => b,
        Err(k) => return Value::Error(k),
    };
    let at = |r: u32, c: u32| cells[(r * cols + c) as usize].clone();
    // A line is a row (by_col=false) or a column (by_col=true); `count` is the number of lines and
    // `line` reads the i-th line's cells in axis order.
    let count = if by_col { cols } else { rows };
    let line = |i: u32| -> Vec<Value> {
        if by_col {
            (0..rows).map(|r| at(r, i)).collect()
        } else {
            (0..cols).map(|c| at(i, c)).collect()
        }
    };
    let lines: Vec<Vec<Value>> = (0..count).map(line).collect();
    let eq = |a: &[Value], b: &[Value]| {
        a.len() == b.len()
            && a.iter()
                .zip(b.iter())
                .all(|(x, y)| value_cmp(x, y) == std::cmp::Ordering::Equal)
    };
    let mut kept: Vec<&Vec<Value>> = Vec::new();
    for l in &lines {
        let occurrences = lines.iter().filter(|o| eq(o, l)).count();
        let take = if exactly_once {
            occurrences == 1
        } else {
            !kept.iter().any(|k| eq(k, l))
        };
        if take {
            kept.push(l);
        }
    }
    if kept.is_empty() {
        return Value::Error(ErrKind::Calc);
    }
    assemble(&kept, by_col, rows, cols)
}

/// `FILTER(array, include, [if_empty])` — keep the rows the boolean vector `include` selects (a
/// COLUMN vector one-per-row filters rows; a ROW vector one-per-column filters columns), Excel-
/// compatible. A shape that matches neither axis is a located `#VALUE!`; a non-boolean-coercible
/// `include` cell propagates its coercion error. When nothing is selected, `if_empty` (arg 3) is
/// returned if present, else a located `#CALC!` (Excel's omitted-`if_empty` empty result).
pub(crate) fn filter_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (rows, cols, cells) = match block(ctx, &args[0]) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let (irows, icols, icells) = match block(ctx, &args[1]) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let at = |r: u32, c: u32| cells[(r * cols + c) as usize].clone();
    // Decide the filter axis from the include vector's shape.
    let by_row = irows == rows && icols == 1;
    let by_col = icols == cols && irows == 1;
    if !by_row && !by_col {
        return Value::Error(ErrKind::Value);
    }
    // Which lines to keep: coerce each include cell to a boolean (an error propagates).
    let mut keep = Vec::new();
    for (i, inc) in icells.iter().enumerate() {
        match coerce_bool(inc) {
            Ok(true) => keep.push(i as u32),
            Ok(false) => {}
            Err(k) => return Value::Error(k),
        }
    }
    if keep.is_empty() {
        return if args.len() > 2 {
            ctx.eval(&args[2])
        } else {
            Value::Error(ErrKind::Calc)
        };
    }
    if by_row {
        let kept: Vec<Vec<Value>> = keep
            .iter()
            .map(|&i| (0..cols).map(|c| at(i, c)).collect())
            .collect();
        let refs: Vec<&Vec<Value>> = kept.iter().collect();
        assemble(&refs, false, rows, cols)
    } else {
        let kept: Vec<Vec<Value>> = keep
            .iter()
            .map(|&j| (0..rows).map(|r| at(r, j)).collect())
            .collect();
        let refs: Vec<&Vec<Value>> = kept.iter().collect();
        assemble(&refs, true, rows, cols)
    }
}

/// Reassemble kept LINES (rows when `by_col` is false, columns when true) into a row-major
/// [`Value::Array`]. Each `line` is already in axis order (a row is left→right; a column is
/// top→bottom); the result's other-axis length is the original `cols`/`rows`.
fn assemble(lines: &[&Vec<Value>], by_col: bool, rows: u32, cols: u32) -> Value {
    if by_col {
        // Kept columns: the result is `rows` x `lines.len()`, filled row-major.
        let out_cols = lines.len() as u32;
        let mut out = Vec::with_capacity((rows as usize) * lines.len());
        for r in 0..rows as usize {
            for line in lines {
                out.push(line[r].clone());
            }
        }
        Value::Array(
            Shape {
                rows,
                cols: out_cols,
            },
            out,
        )
    } else {
        // Kept rows: the result is `lines.len()` x `cols`, each line contributing a full row.
        let out_rows = lines.len() as u32;
        let mut out = Vec::with_capacity(lines.len() * (cols as usize));
        for line in lines {
            out.extend(line.iter().cloned());
        }
        Value::Array(
            Shape {
                rows: out_rows,
                cols,
            },
            out,
        )
    }
}

/// `VSTACK(array1, [array2], …)` — stack blocks VERTICALLY: the result is `Σrows × maxcols`, each
/// input laid down as a band of rows, and any row narrower than the widest input padded on the right
/// with `#N/A` (Excel). An error argument propagates.
pub(crate) fn vstack_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let blocks = match materialize_blocks(ctx, args) {
        Ok(b) => b,
        Err(k) => return Value::Error(k),
    };
    let out_cols = blocks.iter().map(|(_, c, _)| *c).max().unwrap_or(0);
    let out_rows: u32 = blocks.iter().map(|(r, _, _)| *r).sum();
    let mut out = Vec::with_capacity((out_rows as usize) * (out_cols as usize));
    for (r, c, cells) in &blocks {
        for ri in 0..*r {
            for ci in 0..out_cols {
                out.push(if ci < *c {
                    cells[(ri * c + ci) as usize].clone()
                } else {
                    Value::Error(ErrKind::Na)
                });
            }
        }
    }
    Value::Array(
        Shape {
            rows: out_rows,
            cols: out_cols,
        },
        out,
    )
}

/// `HSTACK(array1, [array2], …)` — stack blocks HORIZONTALLY: the result is `maxrows × Σcols`, each
/// input laid down as a band of columns, and any column shorter than the tallest input padded below
/// with `#N/A` (Excel). An error argument propagates.
pub(crate) fn hstack_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let blocks = match materialize_blocks(ctx, args) {
        Ok(b) => b,
        Err(k) => return Value::Error(k),
    };
    let out_rows = blocks.iter().map(|(r, _, _)| *r).max().unwrap_or(0);
    let out_cols: u32 = blocks.iter().map(|(_, c, _)| *c).sum();
    let mut out = Vec::with_capacity((out_rows as usize) * (out_cols as usize));
    for ri in 0..out_rows {
        for (r, c, cells) in &blocks {
            for ci in 0..*c {
                out.push(if ri < *r {
                    cells[(ri * c + ci) as usize].clone()
                } else {
                    Value::Error(ErrKind::Na)
                });
            }
        }
    }
    Value::Array(
        Shape {
            rows: out_rows,
            cols: out_cols,
        },
        out,
    )
}

/// Materialize every argument to a `(rows, cols, cells)` block for the stacking family, propagating
/// the first error argument. The one shared front door VSTACK/HSTACK use.
fn materialize_blocks(
    ctx: &mut EvalCtx,
    args: &[Expr],
) -> Result<Vec<(u32, u32, Vec<Value>)>, ErrKind> {
    args.iter().map(|a| block(ctx, a)).collect()
}

/// `TAKE(array, rows, [columns])` — keep the first `rows` rows and first `columns` columns (a NEGATIVE
/// count takes from the END; an OMITTED/blank count keeps the whole axis). The result is
/// `|rows| × |columns|`; a provided count of `0` (or an empty axis) is a located `#CALC!` (Excel's
/// empty-array result). An error argument propagates. CORE2: a located error, never a panic.
pub(crate) fn take_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (rows, cols, cells) = match block(ctx, &args[0]) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let want_rows = match opt_dim(ctx, args, 1) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let want_cols = match opt_dim(ctx, args, 2) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let sel_rows = take_indices(rows, want_rows);
    let sel_cols = take_indices(cols, want_cols);
    subgrid(&cells, cols, &sel_rows, &sel_cols)
}

/// `DROP(array, rows, [columns])` — remove the first `rows` rows and first `columns` columns (a
/// NEGATIVE count removes from the END; an OMITTED/blank count removes none). Removing an entire axis
/// (`|count| ≥` the axis length) leaves an empty result → a located `#CALC!` (Excel). An error
/// argument propagates. CORE2: a located error, never a panic.
pub(crate) fn drop_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (rows, cols, cells) = match block(ctx, &args[0]) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let want_rows = match opt_dim(ctx, args, 1) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let want_cols = match opt_dim(ctx, args, 2) {
        Ok(v) => v,
        Err(k) => return Value::Error(k),
    };
    let sel_rows = drop_indices(rows, want_rows);
    let sel_cols = drop_indices(cols, want_cols);
    subgrid(&cells, cols, &sel_rows, &sel_cols)
}

/// Read an OPTIONAL integer axis-count argument at `idx`: `None` when the call omits it OR the slot is
/// blank (`TAKE(a,,2)` keeps every row) — the caller reads `None` as "the whole axis"; otherwise the
/// value truncated toward zero (Excel), with an error propagated.
fn opt_dim(ctx: &mut EvalCtx, args: &[Expr], idx: usize) -> Result<Option<i64>, ErrKind> {
    match args.get(idx) {
        None => Ok(None),
        Some(e) => match scalarize(ctx.eval(e)) {
            Value::Blank => Ok(None),
            v => Ok(Some(coerce_num(&v)?.trunc() as i64)),
        },
    }
}

/// The row/col indices TAKE keeps along one axis of length `len`: the whole axis when `want` is `None`,
/// else the first `n` (or last `|n|` when negative), clamped to the axis. A count of `0` selects none.
fn take_indices(len: u32, want: Option<i64>) -> Vec<u32> {
    match want {
        None => (0..len).collect(),
        Some(n) => {
            let mag = n.unsigned_abs().min(u64::from(len)) as u32;
            if n >= 0 {
                (0..mag).collect()
            } else {
                (len - mag..len).collect()
            }
        }
    }
}

/// The row/col indices DROP keeps along one axis of length `len`: the whole axis when `want` is `None`,
/// else all but the first `n` (or all but the last `|n|` when negative), clamped to the axis. Removing
/// the whole axis selects none.
fn drop_indices(len: u32, want: Option<i64>) -> Vec<u32> {
    match want {
        None => (0..len).collect(),
        Some(n) => {
            let mag = n.unsigned_abs().min(u64::from(len)) as u32;
            if n >= 0 {
                (mag..len).collect()
            } else {
                (0..len - mag).collect()
            }
        }
    }
}

/// `CHOOSEROWS(array, row_num1, [row_num2], …)` — the rows of `array` at the given 1-based indices, in
/// the order given (a NEGATIVE index counts from the end; an index argument may itself be an array of
/// indices, flattened). A `0` or out-of-range index is a located `#VALUE!`; an error index propagates.
pub(crate) fn chooserows_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (rows, cols, cells) = match block(ctx, &args[0]) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let sel = match gather_indices(ctx, &args[1..], rows) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    subgrid(&cells, cols, &sel, &(0..cols).collect::<Vec<_>>())
}

/// `CHOOSECOLS(array, col_num1, [col_num2], …)` — the columns of `array` at the given 1-based indices,
/// in the order given (the column dual of [`chooserows_fn`]: a NEGATIVE index counts from the end; an
/// index argument may be an array, flattened). A `0`/out-of-range index is `#VALUE!`; an error
/// propagates.
pub(crate) fn choosecols_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (rows, cols, cells) = match block(ctx, &args[0]) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let sel = match gather_indices(ctx, &args[1..], cols) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    subgrid(&cells, cols, &(0..rows).collect::<Vec<_>>(), &sel)
}

/// Collect the 1-based selector arguments (CHOOSEROWS/CHOOSECOLS) into 0-based axis indices, flattening
/// any array selector, resolving a negative index from the end, and rejecting `0`/out-of-range with a
/// `#VALUE!` (an error selector propagates). `axis_len` is the length of the axis being selected.
fn gather_indices(ctx: &mut EvalCtx, args: &[Expr], axis_len: u32) -> Result<Vec<u32>, ErrKind> {
    let mut sel = Vec::new();
    for e in args {
        let (_, _, idx_cells) = block(ctx, e)?;
        for v in &idx_cells {
            let n = coerce_num(v)?.trunc() as i64;
            match resolve_index(n, axis_len) {
                Some(i) => sel.push(i),
                None => return Err(ErrKind::Value),
            }
        }
    }
    Ok(sel)
}

/// Resolve a 1-based (or negative-from-end) index into a 0-based position on an axis of length `len`.
/// `1..=len` maps to `0..len`; `-1..=-len` maps from the end; `0` or anything out of range is `None`.
fn resolve_index(n: i64, len: u32) -> Option<u32> {
    let len = u64::from(len);
    if n >= 1 && n as u64 <= len {
        Some((n - 1) as u32)
    } else if n <= -1 && n.unsigned_abs() <= len {
        Some((len - n.unsigned_abs()) as u32)
    } else {
        None
    }
}

/// Assemble the sub-grid at the selected `sel_rows` × `sel_cols` (0-based, in the order given) of a
/// `cols`-wide row-major block. An empty selection on either axis is a located `#CALC!` (Excel's
/// empty-array result) — TAKE/DROP reach this when a count zeroes an axis. Row-major output.
fn subgrid(cells: &[Value], cols: u32, sel_rows: &[u32], sel_cols: &[u32]) -> Value {
    if sel_rows.is_empty() || sel_cols.is_empty() {
        return Value::Error(ErrKind::Calc);
    }
    let mut out = Vec::with_capacity(sel_rows.len() * sel_cols.len());
    for &r in sel_rows {
        for &c in sel_cols {
            out.push(cells[(r * cols + c) as usize].clone());
        }
    }
    Value::Array(
        Shape {
            rows: sel_rows.len() as u32,
            cols: sel_cols.len() as u32,
        },
        out,
    )
}

/// `SORTBY(array, by_array1, [sort_order1], [by_array2, sort_order2], …)` — order the ROWS of `array`
/// by one or more parallel key vectors (or its COLUMNS when the key vectors match the column count),
/// each with its own `sort_order` (`1`/omitted ascending, `-1` descending — any other is `#VALUE!`).
/// The axis is decided by the FIRST `by_array`'s length (matching `rows` → sort rows, `cols` → sort
/// columns); every key vector must match that axis length, else `#VALUE!`. Keys rank by the shared
/// [`cmp_dir`] (a `Blank` sorts LAST in either direction); the sort is STABLE, and multiple keys break
/// ties left-to-right. An error argument propagates. CORE2: a located error, never a panic.
pub(crate) fn sortby_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (rows, cols, cells) = match block(ctx, &args[0]) {
        Ok(t) => t,
        Err(k) => return Value::Error(k),
    };
    let mut keys: Vec<(Vec<Value>, bool)> = Vec::new();
    let mut axis_rows = true;
    let mut i = 1;
    while i < args.len() {
        let (kr, kc, kcells) = match block(ctx, &args[i]) {
            Ok(t) => t,
            Err(k) => return Value::Error(k),
        };
        // Excel: a by_array must be one row high OR one column wide, and its ORIENTATION picks the
        // sort axis — a COLUMN (kc==1) matching the row count sorts ROWS; a ROW (kr==1) matching the
        // column count sorts COLUMNS. Length alone is not enough (a 2×1 key on a 3×2 block is a
        // row-count mismatch, not a column sort). Every later key must share that axis and length.
        let sorts_rows = kc == 1 && kr == rows;
        let sorts_cols = kr == 1 && kc == cols;
        if keys.is_empty() {
            axis_rows = if sorts_rows {
                true
            } else if sorts_cols {
                false
            } else {
                return Value::Error(ErrKind::Value);
            };
        } else if (axis_rows && !sorts_rows) || (!axis_rows && !sorts_cols) {
            return Value::Error(ErrKind::Value);
        }
        let order = match args.get(i + 1) {
            None => 1.0,
            Some(e) => match scalarize(ctx.eval(e)) {
                Value::Blank => 1.0,
                v => match coerce_num(&v) {
                    Ok(n) => n,
                    Err(k) => return Value::Error(k),
                },
            },
        };
        let desc = if order == 1.0 {
            false
        } else if order == -1.0 {
            true
        } else {
            return Value::Error(ErrKind::Value);
        };
        keys.push((kcells, desc));
        i += 2;
    }
    let count = if axis_rows { rows } else { cols };
    let mut order: Vec<u32> = (0..count).collect();
    order.sort_by(|&a, &b| {
        for (kcells, desc) in &keys {
            let o = cmp_dir(&kcells[a as usize], &kcells[b as usize], *desc);
            if o != std::cmp::Ordering::Equal {
                return o;
            }
        }
        std::cmp::Ordering::Equal
    });
    let at = |r: u32, c: u32| cells[(r * cols + c) as usize].clone();
    let mut out = Vec::with_capacity(cells.len());
    if axis_rows {
        for &r in &order {
            for c in 0..cols {
                out.push(at(r, c));
            }
        }
    } else {
        for r in 0..rows {
            for &c in &order {
                out.push(at(r, c));
            }
        }
    }
    Value::Array(Shape { rows, cols }, out)
}

/// Compare two SORT keys in the requested direction: [`value_cmp`] ascending, or its reverse when
/// `desc`. A `Blank` key always sorts LAST regardless of direction (Excel places empty cells at the
/// end of both an ascending and a descending sort) rather than resolving against the other key's
/// zero — so a `Blank` never intermixes with a literal `0`/`""`/`FALSE`.
fn cmp_dir(a: &Value, b: &Value, desc: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (matches!(a, Value::Blank), matches!(b, Value::Blank)) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => {
            let o = value_cmp(a, b);
            if desc { o.reverse() } else { o }
        }
    }
}
