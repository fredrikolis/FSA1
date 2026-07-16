<!-- Concern: the SOURCE + tamper-fingerprint of the frozen W1 encoding corpus graduating into prod, so charlie can never silently regenerate its own oracle inputs | Non-concern: the conformance verdict logic (charlie-model/tests/encoding_conformance.rs owns that) and how values are computed (the W4 value oracles, deliberately NOT migrated) | IO: none -->
# PROVENANCE — W1 encoding corpus (frozen contract)

This directory is a **sanctioned graduation** of the W1 encoding corpus from the product-manager
workspace into the production repo. It is a **frozen CONTRACT**: charlie-model is *graded against*
these fixtures and their EXPECTED verdict ledgers — it must never author, edit, or regenerate them.
That is the point of the fingerprint below (**oracle-input purity**): if any byte of an input fixture
or an EXPECTED ledger changes, the manifest check fails, exposing a silent oracle rewrite.

## Source

| | |
|---|---|
| **Workspace** | `project-charlie` (product-manager dev-harness) |
| **Path** | `exploration/experiments/01-encoding-corpus/` |
| **Commit** | `9367ee90cc5417d9983cb70ed8846d6ad772f47b` |
| **Commit subject** | `W1 Substrate: format spec + corpus + purity-checked oracle + QA ladder` |
| **Migrated on** | 2026-07-15 |

## What was migrated (and what was deliberately NOT)

**Migrated — the W2 parse / conformance / overlap contract:**
- `artifacts/` — the full corpus tree, verbatim:
  - the **6 valid category workbooks** (`aggregation`, `conditional`, `dates`, `lookup-join`,
    `model`, `text`) — every `.range` / `.cell` file must PARSE (§2/§4/§5), every literal body must
    CONFORM under §6, and each tab must be OVERLAP-free (§7);
  - `conformance-forms/` — the four broadcast/edge fixtures, each with its per-fixture `EXPECTED.md`;
  - `invalid-forms/` — the six rejection fixtures, each with its per-fixture `EXPECTED.md`.
- `oracle/conformance-forms/EXPECTED.md` and `oracle/invalid-forms/EXPECTED.md` — the two
  consolidated verdict ledgers (the "ruler's quick oracle").
- `FORMAT.md` — the on-disk-format spec the ledgers cite by `§`, copied so those citations resolve
  inside the frozen contract.

**NOT migrated — the W4 rendered-VALUE oracles.** The source `oracle/<category>/compute_oracle.py`,
`expected_values.*`, `*.oracle.csv`, and per-category `PROVENANCE.md` grade **W4 evaluation** (the
numeric result of a formula), not W2 encoding. W2 tests only PARSE + CONFORMANCE + OVERLAP verdicts —
a static shape/grammar decision, no evaluator involved — so those value oracles are out of scope here
and were left in the exploration workspace.

## Fingerprint (oracle-input purity)

`MANIFEST.sha256` records a `sha256sum` line for **every** migrated file (113 files: the corpus, the
per-fixture and consolidated EXPECTED ledgers, and `FORMAT.md`) — everything except this file and the
manifest itself. Re-verify at any time from this directory:

```
sha256sum -c MANIFEST.sha256 --quiet   # exit 0 == corpus is byte-identical to the frozen contract
```

Digest of the manifest itself (a single value pinning the whole set):

```
sha256(MANIFEST.sha256) = daee80c604c7244b157197c589664edceb15a6c7d1202d08110d25d6014eb4e0
```

This check is **mechanically enforced at two sites**, both with the **system** `sha256sum`
(deliberately **not** a crypto crate: `charlie-model` depends only on `charlie-ast`, and pulling a
hashing dependency in to re-hash fixtures would violate the workspace's dependency-minimal posture —
so both call sites shell out to the system tool instead):

1. **CI** — `.github/workflows/build.yml` runs `sha256sum -c MANIFEST.sha256 --quiet` (in this
   directory) on every push and pull request.
2. **The grading site** — `charlie-model/tests/encoding_conformance.rs`
   (`corpus_matches_frozen_fingerprint_at_the_grading_site`) re-runs the SAME `sha256sum -c` during a
   plain `cargo test` (the commit gate's observed test step) by shelling to the system tool. It also
   pins `sha256(MANIFEST.sha256)` to the digest above **in code** and asserts it, so a rewrite that
   regenerates BOTH the fixtures AND the manifest — which would still pass `sha256sum -c` — is caught
   too. This means the corpus's purity is asserted *where verdicts are actually graded*, not only in
   CI: a local grading run can no longer score `charlie-model` against tampered fixtures.

Enforcing it at the grading site is what makes purity load-bearing rather than advisory — do not rely
on the *verdict* assertions to catch an edit: the harness grades the 6 valid workbooks on
parse+conform+overlap only, **not** cell values, so editing a literal inside a valid workbook would
flip no verdict. The fingerprint check (both sites) is what makes that edit visible. If the corpus is
ever deliberately GROWN (the coverage ratchet permits growth, never silent rewrite), regenerate
`MANIFEST.sha256` and update BOTH this pinned digest and `PINNED_MANIFEST_DIGEST` in the harness in
the same reviewed commit.

## Coverage ratchet (seed)

`charlie-model/tests/encoding_conformance.rs` is the machine encoding of the EXPECTED ledgers: every
corpus item is asserted against charlie-model's verdict, each assertion citing the fixture + the
`FORMAT.md §` it exercises. This is the seed of the coverage ratchet — new W2 encoding rules land
with a fixture here and an assertion there, and this corpus may only GROW.
