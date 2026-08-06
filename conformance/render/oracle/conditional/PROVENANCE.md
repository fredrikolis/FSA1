<!-- Concern: how the customer_tiers ground truth was computed, so it is auditable and reproducible | Non-concern: the on-disk FORMAT.md grammar (see the artifact) and FSA1's own evaluator (never used here) | IO: none -->
# PROVENANCE — oracle for `artifacts/conditional/customer_tiers`

**Oracle-input purity.** These expected values were computed **independently of FSA1** by a
plain-Python reimplementation of the sheet's logic — FSA1 cannot evaluate yet, and grading the
tool against itself is forbidden. No spreadsheet engine, no FSA1 CLI, was involved.

## Method

`compute_oracle.py` (in this directory) transcribes the spend inputs verbatim from
`Customers/B2-B13.range` and re-implements, from scratch, the two rules the sheet expresses:

1. **Tier** — an independent reimplementation of
   `=IFERROR(IFS(B2>100000,"Gold",B2>50000,"Silver",TRUE,"Bronze"),"Bronze")`:
   ```python
   def tier(spend):
       if isinstance(spend, Err):   # errored feed -> IFS propagates error -> IFERROR -> "Bronze"
           return "Bronze"
       if spend > 100000:           # STRICT: exactly 100000 is not Gold
           return "Gold"
       if spend > 50000:            # STRICT: exactly 50000 is not Silver
           return "Silver"
       return "Bronze"
   ```
2. **Counts** — independent reimplementation of `=COUNTIF($C$2:$C$13,E2)` and `=SUM(F2:F4)`
   via Python `list.count(...)` over the rendered tier column and a plain sum.

Run to reproduce (Python 3.12; stdlib only, no pandas/numpy needed) — from `conformance/render/`:
```
python3 oracle/conditional/compute_oracle.py
```
The script writes its outputs beside itself, so cwd only sets where the script path resolves. It
regenerates `expected_values.csv` and `expected_values.json` and prints the tier tally.

## Boundary cases deliberately pinned (the point of this sheet)

| Row | Customer | Spend | Tier | Why it is the interesting case |
|-----|----------|-------|------|--------------------------------|
| 3 | Globex | 100001 | Gold | just above the Gold threshold |
| 4 | Cyberdyne | 100000.5 | Gold | fractional, still `> 100000` |
| 5 | Initech | **100000** | **Silver** | **exact threshold — `>` is strict, so NOT Gold** |
| 7 | Soylent | 50001 | Silver | just above the Silver threshold |
| 8 | Massive Dynamic | 50000.5 | Silver | fractional, still `> 50000` |
| 9 | Stark Industries | **50000** | **Bronze** | **exact threshold — `>` is strict, so NOT Silver** |
| 12 | Hooli | -5000 | Bronze | negative spend falls through to the `TRUE` arm |
| 13 | Vandelay | **#VALUE!** | **Bronze** | **errored feed → IFS propagates → IFERROR degrades to Bronze** |

The two exact-threshold rows (Initech 100000, Stark 50000) are the correctness trap: an engine that
uses `>=` instead of `>` would misclassify both and the counts would read Gold:4 Silver:4 Bronze:4.

## Expected result (authoritative tally)

**Gold: 3 · Silver: 4 · Bronze: 5 · Total: 12.**

Full per-cell rendering is in `expected_values.csv` (cell,value) and `expected_values.json`
(address → value). The `#VALUE!` in B13 is an input error literal (renders as-is); every C-column and
F-column value is the evaluated result the FSA1 engine must reproduce.
