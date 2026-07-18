<!-- Concern: the ledger of cases where the `formulas` reference library (not charlie) is wrong or unsupported vs real Excel — each with the case, the observed lib output, the correct Excel/charlie output, and the evidence — so those cases are EXCLUDED from ENG6 pass/fail rather than distorting charlie to a wrong reference | Non-concern: charlie defects (those are fixed, not recorded here) and the harness mechanics (oracle.py + README own those) | IO: none (documentation) -->
# KNOWN-LIB-GAPS.md — where the `formulas` reference is wrong, not charlie

`formulas` v1.3.4 implements a large Excel subset, but not all of it. This ledger records cases
where the **reference library** diverges from real Excel; charlie matches Excel on each. These cases
carry `"lib_gap": true` in the corpus and are **EXCLUDED** from the ENG6 pass/fail count (the oracle
still prints them, showing both values, for transparency). Distorting charlie to chase a wrong
reference is forbidden.

## 1. `SUMPRODUCT(--(range > x))` — unary-minus does not preserve array shape over a range comparison

- **Case:** `edge/sumproduct_bool_coerce` — inputs `A1:A3 = {1;2;3}`, formula
  `=SUMPRODUCT(--(A1:A3>1))`.
- **Correct (Excel & charlie):** `2`. `A1:A3>1` is the array `{FALSE;TRUE;TRUE}`; `--(…)` coerces it
  to `{0;1;1}`; `SUMPRODUCT` sums to `2`. This is *the* canonical SUMPRODUCT counting idiom (and the
  one charlie-cli's own `--guide`/help advertises, `=SUMPRODUCT(--(C2:C11>5))`).
- **`formulas` v1.3.4 output:** `0` — **wrong**.
- **Root cause (probed directly):** the library does not array-broadcast a *range* comparison. A bare
  `=A1:A3>1` returns the scalar `False` (only the first cell is compared) instead of a 3-element
  array, so `--(A1:A3>1)` is `--False = 0` and every `SUM`/`SUMPRODUCT` over it collapses to `0`.
  Tellingly, the multiplication form `=SUMPRODUCT((A1:A3>1)*1)` **does** return `2` in the same
  library — so the defect is specifically the unary-minus (`--`) coercion path over a range
  comparison, not comparison broadcasting in general.
- **Verdict:** `lib-gap` (reference defect). Excluded from pass/fail; charlie is correct.
