// Concern: charlie-ast — the spreadsheet-formula LANGUAGE, exposed as its contract surface AND its working engine: the source-free semantic AST (`Expr`), first-class `Value`/`ErrKind`, A1 references (`RefNode`/`CellRef`/`RangeRef`) and the shared A1 address grammar (`a1`), the `NodeId` identity key, the `Resolver` boundary that is the engine's entire view of the outside world, and — since W3 — the LEXER, Pratt PARSER, tree-walking EVALUATOR, the data-driven function REGISTRY, the located-refusal diagnostic registry (`diag`), and the schema-from-types emitter (`schema`) | Non-concern: the filesystem/cells-on-disk model, tab/range/overlap layout, xlsx serde, and the CLI surface — charlie-model and charlie-cli own those and the AST never learns of them; the ~70-function grind + criteria/lookup/spill land in W3b | IO: a formula `&str` -> `Result<Expr, Diag>` (`parse`) and (`&Expr`, `&dyn Resolver`) -> `Value` (`eval`), over the AST/`Value`/`Resolver` contract TYPE surface and the A1 grammar (`parse_a1`/`format_cell`); the engine is FILESYSTEM-BLIND — its whole outside world is the `Resolver` it is handed
//! # charlie-ast — the formula engine
//!
//! **CHARTER.** `charlie-ast` owns the *formula language* inside a cell: how a formula is
//! shaped ([`Expr`]), what it can evaluate to ([`Value`] / [`ErrKind`]), how it names other cells
//! ([`RefNode`] / [`CellRef`] / [`RangeRef`]), and — since **W3** — how a formula *string* becomes a
//! tree ([`parse`]) and how that tree becomes a value ([`eval`]) against a handed [`Resolver`]. It is
//! the innermost crate of the firewall `charlie-cli → charlie-model → charlie-ast`: it never depends
//! on, and never learns of, the filesystem model, xlsx, or the terminal. Its *entire* view of the
//! outside world is the [`Resolver`] trait — it does **no I/O of its own** (a `schema` golden is
//! `include_str!`-embedded at compile time, not read at runtime), so the engine stays filesystem-blind.
//!
//! This crate follows `ast-standards.md` (PRIMARY): a three-layer, abstract/semantic AST, with a
//! never-panicking parser (the one defended boundary) whose holes are **located refusals** ([`Diag`])
//! *beside* the tree, and whose grammar is single-sourced as a schema generated from these types
//! ([`schema::emit`]).
//!
//! - **Meaning** lives *in* the node ([`Expr`]) — source-free, typed per construct.
//! - **Identity** is a [`NodeId`] key **excluded from equality/hashing** (constant-`Eq`).
//! - **Provenance** (spans on a refusal, later side-channels) lives *off* the node — a [`Diag`]
//!   carries a byte [`Span`]; the `Expr` itself is span-free.
//!
//! ## W3 scope + escalations
//!
//! v1 ships the machinery (lex/parse/eval/registry) plus a *few* foundational functions across
//! categories (`SUM AVERAGE COUNT · IF IFERROR AND OR · ABS ROUND`) to prove the registry; the
//! ~70-function grind and the criteria/lookup/spill semantics are W3b. The reserved `@`/`#` nodes
//! parse-and-preserve (eval deferred to identity / `#CALC!`, scope.md). **Cross-sheet references
//! `Sheet1!A1` are a located refusal here** (`reserved-cross-sheet`): resolving a sheet name is a
//! [`Resolver`] (eval-time) act, but [`RefNode`]'s `sheet: Option<SheetId>` has no way to carry a
//! sheet *name* from parse to eval — closing that gap (a name-carrying ref, or a parse-time resolver
//! seam) is the one substrate decision W3 defers rather than guessing. See the notes in the plan.

pub mod a1;
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
pub use eval::{EvalCtx, eval};
pub use expr::{BinOp, Expr, FuncId, UnOp};
pub use func::FuncDef;
pub use lexer::{Token, TokenKind, tokenize};
pub use node::NodeId;
pub use parser::parse;
pub use refs::{CellRef, RangeRef, RefNode, SheetId};
pub use resolver::Resolver;
pub use value::{ArrayView, ErrKind, Shape, Value};
