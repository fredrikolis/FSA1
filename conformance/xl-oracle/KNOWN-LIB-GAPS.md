<!-- Concern: the ledger of cases where the `formulas` reference library (not charlie) is wrong or unsupported vs real Excel — each with the case, the observed lib output, the correct Excel/charlie output, and the evidence — so those cases are EXCLUDED from ENG6 pass/fail rather than distorting charlie to a wrong reference | Non-concern: charlie defects (those are fixed, not recorded here) and the harness mechanics (oracle.py + README own those) | IO: none (documentation) -->
# KNOWN-LIB-GAPS.md — where the `formulas` reference is wrong, not charlie

`formulas` v1.3.4 implements a large Excel subset, but not all of it. This ledger records cases
where the **reference library** diverges from real Excel; charlie matches Excel on each. These cases
carry `"lib_gap": true` in the per-formula corpus, or an entry in
[`corpus_workbooks/lib_gaps.json`](./corpus_workbooks/lib_gaps.json) for the whole-workbook oracle,
and are **EXCLUDED** from the ENG6 pass/fail count (the oracle still prints them, showing both
values, for transparency). Distorting charlie to chase a wrong reference is forbidden.

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

## 2. `IPMT(rate, per, nper, pv)` — returns `#NUM!` (PPMT of the same arguments works)

- **Case:** `amortization.xlsx` cell `Loan!B15` — `=IPMT(B2/12,1,B3,-B1)` with `B2=0.06`, `B3=6`,
  `B1=20000` (probed directly as `=IPMT(0.005,1,6,-20000)`).
- **Correct (Excel & charlie):** `100`. Period-1 interest on a 20 000 loan at 0.5 %/month is
  `20000 * 0.005 = 100`. charlie returns `100`; the amortization schedule's own balance-chain interest
  cell (`Loan!B7 = D6*$B$2/12`) **also** computes `100` and the reference library *agrees* with that
  (`Loan!B7` is a MATCH at `100`) — so the library is internally inconsistent, not charlie.
- **`formulas` v1.3.4 output:** `#NUM!` — **wrong**. It returns `#NUM!` for IPMT with both cell-ref
  and literal arguments.
- **Root cause (probed directly):** `IPMT` is broken/unsupported in `formulas` v1.3.4, while the
  sibling `PPMT(0.005,1,6,-20000)` returns the correct `3291.909…` in the same library and charlie
  matches it. So the defect is specific to the `IPMT` code path, not annuity functions in general.
- **Verdict:** `lib-gap` (reference defect). Excluded from pass/fail; charlie is correct.

## 3. `SEQUENCE(rows, [cols], [start], [step])` — unimplemented (`#NAME?`)

- **Case:** `array/sequence_topleft` — formula `=SEQUENCE(3)` (no inputs).
- **Correct (Excel & charlie):** a `3x1` array `{1;2;3}`; its top-left (implicit-intersection) value,
  which is what a single-cell `charlie-cli eval` reports, is `1`.
- **`formulas` v1.3.4 output:** `#NAME?` — the function name is not recognized at all.
- **Root cause (probed directly):** `SEQUENCE` is simply absent from the library's function table,
  while its siblings `SORT`/`UNIQUE`/`FILTER`/`TRANSPOSE` ARE present (each graded and Matching in
  `corpus/array.json`). So the gap is specific to `SEQUENCE`, not dynamic-array functions in general.
- **Coverage instead:** charlie's `SEQUENCE` is pinned by Rust tests with hand-verified Excel values
  (`charlie-ast` `func::tests::spill::sequence_generates_row_major_and_refuses_empty` and
  `charlie-model` `workbook::tests::sequence_region_generates_its_counter`).
- **Verdict:** `lib-gap` (reference unsupported). Excluded from pass/fail; charlie is correct.

## 4. Dynamic arrays do not SPILL in the reference — only the anchor (top-left) is computed

- **Observation (probed directly):** for `=SORT(A1:A3)`, `=UNIQUE(A1:A5)`, `=FILTER(A1:A3,B1:B3)`,
  `=TRANSPOSE(A1:C1)` the library computes the function but returns **only a `1x1` anchor value** (the
  array's top-left element) rather than spilling the result across a range — e.g. `=SORT({3;1;2})`
  yields `[[1]]`, not `[[1],[2],[3]]`.
- **Why this is fine for the per-formula oracle:** charlie's single-cell `eval` of an array formula
  ALSO reports the top-left element (GRID5/ENG6: no dynamic spill, a one-cell array formula keeps its
  implicit-intersection top-left), so the anchor values MATCH cell-for-cell — this is exactly what the
  four graded `array/*_topleft` cases assert.
- **Coverage of the FULL region fill:** charlie's GRID5 behavior — a range file whose sole content is
  one `=formula` filling its declared range, element `(r,c)` at coordinate `(r,c)` — cannot be graded
  against a reference that never spills. It is pinned instead by charlie-model Rust tests
  (`sort_region_fills_its_range_sorted`, `unique_region_over_a_column_with_dups`,
  `transpose_region_fills_the_transposed_orientation`, the shape-mismatch/scalar-in-range located
  dimension errors, and the reference-into-a-region case) with hand-verified Excel values.
- **Verdict:** `lib-gap` (reference does not model spill). The per-formula oracle grades the anchor;
  the region fill is Rust-tested. charlie is correct.
