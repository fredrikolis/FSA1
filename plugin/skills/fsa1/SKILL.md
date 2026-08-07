---
name: fsa1
description: >
  Use when the user wants to LOAD, read or edit an Excel file (.xlsx) or an OpenDocument spreadsheet
  (.ods), or to work with an FSA1 workbook — a spreadsheet stored AS a filesystem (tabs = folders,
  each file's name is the A1 range it fills). Reach for it to unpack an Excel file into an editable
  file tree, render/inspect a workbook directory as a grid, lint it, trace or evaluate formulas, or
  pack a tree back into Excel. Only .xlsx and .ods are read; other spreadsheet formats are refused.
  The unpack/pack/render/check/eval/trace tools come from this plugin's MCP server; cells are
  files, so read and edit them with ordinary file tools rather than asking for a write tool.
---

<!-- Concern: teaches an agent when to reach for the FSA1 tools and how to drive them | Non-concern: installing the server (launcher/fsa1-mcp), the format's invariants (docs/format-spec.md) | IO: none -->
# FSA1

FSA1 is a spreadsheet engine over a **filesystem** representation of a workbook. This plugin's MCP
server exposes the six verbs that need the engine — `unpack`, `pack`, `render`, `check`, `eval`,
`trace`. Everything else is ordinary file work: a cell IS a file, so read one with your file-reading
tool and change one with your file-editing tool. There is no write tool here and none is needed.
First call on a new machine may pause briefly while the launcher fetches the native server.

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
write the cell files directly with ordinary file tools, then use `render` or `check` to read them back.

## When to use it

- The user points at a directory that is an FSA1 workbook and wants to see, validate, or query it.
- The user has a `.xlsx` or `.ods` file and wants it as a greppable/diffable/editable file tree
  (`unpack`), or wants an FSA1 tree turned back into a `.xlsx` (`pack`).
- The user asks to evaluate a formula against a workbook, trace a cell's dependencies, or lint for
  overlaps / dimension mismatches / cycles.
- You (the agent) need to author or edit spreadsheet data as plain files and then confirm it loads.

## Tools

Path grammar: `<wb>[/<tab>[/<A1>]]` — the tab and cell/range are PART OF THE PATH, and every tool
that takes a `target` takes it in this form.

- `render` — `target`, optional `mode` (`combined|values|functions`), `format` (`ascii|html`). Draws
  the scope. Bare `<wb>` draws every tab; `<wb>/<Tab>` one tab; `<wb>/<Tab>/A1:D9` a region. Default
  `combined` shows `value ← =formula`.
- `check` — `target`. Lints overlap, dimension mismatch, cycles and broken references. Scope it to a
  tab or range to lint only that.
- `eval` — `target`, `formula`. Evaluates an ad-hoc formula against the workbook, writing nothing. An
  error value like `#REF!` is the ANSWER, not a failure.
- `trace` — `target` (exactly one cell), optional `direction` (`upstream|downstream`) and `depth`.
- `unpack` — `source` (.xlsx or .ods), optional `dest` and `decomposition`. Real spreadsheet → FSA1
  tree. `dest` defaults to `./<source-stem>/`. Refuses a non-empty destination.
- `pack` — `source` (a workbook directory), optional `dest`. FSA1 tree → a fresh `.xlsx` (never
  clobbers; leaves the source untouched).

There is no write tool, and none is needed: **a cell is a file**. Create, change and move cells with
your own file tools, then use `check` or `render` to confirm the workbook still loads.

## Common sequences

Read a spreadsheet you were handed: `unpack` it, then `render` a tab, or just read the range files
directly — they are text.

Change a value: edit the cell's file with your file-editing tool. The filename IS the A1 address, so
`./budget/Sheet1/H3` holds cell H3; writing `=SUM(A1:A2)` into it makes H3 a formula. Then `check`
that path to validate just that cell.

Hand it back: `pack` the directory to get a fresh `.xlsx`.

## Notes & gotchas

- **A refused tool call** answers with `isError` and one line opening `fsa1: <kind>:` — the kinds are
  `invalid-arguments`, `validation`, `conflict`, `not-found` and `io`. A FINDING is not a refusal:
  `check` reporting errors and `eval` yielding `#REF!` both succeed.
- **`:` vs `-` in file NAMES:** a range file may be named `A1:C1` or `A1-C1`; the reader accepts both
  on every platform, so when you *create* one yourself either works (prefer `A1-C1` if the tree may be
  used on Windows, since `:` cannot be written there). In formulas and path selectors always use `:` —
  that is platform-independent. The `fsa1-cli` command (https://fsa1.sh) has a `convert` verb that re-spells an existing tree.
- `unpack` prints a **fidelity report** of anything the conversion changed; it is not an error, but
  read it. `pack` and the never-clobber commands refuse to overwrite existing files.
- Every tool answers with one block of text: the grid, the table, the value or the trace.
