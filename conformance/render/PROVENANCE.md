<!-- Concern: the SOURCE + tamper-fingerprint of the frozen W1 rendered-VALUE oracles graduating into prod, so charlie can never silently regenerate its own oracle inputs, plus the tolerance policy and ratchet contract of the render-conformance harness that grades against them | Non-concern: the conformance verdict logic (charlie-model/tests/render_conformance.rs owns that) and the corpus SHEET files (reused verbatim from conformance/encoding/, fingerprinted by that dir's own MANIFEST.sha256) | IO: none -->
# PROVENANCE — W1 rendered-VALUE oracles (frozen contract)

This directory is a **sanctioned graduation** of the W1 **rendered-value** oracles from the
product-manager workspace into the production repo. It is the W4 companion to
`conformance/encoding/` (which graduated the W2 parse/conformance/overlap contract): where that dir
grades *static shape* verdicts, this dir grades the **computed VALUE** of every cell — bet **B3**
(demand-driven eval), rendered.

It is a **frozen CONTRACT**: `charlie-model` is *graded against* these oracle values — it must never
author, edit, or regenerate them. That is the point of the fingerprint below (**oracle-input
purity**): if any byte of an oracle file changes, the manifest check fails, exposing a silent oracle
rewrite.

## Source

| | |
|---|---|
| **Workspace** | `project-charlie` (product-manager dev-harness) |
| **Path** | `exploration/experiments/01-encoding-corpus/oracle/<category>/` |
| **Commit** | `1db2937f6c2c13d399dbc3c3ad2df48ac7d9c2f9` |
| **Commit subject** | `W1 Substrate: format spec + corpus + purity-checked oracle + QA ladder` |
| **Migrated on** | 2026-07-16 |

## What was migrated (and what was NOT)

**Migrated — the W1 rendered-VALUE oracles** for the six valid category workbooks, verbatim:

| category | oracle file(s) | authored by |
|---|---|---|
| `aggregation`  | `expected_values.json`, `expected_derived.csv` | pandas + hand (W1) |
| `conditional`  | `expected_values.json`, `expected_values.csv`  | plain python (W1) |
| `text`         | `contacts-clean.oracle.json`, `.oracle.csv`    | plain python (W1) |
| `model`        | `oracle_values.json`, `oracle_values.csv`      | closed-form PMT, python (W1) |
| `dates`        | `invoice-aging.oracle.csv`                      | stdlib `datetime`, python (W1) |
| `lookup-join`  | `oracle.json`, `oracle.csv`                     | pandas/python (W1) |

Each category also carries its original `compute_oracle.py` (the **executable provenance** — the
independent computation that produced the values) and its per-category `PROVENANCE.md` (method +
tolerance notes). **ORACLE-INPUT PURITY:** every value here was python- or hand-computed in W1,
**never by charlie** — charlie could not evaluate when these were authored, and grading the tool
against its own output is forbidden. Any divergence charlie later produces is a real charlie finding.

**NOT copied — the corpus SHEET files.** The `.range`/`.cell` workbook files are **reused verbatim**
from `conformance/encoding/artifacts/<category>/` (already fingerprinted by
`conformance/encoding/MANIFEST.sha256` and its code-pinned digest). Duplicating them here would
create a second, drift-prone copy of a frozen input; instead the render harness loads them straight
from the encoding corpus. The two categories with no VALUE oracle (`conformance-forms`,
`invalid-forms`) are out of scope — they grade shape/rejection verdicts, not values.

**NOT copied — `facts-snapshot.tsv`.** That anchor is **charlie-DERIVED** (the grader's own output,
the render ratchet's memory), so it is deliberately excluded from the purity manifest below.

## The grader and the ratchet

`charlie-model/tests/render_conformance.rs` loads each workbook via `charlie_model::Workbook`,
evaluates every oracle cell **demand-driven**, and grades the computed `Value`:

- **Text / error tokens** compare by exact spelling (an oracle `"#N/A"` matches a `Value::Error(Na)`).
- **Numbers** compare **bit-exact** first (IEEE-754 by bit pattern). Exact rationals, integers, and
  date serials must match to the bit. The `model` amortization accumulates IEEE-754 error over 12
  periods and its oracle is 10-dp display-rounded, so those cells are compared under the oracle
  authors' **documented tolerance** (`|Δ| ≤ 1e-6` abs/rel — see `oracle/model/PROVENANCE.md`), never
  bit-exact. The report prints the exact-vs-tolerance split so this is never hidden.

Each cell yields a `Match`/`Diverge` verdict, ratcheted through the committed **`facts-snapshot.tsv`**
anchor: the hard gate is **no cell that Matched in the anchor may Diverge now** (a render
regression). A `Diverge` is a surfaced FACT, not itself a gate failure — growth and improvement never
block — mirroring the formula-conformance ratchet. Re-bless the baseline consciously with
`RENDER_RESNAPSHOT=1 cargo test -p charlie-model --test render_conformance -- --ignored resnapshot`
and record WHY in the carrying commit.

## Resolved B3 finding — drag-fill landed (this harness is now zero-divergence)

At migration this harness recorded a large, **systematic** class of `Diverge`s: charlie's engine
evaluated a scalar `=formula` stored in a multi-cell `.range` file **once** (with its literal
top-left references) and **Filled** the whole declared region with that single result — it did **not**
perform the per-cell **relative-reference offsetting** (drag-fill) that `docs/format.md` §10.2
requires ("a `5×1` drag-fill `Orders/D2:D6.range` with body `=B2*C2` … its per-cell offsetting is a
W3 eval concern"). The oracle computes the drag-fill per row; charlie did not, so **every derived
(drag-fill) column diverged** across all six workbooks — recorded as standing FACTS in the anchor.

**That standing class is now RESOLVED.** Relative fill landed in the W4b delta this doc ships with:
`charlie_ast::offset_refs` (the pure tree-walking ref-shift) drives `Plan::DragFill` in
`charlie-model/src/workbook.rs`, so each cell of a multi-cell scalar `=formula` range now offsets
the body's relative refs by its delta from the top-left anchor (`$`-anchors fixed) instead of the
old single-result Fill. The harness in this directory now enforces a **zero-divergence** gate and
passes **542/542** with `Diverge = 0` — the previously-diverging drag-fill cells all flipped to
`Match`. (The `model` amortization's 62 accumulated-IEEE-754 cells still Match under the oracle
authors' documented `|Δ| ≤ 1e-6` tolerance, counted as Match, not Diverge — see the grader section
above.) The ratchet continues to guard against any future regression: no cell that Matches now may
Diverge later.

## Fingerprint (oracle-input purity)

`MANIFEST.sha256` records a `sha256sum` line for **every** migrated oracle file (the `.json`/`.csv`
value ledgers, each `compute_oracle.py`, and each per-category `PROVENANCE.md`) — everything under
`oracle/`, excluding this file, the manifest itself, and the charlie-derived `facts-snapshot.tsv`.
Re-verify at any time from this directory:

```
sha256sum -c MANIFEST.sha256 --quiet   # exit 0 == oracles are byte-identical to the frozen contract
```

Digest of the manifest itself (a single value pinning the whole set):

```
sha256(MANIFEST.sha256) = 3528f0ca8ad331bb4a019c97bbbbd3f65ba61e7e0bebb89738152eaae80a1b71
```

This is **mechanically enforced at the grading site** by
`render_conformance.rs::render_oracles_match_the_frozen_fingerprint`: it runs `sha256sum -c` (every
file byte-identical) AND checks `sha256(MANIFEST.sha256)` against the pinned digest above (closing the
"regenerate BOTH the oracles and the manifest" hole). Both use the **system** `sha256sum`
deliberately — `charlie-model` depends only on `charlie-ast`, and pulling a hashing crate in to
re-hash fixtures would violate the workspace's dependency-minimal posture. Keep the pinned digest here
in sync with the constant in that test if the oracle set is ever deliberately grown.
