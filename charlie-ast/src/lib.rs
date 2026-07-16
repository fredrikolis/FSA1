// Concern: charlie-ast — the spreadsheet-formula LANGUAGE, exposed as its contract surface: the source-free semantic AST (`Expr`), first-class `Value`/`ErrKind`, A1 references (`RefNode`/`CellRef`/`RangeRef`), the `NodeId` identity key, and the `Resolver` boundary that is the engine's entire view of the outside world | Non-concern: the filesystem/cells-on-disk model, tab/range/overlap layout, xlsx serde, and the CLI surface — charlie-model and charlie-cli own those and the AST never learns of them | IO: none — a W0 skeleton of contract TYPES only; lex, parse, and eval land in later phases
//! # charlie-ast — the formula engine's contract surface
//!
//! **CHARTER.** `charlie-ast` owns the *formula language* inside a cell: how a formula is
//! shaped ([`Expr`]), what it can evaluate to ([`Value`] / [`ErrKind`]), and how it names other
//! cells ([`RefNode`] / [`CellRef`] / [`RangeRef`]). It is the innermost crate of the firewall
//! `charlie-cli → charlie-model → charlie-ast`: it never depends on, and never learns of, the
//! filesystem model, xlsx, or the terminal. Its *entire* view of the outside world is the
//! [`Resolver`] trait it is handed — swap the impl (in-memory, filesystem-backed, a test stub)
//! and the engine is unchanged.
//!
//! This crate follows `ast-standards.md` (PRIMARY): a three-layer, abstract/semantic AST.
//!
//! - **Meaning** lives *in* the node ([`Expr`]) — source-free, typed per construct.
//! - **Identity** is a [`NodeId`] key that is **excluded from equality/hashing** (constant-`Eq`),
//!   so a synthesized node equals a parsed one — the property that unlocks CSE, dedup, and
//!   `emit == parse` round-trip tests.
//! - **Provenance** (spans, located refusals, resolved types) will live in `NodeId`-keyed
//!   side-channels — reserved for later phases, deliberately absent from the node itself.
//!
//! ## W0 posture
//!
//! This is a **skeleton**: the types compile and carry their contracts (float-bit-pattern
//! equality, id-blind identity, the reserved `@`/`#` nodes, the reserved `#SPILL!`/`#CALC!`
//! errors), but no lexer, parser, or evaluator exists yet. Those land in later phases against
//! this frozen shape.

pub mod expr;
pub mod node;
pub mod refs;
pub mod resolver;
pub mod value;

pub use expr::{BinOp, Expr, FuncId, UnOp};
pub use node::NodeId;
pub use refs::{CellRef, RangeRef, RefNode, SheetId};
pub use resolver::Resolver;
pub use value::{ArrayView, ErrKind, Shape, Value};
