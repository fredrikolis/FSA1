<!-- Concern: the PROVISIONAL on-disk charlie format — filename↔range grammar, body grammar, broadcast-conformance rule, per-range annotation, cross-sheet/overlap representation | Non-concern: the formula language (see charlie-v1/architecture.md §3), the QA ladder, engine internals | IO: none -->
# FORMAT.md — the charlie on-disk encoding (PROVISIONAL)

> **PROVISIONAL — READ THIS FIRST.** This is a *provisional substrate spec* authored so that six
> people can hand-write consistent sample sheets against one shared grammar. It exists to be
> **stress-tested and possibly revised by W2 / bet B1**. If the corpus surfaces a sheet pattern this
> grammar cannot express, or a conformance verdict this rule leaves genuinely ambiguous, that is a
> *finding against B1* (see `README.md`), not a bug to paper over. Do not treat any clause here as
> load-bearing for prod until B1 signs off. The authority for the *intent* behind these rules is
> `charlie-v1/architecture.md §4` and `BRIEF.md` locked-decision #4; where this doc and those
> disagree, those win and this doc is wrong.

This document defines a **charlie workbook**: a directory tree that a filesystem *is* the spreadsheet.
No proprietary container — distribution is a zip of the tree.

---

## 1. The three structural levels

```
workbook/            a directory (the .zip root)
  Orders/            a TAB          = a folder
    A1:D1.range      a RANGE cell-block = a file whose name declares a rectangle
    D7.cell          a single CELL      = a file whose name declares one address
```

- **Tab = folder.** The folder's *name* is the sheet name used in cross-sheet references
  (`Orders!D7`). One level of folders under the workbook root; nested folders are **not** sub-sheets
  in v1 (reserved).
- **Cell = `<address>.cell`**, a single scalar location.
- **Range = `<addr>:<addr>.range`**, a rectangular block of ≥2 cells.

A folder's files partition its used region: see §7 (overlap is a hard error; gaps are Blank).

---

## 2. Filename grammar (the address encoding)

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

**Canonical form (enforced, to kill diff noise — one file, one name):**

- Column letters are **uppercase only** (`a1.cell` is illegal).
- Row numbers have **no leading zeros** (`A01.cell` is illegal).
- A range is written **top-left `:` bottom-right**, i.e. `minCol minRow ":" maxCol maxRow`.
  `G8:A3.range`, `A8:G3.range`, `G3:A8.range` are all **illegal spellings of** `A3:G8.range`.
- A degenerate 1×1 range is **illegal** — a single cell is always `.cell`, never `A1:A1.range`.

**Absolute markers (`$`): none in filenames.** A file's own address is intrinsically a fixed
location, so `$` is meaningless on the left of the dot and is **rejected**. The `$A$1` absolute/mixed
markers exist *only inside formula bodies* (§4), where they govern relative-ref offsetting under fill
(§5). Rationale: keeps filenames shell-safe and canonical; keeps the one meaning of `$` (fill anchor
behavior) in one place.

**Whole-column / whole-row ranges** (`A:A`, `3:3`) are **reserved, not v1** — do not author them in
the corpus.

Examples (all legal, canonical):

| Filename            | Declares                                  | Shape (rows × cols) |
|---------------------|-------------------------------------------|---------------------|
| `A1.cell`           | single cell A1                            | 1 × 1               |
| `D7.cell`           | single cell D7                            | 1 × 1               |
| `AA10.cell`         | single cell AA10 (col 27)                 | 1 × 1               |
| `A1:D1.range`       | A1..D1 (one header row)                    | 1 × 4               |
| `A2:A6.range`       | A2..A6 (one column)                        | 5 × 1               |
| `A3:G8.range`       | A3..G8                                     | 6 × 7               |

The **declared shape** of a range is `(rows, cols) = (maxRow−minRow+1, colIndex(maxCol)−colIndex(minCol)+1)`.
This declared shape is the left-hand side of every conformance check in §6.

---

## 3. File encoding envelope

- UTF-8, **LF** line endings. A trailing newline is allowed and ignored.
- **Line 1 is the annotation** (§8) — mandatory, begins with `# ` (hash + space).
- Lines 2..N are the **body** (§4). Blank line 2 is permitted (e.g. an empty `.cell`).
- No BOM. No CRLF (a loader may normalize, but authors write LF).

The annotation is *not* part of the body and never contributes a cell value. The `# ` (hash-space)
prefix is what distinguishes a comment from a literal that merely starts with `#` (e.g. the error
literal `#REF!`, which has no following space) — see §4.3.

---

## 4. Body grammar

The body (everything after the annotation line) is **exactly one** of two forms:

### 4.1 A single `=formula`

The body's first non-empty line begins with `=`. The remainder is one formula in the charlie formula
language (`architecture.md §3`). Exactly one `=formula`; multiple formula lines are illegal.

```
# Concern: per-line revenue | Non-concern: the grand total (D7.cell) | IO: none
=B2*C2
```

Store the **formula text** — never only a cached value. (An optional sibling cached value for
value-diffing is out of scope for hand-authored corpus files; author formulas.)

### 4.2 A literal block

The body is one or more lines of literal values, laid out as a grid (§5). Used for hand-entered data
and headers.

```
# Concern: order-book headers | Non-concern: the rows below | IO: input
Product	Unit Price	Qty	Line Total
```

### 4.3 Literal value lexing (per cell of a literal block, and for a `.cell`)

A single literal token becomes a `Value`:

| Token form                         | Value                          |
|------------------------------------|--------------------------------|
| `123`, `-4`, `3.14`, `1.2e6`       | `Number(f64)`                  |
| `TRUE`, `FALSE`                    | `Bool`                         |
| `#REF!` `#DIV/0!` `#VALUE!` `#NAME?` `#N/A` `#NULL!` `#NUM!` | `Error(kind)` |
| empty field (nothing between tabs) | `Blank`                        |
| anything else                      | `Text`                         |
| `'123` (leading apostrophe)        | `Text` "123" (force-text a numeric-looking value) |
| `"a\tb"` (double-quoted)           | `Text` with the literal contained, tabs/quotes escaped `\t` `\"` |

A `.cell` body is a single such token on line 2 (empty line 2 ⇒ `Blank`). A `.cell` never contains
tabs; use quoting if the text needs one.

---

## 5. Literal-block layout (rows and columns on disk)

A literal block is **TSV**: tab-separated values, one worksheet row per text line.

- **Rows** run top → bottom = line 2 (top row of the range) downward.
- **Columns** run left → right = tab-separated fields, left field = left-most column of the range.
- Field count must be identical on every line (ragged blocks are illegal — a `#VALUE!`-class
  *structural* refusal at load).
- An empty field (`\t\t`) is `Blank`; a wholly empty line is a full row of `Blank`s only if it has
  the right number of tabs — prefer explicit tabs for clarity.

The block's **literal shape** is `(number of body lines, fields per line)`. Example — file
`A3:C4.range` declares shape `2 × 3`; its body must be 2 lines × 3 fields:

```
# Concern: a 2×3 sample block | Non-concern: everything else | IO: input
10	20	30
40	50	60
```

Here A3=10, B3=20, C3=30, A4=40, B4=50, C4=60. The literal shape `2×3` must satisfy §6 against the
declared shape `2×3` (exact match).

---

## 6. Broadcast-conformance dimension rule (the APL/NumPy ⍴ model)

Let the **declared shape** (§2) be `(R, C)`. Evaluate the body to a **result shape** (§6.1), then
apply the single rule below. Precedent: ODF OpenFormula matrix formula
`number-matrix-{rows,columns}-spanned`; the formal model is NumPy/APL broadcasting.

| Result shape        | Conforms iff        | Placement                                   |
|---------------------|---------------------|---------------------------------------------|
| scalar `1 × 1`      | always              | **fill** — every one of the R×C cells       |
| row vector `1 × k`  | `k == C`            | **broadcast down** — copy the row to all R rows |
| col vector `k × 1`  | `k == R`            | **broadcast across** — copy the col to all C cols |
| array `r × c`       | `r == R && c == C`  | **exact** — placed cell-for-cell            |
| anything else       | —                   | **static refusal** (`#SPILL!`-class), located, at load |

This is a **static** check — charlie's advantage over Excel, which only detects the mismatch at
runtime. A non-conforming range file fails to load with a located diagnostic naming the file, the
declared shape, and the result shape.

### 6.1 Where the result shape comes from — and the orientation disambiguator

- **Literal block** → result shape = the literal shape (§5). Orientation is *visually intrinsic*: a
  one-line block is `1×k`, a one-field-per-line block is `k×1`.
- **`=formula`** → evaluate the top-level expression **once, anchored at the range's top-left cell**:
  - If it evaluates to a **scalar** ⇒ result shape `1×1` ⇒ **fill/drag mode**: the formula is a
    drag-fill template; relative refs re-anchor per destination cell, `$`-absolute refs do not (§5 of
    architecture; worked in §5 example below). This is the "one formula, not N copies" construct.
  - If it evaluates to a **vector/array** (an array-returning function, or a bare range ref like
    `=B2:B6`) ⇒ result shape is that array's shape ⇒ **array mode**: placed once by the table above,
    **no per-cell offsetting**.

**The disambiguator (this is the clause B1 must not find ambiguous):** *the axis of a vector is a
property of the vector itself (its `1×k` vs `k×1` shape), never inferred from the declared range.*
So when `R == C`, a `1×C` row vector still broadcasts **down** and a `R×1` col vector still broadcasts
**across** — the square range does not make the verdict ambiguous. Likewise, scalar-vs-array mode for
a `=formula` is decided solely by the *shape of the evaluated top-level expression* (a cell ref `B2`
is scalar ⇒ fill; a range ref `B2:B6` is `k×1` ⇒ array), never by the range's dimensions. If any real
corpus sheet produces two defensible verdicts under these two sentences, **that is the B1 kill
finding** — record it, do not resolve it silently.

---

## 7. Overlap and gaps within a folder

Within one tab (folder), the set of all declared cells/ranges must be **pairwise disjoint** — a
partition of the used region. Gaps are allowed and read as `Blank`.

- **Overlap is a first-class, hard error** (`reject`, never guess a winner). If two files' declared
  regions intersect — `.range`∩`.range`, `.cell`∩`.range`, or duplicate `.cell` — the folder fails
  to load with an ASCII diagnostic **naming both files and the contested cells**, e.g.:

  ```
  error[overlap]: two files claim overlapping cells in tab "Orders"
    A1:D3.range  and  C2.cell
    contested: C2
    precedence: none — reject. Split or delete one file.
  ```

- **Precedence rule (defined now, per BRIEF out-of-scope note): REJECT.** v1 does not resolve
  overlaps by ordering, recency, or specificity — the workbook is invalid until the author removes
  the overlap. (Merge / 3-way-conflict resolution is deliberately out of scope; we only *define* the
  precedence here so the corpus never depends on a guess.)

---

## 8. The per-range annotation convention

**Every** `.cell` and `.range` file's **line 1** is a Concern annotation — a leading comment line.
Because ranges are first-class, the annotation burden is **per-range, not per-cell** (the owner's key
insight for why annotation stays cheap: one line annotates a whole block).

```
# Concern: <what this block is/means> | Non-concern: <what it is deliberately NOT> | IO: <flow>
```

- Prefix is `# ` (hash + space). Exactly one annotation line, and it is line 1.
- Three pipe-separated fields, in order: **Concern**, **Non-concern**, **IO**.
- `IO` values (provisional enum):
  - `input` — hand-entered or externally-sourced leaf data (a literal block/cell that originates
    values; the sheet's real inputs).
  - `none` — a formula-derived block that neither originates nor exports data.
  - `output` — a block designated as the sheet's reported result (e.g. a summary total consumed
    across sheets). Use sparingly; it marks the answer a reader/agent should read.

The annotation mirrors the source-file `// Concern: … | Non-concern: … | IO: …` convention (only the
comment marker differs: `#` for data files, `//` for Rust), so `annotated-tree` and a reader see one
consistent shape across code and workbook.

---

## 9. Cross-sheet references

A file lives in exactly one folder, so a *file* never encodes a cross-sheet target — cross-sheet
refs appear **only inside formula bodies**:

```
SheetName!A1            ; SheetName must exactly match a sibling folder name
'My Orders'!A1          ; single-quote the sheet name if it contains spaces/punctuation
```

- The sheet name is resolved against folder names in the same workbook (`Resolver::sheet_id`).
- An unknown sheet name → located `#REF!`-class refusal.
- 3-D refs (`Sheet1:Sheet3!A1`) are **reserved, not v1** — do not author them.

---

## 10. A tiny worked example

A two-tab workbook: `Orders` (data + a per-row fill formula + a total) and `Summary` (a cross-sheet
read).

```
revenue.zip  (unzipped)
└── revenue/
    ├── Orders/
    │   ├── A1:D1.range      1×4 header row (literal row vector)
    │   ├── A2:A6.range      5×1 product names (literal col vector)
    │   ├── B2:B6.range      5×1 unit prices (literal col vector)
    │   ├── C2:C6.range      5×1 quantities  (literal col vector)
    │   ├── D2:D6.range      5×1 per-row line total (=B2*C2, drag-fill)
    │   └── D7.cell          grand total (=SUM(D2:D6))
    └── Summary/
        ├── A1.cell          label
        └── B1.cell          cross-sheet total (=Orders!D7)
```

**`Orders/A1:D1.range`** — declared `1×4`, literal shape `1×4` ⇒ exact match:

```
# Concern: order-book column headers | Non-concern: the data rows below | IO: input
Product	Unit Price	Qty	Line Total
```

**`Orders/A2:A6.range`** — declared `5×1`, literal shape `5×1` ⇒ exact match:

```
# Concern: product names | Non-concern: prices/quantities | IO: input
Widget
Gadget
Sprocket
Cog
Flange
```

**`Orders/D2:D6.range`** — declared `5×1`; body is a `=formula` whose top-level expr `B2*C2` is a
**scalar** ⇒ fill/drag mode. Relative refs offset per destination row: D2=`B2*C2`, D3=`B3*C3`, …,
D6=`B6*C6`. **One formula, not five copies:**

```
# Concern: per-line revenue = unit price × quantity | Non-concern: the grand total (D7.cell) | IO: none
=B2*C2
```

**`Orders/D7.cell`** — a single cell, scalar formula:

```
# Concern: grand total revenue across all order lines | Non-concern: the per-line breakdown | IO: output
=SUM(D2:D6)
```

**`Summary/B1.cell`** — cross-sheet read of the `Orders` output:

```
# Concern: revenue surfaced on the summary tab | Non-concern: how it was computed (see Orders) | IO: output
=Orders!D7
```

### 10.1 A broadcast example (row vector broadcast down)

`Margins/B2:D4.range` — declared `3×3`; body is a literal **row vector** `1×3` ⇒ conforms
(`k==C==3`), broadcast **down** all 3 rows (B2:D2 = B3:D3 = B4:D4 = `0.1 0.2 0.3`):

```
# Concern: per-column margin rate applied to every row | Non-concern: the base amounts | IO: input
0.1	0.2	0.3
```

Had this same one-line body sat in a `3×3` range but been intended as a *column*, the author would
write it one-field-per-line (a `3×1`, which would then fail `k==C` and pass `k==R`, broadcasting
across). The on-disk orientation is the whole disambiguation — see §6.1.

---

## 11. Quick illegal-forms checklist (for authors)

- `a1.cell`, `A01.cell`, `A1:A1.range`, `G8:A3.range` — non-canonical / degenerate names → reject.
- `$A$1.cell` — `$` in a filename → reject (`$` lives in formula bodies only).
- A `.range` literal block with unequal field counts per line → ragged → `#VALUE!`-class reject.
- Two files with intersecting declared regions in one folder → overlap → reject (§7).
- A `=formula` result shape that is none of scalar/row-vec/col-vec/exact-array → `#SPILL!`-class
  static refusal (§6).
- A body that is both a literal line *and* an `=formula` line → reject (exactly one body form, §4).
- Missing line-1 annotation, or line 1 not starting with `# ` → reject (§8).
