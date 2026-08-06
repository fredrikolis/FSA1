// Concern: the lookup and reference built-ins | Non-concern: the value ordering and wildcard engines (eval.rs, criteria.rs) | IO: (&mut EvalCtx, &[Expr]) -> Value
use super::*;

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
    // Clamped so an absurd magnitude becomes an out-of-bounds #REF! at the caller, not an overflowing cast.
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

/// Over a vector ASSUMED sorted: ascending picks the largest cell `<= needle`, descending the
/// smallest `>= needle`. GUARANTEED to terminate on ANY input — `[lo, hi]` strictly shrinks every
/// iteration and the `mid == 0` case breaks rather than underflowing — so unsorted data yields
/// Excel's undefined position, never a hang.
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

/// An error cell left in a search vector would both fail an exact `=` and corrupt the ordering the
/// approximate search assumes, so it is dropped; `original_indices` maps a hit back to its real row.
fn drop_error_cells(cells: &[Value]) -> (Vec<Value>, Vec<usize>) {
    let mut survivors = Vec::with_capacity(cells.len());
    let mut original_indices = Vec::with_capacity(cells.len());
    for (i, c) in cells.iter().enumerate() {
        if !matches!(c, Value::Error(_)) {
            survivors.push(c.clone());
            original_indices.push(i);
        }
    }
    (survivors, original_indices)
}

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
    let (search, original) = drop_error_cells(&cells);
    let pos = if match_type == 0 {
        linear_exact(&search, &needle, true)
    } else {
        binary_search_approx(&search, &needle, match_type > 0)
    };
    match pos {
        Some(i) => Value::Number((original[i] + 1) as f64),
        None => Value::Error(ErrKind::Na),
    }
}

pub(crate) fn index_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let (rows, cols, cells) = match lookup_block(ctx, &args[0]) {
        Err(k) => return Value::Error(k),
        Ok(t) => t,
    };
    let r = match index_arg(ctx, &args[1]) {
        Err(k) => return Value::Error(k),
        Ok(v) => v,
    };
    // A selector of `None` is the WHOLE span of that dimension — an explicit 0, or an omitted column on a 2-D array.
    let (row_sel, col_sel): (Option<u32>, Option<u32>) = if let Some(ce) = args.get(2) {
        let c = match index_arg(ctx, ce) {
            Err(k) => return Value::Error(k),
            Ok(v) => v,
        };
        (sel(r), sel(c))
    } else if rows == 1 {
        (Some(1), sel(r))
    } else if cols == 1 {
        (sel(r), Some(1))
    } else {
        // A 2-D array with the column omitted selects the whole `r`-th ROW.
        (sel(r), None)
    };
    if row_sel.is_some_and(|ri| ri > rows) || col_sel.is_some_and(|ci| ci > cols) {
        return Value::Error(ErrKind::Ref);
    }
    let at = |ri: u32, ci: u32| -> Value { cells[((ri - 1) * cols + (ci - 1)) as usize].clone() };
    match (row_sel, col_sel) {
        (Some(ri), Some(ci)) => matched_scalar(&at(ri, ci)),
        (Some(ri), None) => {
            let cell_row: Vec<Value> = (1..=cols).map(|ci| at(ri, ci)).collect();
            Value::Array(Shape { rows: 1, cols }, cell_row)
        }
        (None, Some(ci)) => {
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
    let first_col: Vec<Value> = (0..rows)
        .map(|r| cells[(r * cols) as usize].clone())
        .collect();
    let (search, original) = drop_error_cells(&first_col);
    let hit = if approximate {
        binary_search_approx(&search, &needle, true)
    } else {
        linear_exact(&search, &needle, true)
    };
    match hit {
        None => Value::Error(ErrKind::Na),
        Some(i) => {
            let r = original[i] as u32;
            matched_scalar(&cells[(r * cols + (col_index as u32 - 1)) as usize])
        }
    }
}

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
    // The binary modes (+/-2) collapse to the equivalent directional linear scan, identical on the sorted data they assume; only the SIGN drives the search.
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
                // An equal-value candidate keeps the first seen in the search direction, so `order` already encodes the tie-break.
                if better { Some(i) } else { Some(b) }
            }
        };
    }
    best
}

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

pub(crate) fn row_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match args.first() {
        None => Value::Number(ctx.current_row() as f64),
        Some(Expr::Ref(r)) => Value::Number((r.row + 1) as f64),
        Some(Expr::Range(rn)) => {
            if rn.is_open_rows() || rn.is_open_cols() {
                return Value::Error(ErrKind::Ref);
            }
            let top = rn.start_row.min(rn.end_row);
            let bot = rn.start_row.max(rn.end_row);
            let n = bot - top + 1;
            if n == 1 {
                Value::Number((top + 1) as f64)
            } else {
                Value::Array(
                    Shape { rows: n, cols: 1 },
                    (top..=bot).map(|r| Value::Number((r + 1) as f64)).collect(),
                )
            }
        }
        Some(_) => Value::Error(ErrKind::Value),
    }
}

pub(crate) fn column_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match args.first() {
        None => Value::Number(ctx.current_col() as f64),
        Some(Expr::Ref(r)) => Value::Number((r.col + 1) as f64),
        Some(Expr::Range(rn)) => {
            if rn.is_open_rows() || rn.is_open_cols() {
                return Value::Error(ErrKind::Ref);
            }
            let left = rn.start_col.min(rn.end_col);
            let right = rn.start_col.max(rn.end_col);
            let n = right - left + 1;
            if n == 1 {
                Value::Number((left + 1) as f64)
            } else {
                Value::Array(
                    Shape { rows: 1, cols: n },
                    (left..=right)
                        .map(|c| Value::Number((c + 1) as f64))
                        .collect(),
                )
            }
        }
        Some(_) => Value::Error(ErrKind::Value),
    }
}

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
    let first_row: Vec<Value> = (0..cols).map(|c| cells[c as usize].clone()).collect();
    let (search, original) = drop_error_cells(&first_row);
    let hit = if approximate {
        binary_search_approx(&search, &needle, true)
    } else {
        linear_exact(&search, &needle, true)
    };
    match hit {
        None => Value::Error(ErrKind::Na),
        Some(i) => {
            let c = original[i] as u32;
            matched_scalar(&cells[((row_index as u32 - 1) * cols + c) as usize])
        }
    }
}

pub(crate) fn lookup_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let needle = scalarize(ctx.eval(&args[0]));
    if let Value::Error(k) = needle {
        return Value::Error(k);
    }
    let (rows, cols, cells) = match lookup_block(ctx, &args[1]) {
        Err(k) => return Value::Error(k),
        Ok(t) => t,
    };
    // 2-arg is the ARRAY form (which reduces to the vector form for a vector); 3-arg is the classic vector form.
    let Some(result_e) = args.get(2) else {
        return lookup_array_form(&needle, rows, cols, &cells);
    };
    let (search, original) = drop_error_cells(&cells);
    let pos = match binary_search_approx(&search, &needle, true) {
        None => return Value::Error(ErrKind::Na),
        Some(p) => p,
    };
    let (_rr, _rc, result_vec) = match lookup_block(ctx, result_e) {
        Err(k) => return Value::Error(k),
        Ok(t) => t,
    };
    match result_vec.get(original[pos]) {
        Some(v) => matched_scalar(v),
        None => Value::Error(ErrKind::Na),
    }
}

/// The ASPECT-RATIO rule: wider than tall searches the first ROW and returns from the last row;
/// square or taller searches the first COLUMN and returns from the last column.
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
    // Filtered as PAIRS, so a dropped error takes its aligned result cell with it and the alignment survives.
    let (search, result): (Vec<Value>, Vec<Value>) = search
        .into_iter()
        .zip(result)
        .filter(|(s, _)| !matches!(s, Value::Error(_)))
        .unzip();
    match binary_search_approx(&search, needle, true) {
        None => Value::Error(ErrKind::Na),
        Some(pos) => matched_scalar(&result[pos]),
    }
}

pub(crate) fn rows_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match lookup_block(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok((rows, _, _)) => Value::Number(rows as f64),
    }
}

pub(crate) fn columns_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match lookup_block(ctx, &args[0]) {
        Err(k) => Value::Error(k),
        Ok((_, cols, _)) => Value::Number(cols as f64),
    }
}

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
    let search_mode = match args.get(3) {
        None => 1,
        Some(e) => match coerce_num(&scalarize(ctx.eval(e))) {
            Err(k) => return Value::Error(k),
            Ok(n) => n.trunc() as i64,
        },
    };
    if !matches!(search_mode, 1 | -1 | 2 | -2) {
        return Value::Error(ErrKind::Value);
    }
    match xlookup_find(&needle, &cells, match_mode, search_mode < 0) {
        Some(i) => Value::Number((i + 1) as f64),
        None => Value::Error(ErrKind::Na),
    }
}

pub(crate) fn address_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let row = match one_num(ctx, &args[0]) {
        Ok(n) => n.trunc() as i64,
        Err(k) => return Value::Error(k),
    };
    let col = match one_num(ctx, &args[1]) {
        Ok(n) => n.trunc() as i64,
        Err(k) => return Value::Error(k),
    };
    let abs_num = match opt_num(ctx, args, 2, 1.0) {
        Ok(n) => n.trunc() as i64,
        Err(k) => return Value::Error(k),
    };
    if !(1..=4).contains(&abs_num) {
        return Value::Error(ErrKind::Value);
    }
    let a1 = match opt_bool(ctx, args, 3, true) {
        Ok(b) => b,
        Err(k) => return Value::Error(k),
    };
    let sheet = match args.get(4) {
        Some(e) => match arg_text(ctx, e) {
            Ok(s) => s,
            Err(k) => return Value::Error(k),
        },
        None => String::new(),
    };
    // `abs_num` decodes to a per-axis absolute flag: 1 → both, 2 → row only, 3 → column only, 4 → none.
    let row_abs = matches!(abs_num, 1 | 2);
    let col_abs = matches!(abs_num, 1 | 3);
    // A positive 1-based index with NO upper grid bound, so both display styles accept the same coordinate.
    if row < 1 || col < 1 {
        return Value::Error(ErrKind::Value);
    }
    let core = if a1 {
        // A column past `u32::MAX` has no pinned rendering, so it is a located `#VALUE!` rather than a silently wrapped index.
        let Ok(col0) = u32::try_from(col - 1) else {
            return Value::Error(ErrKind::Value);
        };
        let mut s = String::new();
        if col_abs {
            s.push('$');
        }
        s.push_str(&crate::a1::format_column(col0));
        if row_abs {
            s.push('$');
        }
        s.push_str(&row.to_string());
        s
    } else {
        // R1C1: an ABSOLUTE part is a 1-based index (`R2`), a RELATIVE part an offset in brackets (`R[2]`).
        let r = if row_abs {
            format!("R{row}")
        } else {
            format!("R[{row}]")
        };
        let c = if col_abs {
            format!("C{col}")
        } else {
            format!("C[{col}]")
        };
        format!("{r}{c}")
    };
    let out = if sheet.is_empty() {
        core
    } else {
        format!("{}!{}", quote_sheet_name(&sheet), core)
    };
    Value::Text(out)
}

/// Quote a sheet name for an [`address_fn`] prefix as Excel does: a bare identifier (letters/digits/
/// `_`/`.`, not starting with a digit) is used as-is; anything else is wrapped in single quotes with
/// an internal single quote doubled (`O'Brien` → `'O''Brien'`).
fn quote_sheet_name(name: &str) -> String {
    let bare = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    if bare {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

/// The BACKSTOP for the reference-forging functions: a consumer source-rewrites each into a static
/// reference before eval, so this is unreachable via a properly-forged tree — but a forger that DOES
/// reach eval un-rewritten must be a located `#REF!`, never a panic.
pub(crate) fn reserved_ref_eval(_ctx: &mut EvalCtx, _args: &[Expr]) -> Value {
    Value::Error(ErrKind::Ref)
}
