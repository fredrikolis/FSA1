<!-- Concern: the SOURCE + honest provenance of the refuse/ round-trip probes — which committed openpyxl script authored (or which in-repo fixture graduated) each .xlsx, and the ONE out-of-scope trigger (an EXOTIC-TAIL numFmt the self-describing content cannot carry — a conditional switch or a digit mask, on a literal OR a formula — or a package part outside ALLOW ∪ DROP) each isolates so `unpack --strict` refuses it with a located diagnostic naming that cell/part | Non-concern: how the refusal is asserted (fsa1-cli/tests/roundtrip.rs owns the SER3 assertions) and the in-scope corpus (see ../accept/PROVENANCE.md) | IO: none -->
# PROVENANCE — serde `refuse/` corpus (out-of-scope refusal probes)

The **out-of-scope** family of the SER3 round-trip conformance corpus. Each file isolates **exactly
one** refusal trigger — an **exotic-tail numFmt** GRID7 cannot represent as typed content (a
value-dependent conditional switch, or a digit/phone mask), or a package part outside the skeleton's
`ALLOW ∪ DROP` set. `fsa1-cli unpack --strict` must refuse each with a **non-zero exit** and a
**located CORE2 diagnostic naming the offending cell (+ numFmtId + formatCode) or part** (SER3); this is
asserted by `fsa1-cli/tests/roundtrip.rs`.

## Provenance is honest by construction

The synthesized probes are authored by **openpyxl** (a third-party writer) via the committed,
re-runnable `../make_refuse.py` — openpyxl emits the numFmt/chart natively, and the drawing/pivotTable
parts (which openpyxl does not author natively) are injected into the package by the same script. The
one graduated probe is a real in-repo `fsa1-ingest` fixture. **None is FSA1-generated.**

## GRID7 reclassification (plan 07 §8) — what LEFT the refuse set

Two pre-amendment probes are **RETIRED**: `numfmt.xlsx` (a `0.00%` cell) and `literals.xlsx` (a
datetime cell). Under GRID7, **percent is an ACCEPTED category** and a **formatted formula with an
accepted format ACCEPTS** — so a non-default numFmt is no longer a wholesale refusal. The genuine refuse
set is now the **exotic tail**: formats the self-describing content cannot carry, on a literal AND a
formula alike. (The pre-amendment "formatted-formula → refuse" probe is likewise deleted — a formatted
formula is a first-class ACCEPT; see `../accept/`.)

## Source + refusal trigger — per file

| File | Origin | Out-of-scope trigger (what `unpack --strict` names) |
|---|---|---|
| `cond_literal.xlsx` | authored by `../make_refuse.py` | a **conditional-switch** numFmt `[<100]0.00;[>=100]0` on the value literal `Exotic!A1` (numFmtId 164) — a value-dependent format the content cannot carry |
| `mask_literal.xlsx` | authored by `../make_refuse.py` | a **digit/phone mask** `000000000` on the value literal `Exotic!A1` (numFmtId 164) — better modeled as TEXT than a formatted Number |
| `exotic_formula.xlsx` | authored by `../make_refuse.py` | a **conditional-switch** numFmt `[<100]0;[>=100]0.0` on the **FORMULA** cell `Exotic!A2` (numFmtId 164) — proving the exotic tail refuses a formula too (a catalog concern, not the literal-only precision concern) |
| `chart.xlsx` | authored by `../make_refuse.py` (openpyxl `BarChart`) | the part **`xl/charts/chart1.xml`** — neither modeled nor regenerable (SER3) |
| `drawing.xlsx` | authored by `../make_refuse.py` (raw part injected) | the part **`xl/drawings/drawing1.xml`** (SER3) |
| `pivot.xlsx` | authored by `../make_refuse.py` (raw part injected) | the part **`xl/pivotTables/pivotTable1.xml`** (SER3) |
| `resolution.xlsx` | graduated from `fsa1-ingest/tests/fixtures/resolution.xlsx` | the part **`xl/tables/table1.xml`** (+ defined names `TaxRate`/`AllQOne`) — the tables refuse case no synthesized probe covers |

The three exotic-numFmt probes cover the accepted-catalog complement (conditional / mask) on **both**
cell kinds (literal ×2 + formula ×1); `resolution.xlsx` exercises SER3 "tables" — genuine coverage the
synthesized part-probes do not.

## Integrity

`../MANIFEST.sha256` fingerprints every `.xlsx` here (and in `../accept/`); a `sha256sum -c` check in
`../` (wired into CI's `gates` job) fails on any silent byte change. The `.xlsx` are Git-LFS objects,
smudged by CI's `lfs: true` checkout. To regenerate the synthesized probes:
`../.venv/bin/python ../make_refuse.py` (the venv is provisioned by `../run.sh`).
