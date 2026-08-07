<!-- Concern: the in-repo design contract — the crate firewall, the fs<->AST boundary, the v1 function scope | Non-concern: the standing coding rules, the commit gate | IO: none -->
# FSA1 — Architecture

This is the shipping repo's own, self-contained design contract. The source files under
`fsa1-ast/src/` cite this document (never a doc outside the repo) so a clone taken alone resolves
every pointer. Governed by `~/.knowledge-base/coding-standards/ast-standards.md` (PRIMARY),
`language-agnostic-programming-standards.md` (SoC / DbC / DRY), and `repo-standards.md`.

## 1. Crate firewall

```
fsa1-cli    // the argv front end — flags, exit codes, help text
fsa1-mcp    // the MCP front end — tool schemas, JSON-RPC on stdio
   → fsa1-verbs  // a verb named by a path, answering a value or a Refusal; neither front end's own
        → fsa1-model   // tabs/ranges/overlap/demand-driven eval/diagnostics — knows no formula grammar, no xlsx
             → fsa1-ast   // the formula language (lex/parse/eval) — knows nothing of the filesystem or xlsx
fsa1-xlsx           → { fsa1-ast, fsa1-model }   // never depended-on by the core
fsa1-html           → fsa1-model   // a rendered view as one standalone HTML document
```

The only allowed dependency direction is `{cli, mcp} → verbs → model → ast`, plus
`verbs → { ingest, xlsx, html }`, `xlsx → { ast, model }` and `html → model`.
A front end is a shell over the verb layer: neither binary is depended on by anything, neither knows
the other exists, and neither reaches the formula language directly. That is what makes a second one
cheap — the MCP server is a different envelope around the same verbs, not a second implementation.
An output format is its own crate: `fsa1-model` owns the filesystem spreadsheet model and `fsa1-cli`
is a thin argv shell, so neither is where a serializer belongs.
This is enforced mechanically by the `deny` edges in `.annotated-tree.toml`, wired the moment both
crates in a pair exist (a `deny` pair naming a crate that does not yet exist is a fatal
`unknown_deny_package`, so the edges land as the crates do). It encodes the owner's hard rule that the
AST implementation is swappable behind a narrow boundary.

## 2. The fs↔AST boundary (the swappability contract)

`fsa1-ast` evaluates against a trait it is handed, never a concrete store. This trait
(`fsa1-ast/src/resolver.rs`) is the engine's **entire** view of the outside world:

```rust
pub trait Resolver {
    fn value(&self, cell: CellRef) -> Value;          // a single resolved cell
    fn range(&self, range: RangeRef) -> ArrayView<'_>; // a rectangular block (borrowed view)
    fn sheet_id(&self, name: &str) -> Option<SheetId>; // cross-sheet name → id
}
```

Because the AST only ever calls these three methods, it has no knowledge that cells might be files on
disk — swap the impl (in-memory test stub, filesystem-backed in `fsa1-model`, xlsx-backed later)
and the engine is unchanged. This is "the AST impl is swappable in principle" made literal.

**Evaluation is synchronous over a pre-loaded model.** Every `Resolver` impl materializes its backing
store before evaluation begins (`fsa1-model` loads the range-files, memoizing); the evaluator then
walks a fully in-memory model with no lazy per-cell I/O. The trait is therefore deliberately
non-`async` — the load-before-loop shape that the async-I/O standard carves out explicitly. This is a
load-bearing, hard-to-reverse choice once the contract freezes, so it is stated here on purpose.

**Borrowed view, by construction.** `ArrayView<'a>` (`fsa1-ast/src/value.rs`) is a **borrowed**
view — `{ shape: Shape, cells: &'a [Value] }` — over cells the `Resolver` already holds; `range`
returns `ArrayView<'_>` tied to `&self`. Being borrowed makes it **categorically distinct** from the
owned `Value::Array(Shape, Vec<Value>)`: they are two roles (an owned literal value vs. a zero-copy
window into the resolver's store), not two copies of one payload, so there is **no reconciliation
obligation** between them and the evaluator borrows range cells rather than copying them. The store
behind the view is materialized before evaluation (see the synchronous-model note above); the
`'a`/lazy-materialization detail lives entirely inside each `Resolver` impl, never in this contract.

DbC: the parser is the one defended boundary; the evaluator trusts the parser's contract.

## 3. The formula AST (per `ast-standards.md`)

- **Abstract/semantic AST** — a tree we evaluate and pretty-print, not a full-fidelity CST. The
  primary consumer is an evaluator, not an editor rendering broken source.
- **Three layers, identity off the node.** Meaning lives in the node (`fsa1-ast/src/expr.rs`);
  `NodeId` identity (`fsa1-ast/src/node.rs`) is **excluded from `Eq`/`Hash`** so a synthesized node
  equals a parsed one (unlocks CSE, dedup, round-trip tests); spans / refusals / resolved types live
  in id-keyed side-channels. A span in equality is an auto-reject.
- **Source-free core, typed per construct** — `Expr = Lit | Ref | Range | Unary | Binary | Call`, plus
  the two RESERVED nodes below. `RefNode` carries `$`-absolute/relative flags so copy/fill offset math
  is trivial.
- **Values and errors first-class.** `Value = Number(f64) | Text | Bool | Error(ErrKind) | Array |
  Blank`; `ErrKind` covers `#REF! #DIV/0! #VALUE! #NAME? #N/A #NULL! #NUM!` (plus reserved `#SPILL!`
  `#CALC!`). Errors propagate through operators. Floats compare **by bit pattern**, so a round-trip
  that flips `-0.0`↔`0.0` or collapses `NaN` is a real, visible difference, never smoothed.

## 4. v1 function scope and the RESERVED constructs

v1 ships a ~70-function core (math/aggregation, statistical, logical, text, date/time,
lookup/reference, information, a few financial) chosen for coverage-per-effort — "build the engine
around the hard semantics, not the long tail" — over the language substrate (A1 refs, absolute/
relative `$`, ranges, cross-sheet refs, the operator ladder, error propagation, the `*IF(S)` criteria
mini-language, and the broadcast-conformance dimension check).

**Recognized but unimplemented — a located `unsupported-function` refusal naming the function
itself.** `LET` and `LAMBDA` bind their own identifiers and v1 has no binding-scope node, so the
refusal must name the FUNCTION rather than letting the first bound identifier surface as a bogus
"defined name" refusal (`fsa1-ast/src/parser.rs`, `BINDING_FORMS`). Nothing is parsed or preserved.

**Shipped since this list was written**, and no longer reserved: named ranges (FS4 — symlinks and
ref-files, resolved at load), `OFFSET`/`INDIRECT` (parse-accepted, resolved by the gated Pass-0 forge
rewrite), and whole-column/row references (`A:A`, `1:1` — folded to an open-axis `Expr::Range` the
resolver clamps to the tab's used bounds).

**Deferred but AST-RESERVED — parse, preserve, evaluate later.** Dynamic-array spill and the spill
operator `#`; implicit intersection `@`; 3D refs; structured table refs. The AST carries the reserved nodes `Expr::ImplicitIntersect` (`@`) and `Expr::SpillRef`
(`#`), and the reserved `ErrKind::Spill`/`ErrKind::Calc`, **so a round-trip never loses them**;
evaluation is identity/deferred in the scalar-only v1. This is the "scalar-only v1" the source
comments refer to.

## 5. Build phases

The workspace advances in gated phases; the labels below name the same milestones the source and
config comments use.

| Phase | Delivers |
| --- | --- |
| **W0** — Bootstrap | Commit gate wired (`.githooks` + selftests + CI); the `fsa1-ast` contract-types skeleton; posture declared build-out. |
| **W1** — Substrate | Frozen sample-directory corpus + an independent oracle + the QA-ladder harness. `.annotated-tree.toml` `forbid_orphans` + the first `deny` edges become meaningful once `fsa1-model` lands. |
| **W2** — Encoding | On-disk format spec + `fsa1-model` skeleton parsing filename↔range + broadcast-conformance validator + overlap detector. |
| **W3** — AST engine | `fsa1-ast`: lexer + Pratt parser + evaluator for the ~70-function set behind `Resolver`; located refusals; no-panic fuzz. `cargo run -p conformance` reports the corpus's graded facts. |
| **W4** — Model + render | Demand-driven eval wired to `fsa1-ast`; `fsa1-cli render` ASCII output (values/functions modes); overlap/dimension/cycle diagnostics. |
