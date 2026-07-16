<!-- Concern: the LIVING authoritative on-disk encoding contract for charlie-model, AS-BUILT — the filename↔range grammar and canonical-form policy, the body grammar (literal-block vs `=formula` classification and per-token literal lexing), the broadcast-conformance placement rule with its resolved tie-breaks, the overlap rule, the per-file annotation convention, and the single-sourced diagnostic-code registry | Non-concern: the formula LANGUAGE semantics and evaluation (charlie-ast owns lex/parse/eval — a `=formula` body is stored opaque here), xlsx serde and the CLI surface, and the FROZEN provisional snapshot `conformance/encoding/FORMAT.md` that the corpus was authored against (this doc supersedes it as the spec, but that file must not change) | IO: none -->
# format.md — the charlie on-disk encoding (LIVING SPEC, as-built)

> **STATUS — authoritative.** This is the **living, authoritative contract** for `charlie-model`'s
> encoding layer. It describes the **AS-BUILT** behavior of `charlie-model` (`filename`, `body`,
> `conformance`, `overlap`, `diagnostic`, and `parse_file`) and it is the spec a charlie clone-alone
> reads to know its own on-disk format. Where this doc and the code disagree, that is a bug in one of
> them — reconcile in the same reviewed change; do not let them drift.
>
> **Relationship to `conformance/encoding/FORMAT.md`.** That file is the **FROZEN provisional
> snapshot** the W1 encoding corpus was authored against. It is fingerprinted in
> `conformance/encoding/MANIFEST.sha256` (oracle-input purity) and **must not be edited** — its `§`
> citations are what the corpus's EXPECTED ledgers resolve against. This `docs/format.md` is the
> **living authoritative spec going forward**; it carries the same `§` numbering so those citations
> still resolve, and it records the resolutions that the W2 build surfaced when the provisional was
> stress-tested against real code (see [§12](#12-resolutions-w2-hardening)). When the two disagree,
> **this doc wins as the spec** and the frozen file stays as the historical contract the corpus rode
> in on.

A **charlie workbook** is a directory tree that a filesystem *is* the spreadsheet — no proprietary
container; distribution is a zip of the tree. `charlie_model::parse_file(name, contents)` loads one
file end-to-end and returns a `ParsedFile` or the first violation as a located `Diagnostic`; it never
panics and never silently drops (ast-standards PART 5).

---

## 1. The three structural levels

```
workbook/            a directory (the .zip root)
  Orders/            a TAB              = a folder
    A1:D1.range      a RANGE cell-block = a file whose name declares a rectangle
    D7.cell          a single CELL      = a file whose name declares one address
```

- **Tab = folder.** The folder's *name* is the sheet name used in cross-sheet references
  (`Orders!D7`). One level of folders under the workbook root; nested folders are **not** sub-sheets
  in v1 (reserved).
- **Cell = `<address>.cell`**, a single scalar location; declared shape is always `1×1`.
- **Range = `<addr>:<addr>.range`**, a rectangular block of ≥2 cells.

A folder's files partition its used region: see [§7](#7-overlap-and-gaps-within-a-folder) (overlap is
a hard error; gaps are Blank).

---

## 2. Filename grammar (the address encoding)

`parse_filename` (`charlie-model/src/filename.rs`) requires a `.cell` or `.range` suffix, then layers
canonical-form **policy** on top of charlie-ast's shared A1 address grammar (`charlie_ast::a1`, which
tokenizes an address but judges nothing).

```ebnf
cellfile   = address ".cell"
rangefile  = address ":" address ".range"      ; top-left ":" bottom-right

address    = column row
column     = UPPER { UPPER }                    ; bijective base-26: A..Z, AA..ZZ, AAA..
row        = NONZERO { DIGIT }                  ; 1-indexed, decimal, NO leading zero
UPPER      = "A".."Z"
NONZERO    = "1".."9"
DIGIT      = "0".."9"
```

**Canonical form (enforced — one file, one name).** Each rule maps to a stable diagnostic
[`Code`](#appendix-a-diagnostic-code-registry):

- Column letters are **uppercase only** (`a1.cell` → `lowercase-column`).
- Row numbers have **no leading zeros** (`A01.cell` → `leading-zero-row`).
- **No `$` absolute markers** — a file's own address is intrinsically fixed, so `$` is meaningless on
  the left of the dot (`$A$1.cell`, `$A1.cell` → `dollar-in-filename`). The `$A$1` markers exist
  *only inside formula bodies* ([§4](#4-body-grammar)).
- A range is written **top-left `:` bottom-right**, i.e. `minCol minRow ":" maxCol maxRow`. Any
  reversed spelling (`G8:A3.range`, `A8:G3.range`, `G3:A8.range` — all the same rectangle as
  `A3:G8.range`) → `non-canonical-range`. The single ordering check `la.col > ra.col || la.row >
  ra.row` catches every reversed spelling at once.
- A degenerate **1×1 range** is illegal (`A1:A1.range` → `degenerate-range`) — a single cell is
  always `.cell`. This is a **reject, not an accept-and-canonicalize**.
- **Whole-column / whole-row ranges** (`A:A`, `3:3`) are reserved, not v1 → `whole-column-row-
  reserved`. They are detected *before* address parsing (both sides all-alpha, or both all-digit) so
  they get their own named refusal.
- A `.range` needs **exactly one** `:` (zero → `malformed-filename`; more than one →
  `malformed-filename`). Any address that the A1 grammar cannot tokenize (empty, missing column,
  missing row, trailing junk, column/row overflow) → `malformed-filename`, located at the byte
  offset within the name.

The **declared shape** of a range is `(rows, cols) = (maxRow−minRow+1, colIndex(maxCol)−
colIndex(minCol)+1)`; a `.cell` is `1×1`. The declared shape is the left-hand side of every
[§6](#6-broadcast-conformance-dimension-rule) conformance check, and the region (`Rect`, inclusive
zero-based corners) is the input to the [§7](#7-overlap-and-gaps-within-a-folder) overlap detector.

Examples (all legal, canonical): `A1.cell` (1×1), `AA10.cell` (1×1, col 27), `A1:D1.range` (1×4),
`A2:A6.range` (5×1), `A3:G8.range` (6×7).

Hostile names never panic — `parse_filename` returns a located `Diagnostic` for every input,
including empty strings, lone separators, and multi-byte junk.

---

## 3. File encoding envelope

- UTF-8, **LF** line endings. No BOM; authors write LF (a loader may normalize).
- **Line 1 is the annotation** ([§8](#8-the-per-file-annotation-convention)) — mandatory, begins with
  `# ` (hash + space). `parse_file` splits on the first `\n`: everything before it is line 1,
  everything after is the body. A line 1 that does not start with `# ` → `missing-annotation`, located
  at `1:1`.
- Lines 2..N are the **body** ([§4](#4-body-grammar)). A blank body is permitted (see §4 and
  [resolution R2](#r2-empty-range-body-is-a-11-blank-that-scalar-fills)).
- **A single trailing `\n` is stripped and ignored**, and a lone trailing `\r` after it is tolerated
  too (so a stray CRLF at end-of-file does not add a phantom row). Interior CRs are **not** stripped —
  they ride inside the token text.

The annotation is *not* part of the body and never contributes a cell value. Because the annotation is
file line 1, **a body line `n` reports as file line `n + 1`** in diagnostics — every located body
refusal uses the file line number.

---

## 4. Body grammar

The body (everything after line 1) classifies (`classify_body`, `charlie-model/src/body.rs`) into
**exactly one** of two forms. A `=formula` body is stored **opaque** (verbatim, leading `=`
included) — W2 does not evaluate it or know its result shape; that is W3.

### 4.1 A single `=formula`

The body's **first non-empty line** begins with `=`. That line is stored verbatim as the formula;
**leading blank lines are ignored** (a formula may be preceded by empty lines). Exactly one formula is
allowed — see [resolution R3](#r3-the-dual-body-decision-procedure) for the precise dual-body test.

Detection is **anchored to the first non-empty line only** — the classifier deliberately does *not*
scan every line for a leading `=`. A *later* literal field that happens to begin with `=` (e.g. a
second row `=A1\t30` under a literal first row) is a literal **Text** token (§4.3 has no `=`-prefixed
literal token), **not** a formula, so such a block is a literal block, not a false-rejected dual body.

### 4.2 A literal block

The first non-empty line does not begin with `=`. The body is one or more physical lines of literal
values, laid out as a TSV grid ([§5](#5-literal-block-layout)).

### 4.3 Literal value lexing (`lex_literal`, per field)

Each field lexes to a `charlie_ast::Value` by this **ordered** precedence (first match wins):

| # | Token form | Value |
|---|---|---|
| 1 | empty field (nothing between tabs) | `Blank` |
| 2 | `'…` (leading apostrophe) | `Text` of the remainder — force-text (e.g. `'123` → `Text "123"`) |
| 3 | `"…"` (len ≥ 2, starts and ends with `"`) | `Text`, interior unescaped: `\t`→tab, `\"`→quote, `\\`→backslash (an unknown `\x` keeps the backslash) |
| 4 | `TRUE`, `FALSE` | `Bool` — **case-SENSITIVE, uppercase only** ([R5](#r5-booleanerror-literals-are-case-sensitive)) |
| 5 | `#REF!` `#DIV/0!` `#VALUE!` `#NAME?` `#N/A` `#NULL!` `#NUM!` | `Error(kind)` — the seven v1 error literals |
| 6 | a **finite** number (`123`, `-4`, `3.14`, `1.2e6`) | `Number(f64)` — non-finite spellings do **not** match ([R6](#r6-non-finite-numerics-lex-as-text)) |
| 7 | anything else | `Text` |

`#SPILL!` and `#CALC!` are **reserved** and are *not* literal error tokens — they fall through to
`Text`. A `.cell` body is a single such token; an empty `.cell` body is `Blank`.

---

## 5. Literal-block layout (rows and columns on disk)

A literal block is **TSV**: tab-separated values, one worksheet row per physical text line.

- **Rows** run top → bottom (line 2 = top row of the range, downward).
- **Columns** run left → right (tab-separated fields; left field = left-most column).
- The **column count is fixed by the first line** (`lines[0].split('\t').count()`). Every subsequent
  line must split into that **same** field count, or the block is **ragged** → `ragged-block`, a
  `#VALUE!`-class structural refusal located at the offending file line. This structural check
  precedes, and is independent of, the [§6](#6-broadcast-conformance-dimension-rule) shape check.
- An empty field (`\t\t`) is `Blank`. A **blank interior line has one field** (fields = tabs + 1 = 1)
  — see [resolution R4](#r4-blank-interior-lines-are-1-field-tsv-rows).

The block's **literal shape** is `(number of physical body lines, fields per line)`. That shape is
then checked against the declared shape under §6.

---

## 6. Broadcast-conformance dimension rule

Let the **declared shape** ([§2](#2-filename-grammar-the-address-encoding)) be `(R, C)` and the body's
**result shape** be `(rr, rc)`. For a literal body the result shape is the literal shape (§5); for a
`=formula` body the result shape needs evaluation (W3) and so **no conformance verdict is produced in
W2** (the formula is stored opaque, `placement = None`). `classify_placement`
(`charlie-model/src/conformance.rs`) decides the `Placement`, or a `#SPILL!`-class `non-conforming`
refusal naming the file, declared shape, and result shape.

The rule is applied in this **exact precedence** — this ordering *is* the resolution of the primary W2
finding ([R1](#r1-6-tie-break-scalar-fill-exact-rowcol-broadcast)):

| Order | Result shape | Conforms iff | Placement |
|---|---|---|---|
| 1 | scalar `1 × 1` | always | **Fill** — every one of the R×C cells |
| 2 | array `rr × rc` | `rr == R && rc == C` | **Exact** — placed cell-for-cell |
| 3 | row vector `1 × rc` | `rc == C` | **BroadcastDown** — copy the row to all R rows |
| 4 | col vector `rr × 1` | `rr == R` | **BroadcastAcross** — copy the col to all C cols |
| — | anything else | — | **static refusal** (`#SPILL!`-class), located, at load |

This is a **static** check — charlie's advantage over Excel's runtime-only detection.

### 6.1 The orientation disambiguator (the clause B1 must not find ambiguous)

**The axis of a vector is a property of the vector's own on-disk shape** (`1×k` row vs `k×1` col),
never inferred from the declared range. So when **`R == C`**, a `1×C` row vector still broadcasts
**down** and an `R×1` col vector still broadcasts **across** — the square range does **not** make the
verdict ambiguous. A one-line block is `1×k`; a one-field-per-line block is `k×1`; the on-disk
orientation is the whole disambiguation. This clause yields exactly **one** defensible verdict per
file and is **not** a B1 kill (the frozen `square-disambiguator` fixture pins it).

---

## 7. Overlap and gaps within a folder

Within one tab, the set of declared regions must be **pairwise disjoint**. `detect_overlaps`
(`charlie-model/src/overlap.rs`) intersects every pair of `Rect`s; each intersecting pair yields one
located `overlap` diagnostic naming **both files** and the **contested cells** (the intersection
rectangle, rendered as an A1 region).

- **Overlap is a hard error** — `.range`∩`.range`, `.cell`∩`.range`, or duplicate `.cell` all reject.
- **Precedence: REJECT** — v1 never picks a winner by ordering, recency, or specificity. The workbook
  is invalid until the author removes the overlap. Edge-touching (sharing a boundary but no cell) is
  **not** an overlap. **Gaps are allowed and read as Blank.**

---

## 8. The per-file annotation convention

**Every** `.cell` and `.range` file's **line 1** is a Concern annotation. `parse_file` requires only
that line 1 begins with `# ` (hash + space); the field grammar below is the authoring convention.

```
# Concern: <what this block is/means> | Non-concern: <what it is deliberately NOT> | IO: <flow>
```

- Prefix is `# ` (hash + space) — this is what distinguishes a comment from a literal that merely
  starts with `#` (e.g. the error literal `#REF!`, which has no following space).
- Three pipe-separated fields, in order: **Concern**, **Non-concern**, **IO**.
- `IO` enum: `input` (hand-entered / externally-sourced leaf data), `none` (a formula-derived block
  that neither originates nor exports), `output` (the sheet's reported result — use sparingly).

The annotation mirrors the source-file `// Concern: … | Non-concern: … | IO: …` convention (only the
comment marker differs: `#` for data files, `//` for Rust), so `annotated-tree` and a human read one
consistent shape across code and workbook.

---

## 9. Cross-sheet references

A file lives in exactly one folder, so a *file* never encodes a cross-sheet target — cross-sheet refs
appear **only inside formula bodies** (`SheetName!A1`, or `'My Orders'!A1` when the name needs
quoting). The sheet name resolves against sibling folder names (`Resolver::sheet_id`); an unknown
sheet name is a `#REF!`-class refusal (a W3 concern — W2 stores the formula opaque). 3-D refs
(`Sheet1:Sheet3!A1`) are reserved, not v1.

---

## 10. Worked examples

A `1×4` header `Orders/A1:D1.range` with a one-line literal body is **Exact** (declared `1×4`, literal
`1×4` — see [R1](#r1-6-tie-break-scalar-fill-exact-rowcol-broadcast)). A `5×1` drag-fill
`Orders/D2:D6.range` with body `=B2*C2` is stored opaque, `placement = None` (its per-cell offsetting
is a W3 eval concern). A blank `A1.cell` is a `1×1` `Blank` that scalar-**Fills**.

### 10.1 Broadcast (row vector down)

`Margins/B2:D4.range` — declared `3×3`; a one-line literal body `0.1\t0.2\t0.3` is a `1×3` row vector
⇒ `rc == C == 3` ⇒ **BroadcastDown** (B2:D2 = B3:D3 = B4:D4). The same three values authored one-per-
line would be a *different file* (a `3×1` col vector) that broadcasts **across** — one file, one
shape, one verdict.

---

## 11. Quick illegal-forms checklist (for authors)

- `a1.cell`, `A01.cell`, `$A$1.cell`, `A1:A1.range`, `G8:A3.range`, `A:A.range` — non-canonical /
  degenerate / reserved names → reject ([§2](#2-filename-grammar-the-address-encoding)).
- A `.range` literal block with unequal field counts per line → ragged → `#VALUE!`-class reject (§5).
- Two files with intersecting declared regions in one folder → overlap → reject (§7).
- A literal result shape that is none of scalar / row-vec / col-vec / exact-array → `#SPILL!`-class
  static refusal (§6).
- A body that is both a literal line *and* an `=formula` line (or ≥2 `=` lines) → dual-body reject
  ([R3](#r3-the-dual-body-decision-procedure)).
- Missing line-1 annotation, or line 1 not starting with `# ` → reject (§8).

---

## 12. Resolutions (W2 hardening)

The provisional grammar left six points under-determined; stress-testing it against real code (the W2
build) fixed each as a **definitive rule**. They are load-bearing in `charlie-model` and stated here
as the authority — not as open questions.

### R1 — §6 tie-break: scalar Fill → Exact → row/col Broadcast

The §6 conformance cases are **not mutually exclusive** for a `1×C` or `C×1` range: a `1×C` body into
a `1×C` range satisfies **both** the row-vector rule (`rc == C`) **and** the exact-array rule (`rr==R
&& rc==C`). The placed cells are identical either way, but the *label* is under-determined. The code
resolves it by **strongest match**, in this precedence:

> **scalar `Fill` → `Exact` → row/col `Broadcast`** — an exact match wins any tie.

So a single-row range with a single-row body is `Exact`, not a broadcast (the intuitive reading), and
a single-column range with a single-column body is `Exact`, not a broadcast-across. The `R == C`
square case ([§6.1](#61-the-orientation-disambiguator-the-clause-b1-must-not-find-ambiguous)) stays
unambiguous because the vector's axis is read from the **body's own on-disk shape**, and a `1×C`/`C×1`
body is not exact against an `R×C` range with `R, C > 1`. Placement is **behaviorally identical**
either way — no oracle that observes placed cells could distinguish the labels — so the rule only
fixes the *label* deterministically; there is no ledger verdict it can violate.

### R2 — empty `.range` body is a 1×1 Blank that scalar-fills

An **empty body** (nothing after the line-1 annotation, modulo the stripped trailing newline) is
treated as a `1×1` scalar `Blank`, which then **scalar-Fills** the declared region under §6. FORMAT
§3 blessed a blank body explicitly only for `.cell`, leaving a blank `.range` under-specified; this
doc resolves it toward **ACCEPT** (ast-standards PART 6 — a false-reject is the cardinal sin): a blank
`.range` reads as a `1×1` Blank that fills the region, consistent with §7 (unclaimed cells already
read as Blank). This is a deliberate accept-under-uncertainty; the reject alternative would refuse an
input the spec never forbids.

### R3 — the dual-body decision procedure

A body is **dual-body (illegal)** iff, after anchoring the form to its first non-empty line:

> it has **≥1 line starting with `=`** AND **≥1 non-empty non-`=` line**, **OR** it has **≥2 `=`
> lines**.

Otherwise it is **exactly one** form. Operationally: if the first non-empty line starts with `=`, any
*other* non-empty line — whether a second `=formula` or a literal line — makes it dual-body →
`dual-body`, located at the conflicting line, naming both the formula's and the conflicting line's
file line numbers. If the first non-empty line does not start with `=`, the body is a literal block
and a later `=`-prefixed field is just a `Text` token (§4.1), never a formula.

### R4 — blank interior lines are 1-field TSV rows

A literal block splits **every** physical line on tabs, so a line's field count is `tabs + 1`. A
**blank interior line** therefore has exactly **one** field (a single `Blank`), i.e. it is a 1-field
row — **not** an ignored line and **not** an automatic full-width row of blanks. Consequently a blank
interior line **trips the ragged `#VALUE!` check** unless the block is 1-wide (`C == 1`). (Leading
blank lines before a *formula* are still ignored per R3/§4.1; this rule is about interior lines of a
*literal block*, whose width is already fixed by its first line.)

### R5 — Boolean/error literals are case-SENSITIVE

Per the §4.3 lexer table, `TRUE`/`FALSE` and the seven `#…!` error literals match **uppercase only**.
`true`, `True`, `#ref!`, etc. do **not** match — they fall through to `Text`. There is no
case-folding.

### R6 — non-finite numerics lex as Text

A numeric field becomes a `Number` **only if it parses to a finite `f64`**. `inf`, `nan` (in any
case), and overflowing magnitudes like `1e999` (which parse to ±∞) do **not** match rule 6 of §4.3 —
they lex as `Text`, never a non-finite `Number`. This keeps the volatile non-finite float spellings
out of the number domain entirely.
