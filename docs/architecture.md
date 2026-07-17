<!-- Concern: charlie's in-repo design contract — the crate firewall, the fs↔AST boundary (the swappability contract) and its borrowed-view evolution, the AST/value design, and the v1 function scope with its RESERVED (parse-and-preserve) constructs; the single in-clone home the shipping source files cite | Non-concern: the standing coding rules (they live in `~/.knowledge-base/coding-standards/`, cited by path, never restated here) and the commit gate (see `docs/commit-gate.md`) | IO: none — a design document -->
# charlie — Architecture

This is the shipping repo's own, self-contained design contract. The source files under
`charlie-ast/src/` cite this document (never a doc outside the repo) so a clone taken alone resolves
every pointer. Governed by `~/.knowledge-base/coding-standards/ast-standards.md` (PRIMARY),
`language-agnostic-programming-standards.md` (SoC / DbC / DRY), and `repo-standards.md`.

## 1. Crate firewall

```
charlie-cli    // CLI surface (render/check), ASCII output — knows no formula meaning, no fs internals
   → charlie-model   // tabs/ranges/overlap/demand-driven eval/diagnostics — knows no formula grammar, no xlsx
        → charlie-ast   // the formula language (lex/parse/eval) — knows nothing of the filesystem or xlsx
charlie-xlsx (LATER)   → { charlie-ast, charlie-model }   // never depended-on by the core
```

The only allowed dependency direction is `cli → model → ast` (and, later, `xlsx → { ast, model }`).
This is enforced mechanically by the `deny` edges in `.annotated-tree.toml`, wired the moment both
crates in a pair exist (a `deny` pair naming a crate that does not yet exist is a fatal
`unknown_deny_package`, so the edges land as the crates do). It encodes the owner's hard rule that the
AST implementation is swappable behind a narrow boundary.

## 2. The fs↔AST boundary (the swappability contract)

`charlie-ast` evaluates against a trait it is handed, never a concrete store. This trait
(`charlie-ast/src/resolver.rs`) is the engine's **entire** view of the outside world:

```rust
pub trait Resolver {
    fn value(&self, cell: CellRef) -> Value;          // a single resolved cell
    fn range(&self, range: RangeRef) -> ArrayView<'_>; // a rectangular block (borrowed view)
    fn sheet_id(&self, name: &str) -> Option<SheetId>; // cross-sheet name → id
}
```

Because the AST only ever calls these three methods, it has no knowledge that cells might be files on
disk — swap the impl (in-memory test stub, filesystem-backed in `charlie-model`, xlsx-backed later)
and the engine is unchanged. This is "the AST impl is swappable in principle" made literal.

**Evaluation is synchronous over a pre-loaded model.** Every `Resolver` impl materializes its backing
store before evaluation begins (`charlie-model` loads the range-files, memoizing); the evaluator then
walks a fully in-memory model with no lazy per-cell I/O. The trait is therefore deliberately
non-`async` — the load-before-loop shape that the async-I/O standard carves out explicitly. This is a
load-bearing, hard-to-reverse choice once the contract freezes, so it is stated here on purpose.

**Borrowed view, by construction.** `ArrayView<'a>` (`charlie-ast/src/value.rs`) is a **borrowed**
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
- **Three layers, identity off the node.** Meaning lives in the node (`charlie-ast/src/expr.rs`);
  `NodeId` identity (`charlie-ast/src/node.rs`) is **excluded from `Eq`/`Hash`** so a synthesized node
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

**Deferred but AST-RESERVED — parse, preserve, evaluate later.** Dynamic-array spill and the spill
operator `#`; implicit intersection `@`; 3D refs; structured table refs; named ranges; `LET`/`LAMBDA`;
`OFFSET`. The AST carries the reserved nodes `Expr::ImplicitIntersect` (`@`) and `Expr::SpillRef`
(`#`), and the reserved `ErrKind::Spill`/`ErrKind::Calc`, **so a round-trip never loses them**;
evaluation is identity/deferred in the scalar-only v1. This is the "scalar-only v1" the source
comments refer to.

## 5. Build phases

The workspace advances in gated phases; the labels below name the same milestones the source and
config comments use.

| Phase | Delivers |
| --- | --- |
| **W0** — Bootstrap | Commit gate wired (`.githooks` + selftests + CI); the `charlie-ast` contract-types skeleton; posture declared build-out. |
| **W1** — Substrate | Frozen sample-directory corpus + an independent oracle + the QA-ladder harness. `.annotated-tree.toml` `forbid_orphans` + the first `deny` edges become meaningful once `charlie-model` lands. |
| **W2** — Encoding | On-disk format spec + `charlie-model` skeleton parsing filename↔range + broadcast-conformance validator + overlap detector. |
| **W3** — AST engine | `charlie-ast`: lexer + Pratt parser + evaluator for the ~70-function set behind `Resolver`; located refusals; no-panic fuzz; the coverage ratchet. The conformance backslide state-guard is wired into `.githooks/pre-commit` here. |
| **W4** — Model + render | Demand-driven eval wired to `charlie-ast`; `charlie-cli render` ASCII output (values/functions/annotation modes); overlap/dimension/cycle diagnostics. |
