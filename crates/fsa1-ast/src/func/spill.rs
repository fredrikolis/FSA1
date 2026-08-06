// Concern: the dynamic-array built-ins that RETURN an array | Non-concern: where a returned array lands, element-wise broadcasting | IO: (&mut EvalCtx, &[Expr]) -> Value
use super::*;

/// The largest array [`sequence_fn`] will generate before refusing — a `SEQUENCE(1e6, 1e6)` would
/// otherwise allocate ~1e12 cells into an OOM abort. Over the bound is a located `#NUM!` (CORE2:
/// a valid call never crashes the process). Far above any range a file could declare.
const SEQUENCE_MAX_CELLS: u64 = 1_000_000;

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
    // The two non-positive cases split: a NEGATIVE dimension is `#VALUE!`, a ZERO one is `#CALC!`.
    if rows < 0 || cols < 0 {
        return Value::Error(ErrKind::Value);
    }
    if rows < 1 || cols < 1 {
        return Value::Error(ErrKind::Calc);
    }
    // SATURATING: `rows`/`cols` reach `i64::MAX` here, so a plain multiply would panic under overflow-checks and wrap past the cap in release.
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
    // Only 1 or -1; any other `sort_order` is a located `#VALUE!`, never a silent ascending fallback.
    let desc = if sort_order == 1.0 {
        false
    } else if sort_order == -1.0 {
        true
    } else {
        return Value::Error(ErrKind::Value);
    };
    let key_len = if by_col { rows } else { cols };
    // Compared in `i64`, not a truncating `as u32`: an absurd index must refuse, never wrap into range and key the wrong axis.
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
    // A "line" is a row when `by_col` is false and a column when it is true.
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
        // ORIENTATION, not length, picks the sort axis: a 2x1 key on a 3x2 block is a row-count mismatch, not a column sort.
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
