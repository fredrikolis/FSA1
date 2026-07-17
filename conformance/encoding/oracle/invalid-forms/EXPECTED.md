<!-- Concern: consolidated index of expected reject verdicts for the invalid-forms fixtures (cases the spec says must be REJECTED) | Non-concern: the conformance-forms ledger (see ../conformance-forms/EXPECTED.md) | IO: output -->
# EXPECTED — invalid-forms (must be REJECTED)

Consolidated verdict index for `artifacts/invalid-forms/`. Each row's authoritative reasoning lives in
the per-fixture `EXPECTED.md` beside the fixture; this table is the quick oracle. **Every row is a
REJECT** — the ruler passes iff charlie refuses each fixture with the cited reason.

> **Migration note (no-endings + explicit-grid + TSV deserializer).** Filenames dropped their
> `.range`/`.cell` endings (the name IS the closed range, FT‑3). The `shape-mismatch` verdict is now a
> plain **FT‑8 dimension error** (a range file is an explicit grid that must fill its range; there is
> no broadcast, so the old `#SPILL!`-class framing is retired). The `dual-body` fixture — which tested
> the retired "exactly one body form" rule — has been **deleted**: under the TSV deserializer every
> line is just a grid row, so a two-line body in a `1×1` file is simply an FT‑8 dimension error,
> already covered by `shape-mismatch`.

| Fixture | File(s) | Defect (the ONE rule under test) | Diagnostic code | Diagnostic names | SPEC.md |
|---|---|---|---|---|---|
| shape-mismatch | `Grid/A1:C3` | `2×3` grid does not fill the `3×3` range | `dimension-mismatch` | file, grid 2×3, declared 3×3 | FT‑8 |
| overlap | `Orders/A1:C3` + `Orders/B2:D4` | two files claim intersecting ranges | `overlap` | **both files** + contested `B2:C3` | model overlap rule |
| illegal-name | `Sheet/G8:A3` | non-canonical spelling (bottom-right:top-left) | `non-canonical-range` | file; fix → `A3:G8` | FT‑3 |
| ragged-block | `Sheet/A1:C2` | ragged TSV grid (3 fields then 2) | `ragged-grid` | file; offending row | FT‑5 |
| stray-dollar | `Sheet/$A$1` | `$` absolute marker in a filename | `dollar-in-filename` | file; fix → `A1` | FT‑3 |

## Notes on isolation (each fixture triggers exactly ONE rule)
- **illegal-name** and **stray-dollar** carry valid bodies/annotations — the *only* defect is the
  filename, so the reject is unambiguously a filename-grammar refusal (FT‑3), not a body issue.
- **ragged-block** carries a canonical filename/annotation — the *only* defect is the body's unequal
  field counts, isolating the TSV deserializer (FT‑5).
- **shape-mismatch** vs **ragged-block** are distinct classes: shape-mismatch is a *well-formed* `2×3`
  grid that is simply the wrong size for a `3×3` range (FT‑8), whereas ragged-block is a *malformed*
  grid with no defined shape at all (FT‑5) — a structural refusal that precedes the FT‑8 check.
- **overlap** is the only multi-file fixture; the diagnostic must name **both** files and choose no
  winner.
