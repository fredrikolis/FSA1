<!-- Concern: the repo's public orientation — what charlie-cli is, its current status, and the target crate architecture | Non-concern: how work is done here (posture, the commit gate, delegation — CLAUDE.md owns those) and the formula-engine internals (charlie-ast owns those) | IO: none -->
# charlie

**Production Rust workspace for `charlie-cli`** — a spreadsheet whose storage and edit surface
is a plain filesystem (tabs = folders, cells/ranges = annotated files), rendered and linted from
the terminal. Named for Charles Simonyi, the creator of Excel.

## Status

Build-out, **unreleased**. Three crates have landed — `charlie-ast` (the formula-language contract
surface), `charlie-model` (the filesystem spreadsheet model), and `charlie-cli` (the thin
render/lint binary) — realizing the `cli -> model -> ast` firewall end-to-end. Contracts are still
ours to break freely until a real consumer exists. Code lands only when a bet is proven in the
`project-charlie` workspace and a prod-native plan graduates. See `CLAUDE.md` for posture and the
commit gate.

## Using the CLI

The binary is `charlie` (crate `charlie-cli`). Run it stack-natively:

```
cargo run -p charlie-cli -- render <workbook-dir> [--tab <name>] [--range <A3:G8>] [--values|--functions|--annotation]
cargo run -p charlie-cli -- check  <workbook-dir>
cargo run -p charlie-cli -- --help      # the full surface (source-owned; never re-enumerated here)
```

`render` draws a tab (or a sub-range) as an ASCII table with a column-letter header and a
row-number gutter — `--values` (default, demand-driven: only the viewport's cone evaluates),
`--functions` (formula text), or `--annotation` (each range's `# ` annotation). `check` lints the
workbook (overlap, dimension-mismatch, cycle) as an ASCII table pointing at the offending file(s)
and exits non-zero on any error-severity diagnostic. The authoritative flag/exit-code list lives in
`charlie --help`, not this README.

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
