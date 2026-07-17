<!-- Concern: the SOURCE + tamper-fingerprint of the formula-conformance corpus — how its `.fixtures` value probes (context + formula + EXPECTED) were authored, and the ORACLE-INPUT PURITY contract that makes their EXPECTED values authoritative truth the gate and agents never question | Non-concern: the parse/eval grading verdict logic (the `conformance` runner owns that) and the charlie-DERIVED `facts-snapshot.tsv` (a runner output, not an oracle input — deliberately not fingerprinted) | IO: none -->
# Formula-conformance corpus — provenance and the oracle contract

This directory is the **formula-level** conformance corpus for `charlie-ast`. Each fixture is a
value probe: an input **context** (named cells with literal values), a **formula** string, and the
**EXPECTED** value the formula must evaluate to. The `conformance` runner grades charlie by parsing
and evaluating the formula through the shipping `charlie_ast::parse` + `charlie_ast::eval` path,
against a deterministic stub `Resolver` built from the context, and comparing the produced `Value`
to the expected value **bit-exactly** (`Value`'s own `Eq`: `-0.0 ≠ 0.0`, `NaN == NaN`).

## The oracle contract (ORACLE-INPUT PURITY)

**The corpus is the oracle. Its EXPECTED values are truth — authoritative, never questioned by the
gate or an agent.** A divergence is always a defect in charlie, never in the corpus. Correcting an
expected value is a deliberate maintainer act at the corpus-production layer, never an agent
rationalizing a red fixture (a Diverge means *fix charlie*, never *edit the expectation to match
it*).

**Every EXPECTED value was authored INDEPENDENTLY of charlie** — derived by hand from the documented
Excel/spreadsheet semantics that `~/.knowledge-base/coding-standards/ast-standards.md` and
`docs/architecture.md` §3–§4 specify, and cross-checked against Excel's published behavior. **No
expected value was produced by running charlie** (that would make the engine its own oracle —
circular; the thing under test can never be its own judge). This purity is an INPUT-side property of
how the corpus was authored; the `MANIFEST.sha256` fingerprint below is its tamper-evidence.

The interesting behaviors the seed pins, all from Excel semantics:

- **Direct-vs-in-range coercion asymmetry** — a boolean / numeric-text datum coerces when passed
  *directly* as an argument (`SUM(1,TRUE,"2") = 4`) but is ignored *inside* a range; `COUNT` never
  yields an error from its data.
- **Laziness** — `IF(TRUE,1,1/0) = 1` (the unreached `1/0` never surfaces); `IFERROR` evaluates its
  fallback only on an error.
- **Error propagation** — leftmost-first (`#REF!+#DIV/0! = #REF!`); `/0 → #DIV/0!`; a complex or
  overflowing power → `#NUM!`; a non-numeric text in an arithmetic position → `#VALUE!`.
- **The operator ladder** — unary minus binds tighter than `^`; `^` is left-associative
  (`2^3^2 = 64`); `&` concatenation stringifies operands; comparisons are cross-type ranked
  (Number < Text < Bool) and text-equality is case-insensitive.
- **ROUND** — half away from zero (`2.5 → 3`, `-2.5 → -3`), negative digits to the left of the point.
- **Criteria mini-language** (`criteria.fixtures`, the `*IF(S)` family) — comparison operators
  (`">10"`, `"<=5"`, `"<>x"`); case-insensitive text match with wildcards (`*` any run, `?` one
  char, `~` escape); `">"&ref` CONCATENATED criteria (the `&` folds to a string before the criteria
  parser runs). The range-conformance call is charlie's own: every criteria range and the value range
  must share one shape, and a mismatch is a STATIC `#VALUE!` (not Excel's legacy reshape-from-corner).
  Result-shape calls: `AVERAGEIF(S)` over no match is `#DIV/0!`; `MINIFS`/`MAXIFS` over no match is
  `0`; `COUNTIF` counts a matching cell of any type while the summing forms take only numbers.
  A text/wildcard criterion matches TEXT cells ONLY: over a MIXED range a number/bool cell is NEVER
  coerced to its text form, so `COUNTIF([apple,5,10,pear],"*") = 2` and `COUNTIF([15,25,"1x"],"1*")
  = 1` (a number/bool only satisfies `<>` against a text pattern, being "not equal" to it).
- **Canonical zero.** Excel displays every zero as `0`; since `Value`'s `Eq` is bit-exact
  (`-0.0 ≠ 0.0`), a `0`-valued aggregate is authored as `0` and charlie canonicalizes a computed
  `-0.0` (e.g. an empty `SUM`/`SUMPRODUCT`, `[].sum() == -0.0`) to `+0.0` so it does not spuriously
  Diverge.
- **Text batch** (`text.fixtures`, the `CONCAT TEXTJOIN LEFT RIGHT MID LEN FIND SEARCH SUBSTITUTE
  REPLACE TRIM UPPER LOWER TEXT` family). 1-BASED character positions with edge-CLAMPING
  (`LEFT`/`RIGHT`/`MID`); a negative count/start is `#VALUE!`. `FIND` is case-SENSITIVE, `SEARCH` is
  case-INSENSITIVE with `?`/`*` wildcards (`~` escapes); both return the match's START and miss with
  `#VALUE!`, and an empty needle returns `start_num`. `SUBSTITUTE` replaces the Nth or ALL
  non-overlapping occurrences (an empty `old_text` is a no-op; `instance_num < 1` is `#VALUE!`);
  `REPLACE` is positional. `TEXT` renders a SUPPORTED format subset only (the subset charlie-ast's
  `func::text::classify_format` accepts) —
  general / fixed `0.00` / thousands `#,##0` / percent `0%` / date `yyyy-mm-dd`; a value a numeric
  format cannot coerce is `#VALUE!`, an error value propagates, and the **1900 date system WITH
  Excel's leap-year bug** is the epoch decision (serial 61 = `1900-03-01`). A **non-literal** (computed)
  format is ACCEPTED at parse and deferred to eval (`=TEXT(A1, B1)` with `B1="0.00"` computes, exactly
  as Excel does — accept-under-uncertainty, never a parse false-reject); only an unsupported *literal*
  is refused. The EXPECTED values were computed by a standalone python3 model (its authoring-run output
  is pasted at the head of `text.fixtures`) and hand-checked. The TEXT unsupported-*literal*-format
  refusal is a PARSE verdict (`unsupported-format`), so it is on charlie-ast's `diag`/parser test
  surface, not a value fixture.
- **Date/time batch** (`date.fixtures`, the `DATE YEAR MONTH DAY EDATE DATEDIF TODAY NOW` family).
  Every value is an Excel **1900-system date serial** WITH the leap-year bug replicated (serial 60 =
  the fictional `1900-02-29`; `44927` = `2023-01-01`) — the same epoch decision as `TEXT`'s date
  render (charlie-ast's `func::text::serial_to_ymd`). `DATE` truncates its args and NORMALIZES out-of-range
  month/day (with the year `0..=1899` `+1900` fold); `YEAR`/`MONTH`/`DAY` `floor` the serial (`YEAR(60)
  =1900`, `DAY(60)=29`); `EDATE` CLAMPS the day to the target month's end; `DATEDIF` covers
  `Y`/`M`/`D` + `MD`/`YM`/`YD` (case-folded unit; `start>end` and an unknown unit are `#NUM!`).
  `TODAY`/`NOW` are **VOLATILE**: they read the resolver's INJECTABLE clock (`Resolver::now_serial`),
  PINNED in the conformance stub to `PINNED_NOW_SERIAL` = `44927.5` (`2023-01-01T12:00:00`) so the
  fixtures are reproducible (production's default reads system time). The EXPECTED values were
  computed by a standalone python3 model (proleptic-Gregorian via `date.toordinal()`, Excel serial =
  `toordinal − 693594` for dates on/after `1900-03-01`, cross-checked against the well-known anchor
  `44927` = `2023-01-01`) whose authoring-run output is pasted at the head of `date.fixtures`, and
  hand-checked — NEVER by running charlie.

- **Lookup & reference batch** (`lookup.fixtures`, the `XLOOKUP INDEX MATCH VLOOKUP CHOOSE ROW
  COLUMN` family). The EXPECTED values were computed by a standalone python3 oracle that models Excel
  lookup semantics FROM SCRATCH — cross-type ordering `Number < Text < Bool` with case-insensitive
  text, the sorted-vector approximate-match rule, and the Excel `* ? ~` wildcard grammar — never by
  running charlie; its authoring-run output is pasted at the head of `lookup.fixtures`. The load-
  bearing semantics the fixtures pin: **`MATCH`** modes `1`/`0`/`-1` (approximate ascending — largest
  key `<=` needle; exact first-hit with wildcards on a text needle; approximate descending — smallest
  key `>=` needle), a 2-D array or a miss being `#N/A`; **`VLOOKUP`** approximate **by default** (the
  classic must-be-sorted bug — a needle between two keys lands on the lower key, `2.5 -> "two"`) and
  exact only when the 4th arg is `FALSE` (`2.5 -> #N/A`), with `col_index` past the width `#REF!`;
  **`XLOOKUP`** exact **by default** plus an `if_not_found` value returned on a miss and `match_mode`
  `1`/`-1` for the next-larger/next-smaller key; **`INDEX`** 1-based with `r=0`/`c=0` selecting a whole
  column/row (an out-of-bounds index `#REF!`, a blank single cell reading as `0`); **`CHOOSE`** a
  1-based selector (out-of-range `#VALUE!`); **`ROW`/`COLUMN`** the 1-based coordinate of a reference
  NODE (top-left of a range; a non-reference argument `#VALUE!`). **`INDIRECT`/`OFFSET`** are reserved
  reference-returning functions REFUSED at parse (the located `reserved-ref-function` DiagCode), so —
  like every parse-refusal — they are on charlie-ast's `diag`/parser test surface, not a value fixture.
- **Information batch** (`info.fixtures`, the `ISBLANK ISNUMBER ISTEXT ISERROR NA TYPE` family — the
  ONE error-TRANSPARENT group in the engine). The EXPECTED values were computed by a standalone python3
  oracle (`info_oracle.py`, pasted at the head of `info.fixtures`) that models the family FROM SCRATCH:
  each predicate INSPECTS its operand's kind and REPORTS on it — it does NOT route the operand through
  the scalarize/coerce path every OTHER function uses, so an error/blank/array operand is examined, not
  propagated. The load-bearing semantics the fixtures pin: **`ISERROR`** catches ANY error including
  `#N/A` (`ISERROR(1/0)=TRUE`, never the `#DIV/0!` itself) — the defining transparency case; **`TYPE`**
  the Excel codes `1/2/4/16/64` (number/text/logical/error/array) with an empty cell reporting `1` and
  an error operand reporting `16` (transparent, `TYPE(NA())=16` not `#N/A`); **`ISBLANK`** TRUE only for
  a genuinely empty cell — an empty string `""` is a text value, NOT blank; **`ISNUMBER`/`ISTEXT`** that
  do not coerce (numeric-looking text `"42"` is not a number); **`NA`** the sole error-PRODUCING member,
  minting `#N/A` on demand (arity 0) — and the edge that `NA()+1` PROPAGATES `#N/A` because arithmetic,
  not an info fn, is reading it (contrast `ISERROR(NA())=TRUE`). The array operand is expressed as a
  range `A1:B1` (v1 has no `{..}` array-constant node); `TYPE(A1:B1)=64` and the IS-predicates return
  FALSE on it — inspected AS an array, never scalarized into a propagated `#VALUE!`.

- **Financial batch** (`finance.fixtures`, the `PMT NPV IRR` family). The EXPECTED values were computed
  by a standalone python3 hand-formula oracle (`finance_oracle.py`, its authoring print pasted at the
  head of `finance.fixtures`) derived FROM the documented cash-flow-time-value math, NOT from charlie.
  Two bit-exactness disciplines make the irrational-capable results reproducible under the corpus's
  bit-exact (`to_bits`) equality: (1) the oracle discounts/compounds with the SAME integer-power
  **multiply loop** the engine's `pow_int` uses — deliberately not libm `pow`/`powi`, whose rounding
  can differ — so a given f64 op sequence is identical on both sides; (2) the fixtures that CAN be
  chosen exact are (`PMT(0.5,2,…)` uses `1.5^2 = 2.25`, a dyadic-exact power; `NPV(1,…)` uses `rate=1`
  so every `(1+rate)^i = 2^i` is exact), landing on exact rationals (`90`, `110`, `60`, `137.5`,
  `-281.25`). The semantics pinned: **`PMT`** is Excel's sign convention (money out negative) solving
  the annuity balance equation, degenerating to the LINEAR `-(pv+fv)/nper` at `rate==0`, and returning
  `#DIV/0!` on a zero denominator (`nper==0`); **`NPV`** discounts `value1` from period **one**
  (`value1/(1+rate)^1` — the classic "NPV starts one period out" rule), flattening ranges row-major and
  propagating an error value; **`IRR`** is a Newton root-find (hard-capped) with a bounded bisection
  fallback and a final `#NUM!` — the convergent fixture's literal (`0.08896339469334992`) is the
  hand-Newton f64 from guess `0.1`, cross-checked against numpy's polynomial roots
  (`np.roots = 0.088963394693350…`) to ~`1e-12`, and the all-positive fixture pins the no-sign-change
  `#NUM!` (the honest "no real IRR" answer, reached WITHOUT any hang because both iteration loops carry
  hard integer caps).

## What is NOT here (and why)

- **charlie-produced anything.** `facts-snapshot.tsv` (the backslide anchor) IS charlie-derived —
  it records what charlie currently evaluates — so it is **not** an oracle input and is **not**
  fingerprinted here. It is the ratchet's memory, not ground truth.
- **Parse-refusal probes.** This corpus grades VALUES. A formula that charlie refuses to parse
  (e.g. a bare defined-name, or a 3D multi-sheet range `Sheet1!A1:Sheet2!B2`) grades as a Diverge
  here (a value was expected, a refusal is not one); the located-refusal contract is charlie-ast's
  own `diag`/parser test surface, a different axis. (A *single-sheet* cross-sheet reference such as
  `Data!A1` now parses and evaluates — see `crosssheet.fixtures`.)

## How the seed was produced, and how it grows (the W3b grind)

The seed covers the foundational functions implemented in W3 (`SUM AVERAGE COUNT · IF IFERROR AND
OR · ABS ROUND`) plus the operator and error-propagation core, and the W3b grind has since extended
it across the criteria, math, stats, logical, and **text** (`text.fixtures`) batches. The function
grind EXTENDS this per function: add fixtures (context + formula + hand-derived expected), regenerate
the anchor with `cargo run -p conformance -- resnapshot`, and commit. The coverage ratchet (`modeled / target=70`)
counts a function as *modeled* only once it is in the live registry AND has a Matching fixture, so
growth is monotonic under the backslide guard.

## Fixture file grammar (line-based)

```
[fixture-name]           # unique within the file; the key is "<file-stem>/<name>"
funcs: SUM, AVERAGE      # functions exercised (for the coverage ratchet; optional)
cell A1: 1               # zero or more context cells; canonical A1 only (no $/lowercase/leading-zero)
cell Data!A1: 42         # optionally sheet-qualified (`Sheet!A1`) for a cross-sheet reference
cell A2: "text"          # literals: number | "text" | TRUE/FALSE | #ERR! | <blank> | {r,c;r,c}
formula: =SUM(A1:A2)     # required; leading = optional
expect: 3                # required; the EXTERNALLY-derived oracle value
```

## Integrity

`MANIFEST.sha256` fingerprints every `.fixtures` file and this `PROVENANCE.md`. Verify with
`sha256sum -c MANIFEST.sha256` (run from this directory), or via the gated
`conformance/tests/corpus_integrity.rs`, which recomputes each digest with the crate's vendored
SHA-256 and fails loudly on any tamper or on a `.fixtures` file missing from the manifest. After a
DELIBERATE corpus edit, regenerate the manifest (see the header comment in `MANIFEST.sha256`).
