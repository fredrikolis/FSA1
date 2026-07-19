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
- **CORE3 · the filesystem is the write surface.** A spreadsheet is authored and edited by writing its files directly on the filesystem — that is the only write surface. charlie-cli never mutates a workbook: it reads, renders, validates, evaluates, and traces an existing one; it may materialize a new one from a source (`import`, `sample`) into a fresh location; and it may write only derived, non-authoritative data (`.cache/`, FS3). — *no charlie-cli command changes an authoritative cell, tab, or file of an existing workbook, nor accepts author-supplied cell content to write anywhere; the workbook a materializing command produces (`import`, `sample`) is derived wholly from its source or a fixed template — never from cell values passed on the command line — and into a location that is not already a workbook; an agent authors and repairs by writing files itself (guided by `--guide` and `check`); the only bytes charlie-cli writes under an existing workbook are in `.cache/`. The on-disk format is therefore optimised to be ergonomic to edit by hand — e.g. a cell is its own file — rather than routed through a write command.*

## FS — on-disk layout

- **FS1 · workbook / tab / file.** A workbook is a folder of tabs; a tab is a folder of files. — *each sub-folder of the workbook is a tab, except the reserved `.cache/` (FS3); a cross-tab reference is `Tab!A1`.*
- **FS2 · closed range.** A file's name is a closed range — a bounded rectangle of A1 cells with inclusive endpoints. — *the file for rows 2–11 of column F is `F2:F11`; one cell is `A1`; a 3-wide by 8-tall block is `B2:D9`.*
- **FS3 · reserved cache directory.** `.cache/` at the workbook root is not a tab; it holds only regenerable, non-authoritative derived data. — *deleting `.cache/` never changes any cell's value — no value derives from it (VAL2), only the work to recompute one does; every other sub-folder of the workbook is a tab (FS1).*

## GRID — deserialization (content → grid)

- **GRID1 · grid.** A file's content is exactly its grid: for every coordinate in its closed range, one cell — an explicit value or an explicit `=formula` (unless the whole file is a single array formula that fills the range, GRID5). — *the grid of `F2:F11` is exactly ten cells, each a value or a formula; the file holds those cells and carries no header, annotation, or metadata.*
- **GRID2 · deserializer / generator.** A deserializer turns a file's content into its grid. A generator is a deserializer that computes the grid from a compact form rather than reading it cell-for-cell. — *a deserializer takes file content in some format and produces a grid; a generator's grid is the same artifact a file would otherwise list by hand.*
- **GRID3 · content → grid via a deserializer.** The engine operates only on the grid; a file's format lives entirely in its deserializer, so switching format switches only the deserializer. — *`render --functions` shows the same grid whether a file was written as TSV or produced by a generator; the engine is unchanged when the deserializer changes.*
- **GRID4 · the grid fills its range.** A grid fills its file's closed range exactly. — *a `B2:D9` file whose grid is not 3×8 is a located dimension error.*
- **GRID5 · array formula fills its range.** A file's whole content may instead be a single `=formula` whose value is an array; that array fills the file's closed range exactly, one element per coordinate. This is the only form in which one formula spans more than one cell — there is no dynamic spill beyond a file's declared range, so the range is the author's explicit, bounded spill region. — *a `C1:C3` file whose sole content is `=SORT(A1:A3)` is the sorted 3×1 array in C1:C3; element `(r,c)` of the array fills the range's `(r,c)` coordinate; a single formula whose value is not an array of exactly the range's shape and orientation — a scalar, or a wrong-shaped array — is a located dimension error (GRID4), detected at evaluation; a one-cell file holding an array formula keeps only the array's top-left element; importing a spreadsheet's spilled array over a region yields exactly the range file for that region.*
- **GRID6 · error locality.** A cell whose content cannot be deserialized is a located error value (VAL3) in the grid — not a whole-file failure; unrelated cells still load and evaluate. — *an unparseable formula in one cell deserializes to a located error value — one of the six value types (VAL3), carrying its location — so it renders as that error and `charlie-cli check` reports it with a non-zero exit and its location, while every other cell in the file still yields its value: the error cell is visible and located (CORE2), never a silent drop or skip. Because Excel rejects unparseable content at entry rather than storing it, this load-time error value is a deliberate charlie divergence outside the ENG6 parity corpus (cf. GRID5). Structural faults of the file itself — a grid that does not fill its range (GRID4), a malformed filename (FS2), or overlapping files — remain file-level refusals (CORE2), not cell error values.*

### The current deserializer

A grid is deserialized from **TSV**: the **entire file content is the grid** — there is no header,
annotation, or metadata line, and the first line is the first row. Columns are tab-separated, rows are
newline-separated; each cell is a literal or an `=formula`. An empty field is a blank cell — a double
tab (`a⇥⇥b`) makes the middle cell blank, as do leading and trailing empty fields. A field may
contain a tab, newline, or backslash, written as the escapes `\t`, `\n`, and `\\`; a literal backslash
must always be written `\\`. The deserializer decodes every field by this one rule: only an *unescaped*
tab or newline is a column or row delimiter (so a cell can hold multi-line text), and a backslash always
begins one of these three escapes. A backslash followed by anything else — or a trailing backslash — is
a malformed cell and deserializes to a located error value (GRID6), never a silent literal or a
file-level refusal. Because the engine operates only on the grid (GRID3),
TSV is the *current* deserializer; another format, or a generator, is a new deserializer and leaves
the engine untouched.

## VAL — the cell & value model

- **VAL1 · content, not position.** A cell's value derives only from its own content — never from its position. — *two cells with different values have different content; no cell's value depends on where it sits in the grid (an array region is one array-formula cell spanning its range, not many cells sharing a formula, GRID5).*
- **VAL2 · no value from thin air.** Every value is a literal or a formula over other cells. — *every dependency chain bottoms out in written literals.*
- **VAL3 · the value model.** Every cell's value is one of {number, text, boolean, error, blank, array}. — *the type of every value is one of the six; no value carries a unit, label, or metadata field.*

## ENG — the engine (evaluation)

- **ENG1 · engine.** The engine is the evaluator: it computes each cell's value from grids, against an abstract cell resolver. — *the engine takes a grid and a resolver and yields values; it consumes nothing else.*
- **ENG2 · evaluation.** Evaluation is demand-driven, memoized, and cycle-safe. — *rendering a viewport touches only the visible cells' dependency cone; each cell computes at most once; a reference cycle is a located `#REF!`.*
- **ENG3 · two-pass, contained evaluation.** Evaluation is two-pass: a *plan* pass builds one dependency graph of the cells a render demands — built up and merged as render ops are added, so a shared dependency appears once — and an *evaluate* pass computes each cell in dependency order (compute-once is ENG2). The graph is a contained optimization: its type is private to the engine, and it equals a naive per-cell evaluation. — *a differential test evaluates complex dependency chains both ways — naive per-cell and two-pass — and asserts identical values; the dependency-graph type appears in no other module's surface; removing the sharing changes performance, never results.*
- **ENG4 · reuse survives only under unchanged inputs.** A stored result may be reused for a cell if and only if its content is unchanged and no result upstream of it has been invalidated — a cell's result being a deterministic function of its content and its dependencies' results (VAL1, VAL2). — *editing a cell — or any cell upstream of it — invalidates exactly its dependents; a stored result is reused only while its content is unchanged and nothing upstream has been invalidated, and recomputing an unchanged cell reproduces its value.*
- **ENG5 · engine independence.** The engine is independent of storage and presentation. — *the engine runs against an in-memory resolver with no filesystem present; storage or rendering can be replaced without changing it.*
- **ENG6 · Excel-compatible evaluation.** Every formula charlie evaluates yields the value a mainstream spreadsheet (Excel) yields — across functions, array semantics, reference resolution, and the value model. — *a differential conformance corpus grades charlie cell-for-cell against Excel's own outputs as the reference oracle; a divergence on any covered case is a defect. The corpus is the covered surface: parity holds over exactly the cases it covers, and cases where charlie deliberately diverges (e.g. a reference cycle as a located `#REF!`, ENG2; no dynamic spill, so a single-cell array formula keeps only its top-left element, GRID5) fall outside it.*
- **ENG7 · persistent result cache.** Results persist across invocations, keyed by a *computation hash* — a deterministic digest of a cell's own content together with its dependencies' computation hashes; a stored result is reused only when its hash matches, and such reuse is sound. This is the persistent realization of ENG4. — *a reused result equals what recomputation from scratch produces, and a run with `.cache/` deleted (FS3) yields identical values; a cell whose own or upstream content changed gets a new hash and is not reused; a cell on a reference cycle has no computation hash and is never served from the cache — it recomputes to its located `#REF!` (ENG2); the cache is a contained optimization (cf. ENG3), never a source of value (VAL2), living only in `.cache/`; the hash is an opaque change-detector, not a stable cross-version identifier — changing the hashing scheme only invalidates the cache.*

## CLI — the tool

- **CLI1 · self-contained tool.** The tool is charlie-cli, a command-line program; everything an agent needs to author, render, validate, and repair a workbook is obtainable from charlie-cli alone. — *`charlie-cli --guide` states the format's rules, `charlie-cli sample` writes a workbook that loads and renders, and `charlie-cli check` validates one and locates every fault; every refusal carries its location (CORE2); an agent given only charlie-cli — no companion document, repository, or service — can author a valid workbook by writing its files per `--guide` (the filesystem is the write surface, CORE3) and repair a rejected one from `check`'s located diagnostics.*
- **CLI2 · trace (dependency inspection).** charlie-cli reports a cell's upstream dependencies and downstream consumers. — *`charlie-cli trace <cell>` lists the cells it reads (transitively) and `--dependents` lists the cells that read it (transitively); the reported edges are exactly the engine's own dependency relation (ENG3) — dependents are that relation transposed, not a separately derived parse; it is cycle-safe (a cycle is reported, not looped, ENG2); each node carries its value and, unless it lies on a cycle, its computation hash (ENG7); the engine's internal dependency-graph type never appears in the output (ENG3 containment).*
