<!-- Concern: orient a reader to the ENG6 differential oracle(s) — what they grade (charlie vs the `formulas` Excel lib, per formula AND whole real .xlsx workbooks), how to run them, the corpus shapes, and the lib-gap triage rule | Non-concern: the harness internals (oracle.py / workbook_oracle.py annotations own those) and charlie's own semantics (SPEC.md ENG6 + charlie-model own those) | IO: none (documentation) -->
# xl-oracle — the ENG6 differential conformance oracle

**What it is.** A per-formula differential harness that grades charlie *cell-for-cell against an
external Excel reference*, exactly as SPEC.md **ENG6** requires — plus a **whole-workbook** harness
(`workbook_oracle.py`) that runs the same diff over real, `charlie-cli import`ed `.xlsx` files. The
reference oracle is the
[`formulas`](https://pypi.org/project/formulas/) Python library (v1.3.4) — an independent Excel
formula engine — building each case's workbook with `openpyxl`. LibreOffice is unavailable in this
sandbox, so `formulas` stands in as the mainstream-spreadsheet reference.

For every corpus case the harness:
1. builds a tiny `.xlsx` (inputs in their cells, the formula in a target cell), loads and computes
   it with `formulas` → the **reference** value;
2. builds the equivalent **charlie** workbook — a tab folder whose cells are grid-only files (no
   annotation line; charlie files are pure grid content) — and runs
   `charlie-cli eval <wb> --tab S --formula <formula>` → the **charlie** value;
3. compares them, **driven by the reference's value type**: numbers within tolerance, everything
   else (text, boolean, error) string-exact. Driving off the reference type keeps a text result like
   `TEXT(1234.5,"0.00") → "1234.50"` from being silently reparsed as a number.

Each case is classified **MATCH**, **DIVERGE** (a charlie defect — the harness exits non-zero), or
**EXCLUDED** (a declared `formulas`-library gap; see below).

## Run it

```sh
./run.sh              # per-formula corpus (corpus/*.json)
./run.sh --workbook   # whole-workbook corpus (corpus_workbooks/*.xlsx)
```

`run.sh` builds `charlie-cli` if needed, creates the local `.venv` and installs the **pinned**
reference stack from [`requirements.txt`](./requirements.txt) on first run (the `.venv` is
gitignored), then runs `oracle.py` (or `workbook_oracle.py` with `--workbook`). The versions are
locked (`formulas==1.3.4`, `openpyxl==3.1.5`, and the full transitive closure) so the documented
reference is the *enforced* reference — a rerun cannot silently pull a newer `formulas` whose
semantics flip a MATCH/DIVERGE verdict. Exit code is `0` iff no graded case diverges. Set
`CHARLIE_CLI=/path/to/charlie-cli` to grade a different binary.

## The whole-workbook corpus (`corpus_workbooks/*.xlsx`)

The real-file fitness (DoD milestone 3). A few realistic multi-formula workbooks — a mini P&L, a loan
amortization schedule, and a price lookup table — authored by `make_workbooks.py` (committed; the
`.xlsx` are committed too). `workbook_oracle.py` computes each with `formulas`, collects every formula
cell's value, `charlie-cli import`s the file into a temp workbook, `charlie-cli eval`s each formula
cell, and diffs cell-for-cell reusing this module's `compare`/`classify_ref` logic. Per-cell lib-gaps
are declared in `corpus_workbooks/lib_gaps.json` (the whole-workbook analogue of the per-formula
`lib_gap` flag).

## The corpus (`corpus/*.json`)

A category-spanning seed of 42 cases. Each case is
`{name, category, inputs: {A1: literal, …}, formula: "=…", tol?, lib_gap?, lib_gap_reason?}` — the
expected value is **not** hard-coded; it is computed live from the reference. Categories: arithmetic
& operator precedence · SUM/AVERAGE/IF/IFS/ROUND · VLOOKUP/INDEX-MATCH/HLOOKUP ·
PMT/NPV/IRR/XIRR/FV · STDEV/MEDIAN/LARGE/PERCENTILE · LEFT/RIGHT/CONCAT/SUBSTITUTE/TEXT ·
DATE/EOMONTH/WEEKDAY/YEARFRAC · SUMPRODUCT & an array literal `{…}` · error cases (`#DIV/0!`,
`#N/A`) · a `whole_range` set of whole-column/row references (`SUM`/`SUMIF`/`COUNTIF`/`COUNTA(A:A)`,
`SUM(1:1)`, and a mixed `VLOOKUP(x, A:D, n)`, each bound to the used region) · and an `edge` set of
notorious Excel-semantics gotchas (left-associative `^`, `MOD` of a negative, round-half-away-from-zero,
text→number coercion, boolean-array coercion).

## Triage rule (lib-gap vs charlie-bug)

`formulas` covers a large Excel subset, not all of it. When charlie and the reference disagree, the
disagreement is triaged:
- **charlie is wrong vs Excel** → a charlie defect; fix charlie (small, gate-green) and re-run.
- **the `formulas` lib is wrong/unsupported for that case** → recorded in
  [`KNOWN-LIB-GAPS.md`](./KNOWN-LIB-GAPS.md) with the case and why, and the case is flagged
  `"lib_gap": true` in the corpus so it is **EXCLUDED** from pass/fail. charlie is never distorted to
  match a wrong reference.

The current standing is written to [`PARITY.md`](./PARITY.md).
