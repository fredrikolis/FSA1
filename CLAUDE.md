<!-- Concern: the authoritative PROCESS doc for this repo — posture, the commit gate, the AST↔filesystem separation-of-concerns, governing standards, and the first-read path | Non-concern: product facts and domain rules (README.md + the architecture/scope docs own those) and the code itself | IO: none -->
# CLAUDE.md — charlie (production Rust workspace)

**This is the shipping repo for charlie-cli.** It is the graduation target of the outer
`project-charlie` product-manager workspace. Read this file first; it is the authoritative
process document and survives compaction.

---

## Posture (declare every transition)

**Current posture: BUILD-OUT / UNRELEASED.**
Three product crates have landed — `charlie-ast` (the formula-language contract surface),
`charlie-model` (the filesystem spreadsheet model), and `charlie-cli` (the thin CLI shell) — plus the
`conformance` grading crate; all four are committed workspace members. No
external consumer depends on these crates yet, so contracts (the formula AST, the
filesystem-model boundary) are ours to break freely: **break freely and fix every call site in
the same commit — never a backwards-compat shim across a boundary we own both ends of.**
Implementation polish now counts — this is not throwaway scaffolding; the correctness foundation
is reached by a reviewed grind and gated green before "done".

Transitions (bootstrap → build-out → stabilization) are declared here, never silently slid into.
The first real consumer flips this to *stabilization* — back-compat becomes mandatory and a
breaking change becomes a next-major act; re-declare here when that happens.

## What this repo is

charlie-cli renders and lints a spreadsheet that is stored as a **filesystem** (each tab = a
folder; each cell/range = a file whose name encodes its A1 range). The core has two
architecturally-segregated concerns:

- **The formula AST + evaluator** — the math/refs language inside cells. Governed by
  `~/.knowledge-base/coding-standards/ast-standards.md`. Designed so the AST impl is swappable
  in principle (clean boundary), even though we ship one.
- **The filesystem spreadsheet model** — tabs/ranges/overlap-checking/rendering. Consumes the
  AST through a narrow interface; knows nothing of the AST's internals.

The core knows nothing of xlsx/xls. Distribution is a zip of the folder tree. xlsx serde is a
later, separate layer.

## Governing standards (openable paths — resolve after compaction)

- `~/.knowledge-base/coding-standards/ast-standards.md` — the formula AST/engine (PRIMARY)
- `~/.knowledge-base/coding-standards/language-agnostic-programming-standards.md` — SoC, DbC, fail-fast
- `~/.knowledge-base/coding-standards/universal-testing-principles.md` — what/how to test
- `~/.knowledge-base/coding-standards/repo-standards.md` — this repo's posture/layout/gates
- `~/.knowledge-base/coding-standards/cli-interface-standards.md` — the `charlie-cli` surface
- `~/.knowledge-base/coding-standards/preferred-tech-stacks.md` — default picks

The outer workspace (`../`) holds ideation, experiments, and vetted plans. Conclusions graduate
into this repo as prod-native plans; **scratch code never does.**

## Commit gate (wired — `.githooks` + CI)

Enable per clone once: `git config core.hooksPath .githooks`. The gate is three parts (full
rationale: `docs/commit-gate.md`).

**`.githooks/pre-commit`** (mechanical, fail-fast) runs, in order:
1. **Fast checks** — `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`.
2. **Annotations** — `annotated-tree --strict-check --include-tests`, PATH-resolved via `command -v`
   after the hook prepends `$CARGO_HOME/bin` and the npm global bin (non-interactive shells have
   neither on PATH), with an `npx --yes annotated-tree@0.2.1` fallback if no binary is found.
3. **Conformance backslide state-guard** — live and unconditional (`.githooks/pre-commit` builds and
   grades the formula corpus via `cargo run -p conformance -- backslide`, blocking on any regression):
   the `conformance` crate exists and the guard is wired.

The **full test gate** — `cargo test --workspace -- --include-ignored` — is deliberately **NOT**
in the hook; it stays an observed step so it is never chained into the commit action.
`--include-ignored` is mandatory so an `#[ignore]` never silently skips a load-bearing test.

**`.githooks/commit-msg`** (attestation presence-check) requires **two** trailers — **(A or
`Review-skip`) AND (B or `Annotation-skip`)** — severity-tiered, not scored:
- **A — standards review:** `Reviewed: by <reviewer> vs <tag> — major=<n> moderate=<n> minor=<n>`
  (passes iff `major=0 AND moderate=0`; minor is discretionary), or `Review-skip: <reason>`.
- **B — annotation-drift review:** `Annotation-Reviewer: <id>` + `Annotation-Issues: 0`, or
  `Annotation-skip: <reason>`.

The reviews are performed **out-of-band** by an independent reviewer (a workflow / spawned task
agent, never the author self-reviewing) against the standards below; the hook only checks the
trailers are present and well-formed — it cannot run a review. Selftests lock the parser quirks
(`.githooks/{pre-commit,commit-msg}.selftest.sh`) — and CI runs them, so the lock is real, not
aspirational: a hook-parser edit that breaks a locked quirk fails `build.yml`.

**CI** (`.github/workflows/`) mirrors the mechanical checks: `annotations.yml` (the annotation
gate, pinned `annotated-tree@0.2.1`) and `build.yml` (`--locked` fmt + clippy + the full
`--include-ignored` test gate, then both `.selftest.sh` regression tables).

## First-read path

`CLAUDE.md` (this process doc) → `README.md` (orientation) → `SPEC.md` (the product spec — vocabulary
and invariants) → `docs/architecture.md` (the crate firewall and the fs↔AST boundary) →
`docs/commit-gate.md` (the gate). Each shipping source file then carries its own first-line
`Concern | Non-concern | IO` annotation (`annotated-tree` renders the tree from them).
