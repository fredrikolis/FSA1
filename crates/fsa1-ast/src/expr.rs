// Concern: declares the Expr node and its operator/function-handle vocabulary | Non-concern: identity, spans, the reference operators, evaluation | IO: none

use crate::refs::{RangeNode, RefNode};
use crate::value::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnOp {
    Plus,
    Neg,
    Percent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Concat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// A handle into the function registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FuncId(pub u32);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr {
    Lit(Value),
    Ref(RefNode),
    Range(RangeNode),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call(FuncId, Vec<Expr>),
    ImplicitIntersect(Box<Expr>),
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
