<!-- Concern: the standing parity result — match/total over both oracle corpora, exclusions, last-run date | Non-concern: how parity is computed, why cases are excluded | IO: none -->
# PARITY.md — ENG6 differential parity result

**Per-formula result: 542 / 542 graded cases MATCH (parity = 100.0%). 0 FSA1-defect divergences. 33 cases EXCLUDED (lib-gaps).**
Corpus total: 575 cases (broad, systematic — a typical + edge cases for every registered function, plus
60 cross-cutting semantic cases; `./run.sh` grades the full `corpus/*.json` set).
Function coverage: **236 / 253** registered functions have ≥1 graded case; **251 / 253** have ≥1 case
(the 15 with only excluded lib-gap cases are listed in KNOWN-LIB-GAPS.md; the 2 with no case —
`INDIRECT`/`OFFSET` — are deliberate parse-refusals reserved for a later phase, outside ENG6).
Reference oracle: `formulas` v1.3.4 (+ openpyxl). Last run: 2026-07-19.

`./run.sh` exit code: **0** (no FSA1-defect divergence).

**Whole-workbook result (real .xlsx files, DoD milestone 3): 58 / 58 graded formula cells MATCH across
3 workbooks. 0 FSA1-defect divergences. 1 cell EXCLUDED (lib-gap).**
`./run.sh --workbook` exit code: **0**. This is the true `import -> compute -> diff vs reference`
fitness: each workbook is `fsa1-cli unpack`ed, then every formula cell is `fsa1-cli eval`'d and
diffed cell-for-cell against the `formulas` reference (see `workbook_oracle.py`).

## By category

| category    | graded | match | excluded | notes |
|-------------|-------:|------:|---------:|-------|
| arithmetic  | 5      | 5     | 0        | precedence, unary `-2^2`→4, `%`, parens, `&` |
| aggregate   | 5      | 5     | 0        | SUM, AVERAGE, IF, IFS, ROUND |
| crosscut    | 60     | 60    | 0        | operator precedence/associativity, unary-vs-power, text/bool/blank coercion, blank-as-0, the error values + propagation, ROUND half-away, 1900 date serials + phantom leap day, array literals `{…}` |
| count       | 16     | 16    | 0        | COUNT/COUNTA/COUNTBLANK, MAX/MIN, MODE/MODE.SNGL, text-ignored |
| criteria    | 17     | 17    | 0        | SUMIF(S)/COUNTIF(S)/AVERAGEIF(S)/MAXIFS/MINIFS, wildcards, `<>`, no-match |
| logical     | 12     | 12    | 0        | TRUE/FALSE/XOR/IFERROR/IFNA (+ array element-wise) |
| logical2    | 19     | 19    | 0        | AND/OR/NOT/SWITCH, IF edge conditions, bool/number coercion |
| lookup      | 22     | 22    | 0        | VLOOKUP, INDEX/MATCH, HLOOKUP, ADDRESS, XMATCH, error-skip |
| lookup2     | 14     | 14    | 0        | CHOOSE, COLUMNS/ROWS, XLOOKUP, MATCH/INDEX modes |
| math2       | 35     | 35    | 0        | ABS/POWER/SQRT/CEILING/FLOOR/ROUNDUP/ROUNDDOWN/PRODUCT/MOD/INT + edges |
| mathtrig    | 38     | 38    | 7        | trig, logs, MROUND/CEILING.MATH/FLOOR.MATH/EVEN/ODD/GCD/LCM/FACT; excluded: QUOTIENT/COMBIN/SUBTOTAL/AGGREGATE (lib unimplemented) |
| financial   | 19     | 19    | 0        | PMT/NPV/IRR/XIRR/FV/PV/MIRR/CUMIPMT/SLN/SYD/DB/DDB/EFFECT/… |
| financial2  | 9      | 9     | 2        | NPER/RATE/XNPV/PPMT/PMT/FV/PV; excluded: IPMT (lib `#NUM!`) |
| statistical | 40     | 40    | 1        | STDEV/VAR families, percentile/quartile/rank, regression, NORM.*; excluded: MODE.MULT (lib) |
| stat2       | 19     | 19    | 0        | SMALL/RANK/VAR.*/STDEV.*/QUARTILE/PERCENTILE.INC/MEDIAN |
| text        | 37     | 37    | 3        | LEFT/RIGHT/CONCAT/SUBSTITUTE/TEXT format codes/FIXED/NUMBERVALUE/…; excluded: 3 TEXT format-section lib defects |
| text2       | 35     | 35    | 6        | LEN/LOWER/UPPER/PROPER/MID/FIND/SEARCH/EXACT/CODE/REPLACE/REPT/TRIM/VALUE/TEXTJOIN; excluded: DOLLAR×3, SEARCH-wildcard, TRIM-collapse, TEXTJOIN-ignore-empty (lib gaps) |
| date        | 19     | 19    | 0        | DATE/EOMONTH/WEEKDAY/YEARFRAC/DAYS360/ISOWEEKNUM/NETWORKDAYS.INTL/… |
| datetime    | 32     | 32    | 1        | DAY/MONTH/YEAR/DAYS/EDATE/HOUR/MINUTE/SECOND/TIME/WEEKNUM/WORKDAY/NETWORKDAYS/DATEDIF/TODAY/NOW; excluded: DATEDIF `"d"` (lib case-sensitivity) |
| info        | 23     | 23    | 0        | N/IS*/ERROR.TYPE/ISFORMULA + 1×1-range collapse |
| info2       | 14     | 14    | 1        | ISBLANK/ISTEXT/ISNUMBER/ISERROR/TYPE; excluded: ISNUMBER("5") (lib coercion) |
| array       | 13     | 13    | 1        | SUMPRODUCT, SORT/UNIQUE/FILTER/TRANSPOSE/VSTACK/…; excluded: SEQUENCE (lib) |
| lookup*/database | 0 | 0     | 8        | full D* family (DSUM/DAVERAGE/DCOUNT/DCOUNTA/DGET/DMAX/DMIN) — lib unimplemented |
| engineering | 27     | 27    | 2        | CONVERT ratios/SI prefixes/temperature affine; excluded: IEC binary prefixes |
| edge        | 7      | 7     | 1        | `^` left-assoc, MOD(-7,3), ROUND half-away, `"3"+2`, INT(-2.5); excluded: `SUMPRODUCT(--(range>x))` |
| errors      | 2      | 2     | 0        | `#DIV/0!`, `#N/A` |
| random      | 3      | 3     | 0        | RAND/RANDBETWEEN via deterministic bound wrappers |
| **total**   | **542**| **542**| **33**   | parity = 100.0% (542/542) |

## Whole-workbook corpus (real .xlsx)

| workbook            | graded | match | excluded | shape |
|---------------------|-------:|------:|---------:|-------|
| `pnl.xlsx`          | 16     | 16    | 0        | mini P&L: SUM, subtraction, margin %, ROUND tax, cross-sheet Summary |
| `amortization.xlsx` | 27     | 27    | 1        | loan schedule: PMT + per-period interest/principal/balance chain, PPMT; excluded: IPMT lib-gap |
| `lookup.xlsx`       | 15     | 15    | 0        | price table: VLOOKUP, INDEX/MATCH, HLOOKUP, IF tiers, SUMIF rollup |
| **total**           | **58** | **58**| **1**    | |

The excluded `amortization.xlsx!Loan!B15` (IPMT) is a `formulas`-library defect — see
KNOWN-LIB-GAPS.md §2. Declared in `corpus_workbooks/lib_gaps.json`.

## The per-formula exclusions (33 lib-gaps)

Every one of the 33 excluded cases is a **`formulas`-library defect or non-implementation**, not a
FSA1 defect — FSA1 matches real Excel on each, hand-verified per the ENG6 triage rule (the corpus
grades FSA1 against Excel; where the reference is wrong, FSA1 is not distorted to match it). The
full evidence ledger is [`KNOWN-LIB-GAPS.md`](./KNOWN-LIB-GAPS.md), §§1–14:

- **Unimplemented in the lib (`#NAME?`)** — SEQUENCE; QUOTIENT/COMBIN/SUBTOTAL/AGGREGATE; the whole
  Database family DSUM/DAVERAGE/DCOUNT/DCOUNTA/DGET/DMAX/DMIN; DOLLAR.
- **Wrong result in the lib** — IPMT (`#NUM!`); MODE.MULT (`#VALUE!` in the workbook path); CONVERT IEC
  binary prefixes; three TEXT format-section cases; `SUMPRODUCT(--(range>x))`; SEARCH wildcards
  (`#VALUE!`); TRIM internal-space collapse; TEXTJOIN `ignore_empty`; DATEDIF `"d"` case-sensitivity
  (`#NUM!`); ISNUMBER of a numeric-looking text literal (coerced to TRUE).

Not counted as gaps: `INDIRECT`/`OFFSET` are deliberate FSA1 parse-refusals (reserved for a later
phase), so they are given no corpus case rather than a distorted `lib_gap` — outside the ENG6 surface,
like a reference cycle's `#REF!` (see KNOWN-LIB-GAPS.md, final section).
