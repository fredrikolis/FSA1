<!-- Concern: charlie's product specification — the vocabulary and invariants (each a fitness function) the implementation must satisfy | Non-concern: how it is built (the crates implement against this, they do not edit it) and the operating process (see CLAUDE.md) | IO: none -->
# charlie — specification

The highest-level, implementation-free contract for charlie: a **vocabulary** of defined entities and
a set of **invariants** — timeless, positively-stated constraints on what the architecture *is*. Each
invariant carries a **fitness function**: the objective check that proves it.

The specification is **authoritative over the implementation** (spec-first): the code conforms to it,
never the reverse. It is a **living document owned by the product-manager workspace**; an invariant is
added or retired here, and a change to the code may not contradict an established invariant.

## Vocabulary

- **FT‑1 · A1 addressing.** Addressing is Excel A1. — *`A1`, `$C$7`, `F2:F11` resolve to exactly the cells a spreadsheet user expects.*
- **FT‑2 · workbook / tab / file.** A workbook is a folder of tabs; a tab is a folder of files. — *each sub-folder of the workbook is a tab; a cross-tab reference is `Tab!A1`.*
- **FT‑3 · closed range.** A file's name is a closed range — a bounded rectangle of A1 cells with inclusive endpoints. — *the file for rows 2–11 of column F is `F2:F11`; one cell is `A1`; a 3-wide by 8-tall block is `B2:D9`.*
- **FT‑4 · grid.** A file resolves to a grid: for every coordinate in its closed range, one cell — an explicit value or an explicit `=formula`. — *the grid of `F2:F11` is exactly ten cells, each a value or a formula.*
- **FT‑5 · deserializer / generator.** A deserializer turns a file's content into its grid. A generator is a deserializer that computes the grid from a compact form rather than reading it cell-for-cell. — *a deserializer takes file content in some format and produces a grid; a generator's grid is the same artifact a file would otherwise list by hand.*
- **FT‑6 · engine.** The engine is the evaluator: it computes each cell's value from grids, against an abstract cell resolver. — *the engine takes a grid and a resolver and yields values; it consumes nothing else.*

## Invariants

- **FT‑7 · content → grid via a deserializer.** The engine operates only on the grid; a file's format lives entirely in its deserializer, so switching format switches only the deserializer. — *`render --functions` shows the same grid whether a file was written as TSV or produced by a generator; the engine is unchanged when the deserializer changes.*
- **FT‑8 · the grid fills its range.** A grid fills its file's closed range exactly. — *a `B2:D9` file whose grid is not 3×8 is a located dimension error.*
- **FT‑9 · content, not position.** A cell's value derives only from its own content — never from its position. — *two cells with different values have different content; no cell's value depends on where it sits in the grid.*
- **FT‑10 · no value from thin air.** Every value is a literal or a formula over other cells. — *every dependency chain bottoms out in written literals.*
- **FT‑11 · the value model.** Every cell's value is one of {number, text, boolean, error, blank, array}. — *the type of every value is one of the six; no value carries a unit, label, or metadata field.*
- **FT‑12 · evaluation.** Evaluation is demand-driven, memoized, and cycle-safe. — *rendering a viewport touches only the visible cells' dependency cone; each cell computes at most once; a reference cycle is a located `#REF!`.*
- **FT‑13 · totality.** Every input yields a value or a located refusal. — *a malformed file or formula yields a diagnostic pointing at the offending location; no input produces a crash or a silent wrong value.*
- **FT‑14 · engine independence.** The engine is independent of storage and presentation. — *the engine runs against an in-memory resolver with no filesystem present; storage or rendering can be replaced without changing it.*

## The current deserializer

A grid is deserialized from **TSV**: tab-separated columns, newline-separated rows; each cell is a
literal or an `=formula`. An empty field is a blank cell — a double tab (`a⇥⇥b`) makes the middle cell
blank, as do leading and trailing empty fields. Because the engine operates only on the grid (FT‑7),
TSV is the *current* deserializer; another format, or a generator, is a new deserializer and leaves
the engine untouched.
