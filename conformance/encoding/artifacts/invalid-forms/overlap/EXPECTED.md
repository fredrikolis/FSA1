<!-- Concern: the expected overlap-reject verdict naming both offending files and the contested cells | Non-concern: shape-mismatch and illegal-forms (separate fixtures) | IO: output -->
# EXPECTED — overlap → reject (naming both files)

**Fixture:** tab `Orders/` containing two range files: `A1:C3.range` and `B2:D4.range`.
**Rule under test:** FORMAT.md §7 (overlap is a first-class hard error; precedence = REJECT).

## Inputs
- `Orders/A1:C3.range` declares cols A..C × rows 1..3.
- `Orders/B2:D4.range` declares cols B..D × rows 2..4.
- Within one tab, all declared cells must be **pairwise disjoint** (§7). These two intersect.

## Intersection (contested cells)
Cols {B, C} × rows {2, 3} = **B2, C2, B3, C3** (4 cells).

## Verdict: **REJECT — overlap. No precedence; the folder fails to load.**
v1 does not resolve overlaps by ordering, recency, or specificity. The workbook is invalid until the author splits or deletes one file.

## Expected diagnostic (shape, per §7)
An ASCII diagnostic **naming both files and the contested cells**, e.g.:
```
error[overlap]: two files claim overlapping cells in tab "Orders"
  A1:C3.range  and  B2:D4.range
  contested: B2, C2, B3, C3
  precedence: none — reject. Split or delete one file.
```

## Why (citation)
FORMAT.md §7: *"Overlap is a first-class, hard error (`reject`, never guess a winner) … the folder fails to load with an ASCII diagnostic naming both files and the contested cells."* Precedence rule: *"REJECT."* Also §11: *"Two files with intersecting declared regions in one folder → overlap → reject."*
