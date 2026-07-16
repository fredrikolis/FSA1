# CLAUDE.md — charlie (production Rust workspace)

**This is the shipping repo for charlie-cli.** It is the graduation target of the outer
`project-charlie` product-manager workspace. Read this file first; it is the authoritative
process document and survives compaction.

---

## Posture (declare every transition)

**Current posture: BOOTSTRAP / walking-skeleton — UNRELEASED.**
Structure is still forming. Contracts (the formula AST, the filesystem-model boundary) are
being laid and are ours to break freely until the first real consumer exists. Review the
*contracts*, not impl polish; churn is expected. There are **zero product crates** here until
an exploration conclusion in the workspace justifies the first one (exploration-first bootstrap).

Transitions (bootstrap → build-out → stabilization) are declared here, never silently slid into.

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

## Commit gate (wired in Phase 0 of charlie-v1.md)

Every commit must pass, mechanically enforced by `.githooks` + CI:
1. **Tests green** — `cargo test --workspace -- --include-ignored`.
2. **Fast checks** — `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`.
3. **Annotations** — `annotated-tree --strict-check` (full path `/home/olis/.cargo/bin/annotated-tree`
   in hooks: non-interactive shells have no `~/.cargo/bin` on PATH).
4. **Neutral reviewer** score ≥ 9 against the named standard for the change, recorded in the commit.
5. **Annotation-drift review** — every changed first-line annotation re-checked for drift.

Until Phase 0 wires these, treat the list above as the standing bar.

## First-read path

`CLAUDE.md` → `README.md` → (once crates exist) the current crate's plan → `WORKLOG.md`.
