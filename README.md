# charlie

**Production Rust workspace for `charlie-cli`** — a spreadsheet whose storage and edit surface
is a plain filesystem (tabs = folders, cells/ranges = annotated files), rendered and linted from
the terminal. Named for Charles Simonyi, the creator of Excel.

## Status

Bootstrap / walking-skeleton, **unreleased**. Zero product crates yet — this repo bootstraps
exploration-first. Code lands only when a bet is proven in the `project-charlie` workspace and a
prod-native plan graduates. See `CLAUDE.md` for posture and the commit gate.

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
