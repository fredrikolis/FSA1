<!-- Concern: the expected reject verdict for a ragged TSV grid (unequal field counts per line) | Non-concern: the other illegal-forms cases | IO: output -->
# EXPECTED — illegal-forms: ragged TSV grid

**Fixture:** `Sheet/A1-C2`
**Rule under test:** .githooks/commit-msg the encoding (the TSV deserializer) — a grid's rows must have equal field counts.

## Inputs
- Filename is canonical/valid, and the whole file content is the grid (GRID1, no annotation line) —
  the sole defect is the body.
- Body (TSV, GRID2) — the file's entire content, first line is the first row:
  ```
  1	2	3      (3 fields)
  4	5         (2 fields)
  ```
  Field count differs per line (3 then 2) ⇒ **ragged**.

## Verdict: **REJECT — `#VALUE!`-class structural refusal at load.**
The TSV deserializer builds a grid of tab-separated columns; a row whose field count differs from the
first has no well-defined grid shape and is refused structurally, before any fill-the-range (GRID4)
check.

## Expected diagnostic (verbatim)
```
error[ragged-grid]: ragged TSV grid: row 2 has 2 field(s), expected 3 (#VALUE!-class)
  A1-C2 (grid row 2 = file line 2)
```
The refusal is located at file line 2 — the offending grid row. (With the whole file now the grid,
grid row `n` is file line `n`; there is no annotation line to offset by, so this is one line earlier
than under the former annotated format.)

## Why (citation)
.githooks/commit-msg the encoding / CORE2 (totality): a malformed file yields a located refusal pointing at the offending
row, never a crash or a silent wrong value.
