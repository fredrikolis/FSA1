---
name: fsa1
description: >
  Use when the user wants to LOAD, read or edit an Excel file (.xlsx) or an OpenDocument spreadsheet
  (.ods), or to work with an FSA1 workbook — a spreadsheet stored AS a filesystem (tabs = folders,
  each file's name is the A1 range it fills). Reach for it to unpack an Excel file into an editable
  file tree, render/inspect a workbook directory as a grid, lint it, trace or evaluate formulas, or
  pack a tree back into Excel. Only .xlsx and .ods are read; other spreadsheet formats are refused.
  The unpack/pack/render/check/eval/trace verbs come from this plugin's MCP server where it is
  running, and from the fsa1-cli command otherwise — reach for this skill either way; it says how to
  get to them. Cells are files, so read and edit them with ordinary file tools rather than asking
  for a write tool.
---

<!-- Concern: teaches an agent how to reach the FSA1 verbs on either host and how to drive them | Non-concern: what a verb computes, the format's invariants (docs/format-spec.md) | IO: none -->
# FSA1

FSA1 is a spreadsheet engine over a **filesystem** representation of a workbook. Six verbs need the
engine — `unpack`, `pack`, `render`, `check`, `eval`, `trace`. Everything else is ordinary file work:
a cell IS a file, so read one with your file-reading tool and change one with your file-editing tool.
There is no write tool here and none is needed.

## Getting to the verbs

**Look first for the `fsa1` MCP tools.** If tools named `unpack`, `pack`, `render`, `check`, `eval`
and `trace` are available to you, call them and read no further in this section. A first call on a new
machine may pause briefly while `npx` fetches the server package.

**If they are absent**, install the command and drive the same six verbs as shell commands:

```bash
curl -fsSL https://fsa1.sh/install-cli | sh
```

It installs to `~/.local/bin` and only WARNS — it does not fail — when that is not on your `PATH`. So
if the bare name `fsa1-cli` does not resolve, invoke `~/.local/bin/fsa1-cli` by its full path. Use the
`fsa1-cli` column of the table below. The six verb NAMES are the same on both surfaces; four
arguments are not — `formula`, `direction`, `decomposition` and `format` become `--formula`,
`--dependents`, `--decompose` and `--target`, and none can be guessed from the MCP name.

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

Path grammar: `<wb>[/<tab>[/<A1>]]` — the tab and cell/range are PART OF THE PATH. Every MCP tool that
takes a `target` takes it in this form, and on the CLI it is the positional `<path>` argument.

One table, both paths. Optional arguments are in brackets.

| verb | MCP arguments | `fsa1-cli` command |
|---|---|---|
| `render` | `target`, `mode` (`combined\|values\|functions`), `format` (`ascii\|html`) | `fsa1-cli render <path> [--mode <m>] [--format <f>]` |
| `check` | `target`, `xlsx` (boolean) | `fsa1-cli check <path> [--xlsx]` |
| `eval` | `target`, `formula` | `fsa1-cli eval <path> --formula '=<formula>'` |
| `trace` | `target` (one cell), `direction` (`upstream\|downstream`), `depth` | `fsa1-cli trace <path> [--dependents] [--depth <N>]` |
| `unpack` | `source`, `dest`, `decomposition`, `strict` | `fsa1-cli unpack [--strict] [--decompose <policy>] <src> [<dst>]` |
| `pack` | `source`, `dest`, `strict`, `format` (`xlsx`) | `fsa1-cli pack [--strict] [--target xlsx] <workbook-dir> [<dst>]` |

- `render` draws the scope. Bare `<wb>` draws every tab; `<wb>/<Tab>` one tab; `<wb>/<Tab>/A1:D9` a
  region. Default `combined` shows `value ← =formula`. `ascii` cannot draw a FIGURE, so it shows it
  over the cells it covers instead: a figure named for its range (`D2:F6.json`) that no other
  figure's cover reaches into becomes ONE block labelled over two lines, `D2:F6.json` then
  `bar←A1:B3`; every other figure — that one included once another cover intersects it — marks each
  covered cell, `fig` where the cell is empty, `fig! ` before the cell's own text where not. Either
  way it names each figure's cover and bindings in a note, for the figures the path READS: under a
  region path a `<name>.json` is never read and a `<range>.json` only where its range meets the
  region. `tree` marks no cell but NAMES each
  figure beside the tab's entries — `Chart1.json  # bar ← A1:B3`, the mark it draws and the
  ranges it binds — except under a region path, which is a rectangle of cells and lists no figure.
  `format: html` draws the figure itself.
- `check` lints overlap, dimension mismatch, cycles and broken references. Scope it to a tab or range
  to lint only that. `xlsx` (the CLI's `--xlsx`) additionally asks what an `.xlsx` export would NOT
  carry — the same losses `pack` names — and writes no file; it defaults to off on both surfaces.
- `eval` evaluates an ad-hoc formula against the workbook, writing nothing. An error value like
  `#REF!` is the ANSWER, not a failure.
- `trace` walks exactly one cell's dependency chain. **Upstream is the DEFAULT and has no flag at
  all** on the CLI; the downstream direction is `--dependents`. `depth` is `--depth <N>`.
- `unpack` turns a real spreadsheet (.xlsx or .ods) into an FSA1 tree. The MCP `dest` is the CLI's
  optional positional `<dst>` — not a flag — and `decomposition` is `--decompose <policy>`. Omitted,
  the destination derives to `./<source-stem>/`. Refuses a non-empty destination. `strict` (the CLI's
  `--strict`) refuses a source the tree cannot carry back identically instead of unpacking it lossily;
  it defaults to off on both surfaces.
- `pack` turns an FSA1 tree back into a fresh `.xlsx` (never clobbers; leaves the source untouched).
  The MCP `dest` is the CLI's optional positional `<dst>` — **not a flag** — and on both surfaces it
  is used VERBATIM, so its parent directory must already exist (`pack` creates none). Omitted, the
  output name is DERIVED as `./<workbook-basename>.xlsx` in the current directory. MCP `format` is the
  CLI's `--target`; `xlsx` is the only value either accepts, and it names the DERIVED extension only.
  `strict` (the CLI's `--strict`) refuses rather than writing an `.xlsx` that leaves anything out — a
  presentation declaration it cannot carry, or a figure Excel draws no chart for. Without it the file
  is written and every such loss is NAMED, located, under the code `xlsx-not-carried`.

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
  `invalid-arguments`, `validation`, `conflict`, `not-found` and `io`. On the CLI the same refusal is a
  `fsa1-cli: <message>` line on stderr and a non-zero exit. A FINDING is not a refusal: `check`
  reporting errors and `eval` yielding `#REF!` both succeed.
- **`:` vs `-` in file NAMES:** a range file may be named `A1:C1` or `A1-C1`; the reader accepts both
  on every platform, so when you *create* one yourself either works (prefer `A1-C1` if the tree may be
  used on Windows, since `:` cannot be written there). In formulas and path selectors always use `:` —
  that is platform-independent. The `fsa1-cli` command (https://fsa1.sh) has a `convert` verb that re-spells an existing tree.
- **A `.json` file in a tab folder is a FIGURE.** The extension is spent on figures alone, so any
  `<tab>/<name>.json` is read as a Vega-Lite chart spec and `check` lints it — do not park unrelated
  JSON inside a workbook. To author one, write the spec with a `"data": {"name": "A1:D4"}` binding
  (an A1 reference, optionally `<tab>!`-prefixed, naming a rectangle the tab fills whose first row is
  the field names); the stem is a NAME, never a range, so it collides with no cell.
- `unpack` prints a **fidelity report** of anything the conversion changed; it is not an error, but
  read it. `pack` and the never-clobber commands refuse to overwrite existing files.
- Every tool answers with one block of text: the grid, the table, the value or the trace.
