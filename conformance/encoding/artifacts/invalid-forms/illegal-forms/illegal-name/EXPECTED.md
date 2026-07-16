<!-- Concern: the expected reject verdict for a non-canonical range filename (bottom-right:top-left) | Non-concern: the other illegal-forms cases | IO: output -->
# EXPECTED — illegal-forms: non-canonical filename

**Fixture:** `Sheet/G8:A3.range`
**Rule under test:** FORMAT.md §2 (canonical filename form) + §11.

## Inputs
- Filename `G8:A3.range`: left address `G8`, right address `A3`. A range must be written **top-left `:` bottom-right** = `minCol minRow : maxCol maxRow`. Here left `G8` is neither top nor left of right `A3` (`G > A`, `8 > 3`).
- The body (`=1+1`, a scalar formula) is **valid on its own** — the sole defect is the filename, isolating this rule.

## Verdict: **REJECT — non-canonical filename spelling.**
`G8:A3.range` is an illegal spelling of the canonical `A3:G8.range`. The loader rejects at filename parse, before any body/conformance check.

## Expected diagnostic (shape)
```
error[filename]: non-canonical range spelling (must be top-left ":" bottom-right)
  Sheet/G8:A3.range
  fix: rename to  A3:G8.range
```

## Why (citation)
FORMAT.md §2: *"A range is written top-left `:` bottom-right … `G8:A3.range`, `A8:G3.range`, `G3:A8.range` are all illegal spellings of `A3:G8.range`."* Also §11: *"`… G8:A3.range` — non-canonical / degenerate names → reject."*
