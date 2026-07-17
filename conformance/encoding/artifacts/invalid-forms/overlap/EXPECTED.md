<!-- Concern: the expected overlap-reject verdict naming both offending files and the contested cells | Non-concern: shape-mismatch and illegal-forms (separate fixtures) | IO: output -->
# EXPECTED — overlap → reject (naming both files)

**Fixture:** tab `Orders/` containing two range files: `A1:C3` and `B2:D4`, each an explicit grid that
fills its own range.
**Rule under test:** the model's overlap detector — two files in one tab whose closed ranges intersect
are a hard error; precedence is REJECT, never a guessed winner.

## Inputs
- `Orders/A1:C3` declares cols A..C × rows 1..3 (a 3×3 explicit grid).
- `Orders/B2:D4` declares cols B..D × rows 2..4 (a 3×3 explicit grid).
- Both grids fill their ranges (FT‑8), so both load; within one tab their declared cells must be
  pairwise disjoint. These two intersect at B2:C3.

## Verdict: **REJECT — overlap. No precedence; the tab fails to load.**
Overlaps are not resolved by ordering, recency, or specificity. The workbook is invalid until the
author splits or deletes one file.

## Expected diagnostic (verbatim)
```
error[overlap]: two files claim overlapping cells in tab "Orders"
    A1:C3  and  B2:D4
    contested: B2:C3
    precedence: none -- reject. Split or delete one file.
```

## Why (citation)
The overlap diagnostic names **both** files and the contested block and chooses no winner. A gap
between ranges is Blank, but an intersection is a first-class refusal.
