<!-- Concern: the repo's public orientation — what charlie-cli is, its current status, and the target crate architecture | Non-concern: how work is done here (posture, the commit gate, delegation — CLAUDE.md owns those) and the formula-engine internals (charlie-ast owns those) | IO: none -->
# charlie

**Production Rust workspace for `charlie-cli`** — a spreadsheet whose storage and edit surface
is a plain filesystem (tabs = folders, cells/ranges = annotated files), rendered and linted from
the terminal. Named for Charles Simonyi, the creator of Excel.

## Status

Build-out, **unreleased**. The first product crate — `charlie-ast`, the formula-language contract
surface — has landed; contracts are still ours to break freely until a real consumer exists. Code
lands only when a bet is proven in the `project-charlie` workspace and a prod-native plan
graduates. See `CLAUDE.md` for posture and the commit gate.

## Architecture (target)

- `charlie-ast` — the formula language: AST + parser + evaluator. Source-free core, provenance in
  side-channels, located refusals. No knowledge of the filesystem. (`ast-standards.md`)
- `charlie-model` — the filesystem spreadsheet model: tabs, ranges, overlap detection, on-demand
  evaluation. Consumes `charlie-ast` through a narrow trait; nothing of xlsx.
- `charlie-cli` — the thin binary: `render`, `check`, and friends. ASCII table output.
- (later) `charlie-xlsx` — import/export serde. Not part of the core.

## Context

This repo is the graduation target of the outer `project-charlie` product-manager workspace.
Coding standards live in `~/.knowledge-base/coding-standards/`.
