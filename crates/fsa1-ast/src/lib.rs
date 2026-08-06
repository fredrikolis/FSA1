// Concern: parses a formula to an Expr and evaluates it against a Resolver | Non-concern: the fs model, xlsx serde, rendering a value | IO: (&str) -> Expr; (Expr, Resolver) -> Value
//! The innermost crate of the firewall `fsa1-cli -> fsa1-model -> fsa1-ast`: its entire
//! view of the outside world is the [`Resolver`] trait, and it does no I/O of its own.

pub mod a1;
pub(crate) mod criteria;
pub mod diag;
pub mod eval;
pub mod expr;
pub mod func;
pub mod lexer;
pub mod node;
pub mod parser;
pub mod refs;
pub mod resolver;
pub mod schema;
pub mod value;

#[cfg(test)]
mod test_support;

pub use a1::{A1Address, A1Error, format_cell, format_column, parse_a1};
pub use diag::{Diag, DiagCode, Severity, Span};
pub use eval::{EvalCtx, eval, eval_at, num_to_text, scalarize};
pub use expr::{BinOp, Expr, FuncId, UnOp};
pub use func::{FuncDef, format_value, parse_iso_serial, serial_from_ymd};
pub use lexer::{Token, TokenKind, tokenize};
pub use node::NodeId;
pub use parser::parse;
pub use refs::{CellRef, RangeNode, RangeRef, RefNode, SheetId, SheetName};
pub use resolver::{
    PINNED_NOW_SERIAL, Resolver, UNIX_EPOCH_SERIAL, system_now_secs, unix_secs_to_serial,
};
pub use value::{ArrayView, ErrKind, Shape, Value};
