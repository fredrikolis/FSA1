<!-- Concern: the expected verdict for a degenerate 1x1 range filename under the current spec | Non-concern: the invalid-forms fixtures | IO: output -->
# EXPECTED — degenerate 1×1 range (`A1-A1`)

**Fixture:** `Cell/A1-A1`
**Rule under test:** SPEC.md FS2 (a file's name is a *closed range*) + the filename canonical-form policy.

## Inputs
- **Filename** `A1-A1` declares a `left:right` range whose top-left equals its bottom-right ⇒ shape `1×1`.

## Verdict: **REJECT (degenerate range).**
A `left:right` filename must span a rectangle whose endpoints differ; a 1×1 location is written as the
single address `A1`, never as `A1-A1`. The loader rejects the filename before any body/grid check.

## Expected diagnostic (verbatim)
```
error[degenerate-range]: a 1x1 range is illegal; a single cell is written A1
  A1-A1
```

## Why (citation)
SPEC.md FS2: *"A file's name is a closed range — a bounded rectangle of A1 cells with inclusive
endpoints … one cell is `A1`."* The 1×1 `left:right` spelling is redundant with the single-address
form and is refused; the fix is to name the file `A1`.
