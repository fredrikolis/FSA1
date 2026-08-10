<!-- Concern: what an FSA1 workbook is on disk and what it means — the encoding, and the invariants over it | Non-concern: what fsa1-cli must do with one (docs/cli-spec.md owns that) | IO: none -->
# FSA1 — the file-system A1 spreadsheet format

**FSA1** stores a spreadsheet as a filesystem: a workbook is a directory, a tab is a folder, and a
file's *name* is the closed A1 range its content fills. "A1" is Excel's A1 notation, not a version
number.

This document is the **format contract**: what the bytes on disk are, and what they mean. It is
what an independent implementer would read to build a compatible reader or writer. What the
`fsa1-cli` tool must do with a workbook is a separate contract, in `docs/cli-spec.md`.

**Intentionally under-specified.** Anything no clause forbids is admissible. A new capability needs
no clause admitting it, only the absence of one it breaks, so read this to find what a change would
*violate*, never to find permission for it. Over-specify and an agent starts turning down good
requests by citing a clause that was never meant to carry the weight.

**Nothing reads this file mechanically.** No hook under `.githooks/`, no CI job, and no test opens
it. A clause is applied by **judgment**, at triage, before a plan exists, never by a gate. Two things
earn a clause and nothing else does: something certain and permanent, and something an agent has had
to be corrected on more than once. And an invariant is never **derived from the implementation**: a
spec read off the code is cement poured over whatever the code happens to do today, and every
principled refactor afterwards gets fought on the grounds that it goes against spec.

**Enforcement is not spec, and status is not spec.** Which test or corpus proves an invariant is the
repo's business. What is built today, and where FSA1 knowingly diverges, belongs in `ROADMAP.md`. An
invariant that does not yet hold is legitimate here, because it *is* the gap.

## Vocabulary

The nouns the invariants quantify over. A definition is not itself an invariant.

- **workbook** — the file tree FSA1 is pointed at, and everything beneath it.
- **tab** — a folder within a workbook.
- **coordinate** — one A1 position, scoped to a single tab.
- **cell** — the content at one coordinate.
- **value** — what a cell resolves to when computed.
- **the engine** — whatever computes values from cell content, however it is structured.

## FS — the workbook on disk

- **FS1 · the filesystem is the workbook.** A workbook's content is exactly what its file tree holds — a tab per folder, per file the A1 range it fills, the name it declares, the presentation it states over that range or, naming no range, over the tab, or the figure it states over the ranges it binds and fills over the range its own name states, where it states one, and nothing besides — and enters only by an author writing that tree: fsa1-cli reads an existing workbook, derives a new file only where none exists, and never takes cell content from its own invocation.
- **FS3 · tooling coexists with content.** A workbook reserves a named set of tree entries for tooling, and no cell's value derives from any of them.
- **FS5 · every value exists at a coordinate some file declares.** (would violate FS1)
- **FS6 · every coordinate takes its content from at most one file.** (would violate FS1)

## PRES — presentation

- **PRES1 · a selector addresses structure, never a coordinate.** A sidecar's selectors address the shape of its region — the whole of it, an edge row or column, an index, a periodic offset — and no selector names a single cell. One coordinate's presentation is stated by a sidecar whose ROOT is that coordinate.
- **PRES2 · the TRANSPARENT html form carries every sidecar's bytes unchanged and paints the cascade the model resolves.** That form applies a sidecar by copying its bytes into a scoped, layered `<style>` over the region its filename names, adding only wrappers, coordinates, track sizes and that layer — so what the page paints is the model's cascade — specificity, then source order, the browser's own two keys — for every declaration the model CARRIES, which is what `Overlay::cell_style` resolves and `pack` writes, and the author's own bytes for the rest, which reach the page and no other carrier. That byte-for-byte guarantee is over SIDECARS: a figure's spec is not carried unchanged but EXPANDED from its bindings and then re-spelled — every `<` in it becomes `\u003c` so the raw-text `<script>` holding it cannot be ended by a cell's own text — and drawn by the pinned Vega runtime. It is kept permanently as the form to read to learn what the format does.

## VAL — the value model

- **VAL1 · every value is computed, never asserted.** Every cell's value derives from its own content and the values it references, and from nothing else — including any value the engine reuses instead of recomputing, which is therefore the value a fresh computation would yield.
- **VAL3 · the value model.** Every cell's value is one of {number, text, boolean, error, blank, array}.

## ENG — evaluation

- **ENG6 · Excel-compatible evaluation.** Every formula FSA1 evaluates yields the value a mainstream spreadsheet (Excel) yields.

## The encoding

> **NOT YET WRITTEN.** The sections below name what the normative encoding text must cover. Until it
> is written the operative authorities are the invariants above and `fsa1-cli --guide`. This
> placeholder is deliberate: an encoding derived by reading the current implementation would be
> cement, not a contract.

- **Tree layout** — the workbook directory, a tab per folder, a file per range, a `.css` sidecar per
  styled region, a `<range>.json` per figure that fills a range, a `<name>.json` per figure that does
  not, a `<name>.css` sidecar per PLACED one of those — which an imported chart carries wherever its
  source anchor states a position in cell terms — and what nesting means.
- **Filename grammar** — the closed A1 range in canonical spelling (uppercase column, no leading-zero
  row, no `$`, top-left`:`bottom-right, no degenerate `A1:A1`, and no whole-column `A:A`, which a
  RANGE file may not name because a grid fills its range exactly), the
  defined-name entry form, the presentation sidecar's `<range>.css`, whose stem is that same
  range grammar EXTENDED by the open forms a sidecar alone may name (`A:A`, `A:C`, `1:1`, `2:5`),
  whose host separator is a range file's, or the suffix alone naming no range, or a stem that is a
  FIGURE's name — the placement sidecar — and the figure's own
  `<stem>.json`, whose stem is a NAME *or* a canonical closed range inside Excel's grid. The name
  form takes no part in the cascade and collides with no cell; the RANGE form fills the range it
  spells and collides with any file reaching it, and a `.css` beside one is a refusal rather than a
  placement, that figure having stated its own. Which kind a `.css` is is settled by its TAB, not by its spelling: a stem
  with a `<stem>.json` beside it is that figure's placement (so `Chart1.css` beside `Chart1.json`
  places a figure and never roots a cascade), and only where the tab holds no such figure does the
  name decide — a range root, else a refusal, never a defined name.
- **Cell encoding** — TSV; the file is its grid with no header or metadata line; first line is
  the first row; one field per coordinate; an empty field is Blank; the `\t` / `\n` / `\\` escapes.
- **Presentation** — a `<range>.css` sidecar, its FILENAME naming the A1 range in absolute
  coordinates that is its scoping root — closed, or open on one axis and reaching as far as the
  tab's content does — or a stem-less `.css` naming no root: the tab's own
  layer, rooted at everything its range files reach, beneath every rooted sidecar and in no
  contention with one, from which both a selector index's region-relative basis and
  the extent of a bare `fsa1-cell` follow — and, because those indices count in the tab's content
  and so reach into every block, where the tab holds ANY rooted sidecar each of the layer's rules
  selects the whole region or declares only an axis size; the file holds rules directly and no
  prelude, and needs no range file beside it.
  Those selectors, each WRITTEN in its ONE canonical spelling and READ in any spelling of the
  target it names, its rules in ANY order, cascading by specificity then source order and written
  back in the order they were read, the properties — a
  sidecar may declare ANY of them save a size on a selector naming no such axis, the model
  carrying the ones it resolves onto a coordinate or an axis and the rest reaching the page and
  no further — and the
  contention between sidecars — the roots of one tab are DISJOINT, or nest with the inner root a
  SINGLE cell, and that inner cell layers last and wins property by property; a scope is applied
  over one subtree, so crossing roots and a multi-cell root inside another are refused. A FIGURE's
  sidecar is outside that cascade entirely: it names no root, reaches no coordinate, and contends
  with nothing.
- **Figures** — a `.json` entry of a tab with a non-empty stem — ANY of them, the extension being
  spent on figures alone — holding a Vega-Lite spec whose every `data.name`
  is an A1 reference into the workbook: an optional `<tab>!` prefix, then one corner or two joined by
  `:`. A reference, not a filename, so a 1x1 `A1:A1` is admissible and a whole-column `A:A` is not.
  The range resolves to a table whose FIRST ROW is the field names and whose cells contribute their
  VALUES, and the bound document is the file's own spec plus one `datasets` key. The spec itself
  carries no position, so the STEM states which of two forms the figure takes and where it sits. The
  RANGE form is the declarative one: the filename is the placement and the size, and the figure fills
  exactly the cells it names. The NAME form floats, and where it sits is stated beside it, in a
  `<name>.css` holding one `figure` rule whose `anchor` is a cell (a fixed box, sized by `left`,
  `top`, `width`, `height`) or a range (the cells it fills, sized with them) — the arbitrary EMU
  position, with sub-cell offsets, that an imported chart carries and no range expresses. A name-form
  figure with no sidecar is placed by the writer. An IMPORTED chart states in exactly such a sidecar
  the position its source anchor gave it in CELL terms, and a source position no `anchor` spells —
  approximated there, or stated in no sidecar at all — is REPORTED as a named approximation rather
  than carried in silence.
- **In-cell content** — literal forms (number, `TRUE`/`FALSE`, the seven author-writable error
  spellings, `'`-prefixed text), the `=formula` form, and the trailing `~<code>` display-format marker.
- **Coverage and overlap** — the grid fills its declared range exactly, or is a single `=formula`
  whose array value does; every coordinate takes its content from at most one file. A range-named
  figure occupies its range though it contributes no content, so it collides like any other file.
- **Reserved entries** — the tree names reserved for tooling, which never contribute a cell value.
- **Formula language** — the function set, and the Excel-compatibility claim with its declared
  divergences.

## Withdrawn ids

Every id below is **never reused**; each names where its need now lives, so a stale citation
resolves. Ledgers under `conformance/` still cite several of these by their old names, and by the
old filename `SPEC.md`, which this document and `docs/cli-spec.md` replaced.

| withdrawn | read instead | |
|---|---|---|
| CORE1 (A1 addressing) | **FS1** | a naming convention, not a constraint |
| CORE3 (filesystem is the write surface) | **FS1** | folded into FS1's "enters only by an author writing that tree" |
| FS2 (closed range) | **FS1** | a definition; "the A1 range it fills" |
| FS4 (name) | **FS1** | a definition; "or the name it declares" |
| GRID1 (grid) | **FS1** + **FS5** | the rule that nothing outside the declaring file contributes |
| GRID2 (deserializer) | **the encoding, above** | a definition, not an invariant |
| GRID3 (content → grid is engine-independent) | **the crate firewall** | `docs/repo-standards.md`; code structure, not product behaviour |
| GRID4 (the grid fills its range) | **FS1** + **FS5** | a coordinate the file does not fill holds no value |
| GRID5 (array formula fills its range) | **FS1** + **FS5** | content fills the range its name declares, however written |
| GRID6 (error locality) | **CORE2** (`docs/cli-spec.md`) | "a fault in one part never denies the rest its value" |
| GRID7 (typed content) | **FS1** | a display format lives in the cell's own content |
| ENG1 (engine) | **VAL1** | a definition; the computation rule is VAL1 |
| ENG2 (evaluation) | **CORE2** (`docs/cli-spec.md`) | a reference cycle is a located refusal |
| ENG3 (optimization transparency) | **VAL1** | a value depending on what was demanded is not derived from content alone |
| ENG4 (reuse is sound) | **VAL1** | folded in: reuse that differed would not derive from content alone |
| ENG5 (engine independent of storage/presentation) | **the crate firewall** | `docs/repo-standards.md`; code structure, not product behaviour |
| ENG7 (persistent result cache) | **VAL1** | retired: the cross-run cache was built, measured net-negative, and removed |
| VAL2 (no value from thin air) | **VAL1** | folded into "and from nothing else" |

**Deliberately not carried forward.** ENG2's demand-reachability and compute-once clauses describe
today's evaluation strategy, not a product need; an eager engine would still be correct. ENG2's
`#REF!` spelling for a cycle is a knowing divergence from ENG6, which is status, not constraint.
ENG6's corpus-scoping clause and its list of divergences are likewise status. FS4's shadowing rule
and GRID5's case list are enumerated drifts: an agent walks around a list, and the general sentence
already forbids each case. Every entry's fitness function is enforcement, and lives with the tests.
