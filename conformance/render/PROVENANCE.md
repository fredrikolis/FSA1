<!-- Concern: the SOURCE of the W1 rendered-VALUE oracles, plus the tolerance policy of the harness grading against them | Non-concern: the verdict logic (render_conformance.rs owns it), the corpus SHEET files (conformance/encoding/ owns them) | IO: none -->
# PROVENANCE — W1 rendered-VALUE oracles (frozen contract)

This directory is a **sanctioned graduation** of the W1 **rendered-value** oracles into the
production repo. It is the W4 companion to
`conformance/encoding/` (which graduated the W2 parse/conformance/overlap contract): where that dir
grades *static shape* verdicts, this dir grades the **computed VALUE** of every cell — bet **B3**
(demand-driven eval), rendered.

It is a **frozen CONTRACT**: `fsa1-model` is *graded against* these oracle values — it must never
author, edit, or regenerate them. That is the point of the fingerprint below (**oracle-input
purity**): if any byte of an oracle file changes, the manifest check fails, exposing a silent oracle
rewrite.

## Source

| | |
|---|---|
| **Origin** | authored OUTSIDE this repo before graduating here — never FSA1-generated (that is the oracle-input purity claim) |
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
**never by FSA1** — FSA1 could not evaluate when these were authored, and grading the tool
against its own output is forbidden. Any divergence FSA1 later produces is a real FSA1 finding.

**NOT copied — the corpus SHEET files.** The `.range`/`.cell` workbook files are **reused verbatim**
from `conformance/encoding/artifacts/<category>/`. Duplicating them here would
create a second, drift-prone copy of a graded input; instead the render harness loads them straight
from the encoding corpus. The two categories with no VALUE oracle (`conformance-forms`,
`invalid-forms`) are out of scope — they grade shape/rejection verdicts, not values.

## The grader

`fsa1-model/tests/render_conformance.rs` loads each workbook via `fsa1_model::Workbook`,
evaluates every oracle cell **demand-driven**, and grades the computed `Value`:

- **Text / error tokens** compare by exact spelling (an oracle `"#N/A"` matches a `Value::Error(Na)`).
- **Numbers** compare **bit-exact** first (IEEE-754 by bit pattern). Exact rationals, integers, and
  date serials must match to the bit. The `model` amortization accumulates IEEE-754 error over 12
  periods and its oracle is 10-dp display-rounded, so those cells are compared under the oracle
  authors' **documented tolerance** (`|Δ| ≤ 1e-6` abs/rel — see `oracle/model/PROVENANCE.md`), never
  bit-exact. The report prints the exact-vs-tolerance split so this is never hidden.

Each cell yields a `Match`/`Diverge` verdict, and the hard gate is **zero divergence**: every graded
cell must Match its oracle. The report prints the per-category tally either way.

## Resolved B3 finding — the corpus is explicit grids, not drag-fill (this harness is zero-divergence)

At migration this harness recorded a large, **systematic** class of `Diverge`s. The W1 corpus
originally expressed each derived column as a **single scalar `=formula` stored over a multi-cell
`.range`** and expected the reader to **drag-fill** it — to offset the body's relative references
per-cell across the region (the former format spec illustrated this with "a `5×1` drag-fill
`Orders/D2:D6` with body `=B2*C2` … its per-cell offsetting is a W3 eval concern"). FSA1 has **no
such mechanism**: by **VAL1** a cell's value derives *only from its own content*, so there is no
offset/drag-fill anywhere in the engine (`fsa1-model/src/workbook.rs`). FSA1 therefore
evaluated the stored scalar once and Filled the region with that one result, while the oracle
computed the drag-fill per row — so **every derived column diverged** across all six workbooks,
recorded as standing FACTS in the anchor.

**That standing class is now RESOLVED — by aligning the corpus to VAL1, never by adding drag-fill.**
Each derived column is now an **explicit per-cell grid**: the range file spells out one formula per
row with its own literal references (e.g. `conformance/encoding/artifacts/lookup-join/Orders/D2:D11`
writes `=XLOOKUP(B2,Products!$A$2:$A$7,Products!$C$2:$C$7)` through `=XLOOKUP(B11,…)` in full), which
is exactly what FSA1's own-content rule evaluates. The harness in this directory now enforces a
**zero-divergence** gate and passes **542/542** with `Diverge = 0` — the previously-diverging cells
all Match. (The `model` amortization cells still Match under the oracle authors' documented
`|Δ| ≤ 1e-6` tolerance rather than bit-exact, counted as Match, not Diverge — see the grader section
above.) The zero-divergence gate is what guards against a future regression: a cell that stops
Matching fails the suite.

## Fingerprint (oracle-input purity) — REMOVED

**No longer fingerprinted, and no longer ratcheted.** This oracle set carried a `MANIFEST.sha256`, a
pinned `sha256(MANIFEST.sha256)`, and a `facts-snapshot.tsv` anchor. All were removed: the corpus is
slated for replacement by golden `.xlsx` workbooks, and the ratchet was unreachable anyway because
the zero-divergence gate above already requires every cell to Match. Oracle-input purity now rests on
review until the replacement lands.
