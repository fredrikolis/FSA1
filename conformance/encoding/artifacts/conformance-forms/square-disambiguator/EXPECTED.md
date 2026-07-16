<!-- Concern: the expected verdict for the R==C square-range disambiguator fixture, and the explicit B1-kill check | Non-concern: the non-square broadcast cases (separate fixtures) | IO: output -->
# EXPECTED — square-RxC disambiguator (the §6.1 clause B1 must not find ambiguous)

**Fixture:** `Grid/B2:D4.range`
**Rule under test:** FORMAT.md §6.1 — *"the axis of a vector is a property of the vector itself (its `1×k` vs `k×1` shape), never inferred from the declared range."*

## Inputs
- **Declared shape** (from filename `B2:D4`, §2): `(R, C) = (3, 3)` — **square, `R == C`**. This is the case a naive reader might claim is ambiguous: with equal dimensions, could a vector fill down *or* across?
- **Body** (§4.2, §5): a single TSV line `1  2  3` ⇒ **literal shape `1×3`**, a row vector (§6.1: a one-line block is `1×k`).

## Verdict: **VALID — broadcast DOWN. Exactly one defensible answer. NOT ambiguous.**
The square range does **not** make the verdict ambiguous. §6.1 fixes the axis from the body's own shape: the body is `1×3` (a row vector), and a row vector conforms iff `k == C` (`3 == 3` ✓) and is placed **broadcast down**. The equal `R` does not license an "across" reading — "across" is reserved for a `k×1` column vector, which this file is not.

## Expected rendered cells
Broadcast down all 3 rows (rows B2:D2 == B3:D3 == B4:D4):
```
B2=1  C2=2  D2=3
B3=1  C3=2  D3=3
B4=1  C4=2  D4=3
```

## The disambiguator, stated as the single deciding fact
The on-disk orientation of the body is the **entire** disambiguation. Had the same three values been authored one-per-line (a `3×1` column vector), §6 would instead require `k == R` and place them **broadcast across** (B/C/D columns each `[1,2,3]`). One file → one shape → one verdict. Two *different* files, not two verdicts for one file.

## B1 kill-signal check (per README.md / BRIEF.md)
**No kill.** This fixture is the deliberate probe for BRIEF.md's B1 kill criterion (*"a dimension rule that is genuinely ambiguous — two defensible conformance verdicts for one file"*). Under §6.1 this square-range case yields **one** verdict, not two. If a reviewer can construct a real corpus file where §6.1's two sentences produce **two defensible verdicts**, that IS the B1 kill finding and must be recorded loudly in `CONCLUSIONS.md` — but this fixture does not produce one.

## Why (citation)
FORMAT.md §6.1, the bolded disambiguator paragraph, plus §6 table rows for row/col vectors.
