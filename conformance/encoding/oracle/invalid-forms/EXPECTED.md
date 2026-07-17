<!-- Concern: consolidated index of expected reject verdicts for the invalid-forms fixtures (cases the spec says must be REJECTED) | Non-concern: the conformance-forms ledger (see ../conformance-forms/EXPECTED.md) | IO: output -->
# EXPECTED — invalid-forms (must be REJECTED)

Consolidated verdict index for `artifacts/invalid-forms/`. Each row's authoritative reasoning lives in
the per-fixture `EXPECTED.md` beside the fixture; this table is the quick oracle. **Every row is a
REJECT** — the ruler passes iff charlie refuses each fixture with the cited reason.

> **Migration note (no-endings + explicit-grid + TSV deserializer + no annotation line).** Filenames
> dropped their `.range`/`.cell` endings (the name IS the closed range, FS2). Each fixture's former
> line-1 `# Concern…` annotation was **removed** — a file's content is now exactly its grid (GRID1),
> the first line is the first row — so a body-located refusal (ragged-block) now points one file line
> earlier than before. The `shape-mismatch` verdict is now a plain **GRID4 dimension error** (a range
> file is an explicit grid that must fill its range; there is no broadcast, so the old `#SPILL!`-class
> framing is retired). The `dual-body` fixture — which tested the retired "exactly one body form" rule
> — has been **deleted**: under the TSV deserializer every line is just a grid row, so a two-line body
> in a `1×1` file is simply a GRID4 dimension error, already covered by `shape-mismatch`.

| Fixture | File(s) | Defect (the ONE rule under test) | Diagnostic code | Diagnostic names | SPEC.md |
|---|---|---|---|---|---|
| shape-mismatch | `Grid/A1:C3` | `2×3` grid does not fill the `3×3` range | `dimension-mismatch` | file, grid 2×3, declared 3×3 | GRID4 |
| overlap | `Orders/A1:C3` + `Orders/B2:D4` | two files claim intersecting ranges | `overlap` | **both files** + contested `B2:C3` | model overlap rule |
| illegal-name | `Sheet/G8:A3` | non-canonical spelling (bottom-right:top-left) | `non-canonical-range` | file; fix → `A3:G8` | FS2 |
| ragged-block | `Sheet/A1:C2` | ragged TSV grid (3 fields then 2) | `ragged-grid` | file; offending row | GRID2 |
| stray-dollar | `Sheet/$A$1` | `$` absolute marker in a filename | `dollar-in-filename` | file; fix → `A1` | FS2 |

## Notes on isolation (each fixture triggers exactly ONE rule)
- **illegal-name** and **stray-dollar** carry valid bodies — the *only* defect is the filename, so the
  reject is unambiguously a filename-grammar refusal (FS2), not a body issue.
- **ragged-block** carries a canonical filename — the *only* defect is the body's unequal field counts,
  isolating the TSV deserializer (GRID2).
- **shape-mismatch** vs **ragged-block** are distinct classes: shape-mismatch is a *well-formed* `2×3`
  grid that is simply the wrong size for a `3×3` range (GRID4), whereas ragged-block is a *malformed*
  grid with no defined shape at all (GRID2) — a structural refusal that precedes the GRID4 check.
- **overlap** is the only multi-file fixture; the diagnostic must name **both** files and choose no
  winner.
