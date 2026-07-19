// Concern: IMPLICIT ARRAY EVALUATION for function calls — the ONE home for the LOGIC of Excel's rule that an `array` handed to a function's SCALAR-expecting argument maps the call element-wise and yields an `array` (`COUNTIF(range, A1:A6)` -> an array of per-element counts; `LEN(A1:A3)` -> an array of per-cell lengths), broadcasting every other argument whole; it reads each broadcasting function's scalar positions from its registry row (`FuncDef::broadcast`, the single source of that DATA), shares one shape across the mapped positions (a mismatch is a static `#VALUE!`), short-circuits a scalar-error argument to that scalar error (Excel error propagation, never a tiled array of it), and re-dispatches the row's `eval` once per cell — plus `map_if`, the element-wise selection for `IF` over an ARRAY condition (each cell picks its `then`/`else` element, scalar branches broadcast), which lazy `logical::if_fn` calls only once it sees an array condition — so the mapping lives here, not smeared across the individual built-ins or dispatch | Non-concern: the registry table + dispatch entry + the `broadcast` position DATA itself (func/mod.rs owns `FUNCS`/`dispatch`/the row column), the per-function bodies (the family submodules own them), the scalar-vs-array DECISION for `IF` (logical.rs decides, then delegates the map here), REDUCERS that already collapse arrays (SUM/SUMPRODUCT keep their own loops), and element-wise OPERATOR broadcasting (eval.rs owns `binary_broadcast`/`unop_scalar`) | IO: (`&FuncDef`, `EvalCtx`, the call's arg `Expr`s) -> `Value`
//! Implicit array evaluation ([`eval_call`]): the single home for mapping a function element-wise
//! over an array supplied to a scalar-expecting argument (ast-standards PART 6, "accept under
//! uncertainty" — a scalar there still yields a scalar; only a genuine multi-cell array broadcasts).
//! `func::dispatch` routes every call through here; a function with no broadcasting positions is
//! dispatched unchanged, so existing behavior is preserved and the mapping stays out of dispatch.

use super::*;

/// Evaluate a call, applying implicit array evaluation over the function's scalar positions — read
/// from its registry row ([`FuncDef::broadcast`], the single source of that DATA; the LOGIC is here).
/// The arity is already gated by [`super::dispatch`], so `def.eval` may trust it here and below.
pub(crate) fn eval_call(def: &FuncDef, ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let positions = def.broadcast;
    if positions.is_empty() {
        return (def.eval)(ctx, args);
    }
    // Evaluate every argument ONCE, up front. A Reduce-position range is thereby resolved a single
    // time even when the call maps over many cells (each per-cell call reuses the materialized
    // value via an `Expr::Lit` wrapper) — and a scalar-position value can be inspected for an array.
    let vals: Vec<Value> = args.iter().map(|a| ctx.eval(a)).collect();
    let shape = match broadcast_shape(&vals, positions) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let Some(shape) = shape else {
        // No scalar position holds a genuine multi-cell array: one ordinary call over the values.
        let lits: Vec<Expr> = vals.into_iter().map(Expr::Lit).collect();
        return (def.eval)(ctx, &lits);
    };
    // A scalar (non-array) error in ANY argument short-circuits the whole mapped call to that
    // leftmost scalar error — Excel error propagation. Tiling it across the broadcast shape would
    // diverge: `COUNTIF(#REF!, A1:A6)` must be a scalar `#REF!`, not a 6×1 array of `#REF!`. An
    // error INSIDE a broadcasting array is a genuine per-cell datum and is left to `map_elementwise`.
    if let Some(Value::Error(k)) = vals.iter().find(|v| matches!(v, Value::Error(_))) {
        return Value::Error(*k);
    }
    map_elementwise(def, ctx, &vals, positions, shape)
}

/// The common shape to map over: the [`Shape`] shared by every scalar position that holds a genuine
/// (non-1×1) array, or `None` if no scalar position does (an ordinary scalar call). A 1×1 array in a
/// scalar position collapses to its cell downstream, so it does not force a broadcast. Two scalar-
/// position arrays of DIFFERENT shapes are a static `#VALUE!` (the shape-conformance stance the
/// operators and `SUMPRODUCT` already take).
fn broadcast_shape(vals: &[Value], positions: &[usize]) -> Result<Option<Shape>, ErrKind> {
    let mut shape: Option<Shape> = None;
    for &p in positions {
        if let Some(Value::Array(s, _)) = vals.get(p) {
            if s.rows == 1 && s.cols == 1 {
                continue;
            }
            match shape {
                None => shape = Some(*s),
                Some(existing) if existing != *s => return Err(ErrKind::Value),
                Some(_) => {}
            }
        }
    }
    Ok(shape)
}

/// Map the call over each cell of the broadcast shape: a scalar position holding a matching array
/// contributes its i-th cell; every other argument is broadcast whole. The per-cell results tile the
/// shape row-major, so the call yields an `array` [`Value`] — Excel's implicit array result.
fn map_elementwise(
    def: &FuncDef,
    ctx: &mut EvalCtx,
    vals: &[Value],
    positions: &[usize],
    shape: Shape,
) -> Value {
    let count = (shape.rows as usize) * (shape.cols as usize);
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let call_args: Vec<Expr> = vals
            .iter()
            .enumerate()
            .map(|(idx, v)| Expr::Lit(element_at(v, positions.contains(&idx), i)))
            .collect();
        out.push((def.eval)(ctx, &call_args));
    }
    Value::Array(shape, out)
}

/// The value an argument contributes to the i-th mapped call: the i-th cell of a broadcasting
/// scalar-position array (a 1×1 array is treated as a scalar and broadcast whole), or the whole
/// value otherwise. Defensively indexes so a shape/length skew is a `Blank`, never a panic (totality).
fn element_at(v: &Value, is_scalar_pos: bool, i: usize) -> Value {
    if is_scalar_pos
        && let Value::Array(s, cells) = v
        && !(s.rows == 1 && s.cols == 1)
    {
        return cells.get(i).cloned().unwrap_or(Value::Blank);
    }
    v.clone()
}

/// Element-wise `IF` over an ARRAY condition (Excel array `IF`): for each cell of `cond`, coerce it to
/// a boolean and take the matching element of `then_v` (TRUE) or `else_v` (FALSE). A scalar (or 1×1)
/// branch broadcasts whole; a branch that is a matching-shape array contributes its i-th cell. A branch
/// that is a genuinely multi-cell array of a DIFFERENT shape makes the whole result `#VALUE!` — the
/// same shape-conformance stance `binary_broadcast`/`SUMPRODUCT` take. A condition cell that cannot
/// coerce to a boolean (an error cell, or non-logical text) is THAT element's error (per-cell totality,
/// CORE2), never a panic. `logical::if_fn` owns the lazy scalar path and only calls this once it has
/// seen a genuinely multi-cell condition — so array `IF` reuses this array home rather than growing a
/// parallel loop in `logical`.
pub(crate) fn map_if(shape: Shape, cond: &[Value], then_v: &Value, else_v: &Value) -> Value {
    // A branch that is a genuinely multi-cell array must conform to the condition's shape (a scalar or
    // 1×1 broadcasts and is exempt); a mismatch is a static `#VALUE!` before any per-cell work.
    for branch in [then_v, else_v] {
        if let Value::Array(s, _) = branch
            && !(s.rows == 1 && s.cols == 1)
            && *s != shape
        {
            return Value::Error(ErrKind::Value);
        }
    }
    let count = (shape.rows as usize) * (shape.cols as usize);
    let mut out = Vec::with_capacity(count);
    for (i, c) in cond.iter().enumerate().take(count) {
        out.push(match coerce_bool(c) {
            Err(k) => Value::Error(k),
            Ok(true) => branch_cell(then_v, i),
            Ok(false) => branch_cell(else_v, i),
        });
    }
    Value::Array(shape, out)
}

/// The value a branch contributes to the i-th [`map_if`] cell: the i-th cell of a matching-shape array
/// branch (a 1×1 array collapses to its single cell and broadcasts), or the whole scalar branch. An
/// out-of-range index (only reachable on a shape skew already screened by [`map_if`]) is a defensive
/// `#N/A`, never a panic (totality).
fn branch_cell(v: &Value, i: usize) -> Value {
    if let Value::Array(s, cells) = v {
        if s.rows == 1 && s.cols == 1 {
            return cells.first().cloned().unwrap_or(Value::Blank);
        }
        return cells.get(i).cloned().unwrap_or(Value::Error(ErrKind::Na));
    }
    v.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `rows`×`cols` array value (blank-filled — only its [`Shape`] matters here).
    fn arr(rows: u32, cols: u32) -> Value {
        Value::Array(
            Shape { rows, cols },
            vec![Value::Blank; (rows * cols) as usize],
        )
    }

    #[test]
    fn two_scalar_positions_of_different_shapes_are_a_value_error() {
        // The forward-ready multi-position path (the later multi-criteria `*IFS` batch): two
        // broadcasting positions holding differently-shaped arrays share no map shape -> #VALUE!.
        // No v1 function has two broadcast positions, so this branch is only reachable at unit level.
        assert_eq!(
            broadcast_shape(&[arr(3, 1), arr(2, 1)], &[0, 1]),
            Err(ErrKind::Value)
        );
    }

    #[test]
    fn two_scalar_positions_of_equal_shape_share_one_shape() {
        assert_eq!(
            broadcast_shape(&[arr(3, 1), arr(3, 1)], &[0, 1]),
            Ok(Some(Shape { rows: 3, cols: 1 }))
        );
    }

    #[test]
    fn a_1x1_array_in_a_scalar_position_does_not_force_a_broadcast() {
        // A 1×1 array collapses to its cell downstream, so it sets no map shape (an ordinary call).
        assert_eq!(broadcast_shape(&[arr(1, 1)], &[0]), Ok(None));
    }
}
