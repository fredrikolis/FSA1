<!-- Concern: how the lookup-join ground truth was computed, for audit/reproducibility | Non-concern: the charlie encoding of the sheet (see ../../artifacts/lookup-join/) | IO: none -->
# PROVENANCE — oracle/lookup-join

Ground truth for `artifacts/lookup-join/`, computed **independently of charlie**
(ORACLE-INPUT PURITY — the tool never grades itself; charlie cannot evaluate yet anyway).

## Method

Pure Python (`compute_oracle.py`, stdlib only — no charlie, no spreadsheet app).
The sheet's inputs were **hand-transcribed** from the artifact literal blocks into
Python dicts/lists at the top of the script (`products`, `orders`), then the join and
arithmetic were re-implemented from first principles:

- **`xlookup(key)`** — exact-match lookup via `dict.get(key, "#N/A")`. This reproduces
  both the column-D `XLOOKUP(B, Products!$A$2:$A$7, Products!$C$2:$C$7)` (no
  `if_not_found` arg ⇒ unmatched key returns `#N/A`) and the column-E
  `INDEX(Products!$C$2:$C$7, MATCH(B, Products!$A$2:$A$7, 0))` (exact match mode `0` ⇒
  unmatched key returns `#N/A`). D and E are the same exact-match join by construction,
  so the oracle asserts them equal.
- **`mul(a, b)`** — multiplication with Excel-style **error propagation**: if either
  operand is an error literal (`str` starting with `#`), the result is `#N/A`. Used for
  `F = C*D` and `H = C*G`. Money results are `round(…, 2)`.
- **`ifna(v, 0.0)`** — the `IFNA(XLOOKUP(...), 0)` guard in column G: substitutes `0.0`
  only when the wrapped value is `#N/A`, else passes the value through.
- **Totals** — `F12 = SUM(F2:F11)` sees the `#N/A` at `F7` and is therefore `#N/A`
  (aggregate error propagation). `H12 = SUM(H2:H11)` sums the IFNA-guarded column and is
  clean: `148.20`.

No dates, `TODAY`/`NOW`, or `RAND` appear, so nothing is volatile — the computation is
fully deterministic and reproducible. All prices are pinned constants.

## The missing-key case (the designed edge)

Order `O1006` references `product_id = P099`, which is **absent** from the Products
catalog. Expected renders:

| Cell | Value | Why |
|------|-------|-----|
| `Orders!D7` | `#N/A` | XLOOKUP, no `if_not_found` |
| `Orders!E7` | `#N/A` | MATCH exact mode returns `#N/A`; INDEX propagates |
| `Orders!F7` | `#N/A` | `qty * #N/A` propagates |
| `Orders!G7` | `0.00` | `IFNA(…, 0)` fallback |
| `Orders!H7` | `0.00` | `qty * 0` |
| `Orders!F12` | `#N/A` | `SUM` over a range containing `#N/A` |
| `Orders!H12` | `148.20` | `SUM` over the clean IFNA-guarded column |

## Reproduce

From the experiment root (`exploration/experiments/01-encoding-corpus/`):
```
python3 oracle/lookup-join/compute_oracle.py      # regenerates oracle.csv and oracle.json
```

The script writes its outputs beside itself, so cwd only sets where the script path resolves.
Outputs are stable, sorted by (tab, row, address). Diff `oracle.csv` against a future
charlie render to grade the engine cell-for-cell (QA-ladder Tier 6, cell-exact).

## Independent cross-check (H12 = 148.20)

`10.00 + 12.50 + 10.00 + 10.00 + 22.50 + 0.00 + 8.20 + 35.00 + 30.00 + 10.00 = 148.20`
(the `0.00` is the IFNA-guarded missing-key row O1006). Matches the script output.
