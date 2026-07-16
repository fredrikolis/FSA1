<!-- Concern: consolidated index of expected reject verdicts for the invalid-forms fixtures (cases FORMAT.md says must be REJECTED) | Non-concern: the valid conformance-forms ledger (see ../conformance-forms/EXPECTED.md) | IO: output -->
# EXPECTED — invalid-forms (must be REJECTED)

Consolidated verdict index for `artifacts/invalid-forms/`. Each row's authoritative reasoning lives
in the per-fixture `EXPECTED.md` beside the fixture; this table is the ruler's quick oracle. **Every
row is a REJECT** — the ruler passes iff charlie/W2 refuses each fixture with the cited reason.

| Fixture | File(s) | Defect (the ONE rule under test) | Reject class | Diagnostic must name | FORMAT.md § |
|---|---|---|---|---|---|
| shape-mismatch | `Grid/A1:C3.range` | `2×3` body into `3×3` range — neither exact nor broadcastable | `#SPILL!`-class static refusal | file, declared 3×3, result 2×3 | §6 (last row), §11 |
| overlap | `Orders/A1:C3.range` + `Orders/B2:D4.range` | two files claim intersecting regions | overlap reject (no precedence) | **both files** + contested `B2,C2,B3,C3` | §7, §11 |
| illegal-name | `Sheet/G8:A3.range` | non-canonical spelling (bottom-right:top-left) | filename reject | file; fix → `A3:G8.range` | §2, §11 |
| ragged-block | `Sheet/A1:C2.range` | literal block with unequal field counts (3 then 2) | `#VALUE!`-class structural refusal | file; offending line | §5, §11 |
| dual-body | `Sheet/A1.cell` | body has both an `=formula` line and a literal line | body reject (exactly one form) | file; the two conflicting lines | §4, §11 |
| stray-dollar | `Sheet/$A$1.cell` | `$` absolute marker in a filename | filename reject | file; fix → `A1.cell` | §2, §11 |

## Notes on isolation (each fixture triggers exactly ONE rule)
- **illegal-name** and **stray-dollar** carry valid bodies/annotations — the *only* defect is the
  filename, so the reject is unambiguously a filename-grammar refusal (§2), not a body issue.
- **ragged-block** and **dual-body** carry canonical filenames/annotations — the *only* defect is the
  body, isolating §5 and §4 respectively.
- **shape-mismatch** vs **ragged-block** are deliberately distinct classes: shape-mismatch is a
  *well-formed* `2×3` block that fails the §6 broadcast rule (`#SPILL!`), whereas ragged-block is a
  *malformed* block with no defined shape at all (`#VALUE!`, §5) — a structural refusal that precedes
  the §6 check.
- **overlap** is the only multi-file fixture; the diagnostic must name **both** files (§7), never pick
  a winner.
