<!-- Concern: the repo's public orientation — what charlie-cli is, its status, and the crate architecture | Non-concern: how work is done here (posture, the commit gate, delegation — CLAUDE.md owns those), the contract (SPEC.md), and the command/flag surface (charlie-cli --help owns that) | IO: none -->
# charlie

**Production Rust workspace for `charlie-cli`** — a command-line tool that renders, lints, and
evaluates a spreadsheet stored as a plain **filesystem**: tabs are folders, cells and ranges are
files named by their A1 range. The spreadsheet *is* the filesystem; `charlie-cli` reads and computes
it — it is not itself a spreadsheet. Named for Charles Simonyi.

## Status

Build-out, **unreleased**. Five crates realize a `cli → model → ast` firewall: `charlie-ast` (the
formula language), `charlie-model` (the filesystem spreadsheet model), `charlie-ingest`
(`.xlsx`/`.ods` import), `charlie-cli` (the thin binary), and `conformance` (Excel-parity grading).
Contracts are ours to break until a real consumer exists. See `CLAUDE.md` for posture and the commit
gate, and `SPEC.md` for the contract.

## Using it

```
cargo run -p charlie-cli -- --guide     # the on-disk model + authoring, in one screen
cargo run -p charlie-cli -- --help      # the command / flag / exit-code surface (source-owned)
cargo run -p charlie-cli -- sample ./demo && cargo run -p charlie-cli -- tree ./demo
```

`charlie-cli` renders (`render`, `tree`), lints (`check`), evaluates (`eval`), traces dependencies
(`trace`), and imports (`import`) a workbook. The authoritative surface lives in `--help` and
`--guide` — this README stays short on purpose, so it can't go stale.

## Architecture

- `charlie-ast` — the formula language: AST + parser + evaluator. No filesystem knowledge.
- `charlie-model` — the filesystem spreadsheet model: tabs, ranges, overlap, demand-driven eval, the
  persistent result cache. Consumes `charlie-ast` through a narrow trait.
- `charlie-ingest` — `.xlsx` / `.ods` import (the format firewall; `calamine` is confined here).
- `charlie-cli` — the thin binary; text output.
- `conformance` — the Excel-parity grading harness.

## Context

Graduation target of the `charlie-cli-pm` product-manager workspace. Coding standards live in
`~/.knowledge-base/coding-standards/`.
