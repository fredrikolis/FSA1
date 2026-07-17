<!-- Concern: the expected reject verdict for a non-canonical range filename (bottom-right:top-left) | Non-concern: the other illegal-forms cases | IO: output -->
# EXPECTED — illegal-forms: non-canonical filename

**Fixture:** `Sheet/G8:A3`
**Rule under test:** SPEC.md FT‑3 (a file's name is a closed range) + the canonical-form policy
(top-left`:`bottom-right).

## Inputs
- Filename `G8:A3`: left address `G8`, right address `A3`. A closed range is written
  `minCol minRow : maxCol maxRow`. Here left `G8` is neither top nor left of right `A3` (`G > A`, `8 > 3`).
- The body (`=1+1`, a scalar formula filling the implied cell) is valid on its own — the sole defect is
  the filename, isolating this rule.

## Verdict: **REJECT — non-canonical range spelling.**
`G8:A3` is an illegal spelling of the canonical `A3:G8`. The loader rejects at filename parse, before
any body/grid check.

## Expected diagnostic (verbatim)
```
error[non-canonical-range]: a range must be top-left:bottom-right; "G8:A3" should be A3:G8
  G8:A3
```

## Why (citation)
SPEC.md FT‑3: a closed range has inclusive endpoints written top-left`:`bottom-right; `G8:A3`,
`A8:G3`, `G3:A8` are all illegal spellings of `A3:G8`.
