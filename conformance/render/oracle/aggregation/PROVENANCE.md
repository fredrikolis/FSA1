<!-- Concern: how the aggregation/sales_report ground truth was computed (auditable, reproducible, FSA1-independent) | Non-concern: the on-disk FSA1 encoding (see artifacts/) and the B1 verdict | IO: none -->
# PROVENANCE — oracle/aggregation (sales_report)

**Subject workbook:** `artifacts/aggregation/sales_report/` (tabs `Sales`, `Summary`).

**ORACLE-INPUT PURITY.** These expected values were computed **independently of FSA1**
(FSA1 has no evaluator yet, and grading the tool with its own output is forbidden). Ground
truth is produced by `compute_oracle.py` using **pandas group-by / boolean-mask arithmetic** — a
different implementation from the FSA1 criteria-aggregation engine under test. The oracle does
**not** read the FSA1 files; it re-states the sheet's INPUT cells (the 12 order records and the
region/product criteria keys) and computes what each formula *should* render.

## How it was computed

- **Tool:** `python3 3.12.3`, `pandas 2.3.3` (numpy 1.26.4 present). Run from `conformance/render/`: `python3 oracle/aggregation/compute_oracle.py`
  (the script writes its outputs beside itself).
- **Inputs restated:** the 12-row ledger `(OrderID, Region, Product, Units, Revenue)` exactly as in
  `Sales/A2-E13.range`; region keys `[EMEA, AMER, APAC]` (`Summary/A2-A4`); product keys
  `[Widget, Gadget, Sprocket]` (`Summary/A7-A9`). All inputs are constants — no dates, no
  volatiles (`TODAY/NOW/RAND`) — so the result is deterministic and reproducible.
- **Formula → oracle method mapping (independent restatement, not fsa1-cli eval):**
  | FSA1 formula (file)                          | Oracle computation (pandas) |
  |-------------------------------------------------|-----------------------------|
  | `Sales/D14` `=SUM(D2:D13)`                       | `df.Units.sum()` |
  | `Sales/E14` `=SUM(E2:E13)`                       | `df.Revenue.sum()` |
  | `Summary/B2-B4` `=SUMIFS(Sales!E$2:E$13,Sales!B$2:B$13,A2)` | `df[df.Region==k].Revenue.sum()` per region |
  | `Summary/C2-C4` `=COUNTIFS(Sales!B$2:B$13,A2)`   | `len(df[df.Region==k])` |
  | `Summary/D2-D4` `=AVERAGEIFS(Sales!E$2:E$13,Sales!B$2:B$13,A2)` | `df[df.Region==k].Revenue.mean()` |
  | `Summary/B7-B9` `=SUMIFS(...,Sales!C$2:C$13,A7)` | `df[df.Product==k].Revenue.sum()` per product |
  | `Summary/C7-C9` `=COUNTIFS(Sales!C$2:C$13,A7)`   | `len(df[df.Product==k])` |
  | `Summary/D7-D9` `=AVERAGEIFS(...,Sales!C$2:C$13,A7)` | `df[df.Product==k].Revenue.mean()` |
  | `Summary/B11` `=AVERAGE(Sales!E2:E13)`           | `df.Revenue.mean()` |
  | `Summary/B12` `=SUMIFS(...,B..,"EMEA",C..,"Widget")` | `df[(Region==EMEA)&(Product==Widget)].Revenue.sum()` |
  | `Summary/B13` `=SUM(B2:B4)`                      | `sum` of the three region totals |
- **Drag-fill note:** the `Summary/B2-B4`, `C2-C4`, `D2-D4` (and `…7:9`) files carry ONE formula
  anchored at the top-left cell. Under FORMAT.md §6.1 fill mode, the criteria ref (`A2`/`A7`, no
  `$`) re-anchors per row while the `Sales!E$2:E$13` / `Sales!B$2:B$13` ranges are row-locked with
  `$` so they do NOT drift. The oracle reproduces that by iterating the region/product key per row.

## Hand-check cross-validation (independent of pandas)

- Grand total revenue = 1200+750+960+1440+720+600+1800+1080+840+1650+360+600 = **12000**;
  `AVERAGE` = 12000/12 = **1000**; region sums 4080+2910+5010 = 12000 and product sums
  4680+3000+4320 = 12000 both reconcile; `Summary/B13` check = **12000** = `Sales!E14`. Units sum
  = **95**. EMEA∩Widget = 1200+840 = **2040**. All agree with `expected_derived.csv`.

## Files

- `expected_values.json` — every cell (inputs + derived) keyed by `Sheet!Addr`, rendered value.
- `expected_derived.csv` — the 23 derived/output cells only (the grading targets), `cell,value`.
- `compute_oracle.py` — the exact reproducible script that emitted both.
