<!-- Concern: the expected reject verdict for a body whose shape neither exact-matches nor broadcasts | Non-concern: overlap and illegal-forms (separate fixtures) | IO: output -->
# EXPECTED — shape-mismatch → `#SPILL!`-class static refusal

**Fixture:** `Grid/A1:C3.range`
**Rule under test:** FORMAT.md §6 (final "anything else" row of the conformance table).

## Inputs
- **Declared shape** (from filename `A1:C3`, §2): `(R, C) = (3, 3)`.
- **Body** (§4.2, §5): two TSV lines of three fields each ⇒ **literal shape `2×3`** (a 2-D array, not a scalar and not a vector).

## Verdict: **REJECT — `#SPILL!`-class static refusal (located, at load).**
Walk the §6 table against result shape `2×3`:
- scalar `1×1`? No (2 lines).
- row vector `1×k`? No (2 lines).
- col vector `k×1`? No (3 fields).
- exact array `r×c` with `r==R && c==C`? `r=2 != R=3` ⇒ No.
- ⇒ **"anything else" → static refusal.**

The body neither exact-matches (`2×3 ≠ 3×3`) nor broadcasts (it is a 2-D array, so no vector fill axis applies). Charlie refuses this **statically at load** — its advantage over Excel, which only detects the mismatch at runtime.

## Expected diagnostic (shape)
```
error[spill]: body shape does not conform to declared range
  Grid/A1:C3.range  declared 3x3  body 2x3
  body neither exact-matches nor broadcasts (not scalar / row-vec / col-vec)
```
naming the file, the declared shape, and the result shape (§6).

## Why (citation)
FORMAT.md §6, last table row: *"anything else — static refusal (`#SPILL!`-class), located, at load"*; and §11: *"A … result shape that is none of scalar/row-vec/col-vec/exact-array → `#SPILL!`-class static refusal."*
