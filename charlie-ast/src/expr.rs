// Concern: the source-free MEANING layer — the semantic `Expr` node enum (Lit/Ref/Range/Unary/Binary/Call) plus the reserved, round-trip-preserving `ImplicitIntersect`/`SpillRef` nodes, the operator vocabulary (`UnOp`/`BinOp`) with the `FuncId` registry handle, and `offset_refs` — the pure DRAG-FILL transform (re-exported from lib.rs) that returns a copy of an `Expr` with every RELATIVE ref/range-corner shifted by `(d_row, d_col)` and `$`-anchored axes fixed, `None` if any relative ref moves off-sheet (`#REF!`) | Non-concern: node identity (`node::NodeId`), provenance/spans/refusals (id-keyed side-channels, later), and evaluation of any of it (`offset_refs` never evaluates and never touches a `Resolver`) | IO: none — the tree's value type
//! Meaning layer: [`Expr`] and its operator/function vocabulary.

use crate::refs::{RangeNode, RefNode};
use crate::value::Value;

/// A unary operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnOp {
    /// Prefix `+` (identity; preserved so it round-trips).
    Plus,
    /// Prefix `-` (negation).
    Neg,
    /// Postfix `%` (percent — divide by 100).
    Percent,
}

/// A binary operator, spanning arithmetic, string concat, and the comparisons.
///
/// The reference operators (`:` range, ` ` intersection, `,` union) are **not** here: a static
/// range parses directly to [`Expr::Range`], and the dynamic reference-operator forms are reserved
/// for a later phase — so this enum stays the value/comparison vocabulary only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    /// `^` exponentiation.
    Pow,
    /// `&` string concatenation.
    Concat,
    /// `=` equal.
    Eq,
    /// `<>` not equal.
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// A handle into the function registry (`FuncId → { arity, eval_fn }`, built in a later phase).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FuncId(pub u32);

/// A formula expression node — the source-free, meaning-only core of the AST.
///
/// Per `ast-standards.md`: this is an abstract/semantic tree, typed per construct, holding
/// meaning only. Identity ([`crate::NodeId`]) and provenance live off the node. `ImplicitIntersect`
/// (`@`) and `SpillRef` (`#`) are **RESERVED**: the parser preserves them so a round-trip never
/// loses them, but evaluation is deferred in scalar-only v1 (see `docs/architecture.md` §4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr {
    /// A literal value.
    Lit(Value),
    /// A single-cell reference (`A1`, `$A$1`, `Sheet1!A1`).
    Ref(RefNode),
    /// A rectangular range (`A1:B10`, `Sheet1!A1:B10`).
    Range(RangeNode),
    /// A unary operation.
    Unary(UnOp, Box<Expr>),
    /// A binary operation.
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// A function call.
    Call(FuncId, Vec<Expr>),
    /// RESERVED: implicit intersection `@expr`. Parsed and preserved; eval deferred in v1.
    ImplicitIntersect(Box<Expr>),
    /// RESERVED: spill reference `expr#`. Parsed and preserved; eval deferred in v1.
    SpillRef(Box<Expr>),
}

/// DRAG-FILL an expression: return a copy of `expr` with every RELATIVE reference (single-cell or
/// range corner) shifted by `(d_row, d_col)`, leaving `$`-anchored axes fixed (`docs/format.md` §10).
///
/// This is the drag-fill transform a multi-cell `=formula` range applies once per non-anchor cell: the
/// anchor (top-left) cell holds the authored formula, and each other cell at delta `(d_row, d_col)`
/// from the anchor evaluates `offset_refs(&anchor_formula, d_row, d_col)`. Only [`Expr::Ref`] and
/// [`Expr::Range`] carry coordinates; every other node just maps its children. Returns `None` iff any
/// relative reference would move off-sheet (a coordinate below `1` / row 0, or past the `u32` grid) —
/// the evaluator maps that whole-cell failure to `#REF!`. Literals and the reserved `@`/`#` wrappers
/// are structurally preserved. It never evaluates and never touches a `Resolver` — a pure rewrite.
pub fn offset_refs(expr: &Expr, d_row: i64, d_col: i64) -> Option<Expr> {
    Some(match expr {
        Expr::Lit(v) => Expr::Lit(v.clone()),
        Expr::Ref(r) => Expr::Ref(r.offset(d_row, d_col)?),
        Expr::Range(rn) => Expr::Range(rn.offset(d_row, d_col)?),
        Expr::Unary(op, inner) => Expr::Unary(*op, Box::new(offset_refs(inner, d_row, d_col)?)),
        Expr::Binary(op, l, r) => Expr::Binary(
            *op,
            Box::new(offset_refs(l, d_row, d_col)?),
            Box::new(offset_refs(r, d_row, d_col)?),
        ),
        Expr::Call(fid, args) => Expr::Call(
            *fid,
            args.iter()
                .map(|a| offset_refs(a, d_row, d_col))
                .collect::<Option<Vec<_>>>()?,
        ),
        Expr::ImplicitIntersect(inner) => {
            Expr::ImplicitIntersect(Box::new(offset_refs(inner, d_row, d_col)?))
        }
        Expr::SpillRef(inner) => Expr::SpillRef(Box::new(offset_refs(inner, d_row, d_col)?)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::refs::{RangeNode, RefNode};
    use crate::value::Value;

    fn lit(n: f64) -> Box<Expr> {
        Box::new(Expr::Lit(Value::Number(n)))
    }

    #[test]
    fn structurally_identical_trees_are_equal() {
        // Two independently-built `1 + 2` trees ("synthesized == parsed"): meaning-only equality.
        let a = Expr::Binary(BinOp::Add, lit(1.0), lit(2.0));
        let b = Expr::Binary(BinOp::Add, lit(1.0), lit(2.0));
        assert_eq!(a, b);
    }

    #[test]
    fn operator_and_operand_differences_are_visible() {
        let add = Expr::Binary(BinOp::Add, lit(1.0), lit(2.0));
        let sub = Expr::Binary(BinOp::Sub, lit(1.0), lit(2.0));
        let add_swapped = Expr::Binary(BinOp::Add, lit(2.0), lit(1.0));
        assert_ne!(add, sub);
        assert_ne!(add, add_swapped);
    }

    #[test]
    fn reserved_nodes_are_constructible_and_comparable() {
        let a = Expr::ImplicitIntersect(lit(1.0));
        let b = Expr::SpillRef(lit(1.0));
        assert_ne!(a, b);
        assert_eq!(a, Expr::ImplicitIntersect(lit(1.0)));
    }

    fn rel(col: u32, row: u32) -> Expr {
        Expr::Ref(RefNode {
            col,
            row,
            col_abs: false,
            row_abs: false,
            sheet: None,
        })
    }

    #[test]
    fn offset_refs_drags_a_whole_tree() {
        // `=C2*D2` (the F2:F11 body) dragged down one row -> `=C3*D3`: both relative refs shift.
        let body = Expr::Binary(BinOp::Mul, Box::new(rel(2, 1)), Box::new(rel(3, 1)));
        let dragged = offset_refs(&body, 1, 0).unwrap();
        let want = Expr::Binary(BinOp::Mul, Box::new(rel(2, 2)), Box::new(rel(3, 2)));
        assert_eq!(dragged, want);
    }

    #[test]
    fn offset_refs_pins_absolute_refs_inside_a_call() {
        // `=COUNTIF($C$2:$C$13, E2)` dragged down 2 rows: the absolute range is untouched, E2 -> E4.
        let range = Expr::Range(RangeNode {
            start_col: 2,
            start_row: 1,
            end_col: 2,
            end_row: 12,
            start_col_abs: true,
            start_row_abs: true,
            end_col_abs: true,
            end_row_abs: true,
            sheet: None,
        });
        let call = Expr::Call(FuncId(7), vec![range.clone(), rel(4, 1)]);
        let dragged = offset_refs(&call, 2, 0).unwrap();
        let want = Expr::Call(FuncId(7), vec![range, rel(4, 3)]);
        assert_eq!(dragged, want);
    }

    #[test]
    fn offset_refs_off_sheet_relative_ref_is_none() {
        // A1 (relative) dragged UP one row would land at row -1 -> off-sheet -> None (#REF!).
        assert_eq!(offset_refs(&rel(0, 0), -1, 0), None);
        // A literal never fails and never changes.
        let l = Expr::Lit(Value::Number(3.0));
        assert_eq!(offset_refs(&l, -100, -100), Some(l.clone()));
    }
}
