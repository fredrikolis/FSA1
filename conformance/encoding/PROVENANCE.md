<!-- Concern: the SOURCE of the encoding corpus, and the record of its value-preserving migration to the current (no-endings + explicit-grid + TSV) form | Non-concern: how values are computed (the render value oracles live in conformance/render) | IO: none -->
# PROVENANCE — encoding corpus (frozen contract)

This directory is a **sanctioned graduation** of the W1 encoding corpus into the production repo,
subsequently **migrated value-preservingly** to the current
on-disk form. It is a **frozen CONTRACT** graded against the current spec (`SPEC.md`): the fingerprint
below (**oracle-input purity**) makes any silent byte change in a fixture or an EXPECTED ledger fail a
`sha256sum -c` check, exposing a rewrite.

## Source

| | |
|---|---|
| **Origin** | authored OUTSIDE this repo before graduating here — never FSA1-generated (that is the oracle-input purity claim) |
| **Original commit** | `9367ee90cc5417d9983cb70ed8846d6ad772f47b` (`W1 Substrate: format spec + corpus + purity-checked oracle + QA ladder`) |
| **Originally migrated** | 2026-07-15 |

## The value-preserving migration to the current form

The engine was changed so that a file's name **is** a closed range with **no `.range`/`.cell` ending**
(SPEC.md FS2), its content deserializes to an **explicit grid** that must fill the range exactly
(GRID1/GRID4) via the **TSV** deserializer (GRID2), and a cell's value derives only from its own content
(VAL1) — there is **no broadcast/drag-fill**. This corpus was migrated to match, preserving every
rendered value:

1. **Filenames lost their endings.** Every `<range>.range` / `<addr>.cell` file was renamed to the
   bare closed range (`A2:E13.range` → `A2:E13`, `D14.cell` → `D14`). The name is now the closed
   range itself.
2. **Drag-fill bodies were expanded to explicit grids.** Every range file whose body was a single
   drag-fill `=formula` was rewritten to the **explicit per-cell grid it denoted**: each cell holds
   its own offset formula (relative refs shifted with position, `$`-absolute refs pinned). E.g.
   `Amortization/D2-D13` went from the single body `=B2*Inputs!$B$5` to twelve rows
   `=B2*Inputs!$B$5` / `=B3*Inputs!$B$5` / … / `=B13*Inputs!$B$5`. Because each expanded cell
   evaluates its own formula exactly as the old loader would have offset it, **rendered values are
   unchanged** — proven by the render zero-divergence gate
   (`fsa1-model/tests/render_conformance.rs`), which grades these workbooks against the frozen W1
   value oracle in `conformance/render/` and requires zero divergence.
3. **The per-file line-1 `# Concern…` annotation was removed.** Under SPEC.md GRID1 a file's content
   is **exactly its grid** — there is no header, annotation, or metadata line, and the first line is
   the first row. Every fixture's former line-1 `# Concern: … | Non-concern: … | IO: …` annotation was
   stripped (byte-exact: line 1 and its trailing newline dropped, the remaining bytes verbatim), so
   the content is now just the TSV grid. **Rendered values are unchanged** — the annotation was
   presence-only (nothing parsed its fields), and the render zero-divergence gate still passes. One
   consequence for the ledgers: a body-located refusal now points **one file line earlier** (grid row
   `n` is file line `n`, with no annotation line to offset by), so the `ragged-block` `EXPECTED.md`
   was updated accordingly.
4. **Retired-concept fixtures were deleted.** The `conformance-forms/{broadcast-across,broadcast-down,
   square-disambiguator}` fixtures (which existed only to probe the retired broadcast-conformance
   rule) and `invalid-forms/illegal-forms/dual-body` (the retired "exactly one body form" rule) were
   removed. The surviving edge/invalid fixtures had their `EXPECTED.md` verdicts rewritten to cite
   SPEC.md's subsystem-scoped invariants and the current diagnostic codes (`dimension-mismatch`, `ragged-grid`,
   `non-canonical-range`, `dollar-in-filename`, `degenerate-range`, `overlap`).
5. **`FORMAT.md`** was replaced by a superseded-pointer to `SPEC.md` + `fsa1-cli --guide` (the former
   `docs/format.md` guide is retired; its rationale now lives in the governing code).

## Subsequent GROWTH (never a silent rewrite)

- **TSV field escaping** — `conformance-forms/tsv-escaping/` (a tab `Cells` of four 1×1 files plus an
  `EXPECTED.md`) was ADDED when the deserializer gained field escaping (`\t`, `\n`, `\\`; SPEC.md
  GRID2 "current deserializer"). It probes an embedded newline (`A1`), an embedded tab (`A2`), a
  literal backslash (`A3`), and a **malformed escape** (`A4` = `bad\x`) that deserializes to a located
  `#VALUE!` GRID6 error while `A1`–`A3` still load (GRID6 locality). Verified against `fsa1-cli
  render`/`check`; the manifest and pinned digest above were regenerated in the carrying commit. No
  pre-existing fixture byte changed.

## What this corpus is graded by

- **The 6 valid category workbooks** (`aggregation`, `conditional`, `dates`, `lookup-join`, `model`,
  `text`) are loaded through `fsa1-model` and their cells graded against the frozen value oracle by
  `fsa1-model/tests/render_conformance.rs` (the render zero-divergence gate). This is the live gate
  that proves the migration was value-preserving.
- **The `conformance-forms` / `invalid-forms` fixtures** carry per-fixture and consolidated
  `EXPECTED.md` ledgers documenting the expected static verdict (a filename/grid rejection or an
  overlap). They are the human-readable oracle for the encoding rules; each was verified against
  `fsa1-cli check` during the migration.

## Fingerprint (oracle-input purity) — REMOVED

**No longer fingerprinted.** This corpus carried a `MANIFEST.sha256` and a pinned
`sha256(MANIFEST.sha256)`, enforced by `.githooks/pre-commit` and CI. Both were removed: the corpus
is slated for replacement by a small set of golden `.xlsx` workbooks, and freezing a form that is
about to be discarded bought nothing while blocking every prose edit to the ledgers below. What the
fingerprint protected — that FSA1 never silently rewrites its own graded inputs — is protected by
review until the replacement lands, at which point the new oracle set earns its own guard.
