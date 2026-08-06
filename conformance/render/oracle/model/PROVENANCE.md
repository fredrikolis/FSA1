<!-- Concern: how the loan_amortization ground truth was computed (auditable, reproducible, FSA1-free) | Non-concern: the FSA1 encoding of the sheet (see artifacts/model/loan_amortization) | IO: none -->
# PROVENANCE — oracle/model (loan_amortization)

Ground truth for `artifacts/model/loan_amortization/` was computed **independently of FSA1**
(FSA1 cannot evaluate yet, and ORACLE-INPUT PURITY forbids the tool grading itself). It was
produced by plain Python arithmetic in `compute_oracle.py`, which reimplements the sheet's formulas
directly — it never invokes FSA1 or any spreadsheet engine.

## Method

- **Interpreter:** `python3` (CPython 3, IEEE-754 `float`), stdlib only (`json`, `csv`). No
  `numpy_financial`, so `PMT` is computed from its closed form rather than a library call.
- **Inputs are pinned constants** (no volatiles, no dates): principal `P = 20000`, annual nominal
  rate `= 0.06`, term `n = 12` months. Monthly rate `r = 0.06/12 = 0.005`.
- **PMT identity used** (Excel `PMT(rate, nper, pv, fv=0, type=0)`, ordinary annuity, `fv=0`):

  ```
  PMT = -pv * rate * (1+rate)**nper / ((1+rate)**nper - 1)
  ```

  The sheet stores a **positive** payment via `=-PMT(B5,B4,B2)` (pv `B2` positive, result negated),
  so the oracle uses `payment = -PMT(r, n, P) = +P*r*(1+r)**n / ((1+r)**n - 1)`.
- **Schedule recurrence** (period `i = 1..12`, `begin_1 = P`):

  ```
  interest_i       = begin_i * r
  principal_paid_i = payment - interest_i
  end_i            = begin_i - principal_paid_i
  begin_{i+1}      = end_i
  ```

- **Summary:** `Total Interest = sum(interest_i)`, `Total Paid = sum(payment)`,
  `Payoff Month = COUNTIF(begin_i > 0) = 12`, `Final Balance = end_12` (renders ~0).

This mirrors, cell-for-cell, the formulas authored in the FSA1 workbook — but is derived from the
math, not from running the tool. Any divergence FSA1 later produces is a real FSA1 finding.

## Reproduce

From `conformance/render/`:
```
python3 oracle/model/compute_oracle.py        # writes oracle_values.json and oracle_values.csv
```
The script writes its outputs beside itself, so cwd only sets where the script path resolves.

## Notable / auditable results

- `Inputs!B6` payment = **1721.3285941416** per month.
- `Summary!B2` total interest = **655.9431296993**.
- `Summary!B3` total paid = **20655.9431296998** (= 20000 principal + 655.94 interest).
- `Summary!B4` payoff month = **12**.
- `Summary!B5` final balance = **-4.2e-10** — i.e. **0** to within IEEE-754 float noise. A grader
  should treat `Amortization!F13` / `Summary!B5` as `0` under a small absolute tolerance (`abs < 1e-6`);
  the tiny residual is float accumulation, not a modelling error, and is the point of the many-period
  correctness check.

## Files

- `compute_oracle.py` — the independent computation (this is the executable provenance).
- `oracle_values.json` — full rendered values keyed by `Sheet!Address` (10-dp rounded for display;
  near-zero residual shown as `-4e-10`).
- `oracle_values.csv` — same, as a 2-column `address,value` diffable table.

## Tolerance note for the grader

All numeric cells are exact rationals of the closed-form payment except where IEEE-754 rounding
accumulates over 12 periods. Compare numbers with a relative/absolute tolerance (e.g. `1e-6`), not
bit-exact equality. Text cells (labels/headers) compare exactly.
