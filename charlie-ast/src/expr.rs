// Concern: the source-free MEANING layer — the semantic `Expr` node enum (Lit/Ref/Range/WholeRange/Unary/Binary/Call) plus the reserved, round-trip-preserving `ImplicitIntersect`/`SpillRef` nodes and the operator vocabulary (`UnOp`/`BinOp`) with the `FuncId` registry handle | Non-concern: node identity (`node::NodeId`), provenance/spans/refusals (id-keyed side-channels, later), and evaluation of any of it | IO: none — the tree's value type
//! Meaning layer: [`Expr`] and its operator/function vocabulary.

use crate::refs::{RangeNode, RefNode, WholeRangeNode};
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
    /// A whole-column / whole-row reference (`A:A`, `B:D`, `1:1`, `Sheet1!A:A`) — axis-unbounded.
    /// The filesystem model clamps its open axis to the sheet's used region, rewriting it to a
    /// bounded [`Expr::Range`] BEFORE the engine runs (charlie-ast is bounds-blind), so a
    /// bounds-blind [`crate::eval`] that meets one unbound treats it as `#REF!`.
    WholeRange(WholeRangeNode),
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
