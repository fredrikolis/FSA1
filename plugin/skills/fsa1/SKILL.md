---
name: fsa1
description: >
  Use when the user wants to LOAD, read or edit an Excel file (.xlsx) or an OpenDocument spreadsheet
  (.ods), or to work with an FSA1 workbook — a spreadsheet stored AS a filesystem (tabs = folders,
  each file's name is the A1 range it fills). Reach for it to unpack an Excel file into an editable
  file tree, render/inspect a workbook directory as a grid, lint it, trace or evaluate formulas, or
  pack a tree back into Excel. Only .xlsx and .ods are read; other spreadsheet formats are refused.
  The `fsa1-cli` command is on PATH while this plugin is enabled; invoke it via Bash.
---

<!-- Concern: teaches an agent when to reach for fsa1-cli and how to drive it | Non-concern: installing the binary (bin/fsa1-cli), the format's invariants (docs/format-spec.md) | IO: none -->
# fsa1-cli

`fsa1-cli` is a spreadsheet engine over a **filesystem** representation of a workbook. It is on PATH
while this plugin is enabled — run it with the Bash tool. First run on a new machine may pause briefly
while the launcher fetches or builds the native binary (subsequent runs are instant).

## The model in one paragraph

A workbook is a **directory**. Each **tab is a folder** inside it. Each **cell or range is a file**
whose *name is its A1 address* and whose *content is its grid* (tab-separated cells, newline-separated
rows). A single cell is `A1`; a filled rectangle is a range file with two legal name spellings:
`A1:C1` (POSIX-native) and `A1-C1` (portable / Windows-safe, because `:` is illegal in a Windows
filename). The **reader accepts both on every platform**; each host *writes* the one native to it
(`:` on POSIX, `-` on Windows), and `convert` re-spells a tree for another OS. The logical range
operator stays `:` everywhere it is a *value* (formulas, path selectors, `check`/`trace` output)
regardless of platform — only the on-disk file NAME differs. Named cells/ranges are separate files
(a `Name` ref-file, or `Name.begin` / `Name.end` for a range).

**There is no `write` command by design** — the filesystem *is* the write surface. To edit a workbook,
write the cell files directly with ordinary file tools, then use `fsa1-cli` to read them back.

## When to use it

- The user points at a directory that is an FSA1 workbook and wants to see, validate, or query it.
- The user has a `.xlsx` or `.ods` file and wants it as a greppable/diffable/editable file tree
  (`unpack`), or wants an FSA1 tree turned back into a `.xlsx` (`pack`).
- The user asks to evaluate a formula against a workbook, trace a cell's dependencies, or lint for
  overlaps / dimension mismatches / cycles.
- You (the agent) need to author or edit spreadsheet data as plain files and then confirm it loads.

## Commands

Path grammar: `<wb>[/<tab>[/<A1>]]` — the tab and cell/range are PART OF THE PATH.

- `fsa1-cli render <path> [--mode combined|values|functions] [--format ascii|html]` — draw the scope
  as ASCII (or standalone HTML). Bare `<wb>` draws every tab; `<wb>/<Tab>` one tab; `<wb>/<Tab>/A1:D9`
  a region. Default `combined` shows `value ← =formula`.
- `fsa1-cli check <path>` — lint (overlap, dimension mismatch, cycles). Non-zero exit on error-severity
  diagnostics. Scope to a tab/range to lint only that.
- `fsa1-cli eval <path> --formula '=<formula>'` — evaluate an ad-hoc formula against the workbook.
- `fsa1-cli trace <path> [--dependents] [--depth N]` — a cell's upstream deps (or downstream consumers).
- `fsa1-cli tree <path> [--mode ...]` — the whole structure (every tab, cell, name) as a nested view.
- `fsa1-cli sample <dir>` — write a live tutorial workbook (refuses a non-empty dir).
- `fsa1-cli unpack [--strict] [--decompose <policy>] <src.xlsx|src.ods> [<dst>]` — real spreadsheet →
  FSA1 tree. `<dst>` defaults to `./<src-stem>/`. Refuses a non-empty destination.
- `fsa1-cli pack <workbook-dir> [--target xlsx]` — FSA1 tree → a fresh `./<basename>.xlsx` (never
  clobbers; leaves the source untouched).
- `fsa1-cli convert <workbook-dir> [--to posix|windows|auto]` — re-spell range file names between
  `A1:C1` (posix) and `A1-C1` (windows/portable) so a raw tree checks out on another OS. Only range
  file names change. `--to posix` only works on a POSIX host (`:` is not a legal Windows filename).
- `fsa1-cli --help` / `fsa1-cli <command> --help` / `fsa1-cli --guide` — full surface and the on-disk grammar.

## Common invocations

```bash
fsa1-cli sample ./demo && fsa1-cli render ./demo         # see it work end to end
fsa1-cli unpack book.xlsx && fsa1-cli render ./book      # xlsx -> tree, then draw it
fsa1-cli check ./budget                                  # lint a workbook
fsa1-cli eval ./budget/Orders --formula '=SUM(D2:D4)'    # ad-hoc formula
fsa1-cli pack ./budget                                   # tree -> ./budget.xlsx
```

Authoring a cell directly (no write command — the file IS the cell):

```bash
mkdir -p ./budget/Sheet1
printf '=SUM(A1:A2)' > ./budget/Sheet1/H3    # filename IS the A1 address; ':' ranges are fine here
fsa1-cli check ./budget/Sheet1/H3            # validate just that cell
```

## Notes & gotchas

- **Exit codes:** `0` ok · `1` I/O · `2` bad args · `3` validation error (or a workbook that won't
  load) · `4` conflict (never-clobber refusal) · `24` not found. Check them when scripting.
- **`:` vs `-` in file NAMES:** a range file may be named `A1:C1` or `A1-C1`; the reader accepts both
  on every platform, so when you *create* one yourself either works (prefer `A1-C1` if the tree may be
  used on Windows, since `:` cannot be written there). In formulas and path selectors always use `:` —
  that is platform-independent. Use `fsa1-cli convert` to normalize an existing tree's spelling.
- `unpack` prints a **fidelity report** of anything the conversion changed; it is not an error, but
  read it. `pack` and the never-clobber commands refuse to overwrite existing files.
- Output goes to **stdout**; diagnostics and the launcher's own messages go to **stderr**. When you
  need just the data, read stdout.
