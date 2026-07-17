<!-- Concern: the expected reject verdict for a ragged TSV grid (unequal field counts per line) | Non-concern: the other illegal-forms cases | IO: output -->
# EXPECTED — illegal-forms: ragged TSV grid

**Fixture:** `Sheet/A1:C2`
**Rule under test:** SPEC.md GRID2 (the TSV deserializer) — a grid's rows must have equal field counts.

## Inputs
- Filename & annotation are canonical/valid — the sole defect is the body.
- Body (TSV, GRID2):
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
  A1:C2 (body row 2)
```

## Why (citation)
SPEC.md GRID2 / CORE2 (totality): a malformed file yields a located refusal pointing at the offending
row, never a crash or a silent wrong value.
