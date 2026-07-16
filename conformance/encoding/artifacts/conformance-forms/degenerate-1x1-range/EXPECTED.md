<!-- Concern: the expected verdict for a degenerate 1x1 .range filename, and which resolution FORMAT.md mandates | Non-concern: the broadcast fixtures | IO: output -->
# EXPECTED — degenerate 1×1 range (`A1:A1.range`)

**Fixture:** `Cell/A1:A1.range`
**Rule under test:** FORMAT.md §2 (canonical filename grammar) + §11 (illegal-forms checklist).

> Placed under `conformance-forms/` because the brief lists it as an *edge case* to pin down; its
> resolved verdict is nonetheless a **rejection**. FORMAT.md leaves no "valid-but-canonicalize"
> option for this form.

## Inputs
- **Filename** `A1:A1.range` declares a range whose top-left equals its bottom-right ⇒ shape `1×1`.

## Verdict: **REJECT (non-canonical / degenerate name).** FORMAT.md mandates **reject in favor of `A1.cell`** — not valid-but-canonicalize.
A `.range` file must declare a rectangle of **≥2 cells** (§1: *"a RANGE … = a rectangular block of ≥2 cells"*). A 1×1 location is always a `.cell`, never a `.range`. The loader rejects the filename before any body/conformance check.

## Expected diagnostic (shape)
A located, filename-grammar refusal, e.g.:
```
error[filename]: degenerate 1x1 range is not a legal .range spelling
  Cell/A1:A1.range  declares a single cell (top-left == bottom-right)
  fix: rename to  A1.cell
```

## Why (citation)
- FORMAT.md §2: *"A degenerate 1×1 range is illegal — a single cell is always `.cell`, never `A1:A1.range`."*
- FORMAT.md §1: a `.range` is *"a rectangular block of ≥2 cells."*
- FORMAT.md §11: *"`A1:A1.range` … → reject."*

**Which the format mandates:** REJECT (canonicalize *by renaming to `A1.cell`*, i.e. the `.range` spelling itself is never accepted). There is no accept-and-rewrite path in the spec.
