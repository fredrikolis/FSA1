<!-- Concern: how the invoice-aging ground truth was computed, independently of charlie | Non-concern: the charlie encoding under test (the workbook itself lives in ../../artifacts/dates/invoice-aging/) | IO: none -->
# PROVENANCE — oracle/dates/invoice-aging

Ground truth for `artifacts/dates/invoice-aging/` (tab `Invoices`), rendered values in
`invoice-aging.oracle.csv`. Computed **independently of charlie** (ORACLE-INPUT PURITY: charlie
cannot evaluate yet and must never grade itself).

## Method

Plain Python 3 with the stdlib `datetime` module only — no spreadsheet engine, no charlie. The exact
script is committed alongside this file as `compute_oracle.py`; re-running it reproduces every value
byte-for-byte. It computes each result **twice by two independent routes** and they agree:

1. Per-row: `days = ref_serial - invoice_serial`; bucket by an `if`-ladder mirroring the sheet's
   `IFS`. Rollups summed by grouping on that bucket label.
2. Cross-check: the same counts/amounts recomputed by numeric predicates on `days` that mirror the
   sheet's `COUNTIFS`/`SUMIFS`/`COUNTIF`/`SUMIF` criteria (`>=lo & <=hi`, `>90`).

Both routes yield count 3 and the same dollar totals for every bucket, and the grand total
reconciles (count 12, amount 31421.25).

## Pinned constants (reproducibility)

- **Reference date is FIXED**: `Invoices!I1 = DATE(2026,3,31)`. The sheet does **not** use volatile
  `TODAY`, so `Days Outstanding` is deterministic and this oracle never goes stale.
- **Date system**: 1900 serial date system (the spreadsheet default), computed with epoch
  `1899-12-30`, i.e. `serial(d) = (d - date(1899,12,30)).days`. This matches how Excel/Sheets store
  dates and what charlie's `DATE()` is expected to return. `2026-03-31` -> serial `46112`.
  Note: the day-count (`DATEDIF(...,"d")`) is a *difference* of serials, so it is independent of the
  epoch choice — only the raw serial literals in column C depend on it. If charlie adopts a different
  epoch, column C serials shift by a constant but E/F/summary values are unaffected.

## Function semantics assumed

- `DATEDIF(start, end, "d")` = whole days from `start` to `end`, `end >= start` (true for every row
  since all invoice dates precede the reference date). Equals `end_serial - start_serial`.
- `IFS(...)` = first true branch wins; bucket boundaries are inclusive-upper
  (`<=30`, `<=60`, `<=90`, else `90+`).
- `COUNTIFS`/`SUMIFS` criteria are inclusive on both ends per bucket; `90+` uses `>90` (strictly
  greater), so day-count 90 lands in `61-90` and 91 in `90+`. The boundary rows (days = 0, 30, 31,
  60, 61, 90, 91) are deliberately present to exercise the bin edges.

## Regenerate

From the experiment root (`exploration/experiments/01-encoding-corpus/`):
```
python3 oracle/dates/compute_oracle.py
```
The script prints the per-row serials/day-counts/buckets and the reconciling rollups (count 12,
amount 31421.25) used to cross-check `invoice-aging.oracle.csv`.
