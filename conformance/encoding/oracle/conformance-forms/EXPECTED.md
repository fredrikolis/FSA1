<!-- Concern: consolidated index of expected verdicts for the conformance-forms fixtures (edge cases) | Non-concern: the invalid-forms ledger (see ../invalid-forms/EXPECTED.md) and how values are computed | IO: output -->
# EXPECTED — conformance-forms (edge cases)

Consolidated verdict index for `artifacts/conformance-forms/`. Each row's authoritative reasoning
lives in the per-fixture `EXPECTED.md` beside the fixture; this table is the quick oracle.

**Nature of these verdicts.** These are **static structural** verdicts (a filename/grid shape decision
made at load), not numeric evaluations. No FSA1 evaluator is involved, so oracle-input purity is
trivially preserved.

> **Migration note (value-preserving, no-endings + explicit-grid + TSV deserializer + no annotation
> line).** Under the current spec (SPEC.md) a file's name IS a closed range (no `.range`/`.cell`
> ending) and a range file is an **explicit grid** that fills its range exactly (GRID4) — there is no
> broadcast/drag-fill. Each fixture's former line-1 `# Concern…` annotation was **removed**: a file's
> content is now exactly its grid (GRID1), the first line is the first row. The former
> `broadcast-across`, `broadcast-down`, and `square-disambiguator` fixtures existed only to probe the
> retired broadcast-conformance rule and have been **deleted**. Only the degenerate-range edge case
> survives, because it is still a filename-grammar rejection.

| Fixture | File | Declared (R×C) | Verdict | Diagnostic code | SPEC.md |
|---|---|---|---|---|---|
| degenerate-1×1-range | `Cell/A1:A1` | 1×1 (rejected at filename) | **REJECT** | `degenerate-range` | FS2 |

## The degenerate-range edge case
`A1:A1` is a `left:right` spelling of a single cell. A closed range's endpoints must differ; a 1×1
location is written `A1`. The loader refuses `A1:A1` at filename parse and points the author at `A1`.
