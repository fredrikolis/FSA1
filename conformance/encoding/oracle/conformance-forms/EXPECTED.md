<!-- Concern: consolidated index of expected verdicts for the conformance-forms fixtures (valid broadcast/edge cases) | Non-concern: the invalid-forms ledger (see ../invalid-forms/EXPECTED.md) and how values are computed (these are static shape verdicts, not evaluated) | IO: output -->
# EXPECTED — conformance-forms (valid broadcast / edge cases)

Consolidated verdict index for `artifacts/conformance-forms/`. Each row's authoritative reasoning
lives in the per-fixture `EXPECTED.md` beside the fixture; this table is the ruler's quick oracle.

**Nature of these verdicts.** These are **static broadcast-conformance** verdicts (FORMAT.md §6/§6.1)
— a shape decision made at load, not a numeric evaluation. No charlie evaluator is involved, so
oracle-input purity is trivially preserved (there is no computed value to grade, only a
conform/reject decision and a placement).

| Fixture | File | Declared (R×C) | Body result shape | Verdict | Placement | FORMAT.md § |
|---|---|---|---|---|---|---|
| broadcast-down | `Rates/A1:C2.range` | 2×3 (R>1) | `1×3` row vector | **VALID** | broadcast DOWN; every row = `[0.1,0.2,0.3]` | §6 (row-vec), §6.1 |
| broadcast-across | `Rates/A1:B3.range` | 3×2 (C>1) | `3×1` col vector | **VALID** | broadcast ACROSS; every col = `[10,20,30]` | §6 (col-vec), §6.1 |
| square-disambiguator | `Grid/B2:D4.range` | 3×3 (R==C) | `1×3` row vector | **VALID — single verdict** | broadcast DOWN (not across); NOT ambiguous, NO B1 kill | §6.1 disambiguator |
| degenerate-1×1-range | `Cell/A1:A1.range` | 1×1 | — (rejected at filename) | **REJECT** | rename to `A1.cell` (format mandates reject, not accept-and-canonicalize) | §1, §2, §11 |

## The R==C disambiguator (called out — this is the clause B1 must not find ambiguous)
`Grid/B2:D4.range` yields **exactly one** defensible verdict: broadcast **down**. §6.1 fixes the axis
from the body's own on-disk shape (`1×3` ⇒ row vector ⇒ down), never from the square range. The same
values authored one-per-line would be a *different file* (`3×1`) that broadcasts across — one file,
one shape, one verdict. **No two-defensible-verdicts case; no B1 kill signal from this fixture.** If a
reviewer finds a real corpus file where §6.1 genuinely yields two defensible verdicts, that is the B1
kill finding for `CONCLUSIONS.md` — none is present here.
