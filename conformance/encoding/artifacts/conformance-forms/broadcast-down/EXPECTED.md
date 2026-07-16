<!-- Concern: the expected conformance verdict for the broadcast-down fixture, citing the FORMAT.md rule | Non-concern: broadcast-across or the square disambiguator (separate fixtures) | IO: output -->
# EXPECTED — broadcast-down

**Fixture:** `Rates/A1:C2.range`
**Rule under test:** FORMAT.md §6 (row-vector row of the conformance table) + §6.1 (result shape of a literal block).

## Inputs
- **Declared shape** (from filename `A1:C2`, §2): `(R, C) = (2, 3)` — rows 1..2, cols A..C. `R = 2 > 1`.
- **Body** (§4.2, §5): a single TSV line `0.1  0.2  0.3` ⇒ **literal shape `1×3`** (one line, three fields). Per §6.1 a one-line block is intrinsically a `1×k` row vector.

## Verdict: **VALID — broadcast DOWN**
Result shape `1×3` is a **row vector**. §6 table: a row vector `1×k` *conforms iff `k == C`*. Here `k = 3 == C = 3` ⇒ conforms; placement is **broadcast down — copy the row to all R rows**.

## Expected rendered cells
Every one of the 2 rows equals the vector:
```
A1=0.1  B1=0.2  C1=0.3
A2=0.1  B2=0.2  C2=0.3
```
i.e. row A1:C1 == row A2:C2 == `[0.1, 0.2, 0.3]`.

## Why (citation)
FORMAT.md §6, row *"row vector `1 × k` — conforms iff `k == C` — broadcast down, copy the row to all R rows."* Orientation is read from the on-disk shape of the body (§6.1: a one-line block is `1×k`), not inferred from the range. Single, unambiguous verdict.
