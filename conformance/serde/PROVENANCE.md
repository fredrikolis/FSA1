<!-- Concern: the ECMA-376 derivation and external origin of `golden_numfmt.json` | Non-concern: the round-trip corpus's provenance, rendering, grading | IO: none -->
# Provenance — `golden_numfmt.json` (the numFmt render-equivalence external anchor)

`golden_numfmt.json` is the **external anchor** for the SER2 render-equivalence FORMAT dimension. The
value dimension of the serde oracle is already anchored to external truth by the `formulas` reference
solver; the *format* dimension must be anchored too, or two in-house renderers (fsa1-ast's numFmt
engine and `numfmt_render.py`) could vouch for each other while both diverge from real Excel. These
vectors are that independent third party.

## What it is

12 `(format_code, value) → expected display string` vectors, **≥1 per accepted numFmt category**
(fixed-decimal, thousands-grouped, percent, currency, accounting, date, time, datetime) and **two
negative-value cases** (accounting `-1234` → `($1,234.00)`; fixed `-3.5` → `-3.50`). The count `12` is
stated in the file header so a reviewer can confirm no category was dropped.

## How it was authored — NEVER FSA1-generated

Every `expected` string is **hand-derived from the published OOXML / ECMA-376 numFmt specification** —
the built-in numFmt id semantics (§18.8.30 `numFmt`, the built-in id → format-code table) and the
format-code grammar (digit placeholders `0`/`#`, the thousands separator `,`, the `%` scaler, section
splitting `positive;negative;…`, the date/time mask letters `y`/`m`/`d`/`h`/`s`, and the Excel 1900
serial↔civil map). Serial `44331` is `2021-05-15`; `0.5625` day-fraction is `13:30:00`. The negative
accounting section `($#,##0.00)` renders the value's **magnitude** with the section's own literal
parens, per the ECMA sign-section rule.

The vectors are **NOT** FSA1 output and **NOT** a live Excel / LibreOffice render (neither is
available in this environment, and using FSA1's own output as its own oracle would be circular).
This follows the same "the oracle is authored externally, never FSA1-generated" discipline the
formula conformance corpus already documents.

## How it is enforced — both renderers, any disagreement is a BUILD FAILURE

- **Rust (`cargo test`):** `conformance/tests/golden_numfmt.rs` reads this file and asserts
  `fsa1_ast::format_value(Number(value), format_code)` reproduces every `expected` string.
- **Python (`run.sh` / CI serde job):** `numfmt_render.py`'s self-test asserts `numfmt_render(code,
  value)` reproduces every `expected` string.

A single external vector set graded by both renderers means a consistent-but-wrong renderer cannot
pass. If a vector ever changes, it is because the ECMA-376 reading was corrected — never to chase a
FSA1 regression (an FSA1 divergence is an FSA1 bug to fix, not a golden to edit).
