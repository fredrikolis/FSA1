// Concern: maps a call element-wise over an array in a scalar position | Non-concern: which positions broadcast (the registry row holds it) | IO: (&FuncDef, &mut EvalCtx, &[Expr]) -> Value

use super::*;

/// The arity is already gated by [`super::dispatch`], so `def.eval` may trust it here and below.
pub(crate) fn eval_call(def: &FuncDef, ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let positions = def.broadcast;
    if positions.is_empty() {
        return (def.eval)(ctx, args);
    }
    // Evaluated ONCE up front, so a consumed range resolves a single time however many cells map.
    let vals: Vec<Value> = args.iter().map(|a| ctx.eval(a)).collect();
    let shape = match broadcast_shape(&vals, positions) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let Some(shape) = shape else {
        let lits: Vec<Expr> = vals.into_iter().map(Expr::Lit).collect();
        return (def.eval)(ctx, &lits);
    };
    // A SCALAR error argument short-circuits: tiling it would make `COUNTIF(#REF!, A1:A6)` an array of #REF!, not a scalar one.
    if let Some(Value::Error(k)) = vals.iter().find(|v| matches!(v, Value::Error(_))) {
        return Value::Error(*k);
    }
    map_elementwise(def, ctx, &vals, positions, shape)
}

/// `None` means no scalar position holds a genuine array, so the call is an ordinary scalar one; a
/// 1x1 collapses downstream and never forces a broadcast.
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

/// Indexes defensively, so a shape/length skew yields a `Blank` rather than panicking.
fn element_at(v: &Value, is_scalar_pos: bool, i: usize) -> Value {
    if is_scalar_pos
        && let Value::Array(s, cells) = v
        && !(s.rows == 1 && s.cols == 1)
    {
        return cells.get(i).cloned().unwrap_or(Value::Blank);
    }
    v.clone()
}

/// Called only once `logical::if_fn` has a genuinely multi-cell condition, so array `IF` reuses this
/// home instead of growing a parallel loop. A condition cell that will not coerce is THAT cell's error.
pub(crate) fn map_if(shape: Shape, cond: &[Value], then_v: &Value, else_v: &Value) -> Value {
    // A multi-cell branch must conform to the condition's shape; a scalar or 1x1 broadcasts.
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

/// The [`map_if`] shape rule, applied to the `IFERROR`/`IFNA` fallback. `logical::catch_errors` owns
/// the lazy decision and calls this only once at least one cell is caught.
pub(crate) fn map_catch(
    shape: Shape,
    cells: &[Value],
    fallback: &Value,
    caught: fn(&Value) -> bool,
) -> Value {
    if let Value::Array(s, _) = fallback
        && !(s.rows == 1 && s.cols == 1)
        && *s != shape
    {
        return Value::Error(ErrKind::Value);
    }
    let out = cells
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if caught(c) {
                branch_cell(fallback, i)
            } else {
                c.clone()
            }
        })
        .collect();
    Value::Array(shape, out)
}

/// Indexes defensively: an out-of-range index yields `#N/A` rather than panicking.
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

    fn arr(rows: u32, cols: u32) -> Value {
        Value::Array(
            Shape { rows, cols },
            vec![Value::Blank; (rows * cols) as usize],
        )
    }

    #[test]
    fn two_scalar_positions_of_different_shapes_are_a_value_error() {
        // No v1 function has two broadcast positions, so this path is only reachable at unit level.
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
        assert_eq!(broadcast_shape(&[arr(1, 1)], &[0]), Ok(None));
    }
}
