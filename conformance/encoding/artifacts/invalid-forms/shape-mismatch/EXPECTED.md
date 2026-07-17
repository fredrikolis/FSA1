<!-- Concern: the expected reject verdict for a grid that does not fill its declared closed range | Non-concern: overlap and illegal-forms (separate fixtures) | IO: output -->
# EXPECTED — shape-mismatch → dimension error (FT‑8)

**Fixture:** `Grid/A1:C3`
**Rule under test:** SPEC.md FT‑8 (the grid fills its file's closed range exactly).

## Inputs
- **Declared shape** (from filename `A1:C3`, FT‑3): `(R, C) = (3, 3)`.
- **Body** (TSV, FT‑5): two lines of three fields each ⇒ a `2×3` grid.

## Verdict: **REJECT — located dimension error at load.**
FT‑8 requires the deserialized grid to fill the declared closed range exactly. The body is `2×3` but
the range declares `3×3`, so the grid does not fill it. There is no broadcast or fill mechanism: a
range file is an explicit grid (FT‑9), so a `2×3` body into a `3×3` range is simply short. Charlie
refuses this **statically at load**.

## Expected diagnostic (verbatim)
```
error[dimension-mismatch]: the grid is 2x3 but the file's range "A1:C3" declares 3x3: the grid must fill the closed range exactly (FT-8)
  A1:C3
```

## Why (citation)
SPEC.md FT‑8: *"A grid fills its file's closed range exactly. — a `B2:D9` file whose grid is not 3×8
is a located dimension error."*
