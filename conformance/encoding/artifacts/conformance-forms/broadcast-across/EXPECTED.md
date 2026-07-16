<!-- Concern: the expected conformance verdict for the broadcast-across fixture, citing the FORMAT.md rule | Non-concern: broadcast-down or the square disambiguator (separate fixtures) | IO: output -->
# EXPECTED — broadcast-across

**Fixture:** `Rates/A1:B3.range`
**Rule under test:** FORMAT.md §6 (col-vector row of the conformance table) + §6.1 (result shape of a literal block).

## Inputs
- **Declared shape** (from filename `A1:B3`, §2): `(R, C) = (3, 2)` — rows 1..3, cols A..B. `C = 2 > 1`.
- **Body** (§4.2, §5): three TSV lines, one field each (`10` / `20` / `30`) ⇒ **literal shape `3×1`** (three lines, one field). Per §6.1 a one-field-per-line block is intrinsically a `k×1` column vector.

## Verdict: **VALID — broadcast ACROSS**
Result shape `3×1` is a **column vector**. §6 table: a col vector `k×1` *conforms iff `k == R`*. Here `k = 3 == R = 3` ⇒ conforms; placement is **broadcast across — copy the col to all C cols**.

## Expected rendered cells
Every one of the 2 columns equals the vector:
```
A1=10  B1=10
A2=20  B2=20
A3=30  B3=30
```
i.e. col A1:A3 == col B1:B3 == `[10, 20, 30]`.

## Why (citation)
FORMAT.md §6, row *"col vector `k × 1` — conforms iff `k == R` — broadcast across, copy the col to all C cols."* The vector's axis is a property of its own `k×1` on-disk shape (§6.1), not the range's dimensions. Single, unambiguous verdict.
