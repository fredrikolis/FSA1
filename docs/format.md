<!-- Concern: the format guide for charlie's on-disk encoding, AS-BUILT and written against SPEC.md — the filename↔closed-range grammar and canonical-form policy (filename.rs), the TSV DESERIALIZER and the explicit GRID it produces (grid.rs: per-field literal-vs-`=formula` classification, per-token literal lexing, ragged refusal), the grid-fills-the-range dimension rule (FT-8), the content-not-position eval rule (FT-9), the overlap rule (overlap.rs), the per-file annotation convention, plus (§13) the charlie-ast `TEXT()` number-format-code subset and (§14) the charlie-ast date/time batch with its shared 1900 date-serial epoch | Non-concern: the rest of the formula LANGUAGE semantics and evaluation (charlie-ast owns lex/parse/eval — a `=formula` field is stored as a parsed Expr; §13/§14 are the two formula-layer exceptions, documented here because both hang off this doc's format-code / date-serial vocabulary), xlsx serde, and the CLI surface | IO: none -->
# format.md — the charlie on-disk encoding (format guide, as-built)

> **CONTRACT vs GUIDE.** The authoritative contract is **`SPEC.md`** (repo root): the vocabulary
> (FT‑1 … FT‑6) and invariants (FT‑7 … FT‑14) the implementation satisfies, each with a fitness
> function. This document is the **as-built format guide** written against it — it describes how
> `charlie-model` (`filename`, `grid`, `overlap`, `diagnostic`, `render`, `workbook`, and
> `parse_file`) realizes those invariants on disk, so a charlie clone-alone can reproduce the format.
> Where this guide and `SPEC.md` disagree, **SPEC.md wins**; where this guide and the code disagree,
> that is a bug in one — reconcile in the same reviewed change, never let them drift.

A **charlie workbook** is a directory tree that a filesystem *is* the spreadsheet — no proprietary
container; distribution is a zip of the tree. `charlie_model::parse_file(name, contents)` loads one
file end-to-end and returns a `ParsedFile` or the first violation as a located `Diagnostic`; it never
panics and never silently drops (ast-standards PART 5).

---

## 1. The three structural levels

```
workbook/          a directory (the .zip root)
  Orders/          a TAB   = a folder
    A1:D1          a file whose NAME is a closed range (a 1×4 header row)
    D7             a file whose name is a single-cell closed range (D7)
```

- **Tab = folder.** The folder's *name* is the sheet name used in cross-sheet references
  (`Orders!D7`). One level of folders under the workbook root; nested folders are **not** sub-sheets
  in v1 (reserved).
- **A file's NAME is a closed range** (SPEC.md FT‑3) — a bounded rectangle of A1 cells with inclusive
  endpoints, **with no filename ending**. A single cell is the address `A1`; a rectangle is
  `top-left:bottom-right` (`A1:D1`, `B2:D9`). There is no `.cell` / `.range` suffix — the bare name is
  the range.

A folder's files partition its used region: see [§7](#7-overlap-and-gaps-within-a-folder) (overlap is
a hard error; gaps are Blank).

---

## 2. Filename grammar (the closed-range encoding)

`parse_filename` (`charlie-model/src/filename.rs`) parses a **bare** filename as a closed A1 range,
layering canonical-form **policy** on top of charlie-ast's shared A1 address grammar
(`charlie_ast::a1`, which tokenizes an address but judges nothing). There is no filename suffix.

```ebnf
filename   = address                            ; a single-cell closed range
           | address ":" address                ; top-left ":" bottom-right

address    = column row
column     = UPPER { UPPER }                     ; bijective base-26: A..Z, AA..ZZ, AAA..
row        = NONZERO { DIGIT }                   ; 1-indexed, decimal, NO leading zero
UPPER      = "A".."Z"
NONZERO    = "1".."9"
DIGIT      = "0".."9"
```

**Canonical form (enforced — one file, one name).** Each rule maps to a stable diagnostic `Code`:

- Column letters are **uppercase only** (`a1` → `lowercase-column`).
- Row numbers have **no leading zeros** (`A01` → `leading-zero-row`).
- **No `$` absolute markers** — a file's own address is intrinsically fixed, so `$` is meaningless in a
  filename (`$A$1`, `$A1` → `dollar-in-filename`). The `$A$1` markers exist *only inside formula
  bodies* ([§4](#4-the-grid-and-its-tsv-deserializer)).
- A range is written **top-left `:` bottom-right**, i.e. `minCol minRow ":" maxCol maxRow`. Any
  reversed spelling (`G8:A3`, `A8:G3`, `G3:A8` — all the same rectangle as `A3:G8`) →
  `non-canonical-range`. The single ordering check `la.col > ra.col || la.row > ra.row` catches every
  reversed spelling at once.
- A degenerate **1×1 range** is illegal (`A1:A1` → `degenerate-range`) — a single cell is the address
  `A1`. This is a **reject, not an accept-and-canonicalize**.
- **Whole-column / whole-row ranges** (`A:A`, `3:3`) are reserved, not v1 → `whole-column-row-
  reserved`. They are detected *before* address parsing (both sides all-alpha, or both all-digit) so
  they get their own named refusal.
- A range needs **exactly one** `:` (more than one → `malformed-filename`). Any address the A1 grammar
  cannot tokenize (empty, missing column, missing row, trailing junk — including a legacy `A1.cell` /
  `A1.range`, whose trailing `.cell`/`.range` is now unparseable — or column/row overflow) →
  `malformed-filename`, located at the byte offset within the name.

The **declared shape** of a file is `(rows, cols) = (maxRow−minRow+1, colIndex(maxCol)−
colIndex(minCol)+1)`; a single-cell file is `1×1`. The declared shape is the right-hand side of the
[§5](#5-the-grid-fills-the-range-exactly-ft-8) fill-the-range check, and the region (`Rect`, inclusive
zero-based corners) is the input to the [§7](#7-overlap-and-gaps-within-a-folder) overlap detector.

Examples (all legal, canonical): `A1` (1×1), `AA10` (1×1, col 27), `A1:D1` (1×4), `A2:A6` (5×1),
`A3:G8` (6×7).

Hostile names never panic — `parse_filename` returns a located `Diagnostic` for every input,
including empty strings, lone separators, and multi-byte junk.

---

## 3. File encoding envelope

- UTF-8, **LF** line endings. No BOM; authors write LF (a loader may normalize).
- **Line 1 is the annotation** ([§8](#8-the-per-file-annotation-convention)) — mandatory, begins with
  `# ` (hash + space). `parse_file` splits on the first `\n`: everything before it is line 1,
  everything after is the body. A line 1 that does not start with `# ` → `missing-annotation`, located
  at `1:1`.
- Lines 2..N are the **body** ([§4](#4-the-grid-and-its-tsv-deserializer)). An empty body is permitted
  (see [R2](#r2-an-empty-body-is-a-11-blank)).
- **A single trailing `\n` is stripped and ignored**, and a lone trailing `\r` after it is tolerated
  too (so a stray CRLF at end-of-file does not add a phantom row). Interior CRs are **not** stripped —
  they ride inside the token text.

The annotation is *not* part of the body and never contributes a cell value. Because the annotation is
file line 1, **a body line `n` reports as file line `n + 1`** in diagnostics — every located body
refusal uses the file line number.

---

## 4. The grid and its TSV deserializer

The body deserializes to a **grid** (SPEC.md FT‑4): for every coordinate in the file's closed range,
exactly one cell — an explicit literal value or a parsed `=formula`. The current deserializer is
**TSV** (FT‑5), and it is the whole of the format: switch the format and you switch only the
deserializer (FT‑7). `deserialize_tsv` (`charlie-model/src/grid.rs`) builds the grid.

- **Rows** are newline-separated; **columns** are tab-separated (`\t`). Rows run top → bottom (body
  line 2 = the range's top row, downward); fields run left → right (left field = left-most column).
- **Each field** is classified independently: a field beginning with `=` is a parsed **formula** cell
  (its `Expr` plus the verbatim source text, so `--functions` can echo it); any other field is a
  **literal** value ([§4.1](#41-literal-value-lexing-lex_literal-per-field)); an **empty field**
  (nothing between tabs) is a **Blank** cell. A double tab (`a⇥⇥b`) blanks the middle cell; leading and
  trailing empty fields are Blank cells too.
- **The column count is fixed by the first line** (`lines[0].split('\t').count()`). Every subsequent
  line must split into that **same** field count, or the grid is **ragged** → `ragged-grid`, a
  `#VALUE!`-class structural refusal located at the offending file line. This precedes, and is
  independent of, the [§5](#5-the-grid-fills-the-range-exactly-ft-8) dimension check.
- An **unparseable `=formula`** field is a located `formula-syntax` refusal at the field's byte column.

A `=formula` field is stored as a **parsed `Expr`** (plus its source text); the engine evaluates it
(charlie-ast owns lex/parse/eval). The grid's own dimensions come from the content; whether they fill
the declared range is [§5](#5-the-grid-fills-the-range-exactly-ft-8).

### 4.1 Literal value lexing (`lex_literal`, per field)

Each non-formula field lexes to a `charlie_ast::Value` by this **ordered** precedence (first match
wins):

| # | Token form | Value |
|---|---|---|
| 1 | empty field (nothing between tabs) | `Blank` |
| 2 | `'…` (leading apostrophe) | `Text` of the remainder — force-text (e.g. `'123` → `Text "123"`) |
| 3 | `"…"` (len ≥ 2, starts and ends with `"`) | `Text`, interior unescaped: `\t`→tab, `\"`→quote, `\\`→backslash (an unknown `\x` keeps the backslash) |
| 4 | `TRUE`, `FALSE` | `Bool` — **case-SENSITIVE, uppercase only** ([R3](#r3-booleanerror-literals-are-case-sensitive)) |
| 5 | `#REF!` `#DIV/0!` `#VALUE!` `#NAME?` `#N/A` `#NULL!` `#NUM!` | `Error(kind)` — the seven v1 error literals |
| 6 | a **finite** number (`123`, `-4`, `3.14`, `1.2e6`) | `Number(f64)` — non-finite spellings do **not** match ([R4](#r4-non-finite-numerics-lex-as-text)) |
| 7 | anything else | `Text` |

`#SPILL!` and `#CALC!` are **reserved** and are *not* literal error tokens — they fall through to
`Text`. A single-cell file's body is a single such token; an empty single-cell body is `Blank`.

---

## 5. The grid fills the range exactly (FT‑8)

The deserialized grid's shape must **fill the file's declared closed range exactly** (SPEC.md FT‑8).
`parse_file` compares the grid shape to the declared shape; a mismatch is a located
`dimension-mismatch` refusal naming the file, the grid shape, and the declared shape.

There is **no broadcast, fill, or spill** mechanism: a range file is an **explicit grid** — every
coordinate carries its own cell. A `B2:D9` file must deserialize to a 3×8 grid; a 1×3 body into a 3×3
range is simply short, and is a dimension error (not a broadcast-down). This is a **static** check —
charlie's advantage over Excel's runtime-only detection.

---

## 6. Content, not position (FT‑9) — no drag-fill

A cell's value derives **only from its own content, never from its position** (SPEC.md FT‑9). A
`=formula` field is evaluated **exactly as written**: its references are absolute A1 addresses
resolved against the workbook, independent of where the cell sits. `=B2*C2` in cell F2 means `B2*C2`;
the same text in F3 also means `B2*C2`.

There is **no single-formula drag-fill anywhere** in the model. A column of per-row formulas is
authored as an **explicit grid**, one offset formula per cell — the values a spreadsheet user would
see after filling `=B2*C2` down are written out literally: `=B2*C2` / `=B3*C3` / … one per row.
Relative references are shifted with position by the *author* (or the tool that produced the file);
`$`-absolute references are pinned. The engine never offsets — it only evaluates what each cell holds.

Evaluation is **demand-driven, memoized per resolved `(sheet, col, row)`, and cycle-safe** (FT‑12):
only transitively-requested cells compute, each at most once, and a reference cycle is a located
`#REF!`-class refusal rather than a hang (`charlie-model/src/workbook.rs`).

---

## 7. Overlap and gaps within a folder

Within one tab, the set of declared regions must be **pairwise disjoint**. `detect_overlaps`
(`charlie-model/src/overlap.rs`) intersects every pair of `Rect`s; each intersecting pair yields one
located `overlap` diagnostic naming **both files** and the **contested cells** (the intersection
rectangle, rendered as an A1 region).

- **Overlap is a hard error** — any two files whose closed ranges intersect (a range∩range, a
  cell∩range, or two identical single-cell files) reject.
- **Precedence: REJECT** — v1 never picks a winner by ordering, recency, or specificity. The workbook
  is invalid until the author removes the overlap. Edge-touching (sharing a boundary but no cell) is
  **not** an overlap. **Gaps are allowed and read as Blank.**

---

## 8. The per-file annotation convention

**Every** file's **line 1** is a Concern annotation. `parse_file` requires only that line 1 begins
with `# ` (hash + space); the field grammar below is the authoring convention.

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
sheet name is a `#REF!`-class refusal. 3-D refs (`Sheet1:Sheet3!A1`) are reserved, not v1.

---

## 10. A worked example (explicit grid)

`Orders/A1:D1` is a `1×4` header — a single body line of four tab-separated literals — filling the
range exactly:

```
# Concern: order-book column headers | Non-concern: the data rows below | IO: input
Product	Unit Price	Qty	Line Total
```

`Orders/D2:D6` is the per-row line total. Under the old form this was a single drag-fill `=B2*C2`;
under the current form it is the **explicit grid** it denoted — one offset formula per cell, so
rendered values are identical:

```
# Concern: per-line revenue = unit price × quantity | Non-concern: the grand total (D7) | IO: none
=B2*C2
=B3*C3
=B4*C4
=B5*C5
=B6*C6
```

`Orders/D7` is a single-cell aggregate `=SUM(D2:D6)`. `Summary/B1` reads it cross-sheet with
`=Orders!D7`. An empty single-cell file `Notes/A1` (annotation only, no body) is a `1×1` Blank.

---

## 11. Quick illegal-forms checklist (for authors)

- `a1`, `A01`, `$A$1`, `A1:A1`, `G8:A3`, `A:A`, or a legacy `A1.cell` / `A1:D1.range` (the trailing
  ending is now unparseable) — non-canonical / degenerate / reserved / malformed names → reject
  ([§2](#2-filename-grammar-the-closed-range-encoding)).
- A TSV grid with unequal field counts per line → ragged → `#VALUE!`-class reject (§4).
- A grid that does not fill the declared closed range → `dimension-mismatch` (§5).
- Two files with intersecting declared regions in one folder → overlap → reject (§7).
- Missing line-1 annotation, or line 1 not starting with `# ` → reject (§8).

---

## 12. Resolutions (deserializer edge cases)

Points the deserializer pins definitively; they are load-bearing in `charlie-model`.

### R2 — an empty body is a 1×1 Blank

An **empty body** (nothing after the line-1 annotation, modulo the stripped trailing newline)
deserializes to a `1×1` grid holding a single `Blank`. This fills a single-cell file (`A1`) exactly;
for a multi-cell range it is short and is a §5 `dimension-mismatch` (the range must be an explicit
grid). This resolves toward ACCEPT for the single-cell case (ast-standards PART 6 — a false-reject is
the cardinal sin), consistent with §7 (unclaimed cells already read as Blank).

### R3 — Boolean/error literals are case-SENSITIVE

Per the §4.1 lexer table, `TRUE`/`FALSE` and the seven `#…!` error literals match **uppercase only**.
`true`, `True`, `#ref!`, etc. do **not** match — they fall through to `Text`. There is no
case-folding.

### R4 — non-finite numerics lex as Text

A numeric field becomes a `Number` **only if it parses to a finite `f64`**. `inf`, `nan` (in any
case), and overflowing magnitudes like `1e999` (which parse to ±∞) do **not** match rule 6 of §4.1 —
they lex as `Text`, never a non-finite `Number`. This keeps the volatile non-finite float spellings
out of the number domain entirely.

---

## 13. `TEXT()` number-format subset (the charlie-ast formula layer)

> **Scope note.** §1–§12 above are the `charlie-model` on-disk contract. This section is the ONE
> formula-layer item this doc owns: the format-code vocabulary the `charlie-ast` `TEXT(value,
> format)` function accepts. It lives here because `TEXT`'s second argument *is* a format code, and
> the batch spec places its documentation in `format.md`; the format-code classifier
> (`func::classify_format`) is the single source of truth that both the parse-time gate and the
> render path read, so this table and the code cannot drift.

`TEXT(value, format)` renders a value as text under a **deliberately small, documented subset** of
Excel's format-code language. The subset is enough for the common reporting cases (fixed decimals,
grouped thousands, percent, an ISO date); everything else is **refused, never guessed** — a
wrong-format render is worse than an honest refusal.

### 13.1 The supported codes

| Format code | Kind | Example | `TEXT` result |
|---|---|---|---|
| `General` (any case) | general text of the value | `TEXT(5, "General")` | `5` |
| `0`, `00`, `0.00`, `0.000`, … | fixed decimals; leading `0`s set the minimum integer width, trailing `0`s the decimal places | `TEXT(3.14159, "0.00")` | `3.14` |
| `#,##0`, `#,##0.00`, … | thousands-grouped integer, optional fixed decimals | `TEXT(1234567, "#,##0")` | `1,234,567` |
| `0%`, `0.00%`, … | percent: value ×100, a `0`-mask, trailing `%` | `TEXT(0.5, "0%")` | `50%` |
| `yyyy-mm-dd` (any case) | ISO date from a serial (1900 date system) | `TEXT(44927, "yyyy-mm-dd")` | `2023-01-01` |

- **Rounding** is half-away-from-zero (`TEXT(2.5, "0")` → `3`), matching `ROUND`.
- **A negative value** carries a leading `-`; a value that rounds to zero prints unsigned
  (`TEXT(-0.001, "0.00")` → `0.00`, never `-0.00`).
- **Coercion.** A numeric/date/percent format coerces `value` to a number (numeric text parses); a
  value that cannot coerce (e.g. `TEXT("abc", "0.00")`) is `#VALUE!`. An **error** `value` propagates
  unchanged (`TEXT(1/0, "0.00")` → `#DIV/0!`).

### 13.2 The date epoch decision (worth a reviewer's eye)

`yyyy-mm-dd` reads the value as an Excel **date serial** in the **1900 date system, and it
replicates Excel's 1900 leap-year bug** for round-trip fidelity with Excel-authored serials:

- serial `1` = `1900-01-01`; serial `59` = `1900-02-28`;
- serial `60` = the **fictional `1900-02-29`** (Excel invented this day) — charlie prints it verbatim;
- serial `61` = `1900-03-01`; from here serials re-align with the real proleptic-Gregorian calendar
  (so serial `44927` = `2023-01-01`).
- The integer day is `floor`ed from the serial; a serial `< 1` (before the epoch) is `#VALUE!`
  (rather than Excel's fictional `1900-01-00`).

### 13.3 Everything else is a located refusal — NOT a guess

Any format string **literal** outside §13.1 — a currency `$#,##0.00`, a scientific `0.00E+00`, a
custom `[Red]0;…`, a bare `mmm` month name, a fraction `# ?/?` — is **refused at parse time** with the
named `unsupported-format` diagnostic (`DiagCode::UnsupportedFormat`), located on the `TEXT` call: the
code is *statically* known-wrong, so we catch it up front rather than mis-render it. The format subset
is small and knowable, so an unknown *literal* code is a refusal we can locate, not a `#VALUE!` we'd
have to guess our way into.

A **non-literal** format argument (`TEXT(A1, B1)`) is a different case: v1 cannot vet a *computed*
format statically, and **rejecting it up front would be a false-reject** — a dynamic format that
RESOLVES to a supported code (`B1 = "0.00"`) is a formula real Excel accepts and computes, so refusing
the whole call would diverge from the oracle. So a non-literal format is **accepted at parse and
deferred to eval**: `text_fn` classifies the *resolved* string and returns `#VALUE!` **iff** it turns
out unsupported. This is **accept-under-uncertainty** (ast-standards §6 / PART 6, the cardinal rule): a
false-reject is the cardinal sin, and the deferred gap is only a false-*negative* (an unsupported
dynamic format surfaces as eval's `#VALUE!`, never a parse refusal). Widening the supported subset is a
deliberate, tested change to `classify_format` plus this table.

## 14. Date & time functions (the charlie-ast formula layer)

> **Scope note.** Like §13, this is a formula-layer item documented here because the batch spec
> places the date-epoch decision in `format.md`. The functions live in `charlie-ast` (`func.rs`'s
> date section); §13.2 already fixes the shared serial↔date mapping, and this section covers the
> `DATE YEAR MONTH DAY EDATE DATEDIF TODAY NOW` batch that reads/writes those serials.

**Epoch (worth a reviewer's eye).** Every date value is an Excel **1900-system date serial** — the
exact convention §13.2 pins, **including Excel's 1900 leap-year bug** (serial `60` = the fictional
`1900-02-29`; serials `≥ 61` shift back one day, so `44927` = `2023-01-01`). The bug is replicated
deliberately for later xlsx round-trip fidelity — a serial authored in Excel maps to the same civil
date here. `serial_to_ymd` (forward) and `serial_from_ymd` (inverse) are the single home of the bug;
`DATE`/`EDATE` build serials by day-offset arithmetic in the contiguous serial space, so
`DATE(1900,2,29)` reproduces the phantom serial `60` with no special case. The valid serial band is
`[1, 2958465]` (`1900-01-01` … `9999-12-31`); outside it is `#NUM!`.

| Function | Behavior | Excel-semantics call |
|---|---|---|
| `DATE(y, m, d)` | serial for the date | truncates args toward zero, **normalizes** out-of-range month/day (roll-over), folds year `0..=1899` by `+1900`; year outside `0..=9999` or a result outside the band is `#NUM!` |
| `YEAR/MONTH/DAY(serial)` | the date component | `floor`s the serial; leap-bug faithful (`MONTH(60)=2`, `DAY(60)=29`); serial `< 1` is `#NUM!` |
| `EDATE(start, months)` | date `months` months on | **clamps** the day to the target month's last day (`EDATE(2020-01-31, 1)`=`2020-02-29`) |
| `DATEDIF(start, end, unit)` | elapsed time | units `"Y"/"M"/"D"` (complete years/months/days) + `"MD"/"YM"/"YD"` (remainders); unit folds case; `start>end` and an unknown unit are `#NUM!` |
| `TODAY()` | today's integer serial | **VOLATILE**; `floor`s the injected clock |
| `NOW()` | date+time serial | **VOLATILE**; keeps the time-of-day fraction (noon = `.5`) |

### 14.1 The injectable clock (why TODAY/NOW are reproducible)

`TODAY`/`NOW` are **volatile** — flagged `volatile: true` in the registry — because their value
depends on the wall clock, not only their (absent) arguments. To keep them from making conformance
non-deterministic, the engine reads "now" through **one seam**: `Resolver::now_serial()` (surfaced to
built-ins as `EvalCtx::now_serial`), never `std::time` inline. The trait's **default** reads the real
system clock (production gets wall-clock time for free); a deterministic resolver — the engine's test
grid and the conformance stub — **overrides** it to the pinned instant `PINNED_NOW_SERIAL` =
`44927.5` (`2023-01-01T12:00:00`). So every `TODAY()`/`NOW()` fixture is reproducible, and the `#NUM!`
volatility never leaks into the corpus. (`TODAY`/`NOW` have no runtime error path — they are nullary
and total; an over-arity call like `=TODAY(1)` is a *parse-time* refusal, not a value.)
