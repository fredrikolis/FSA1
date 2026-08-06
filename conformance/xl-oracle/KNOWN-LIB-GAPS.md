<!-- Concern: the ledger of cases where the formulas reference library diverges from real Excel | Non-concern: FSA1 defects, the harness mechanics | IO: none -->
# KNOWN-LIB-GAPS.md — where the `formulas` reference is wrong, not FSA1

`formulas` v1.3.4 implements a large Excel subset, but not all of it. This ledger records cases
where the **reference library** diverges from real Excel; FSA1 matches Excel on each. These cases
carry `"lib_gap": true` in the per-formula corpus, or an entry in
[`corpus_workbooks/lib_gaps.json`](./corpus_workbooks/lib_gaps.json) for the whole-workbook oracle,
and are **EXCLUDED** from the ENG6 pass/fail count (the oracle still prints them, showing both
values, for transparency). Distorting FSA1 to chase a wrong reference is forbidden.

## 1. `SUMPRODUCT(--(range > x))` — unary-minus does not preserve array shape over a range comparison

- **Case:** `edge/sumproduct_bool_coerce` — inputs `A1:A3 = {1;2;3}`, formula
  `=SUMPRODUCT(--(A1:A3>1))`.
- **Correct (Excel & FSA1):** `2`. `A1:A3>1` is the array `{FALSE;TRUE;TRUE}`; `--(…)` coerces it
  to `{0;1;1}`; `SUMPRODUCT` sums to `2`. This is *the* canonical SUMPRODUCT counting idiom (and the
  one fsa1-cli's own `--guide`/help advertises, `=SUMPRODUCT(--(C2:C11>5))`).
- **`formulas` v1.3.4 output:** `0` — **wrong**.
- **Root cause (probed directly):** the library does not array-broadcast a *range* comparison. A bare
  `=A1:A3>1` returns the scalar `False` (only the first cell is compared) instead of a 3-element
  array, so `--(A1:A3>1)` is `--False = 0` and every `SUM`/`SUMPRODUCT` over it collapses to `0`.
  Tellingly, the multiplication form `=SUMPRODUCT((A1:A3>1)*1)` **does** return `2` in the same
  library — so the defect is specifically the unary-minus (`--`) coercion path over a range
  comparison, not comparison broadcasting in general.
- **Verdict:** `lib-gap` (reference defect). Excluded from pass/fail; FSA1 is correct.

## 2. `IPMT(rate, per, nper, pv)` — returns `#NUM!` (PPMT of the same arguments works)

- **Case:** `amortization.xlsx` cell `Loan!B15` — `=IPMT(B2/12,1,B3,-B1)` with `B2=0.06`, `B3=6`,
  `B1=20000` (probed directly as `=IPMT(0.005,1,6,-20000)`).
- **Correct (Excel & FSA1):** `100`. Period-1 interest on a 20 000 loan at 0.5 %/month is
  `20000 * 0.005 = 100`. FSA1 returns `100`; the amortization schedule's own balance-chain interest
  cell (`Loan!B7 = D6*$B$2/12`) **also** computes `100` and the reference library *agrees* with that
  (`Loan!B7` is a MATCH at `100`) — so the library is internally inconsistent, not FSA1.
- **`formulas` v1.3.4 output:** `#NUM!` — **wrong**. It returns `#NUM!` for IPMT with both cell-ref
  and literal arguments.
- **Root cause (probed directly):** `IPMT` is broken/unsupported in `formulas` v1.3.4, while the
  sibling `PPMT(0.005,1,6,-20000)` returns the correct `3291.909…` in the same library and FSA1
  matches it. So the defect is specific to the `IPMT` code path, not annuity functions in general.
- **Verdict:** `lib-gap` (reference defect). Excluded from pass/fail; FSA1 is correct.

## 3. `SEQUENCE(rows, [cols], [start], [step])` — unimplemented (`#NAME?`)

- **Case:** `array/sequence_topleft` — formula `=SEQUENCE(3)` (no inputs).
- **Correct (Excel & FSA1):** a `3x1` array `{1;2;3}`; its top-left (implicit-intersection) value,
  which is what a single-cell `fsa1-cli eval` reports, is `1`.
- **`formulas` v1.3.4 output:** `#NAME?` — the function name is not recognized at all.
- **Root cause (probed directly):** `SEQUENCE` is simply absent from the library's function table,
  while its siblings `SORT`/`UNIQUE`/`FILTER`/`TRANSPOSE` ARE present (each graded and Matching in
  `corpus/array.json`). So the gap is specific to `SEQUENCE`, not dynamic-array functions in general.
- **Coverage instead:** FSA1's `SEQUENCE` is pinned by Rust tests with hand-verified Excel values
  (`fsa1-ast` `func::tests::spill::sequence_generates_row_major_and_refuses_empty` and
  `fsa1-model` `workbook::tests::sequence_region_generates_its_counter`).
- **Verdict:** `lib-gap` (reference unsupported). Excluded from pass/fail; FSA1 is correct.

## 4. Dynamic arrays do not SPILL in the reference — only the anchor (top-left) is computed

- **Observation (probed directly):** for `=SORT(A1:A3)`, `=UNIQUE(A1:A5)`, `=FILTER(A1:A3,B1:B3)`,
  `=TRANSPOSE(A1:C1)` the library computes the function but returns **only a `1x1` anchor value** (the
  array's top-left element) rather than spilling the result across a range — e.g. `=SORT({3;1;2})`
  yields `[[1]]`, not `[[1],[2],[3]]`.
- **Why this is fine for the per-formula oracle:** FSA1's single-cell `eval` of an array formula
  ALSO reports the top-left element (GRID5/ENG6: no dynamic spill, a one-cell array formula keeps its
  implicit-intersection top-left), so the anchor values MATCH cell-for-cell — this is exactly what the
  four graded `array/*_topleft` cases assert.
- **Coverage of the FULL region fill:** FSA1's GRID5 behavior — a range file whose sole content is
  one `=formula` filling its declared range, element `(r,c)` at coordinate `(r,c)` — cannot be graded
  against a reference that never spills. It is pinned instead by fsa1-model Rust tests
  (`sort_region_fills_its_range_sorted`, `unique_region_over_a_column_with_dups`,
  `transpose_region_fills_the_transposed_orientation`, the shape-mismatch/scalar-in-range located
  dimension errors, and the reference-into-a-region case) with hand-verified Excel values.
- **Verdict:** `lib-gap` (reference does not model spill). The per-formula oracle grades the anchor;
  the region fill is Rust-tested. FSA1 is correct.

## 5. `QUOTIENT` / `COMBIN` / `SUBTOTAL` / `AGGREGATE` — unimplemented (`#NAME?`)

- **Cases:** `mathtrig/quotient_basic` (`=QUOTIENT(5,2)`), `mathtrig/quotient_negative`
  (`=QUOTIENT(-5,2)`), `mathtrig/combin_basic` (`=COMBIN(8,2)`), `mathtrig/subtotal_sum`
  (`=SUBTOTAL(9,A1:A5)`), `mathtrig/subtotal_average` (`=SUBTOTAL(1,A1:A5)`), `mathtrig/aggregate_sum`
  (`=AGGREGATE(9,4,A1:A5)`), `mathtrig/aggregate_large` (`=AGGREGATE(14,4,A1:A5,2)`).
- **Correct (Excel & FSA1):** `2`, `-2`, `28`, `14`, `2.8`, `14`, `4` respectively. QUOTIENT
  truncates the quotient toward zero; COMBIN(8,2) is the binomial coefficient; SUBTOTAL/AGGREGATE
  aggregate the range by their leading function number (9 = SUM, 1 = AVERAGE, 14 = LARGE).
- **`formulas` v1.3.4 output:** `#NAME?` for all seven — the function names are not recognized at all.
- **Root cause (probed directly):** these four functions are simply absent from the library's function
  table, while their Math/Trig siblings ARE present (GCD/LCM/FACT/SUMSQ/MROUND/CEILING.MATH/FLOOR.MATH/
  TRUNC/SIGN/EVEN/ODD all grade and Match in `corpus/mathtrig.json`). So the gap is specific to these
  four names, not Math/Trig functions in general.
- **Coverage instead:** FSA1's QUOTIENT/COMBIN/SUBTOTAL/AGGREGATE are pinned by Rust tests with
  hand-verified Excel values (`fsa1-ast` `func::tests::math`, `func::tests::combinatorics`, and
  `func::tests::subtotal`).
- **Verdict:** `lib-gap` (reference unsupported). Excluded from pass/fail; FSA1 is correct.

## 6. `MODE.MULT(range)` — whole-workbook model returns `#VALUE!` (MODE.SNGL / FREQUENCY work)

- **Case:** `statistical/mode_mult_topleft` — inputs `A1:A5 = {1;2;2;3;3}`, formula
  `=MODE.MULT(A1:A5)`.
- **Correct (Excel & FSA1):** the multi-mode array `{2;3}` (both values occur twice), whose
  top-left (implicit-intersection) value — what a single-cell `fsa1-cli eval` reports — is `2`.
- **`formulas` v1.3.4 output:** `#VALUE!` — **wrong**, specifically on the whole-workbook
  `ExcelModel` path the oracle uses (`ExcelModel().loads(xlsx).finish().calculate()`).
- **Root cause (probed directly):** the SAME library computes `MODE.MULT({1,2,2,3,3})` correctly as
  `2` through its inline `Parser().ast(...).compile()` path, and its siblings `MODE.SNGL(A1:A5)` (→ `2`)
  and the array-returning `FREQUENCY(A1:A5,B1:B2)` (→ `2`) both compute correctly in the ExcelModel
  path. So the defect is specific to MODE.MULT's dynamic-array handling in the workbook model, not
  array-returning statistical functions in general.
- **Coverage instead:** FSA1's MODE.MULT is pinned by a Rust test with a hand-verified Excel value
  (`fsa1-ast` `func::tests::stats_rank::mode_mult_returns_all_modes_as_a_column`, asserting the full
  `{2;3}` column array).
- **Verdict:** `lib-gap` (reference defect). Excluded from pass/fail; FSA1 is correct.

## 7. `DSUM` / `DAVERAGE` / `DCOUNT` / `DCOUNTA` / `DGET` / `DMAX` / `DMIN` — unimplemented (`#NAME?`)

- **Cases (probed directly):** the whole Database family over the canonical Excel orchard database
  (`Tree Height Age Yield Profit` + six records) with a criteria block, e.g.
  `=DSUM(A1:E7,"Profit",A10:C11)`, `=DGET(A1:E7,"Yield",A10:C11)`, `=DCOUNT(A1:E7,"Age",A10:C11)`.
- **Correct (Excel & FSA1):** for `Tree=Apple AND Height>10 AND Age>12` (two matching records):
  DSUM Profit = `180`, DAVERAGE Yield = `12`, DCOUNT Age = `2`, DCOUNTA Tree = `2`, DMAX Profit =
  `105`, DMIN Profit = `75`; DGET is `#NUM!` (two matches), `14` for a single-match criterion, and
  `#VALUE!` for no match.
- **`formulas` v1.3.4 output:** `#NAME?` for all seven — the function names are not recognized at all
  (verified by loading each into the `ExcelModel` path the oracle uses).
- **Root cause (probed directly):** the entire `D*` database family is absent from the library's
  function table, while the sibling criteria-aggregation `*IF(S)` forms (SUMIF/COUNTIFS/… graded in
  `corpus/aggregate.json`) ARE present. So the gap is the whole Database family, not criteria matching
  in general.
- **Coverage instead:** FSA1's Database family is pinned by Rust tests with hand-verified Excel
  values (`fsa1-ast` `func::tests::database` — field-by-name and by 1-based number, the OR/AND
  criteria semantics, blank-row match-all, the counts vs numeric reducers, DGET's single/no/multi
  contract, and the error-propagation rules), plus the DATABASE criteria grammar itself: bare text is
  matched BEGINS-WITH (criterion `Apple` selects both `Apple` and `Apple2`) while a leading `=` forces
  exact — pinned with a strict-prefix fixture in
  `func::tests::database::bare_text_criteria_match_begins_with_and_leading_eq_forces_exact`, so the
  begins-with-vs-exact divergence from the `*IF(S)` grammar is exercised, not just assumed.
- **Known limitations (recorded):** a criteria label naming no database column is ignored rather than
  evaluated as Excel COMPUTED CRITERIA (can over-accept vs Excel), and a non-integer field number is
  truncated toward zero — both defensible scope cuts for a fresh D* landing, neither lib-verifiable.
- **Verdict:** `lib-gap` (reference unsupported). Excluded from pass/fail; FSA1 is correct.

## 8. `CONVERT(number, from, to)` with IEC binary prefixes (`ki`/`Mi`/`Gi`/…) — reference uses non-power-of-two multipliers

- **Cases:** `engineering/convert_bin_kibyte_byte` (`=CONVERT(1,"kibyte","byte")`) and
  `engineering/convert_bin_gibit_bit` (`=CONVERT(1,"Gibit","bit")`).
- **Correct (Excel & FSA1):** `1024` and `1073741824` (=2^30). The IEC binary prefixes scale by
  powers of two: `ki`=2^10, `Mi`=2^20, `Gi`=2^30, … `Yi`=2^80. So a kibibyte is `1024` bytes.
- **`formulas` v1.3.4 output:** `8` and `28` — **wrong**. The library's `units.json` tabulates the
  binary-prefixed information units with small integer multipliers (e.g. `kibit`=8, `Gibit`=28)
  instead of `2^(10·n)`, so every binary-prefix conversion is off by orders of magnitude. The library
  is correct on the SI (decimal) information prefixes in the same table (`kbit`=1000, graded and
  Matching as `engineering/convert_info_kbit_bit`), so the defect is specific to the binary prefixes.
- **Coverage instead:** FSA1's binary prefixes are pinned by a Rust test with hand-verified Excel
  values (`fsa1-ast` `func::tests::engineering::binary_prefixes_are_excel_exact_powers_of_two`:
  `kibyte→byte=1024`, `Mibyte→byte=1048576`, `kibit→bit=1024`, `Gibit→bit=2^30`).
- **Verdict:** `lib-gap` (reference defect). Excluded from pass/fail; FSA1 is correct.

## 9. `DOLLAR(number, [decimals])` — unimplemented (`#NAME?`)

- **Cases:** `text2/dollar_two_decimals` (`=DOLLAR(1234.5,2)`), `text2/dollar_negative_parens`
  (`=DOLLAR(-1234.5,2)`), `text2/dollar_neg_decimals_rounds` (`=DOLLAR(1234.5,-2)`).
- **Correct (Excel & FSA1):** `"$1,234.50"`, `"($1,234.50)"` (negative currency in parentheses),
  and `"$1,200"` (a negative `decimals` rounds left of the decimal point).
- **`formulas` v1.3.4 output:** `#NAME?` for all three — the function name is not recognized.
- **Root cause (probed directly):** `DOLLAR` is absent from the library's function table, while its
  text-format sibling `FIXED` IS present (graded and Matching in `corpus/text.json`).
- **Coverage instead:** FSA1's `DOLLAR` is hand-verified vs Excel and Rust-pinned alongside the
  other `text_format` currency/number-format tests.
- **Verdict:** `lib-gap` (reference unsupported). Excluded from pass/fail; FSA1 is correct.

## 10. `SEARCH(find, within)` — wildcards `?`/`*` return `#VALUE!`

- **Case:** `text2/search_wildcard` — `=SEARCH("b?d","abcde")`.
- **Correct (Excel & FSA1):** `2`. `SEARCH` honors the `?` (single char) and `*` (run) wildcards,
  so `"b?d"` matches `"bcd"` starting at position 2. (`=SEARCH("*d","abcde")` likewise.)
- **`formulas` v1.3.4 output:** `#VALUE!` — it does not implement wildcard matching in `SEARCH`; a
  plain (non-wildcard) `SEARCH` works (graded and Matching as `text2/search_case_insensitive`).
- **Verdict:** `lib-gap` (reference unsupported). Excluded from pass/fail; FSA1 is correct.

## 11. `TRIM(text)` — does not collapse internal runs of spaces

- **Case:** `text2/trim_internal_collapse` — `=TRIM("  a   b  ")`.
- **Correct (Excel & FSA1):** `"a b"`. Excel's `TRIM` removes leading/trailing spaces AND collapses
  every internal run of spaces to a single space.
- **`formulas` v1.3.4 output:** `"a   b"` — it strips only the leading/trailing spaces and leaves the
  internal run intact. The leading/trailing-only case (`=TRIM("  abc  ")`→`"abc"`) agrees and is graded
  and Matching (`text2/trim_leading_trailing`).
- **Verdict:** `lib-gap` (reference defect). Excluded from pass/fail; FSA1 is correct.

## 12. `TEXTJOIN(delim, ignore_empty, …)` — ignores the `ignore_empty` flag

- **Case:** `text2/textjoin_ignore_empty` — `=TEXTJOIN("-",TRUE,"a","","b")`.
- **Correct (Excel & FSA1):** `"a-b"` with `ignore_empty=TRUE` (the empty argument is skipped).
- **`formulas` v1.3.4 output:** `"a--b"` — it emits the empty argument regardless of the flag (it
  returns `"a--b"` for BOTH `TRUE` and `FALSE`). The `ignore_empty=FALSE` case agrees (`"a--b"`) and is
  graded and Matching (`text2/textjoin_keep_empty`).
- **Verdict:** `lib-gap` (reference defect). Excluded from pass/fail; FSA1 is correct.

## 13. `DATEDIF(start, end, unit)` — case-sensitive on the unit code (`"d"` → `#NUM!`)

- **Case:** `datetime/datedif_days` — `=DATEDIF(DATE(2020,1,1),DATE(2020,1,31),"d")`.
- **Correct (Excel & FSA1):** `30`. Excel's `DATEDIF` unit code is case-insensitive.
- **`formulas` v1.3.4 output:** `#NUM!` for lowercase `"d"`; uppercase `"D"` returns `30`, and
  lowercase `"m"`/`"y"` work (graded and Matching as `datetime/datedif_months` and
  `datetime/datedif_years`). So the defect is the case-sensitivity of the `"d"` unit specifically.
- **Verdict:** `lib-gap` (reference defect). Excluded from pass/fail; FSA1 is correct.

## 14. `ISNUMBER("5")` — reference coerces numeric-looking text literals to numbers

- **Case:** `info2/isnumber_false_on_text` — `=ISNUMBER("5")`.
- **Correct (Excel & FSA1):** `FALSE`. A text literal is text, even when it looks numeric; only a
  genuine number value is `ISNUMBER`-true.
- **`formulas` v1.3.4 output:** `TRUE` — it coerces the numeric-looking string to a number before the
  type test. Non-numeric text agrees (`=ISNUMBER("abc")`→`FALSE` in both), so the defect is specific to
  numeric-looking string literals.
- **Verdict:** `lib-gap` (reference defect). Excluded from pass/fail; FSA1 is correct.

## 15. `OFFSET` unimplemented / `INDIRECT` with a concatenated arg — `formulas` returns `#NAME?`

- **Cases:** `forging.xlsx` on the `Forge` sheet — `C1 =SUM(OFFSET($A$1,0,0,3,1))`,
  `C2 =SUM(OFFSET($A$1,0,0,COUNT($A$1:$A$4),1))`, `C3 =OFFSET($A$1,1,0)`, `C4 =INDIRECT("A"&2)`,
  `C5 =SUM(OFFSET($A$1,1,0,2,1))` (over `A1:A4 = {10,20,30,40}`).
- **Correct (Excel & FSA1):** `60`, `100`, `20`, `20`, `50` respectively — FSA1 source-rewrites
  each forger (ENG6) to its static reference (`SUM($A$1:$A$3)`, `SUM($A$1:$A$4)`, `A2`, `A2`,
  `SUM($A$2:$A$3)`) and evaluates it.
- **`formulas` v1.3.4 output:** `#NAME?` for all five. `OFFSET` is absent from the library's function
  table, so every OFFSET cell is `#NAME?`; and `INDIRECT` with a *concatenated* text argument
  (`"A"&2`) also returns `#NAME?`, though a plain-literal cross-sheet `INDIRECT("Data!B2")` DOES work
  and is graded and Matching (`forging.xlsx` `Forge!C6 = 77`).
- **Coverage instead:** FSA1's forging is pinned by the fsa1-model forge fitness tests
  (`workbook::tests::forge::*` — the dynamic OFFSET range, static SUM(OFFSET(...)), INDIRECT A1 /
  cross-sheet resolution, nested-forging / forger-arg-cycle / off-grid / over-large refusals, the
  two-pass==naive differential, and the zero-overhead gate) with hand-verified Excel values.
- **Verdict:** `lib-gap` (reference unsupported). Excluded from pass/fail; FSA1 is correct. Note the
  graded MATCH at `Forge!C6` proves the cross-sheet INDIRECT forge against the reference where the lib
  DOES support it.

## Deliberate FSA1 divergences on forging (outside ENG6, NOT lib-gaps)

`INDIRECT`/`OFFSET` now PARSE and FORGE (ENG6): a call with non-forging arguments is source-rewritten
to a static reference and graded in the parity corpus (`forging.xlsx`, gap #15 above for the OFFSET
cells the reference lib cannot compute). Three forging shapes remain **deliberate FSA1 divergences**
(SPEC ENG6), given no parity-corpus case and pinned instead by fsa1-model refusal tests: NESTED
forging (a forger whose own argument forges, e.g. `INDIRECT(INDIRECT(...))`) is a located `#REF!`
(restricted v1); a forger-arg CYCLE (an argument depending on the forger's own output) is a located
`#REF!`; and a forged OVER-LARGE range is a located `#NUM!` (the shared `MAX_RANGE_CELLS` bound, where
Excel would instead give `#REF!` for exceeding the grid). These fall outside the ENG6 parity surface
exactly like a reference cycle's `#REF!` or the no-dynamic-spill rule.
