<!-- Concern: the expected reject verdict for a ragged literal block (unequal field counts per line) | Non-concern: the other illegal-forms cases | IO: output -->
# EXPECTED — illegal-forms: ragged literal block

**Fixture:** `Sheet/A1:C2.range`
**Rule under test:** FORMAT.md §5 (literal-block layout; equal field counts) + §11.

## Inputs
- Filename & annotation are canonical/valid — the sole defect is the body.
- Body (§4.2, §5):
  ```
  1  2  3      (3 fields)
  4  5         (2 fields)
  ```
  Field count differs per line (3 then 2) ⇒ **ragged**.

## Verdict: **REJECT — `#VALUE!`-class structural refusal at load.**
A literal block is TSV with an identical field count on every line (§5). Because the field counts are unequal, the block has no well-defined literal shape; charlie refuses it **structurally** — before, and independent of, the §6 broadcast-conformance check.

## Expected diagnostic (shape)
```
error[value]: ragged literal block — field count differs per line
  Sheet/A1:C2.range  line 2 has 3 fields, line 3 has 2
```

## Why (citation)
FORMAT.md §5: *"Field count must be identical on every line (ragged blocks are illegal — a `#VALUE!`-class structural refusal at load)."* Also §11: *"A `.range` literal block with unequal field counts per line → ragged → `#VALUE!`-class reject."*
