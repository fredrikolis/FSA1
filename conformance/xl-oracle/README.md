<!-- Concern: orients a reader to the differential oracles — what they grade, how to run them, the corpora | Non-concern: the harness internals, FSA1's own semantics | IO: none -->
# xl-oracle — the ENG6 differential conformance oracle

**What it is.** A per-formula differential harness that grades FSA1 *cell-for-cell against an
external Excel reference*, exactly as SPEC.md **ENG6** requires — plus a **whole-workbook** harness
(`workbook_oracle.py`) that runs the same diff over real, `fsa1-cli unpack`ed `.xlsx` files. The
reference oracle is the
[`formulas`](https://pypi.org/project/formulas/) Python library (v1.3.4) — an independent Excel
formula engine — building each case's workbook with `openpyxl`. LibreOffice is unavailable in this
sandbox, so `formulas` stands in as the mainstream-spreadsheet reference.

For every corpus case the harness:
1. builds a tiny `.xlsx` (inputs in their cells, the formula in a target cell), loads and computes
   it with `formulas` → the **reference** value;
2. builds the equivalent **FSA1** workbook — a tab folder whose cells are grid-only files (no
   annotation line; FSA1 files are pure grid content) — and runs
   `fsa1-cli eval <wb>/S --formula <formula>` → the **FSA1** value;
3. compares them, **driven by the reference's value type**: numbers within tolerance, everything
   else (text, boolean, error) string-exact. Driving off the reference type keeps a text result like
   `TEXT(1234.5,"0.00") → "1234.50"` from being silently reparsed as a number.

Each case is classified **MATCH**, **DIVERGE** (an FSA1 defect — the harness exits non-zero), or
**EXCLUDED** (a declared `formulas`-library gap; see below).

## Run it

```sh
./run.sh              # per-formula corpus (corpus/*.json)
./run.sh --workbook   # whole-workbook corpus (corpus_workbooks/*.xlsx)
```

`run.sh` builds `fsa1-cli` if needed, creates the local `.venv` and installs the **pinned**
reference stack from [`requirements.txt`](./requirements.txt) on first run (the `.venv` is
gitignored), then runs `oracle.py` (or `workbook_oracle.py` with `--workbook`). The versions are
locked (`formulas==1.3.4`, `openpyxl==3.1.5`, and the full transitive closure) so the documented
reference is the *enforced* reference — a rerun cannot silently pull a newer `formulas` whose
semantics flip a MATCH/DIVERGE verdict. Exit code is `0` iff no graded case diverges. Set
`FSA1_CLI=/path/to/fsa1-cli` to grade a different binary.

## The whole-workbook corpus (`corpus_workbooks/*.xlsx`)

The real-file fitness (DoD milestone 3). A few realistic multi-formula workbooks — a mini P&L, a loan
amortization schedule, and a price lookup table — authored by `make_workbooks.py` (committed; the
`.xlsx` are committed too). `workbook_oracle.py` computes each with `formulas`, collects every formula
cell's value, `fsa1-cli unpack`s the file into a temp workbook, `fsa1-cli eval`s each formula
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
`#N/A`) · and an `edge` set of notorious Excel-semantics gotchas (left-associative `^`, `MOD` of a
negative, round-half-away-from-zero, text→number coercion, boolean-array coercion).

## Triage rule (lib-gap vs FSA1-bug)

`formulas` covers a large Excel subset, not all of it. When FSA1 and the reference disagree, the
disagreement is triaged:
- **FSA1 is wrong vs Excel** → an FSA1 defect; fix FSA1 (small, gate-green) and re-run.
- **the `formulas` lib is wrong/unsupported for that case** → recorded in
  [`KNOWN-LIB-GAPS.md`](./KNOWN-LIB-GAPS.md) with the case and why, and the case is flagged
  `"lib_gap": true` in the corpus so it is **EXCLUDED** from pass/fail. FSA1 is never distorted to
  match a wrong reference.

The current standing is written to [`PARITY.md`](./PARITY.md).
