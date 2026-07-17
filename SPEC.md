<!-- Concern: charlie's product specification — the vocabulary and invariants (each a fitness function), grouped by subsystem, that the implementation must satisfy | Non-concern: how it is built (the crates implement against this, they do not edit it) and the operating process (see CLAUDE.md) | IO: none -->
# charlie — specification

The highest-level, implementation-free contract for charlie: a **vocabulary** of defined entities and
a set of **invariants** — timeless, positively-stated constraints on what the architecture *is*. Each
invariant carries a **fitness function**: the objective check that proves it.

The specification is **authoritative over the implementation** (spec-first): the code conforms to it,
never the reverse. It is a **living document owned by the product-manager workspace**; an invariant is
added or retired here, and a change to the code may not contradict an established invariant.

Items are grouped by **subsystem** and named by a stable, subsystem-scoped id — `CORE`, `FS`, `GRID`,
`VAL`, `ENG`, `CLI`. An id names one item for good (grep it to find every reference); new items append
within their subsystem. Within each subsystem the **definitions** come first, then the **invariants**
that constrain them.

## CORE — foundations (cross-cutting)

- **CORE1 · A1 addressing.** Addressing is Excel A1. — *`A1`, `$C$7`, `F2:F11` resolve to exactly the cells a spreadsheet user expects.*
- **CORE2 · totality.** Every input yields a value or a located refusal. — *a malformed file or formula yields a diagnostic pointing at the offending location; no input produces a crash or a silent wrong value.*

## FS — on-disk layout

- **FS1 · workbook / tab / file.** A workbook is a folder of tabs; a tab is a folder of files. — *each sub-folder of the workbook is a tab; a cross-tab reference is `Tab!A1`.*
- **FS2 · closed range.** A file's name is a closed range — a bounded rectangle of A1 cells with inclusive endpoints. — *the file for rows 2–11 of column F is `F2:F11`; one cell is `A1`; a 3-wide by 8-tall block is `B2:D9`.*

## GRID — deserialization (content → grid)

- **GRID1 · grid.** A file resolves to a grid: for every coordinate in its closed range, one cell — an explicit value or an explicit `=formula`. — *the grid of `F2:F11` is exactly ten cells, each a value or a formula.*
- **GRID2 · deserializer / generator.** A deserializer turns a file's content into its grid. A generator is a deserializer that computes the grid from a compact form rather than reading it cell-for-cell. — *a deserializer takes file content in some format and produces a grid; a generator's grid is the same artifact a file would otherwise list by hand.*
- **GRID3 · content → grid via a deserializer.** The engine operates only on the grid; a file's format lives entirely in its deserializer, so switching format switches only the deserializer. — *`render --functions` shows the same grid whether a file was written as TSV or produced by a generator; the engine is unchanged when the deserializer changes.*
- **GRID4 · the grid fills its range.** A grid fills its file's closed range exactly. — *a `B2:D9` file whose grid is not 3×8 is a located dimension error.*

### The current deserializer

A grid is deserialized from **TSV**: tab-separated columns, newline-separated rows; each cell is a
literal or an `=formula`. An empty field is a blank cell — a double tab (`a⇥⇥b`) makes the middle cell
blank, as do leading and trailing empty fields. Because the engine operates only on the grid (GRID3),
TSV is the *current* deserializer; another format, or a generator, is a new deserializer and leaves
the engine untouched.

## VAL — the cell & value model

- **VAL1 · content, not position.** A cell's value derives only from its own content — never from its position. — *two cells with different values have different content; no cell's value depends on where it sits in the grid.*
- **VAL2 · no value from thin air.** Every value is a literal or a formula over other cells. — *every dependency chain bottoms out in written literals.*
- **VAL3 · the value model.** Every cell's value is one of {number, text, boolean, error, blank, array}. — *the type of every value is one of the six; no value carries a unit, label, or metadata field.*

## ENG — the engine (evaluation)

- **ENG1 · engine.** The engine is the evaluator: it computes each cell's value from grids, against an abstract cell resolver. — *the engine takes a grid and a resolver and yields values; it consumes nothing else.*
- **ENG2 · evaluation.** Evaluation is demand-driven, memoized, and cycle-safe. — *rendering a viewport touches only the visible cells' dependency cone; each cell computes at most once; a reference cycle is a located `#REF!`.*
- **ENG3 · two-pass, contained evaluation.** Evaluation is two-pass: a *plan* pass builds one dependency graph of the cells a render demands — built up and merged as render ops are added, so a shared dependency appears once — and an *evaluate* pass computes each cell in dependency order (compute-once is ENG2). The graph is a contained optimization: its type is private to the engine, and it equals a naive per-cell evaluation. — *a differential test evaluates complex dependency chains both ways — naive per-cell and two-pass — and asserts identical values; the dependency-graph type appears in no other module's surface; removing the sharing changes performance, never results.*
- **ENG4 · reuse survives only under unchanged inputs.** A stored result may be reused for a cell if and only if its content is unchanged and no result upstream of it has been invalidated — a cell's result being a deterministic function of its content and its dependencies' results (VAL1, VAL2). — *editing a cell — or any cell upstream of it — invalidates exactly its dependents; a stored result is reused only while its content is unchanged and nothing upstream has been invalidated, and recomputing an unchanged cell reproduces its value.*
- **ENG5 · engine independence.** The engine is independent of storage and presentation. — *the engine runs against an in-memory resolver with no filesystem present; storage or rendering can be replaced without changing it.*

## CLI — the tool

- **CLI1 · self-contained tool.** The tool is charlie-cli, a command-line program; everything needed to author, render, validate, and repair a workbook is reachable from charlie-cli alone. — *`charlie-cli --guide` states the format's rules and `charlie-cli sample` writes a workbook that loads and renders; every refusal carries its location (CORE2); an agent given only charlie-cli — no companion document, repository, or service — can author a valid workbook and repair a rejected one.*
