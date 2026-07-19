<!-- Concern: the standing ENG6 parity result — the match/total over BOTH the per-formula corpus and the whole-workbook real-file corpus, the excluded lib-gaps, and the last-run date — as the human-readable snapshot of `run.sh`'s output | Non-concern: how parity is computed (oracle.py / workbook_oracle.py) and why the excluded cases are lib-gaps (KNOWN-LIB-GAPS.md) | IO: none (documentation snapshot; regenerate by running ./run.sh and ./run.sh --workbook) -->
# PARITY.md — ENG6 differential parity result

**Per-formula result: 68 / 68 graded cases MATCH. 0 charlie-defect divergences. 3 cases EXCLUDED (lib-gaps).**
Corpus total: 71 cases (curated highlights below; `./run.sh` grades the full `corpus/*.json` set).
Reference oracle: `formulas` v1.3.4 (+ openpyxl). Last run: 2026-07-19.

`./run.sh` exit code: **0** (no charlie-defect divergence).

**Whole-workbook result (real .xlsx files, DoD milestone 3): 58 / 58 graded formula cells MATCH across
3 workbooks. 0 charlie-defect divergences. 1 cell EXCLUDED (lib-gap).**
`./run.sh --workbook` exit code: **0**. This is the true `import -> compute -> diff vs reference`
fitness: each workbook is `charlie-cli import`ed, then every formula cell is `charlie-cli eval`'d and
diffed cell-for-cell against the `formulas` reference (see `workbook_oracle.py`).

## By category

| category    | graded | match | excluded | notes |
|-------------|-------:|------:|---------:|-------|
| arithmetic  | 5      | 5     | 0        | precedence, unary `-2^2`→4, `%`, parens, `&` |
| aggregate   | 5      | 5     | 0        | SUM, AVERAGE, IF, IFS, ROUND |
| lookup      | 3      | 3     | 0        | VLOOKUP, INDEX/MATCH, HLOOKUP |
| financial   | 5      | 5     | 0        | PMT, NPV, IRR, XIRR, FV |
| statistical | 4      | 4     | 0        | STDEV, MEDIAN, LARGE, PERCENTILE |
| text        | 4      | 4     | 0        | LEFT/RIGHT, CONCAT, SUBSTITUTE, TEXT (`0.00`) |
| date        | 4      | 4     | 0        | DATE, EOMONTH, WEEKDAY, YEARFRAC (serials) |
| array       | 2      | 2     | 0        | SUMPRODUCT pair, array literal `{…}` |
| errors      | 2      | 2     | 0        | `#DIV/0!`, `#N/A` |
| edge        | 7      | 7     | 1        | `^` left-assoc, MOD(-7,3)=2, ROUND half-away, `"3"+2`, INT(-2.5); excluded: `SUMPRODUCT(--(range>x))` |
| engineering | 27     | 27    | 2        | CONVERT: mass/distance/time/volume/force/power/pressure ratios, SI prefixes (incl. area²/vol³ exponent + deka alias), temperature affine (F/C/K/Rank/Reau), `pc` parsec-vs-picocalorie, `#N/A` on unknown/cross-system/bare-prefix; excluded: IEC binary prefixes `kibyte`/`Gibit` (formulas-lib defect, Excel-correct in charlie) |
| **total**   | **68** | **68**| **3**    | |

## Whole-workbook corpus (real .xlsx)

| workbook            | graded | match | excluded | shape |
|---------------------|-------:|------:|---------:|-------|
| `pnl.xlsx`          | 16     | 16    | 0        | mini P&L: SUM, subtraction, margin %, ROUND tax, cross-sheet Summary |
| `amortization.xlsx` | 27     | 27    | 1        | loan schedule: PMT + per-period interest/principal/balance chain, PPMT; excluded: IPMT lib-gap |
| `lookup.xlsx`       | 15     | 15    | 0        | price table: VLOOKUP, INDEX/MATCH, HLOOKUP, IF tiers, SUMIF rollup |
| **total**           | **58** | **58**| **1**    | |

The excluded `amortization.xlsx!Loan!B15` (IPMT) is a `formulas`-library defect — see
KNOWN-LIB-GAPS.md §2. Declared in `corpus_workbooks/lib_gaps.json`.

## The per-formula exclusion

`edge/sumproduct_bool_coerce` — `=SUMPRODUCT(--(A1:A3>1))` — charlie returns `2` (correct Excel);
`formulas` v1.3.4 returns `0`. This is a **reference-library defect**, not a charlie defect: the lib
fails to array-broadcast a range comparison through unary minus. Full evidence in
[`KNOWN-LIB-GAPS.md`](./KNOWN-LIB-GAPS.md). Excluded from pass/fail per the ENG6 triage rule — the
corpus grades charlie against Excel, and the reference here is wrong, so charlie is not distorted to
match it.
