<!-- Concern: the SOURCE + tamper-fingerprint of the encoding corpus, and the record of its value-preserving migration to the current (no-endings + explicit-grid + TSV) form | Non-concern: how values are computed (the render value oracles live in conformance/render) | IO: none -->
# PROVENANCE — encoding corpus (frozen contract)

This directory is a **sanctioned graduation** of the W1 encoding corpus from the product-manager
workspace into the production repo, subsequently **migrated value-preservingly** to the current
on-disk form. It is a **frozen CONTRACT** graded against the current spec (`SPEC.md`): the fingerprint
below (**oracle-input purity**) makes any silent byte change in a fixture or an EXPECTED ledger fail a
`sha256sum -c` check, exposing a rewrite.

## Source

| | |
|---|---|
| **Workspace** | `project-charlie` (product-manager dev-harness) |
| **Path** | `exploration/experiments/01-encoding-corpus/` |
| **Original commit** | `9367ee90cc5417d9983cb70ed8846d6ad772f47b` (`W1 Substrate: format spec + corpus + purity-checked oracle + QA ladder`) |
| **Originally migrated** | 2026-07-15 |

## The value-preserving migration to the current form

The engine was changed so that a file's name **is** a closed range with **no `.range`/`.cell` ending**
(SPEC.md FT‑3), its content deserializes to an **explicit grid** that must fill the range exactly
(FT‑4/FT‑8) via the **TSV** deserializer (FT‑5), and a cell's value derives only from its own content
(FT‑9) — there is **no broadcast/drag-fill**. This corpus was migrated to match, preserving every
rendered value:

1. **Filenames lost their endings.** Every `<range>.range` / `<addr>.cell` file was renamed to the
   bare closed range (`A2:E13.range` → `A2:E13`, `D14.cell` → `D14`). The name is now the closed
   range itself.
2. **Drag-fill bodies were expanded to explicit grids.** Every range file whose body was a single
   drag-fill `=formula` was rewritten to the **explicit per-cell grid it denoted**: each cell holds
   its own offset formula (relative refs shifted with position, `$`-absolute refs pinned). E.g.
   `Amortization/D2:D13` went from the single body `=B2*Inputs!$B$5` to twelve rows
   `=B2*Inputs!$B$5` / `=B3*Inputs!$B$5` / … / `=B13*Inputs!$B$5`. Because each expanded cell
   evaluates its own formula exactly as the old loader would have offset it, **rendered values are
   unchanged** — proven by the render zero-divergence gate
   (`charlie-model/tests/render_conformance.rs`), which grades these workbooks against the frozen W1
   value oracle in `conformance/render/` and requires zero divergence.
3. **Retired-concept fixtures were deleted.** The `conformance-forms/{broadcast-across,broadcast-down,
   square-disambiguator}` fixtures (which existed only to probe the retired broadcast-conformance
   rule) and `invalid-forms/illegal-forms/dual-body` (the retired "exactly one body form" rule) were
   removed. The surviving edge/invalid fixtures had their `EXPECTED.md` verdicts rewritten to cite
   SPEC.md's FT‑invariants and the current diagnostic codes (`dimension-mismatch`, `ragged-grid`,
   `non-canonical-range`, `dollar-in-filename`, `degenerate-range`, `overlap`).
4. **`FORMAT.md`** was replaced by a superseded-pointer to `SPEC.md` + `charlie-cli --guide` (the former
   `docs/format.md` guide is retired; its rationale now lives in the governing code).

## What this corpus is graded by

- **The 6 valid category workbooks** (`aggregation`, `conditional`, `dates`, `lookup-join`, `model`,
  `text`) are loaded through `charlie-model` and their cells graded against the frozen value oracle by
  `charlie-model/tests/render_conformance.rs` (the render zero-divergence gate). This is the live gate
  that proves the migration was value-preserving.
- **The `conformance-forms` / `invalid-forms` fixtures** carry per-fixture and consolidated
  `EXPECTED.md` ledgers documenting the expected static verdict (a filename/grid rejection or an
  overlap). They are the human-readable oracle for the encoding rules; each was verified against
  `charlie-cli check` during the migration.

## Fingerprint (oracle-input purity)

`MANIFEST.sha256` records a `sha256sum` line for **every** corpus file (105 files: `FORMAT.md`, the
`artifacts/` tree, and the two `oracle/*/EXPECTED.md` ledgers) — everything except this file and the
manifest itself. Re-verify at any time from this directory:

```
sha256sum -c MANIFEST.sha256 --quiet   # exit 0 == corpus is byte-identical to the frozen contract
```

Digest of the manifest itself (a single value pinning the whole set):

```
sha256(MANIFEST.sha256) = 75f106aefb3d882f9351c07aecea876b0b3c5bd40f19c444dd3c80173daf61d0
```

`.github/workflows/build.yml` runs `sha256sum -c MANIFEST.sha256 --quiet` in this directory on every
push and pull request. If the corpus is ever deliberately GROWN or re-migrated (never silently
rewritten), regenerate `MANIFEST.sha256` and update the pinned digest above in the same reviewed
commit.
