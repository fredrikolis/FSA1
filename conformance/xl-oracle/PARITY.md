<!-- Concern: the standing ENG6 parity result — the match/total over the current corpus, the one excluded lib-gap, and the last-run date — as the human-readable snapshot of `run.sh`'s output | Non-concern: how parity is computed (oracle.py) and why the excluded case is a lib-gap (KNOWN-LIB-GAPS.md) | IO: none (documentation snapshot; regenerate by running ./run.sh) -->
# PARITY.md — ENG6 differential parity result

**Result: 41 / 41 graded cases MATCH. 0 charlie-defect divergences. 1 case EXCLUDED (lib-gap).**
Corpus total: 42 cases. Reference oracle: `formulas` v1.3.4 (+ openpyxl). Last run: 2026-07-17.

`./run.sh` exit code: **0** (no charlie-defect divergence).

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
| **total**   | **41** | **41**| **1**    | |

## The one exclusion

`edge/sumproduct_bool_coerce` — `=SUMPRODUCT(--(A1:A3>1))` — charlie returns `2` (correct Excel);
`formulas` v1.3.4 returns `0`. This is a **reference-library defect**, not a charlie defect: the lib
fails to array-broadcast a range comparison through unary minus. Full evidence in
[`KNOWN-LIB-GAPS.md`](./KNOWN-LIB-GAPS.md). Excluded from pass/fail per the ENG6 triage rule — the
corpus grades charlie against Excel, and the reference here is wrong, so charlie is not distorted to
match it.
