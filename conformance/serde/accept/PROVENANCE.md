<!-- Concern: the SOURCE + honest provenance of the accept/ round-trip corpus — which in-repo corpus each General .xlsx was graduated from, and (plan 07 §8) which openpyxl author script + exact source formatCode each FORMATTED fixture carries, across the GRID7 accepted numFmt catalog on value literals AND formulas, so every file is SER2-round-trippable (RENDER-equivalently) by the skeleton | Non-concern: how the round-trip is graded (../roundtrip_oracle.py owns the SER2 value + FORMAT grading, fsa1-cli/tests/roundtrip.rs owns the reopened-workbook leg of SER2), rendering a numFmt (../numfmt_render.py owns it), and the refuse corpus (see ../refuse/PROVENANCE.md) | IO: none -->
# PROVENANCE — serde `accept/` corpus (in-scope round-trip fixtures)

The **in-scope** family of the SER2 round-trip conformance corpus. Every file round-trips
**RENDER-equivalently**: `fsa1-cli unpack --strict` accepts it, `pack` re-emits it, and `formulas`
(the ENG6 mainstream-spreadsheet proxy) recomputes the export to the same values as the source (SER2
value gate); for a formatted cell, the source and export numFmts render the same display string (SER2
FORMAT gate, GRID7); and re-unpacking that export reopens the same values, formulas, display formats
and resolved styles, whatever block shape the decomposition cuts them into. A figure crosses back as a native chart over the same
ranges (SER2 CHART gate). All are graded by `../roundtrip_oracle.py` (value + FORMAT + CHART) and
`fsa1-cli/tests/roundtrip.rs` (the reopened workbook).

That last leg was written down as **SER4 (re-import idempotence)** until `docs/cli-spec.md` withdrew
that id into SER2. What SER2 promises a reopened workbook is content, not a file tree, and the tree is
not the corpus's to freeze anyway: `pack` writes one `<cellXfs>` entry per look, so two distinct xf
indices that draw alike collapse into one, and the default decomposition cuts blocks by appearance
signature — a re-unpack may legitimately land on different blocks and different file names.

## Provenance is honest by construction

These fixtures are **NEVER FSA1-generated** (grading a codec against its own output is void). Each
was authored by a third-party writer (**openpyxl**) — the General family via
`../../xl-oracle/make_workbooks.py`, the formatted family via the committed `../make_accept.py` — or a
real spreadsheet application, and graduated here unchanged; the reference values are computed by
**`formulas`**, never by FSA1.

## General family (values / formulas, default formatting) — graduated

| File | Graduated from | Shape it covers |
|---|---|---|
| `pnl.xlsx` | `conformance/xl-oracle/corpus_workbooks/pnl.xlsx` (openpyxl) | SUM / subtraction / % / division over a two-column P&L, plus a cross-sheet `Summary` |
| `amortization.xlsx` | `conformance/xl-oracle/corpus_workbooks/amortization.xlsx` (openpyxl) | PMT + a per-period interest/principal/balance chain, IPMT/PPMT, a SUM total |
| `lookup.xlsx` | `conformance/xl-oracle/corpus_workbooks/lookup.xlsx` (openpyxl) | VLOOKUP / INDEX-MATCH / HLOOKUP / IF tiers / SUMIF over a price table |
| `forging.xlsx` | `conformance/xl-oracle/corpus_workbooks/forging.xlsx` (openpyxl) | OFFSET / INDIRECT reference-forging + a cross-sheet `Data` tab |
| `functions.xlsx` | `fsa1-ingest/tests/fixtures/functions.xlsx` | mixed value types + a lookup sheet |
| `smoke.xlsx` | `fsa1-ingest/tests/fixtures/smoke.xlsx` | the multi-sheet smoke shape (Sheet1 + Sheet2 cross-ref) |
| `blanks_repeats.xlsx` | `fsa1-ingest/tests/fixtures/blanks_repeats.xlsx` | blank cells + repeated values (gap handling) |

## Charted family (a figure crosses out as a native chart) — graduated

| File | Graduated from | What it exercises |
|---|---|---|
| `chart_bar_one_series.xlsx` | `conformance/presentation/fixtures/chart_bar_one_series.xlsx` (openpyxl `BarChart`) | a one-series bar chart over `Sheet1!A1:B4`, titled — the read leg makes it a `.json`, the write leg makes it a `<c:barChart>` again |
| `chart_line_two_series.xlsx` | `conformance/presentation/fixtures/chart_line_two_series.xlsx` (openpyxl `LineChart`) | two series, hence two layers, each binding its own rectangle and each written back as its own `<c:ser>` |

Both are openpyxl-authored, so FSA1 authors no byte it is graded against here either. They are the
only fixtures the **CHART gate** grades: `../roundtrip_oracle.py` reopens the PACKED file with
openpyxl and compares the chart class and every series' three references against the source's. A
reference is compared as the RANGE it addresses, since the `$` and the quotes around a sheet name are
Excel's spelling of one — openpyxl writes `'Sheet1'!B1` where FSA1 writes `Sheet1!$B$1`.

A charted workbook that does NOT round-trip stays in `../refuse/`, where `chart.xlsx` and
`drawing.xlsx` are: `xl/charts/` and `xl/drawings/` are `ALLOW` parts now, so what refuses is content
that yields no figure, never the part's presence.

## Formatted family (GRID7 typed content) — authored by `../make_accept.py`

Each single-category fixture carries a formatted **value LITERAL** cell AND a formatted **FORMULA**
cell (exercising ENG1 — the formula computes on the pure value, the format is presentation only),
except accounting (FORMULA-ONLY, §4.3). Every literal value is **DISPLAY-EXACT** under its format, the
precondition `unpack --strict` requires (§4.1). The **source `formatCode`** each cell carries is
recorded so a reviewer can confirm the render-equivalence claim (source code vs FSA1's canonical
export code, both rendered by `../numfmt_render.py`).

| File | Cells (source `formatCode`) | Category | Render (value → display) |
|---|---|---|---|
| `fmt_date.xlsx` | A1 literal, A2 `=A1+1` — both `m/d/yyyy` (custom) | date | `44331`→`5/15/2021`, `44332`→`5/16/2021` |
| `fmt_datetime.xlsx` | A1 literal, A2 `=A1+1` — both `m/d/yy h:mm` (built-in 22) | datetime | `44331.5625`→`5/15/21 13:30` |
| `fmt_time.xlsx` | A1 literal, A2 `=A1-0.25` — both `h:mm:ss` (built-in 21) | time | `0.5625`→`13:30:00`, `0.3125`→`7:30:00` |
| `fmt_percent.xlsx` | A1 literal, A2 `=A1/2` — both `0.00%` (built-in 10) | percent | `0.125`→`12.50%`, `0.0625`→`6.25%` |
| `fmt_currency.xlsx` | A1 literal, A2 `=A1*2` — both `$#,##0.00` (custom, quote-free) | currency | `1234.5`→`$1,234.50`, `2469`→`$2,469.00` |
| `fmt_thousands.xlsx` | A1 literal, A2 `=A1+1000` — both `#,##0.00` (built-in 4) | thousands-grouped | `1234`→`1,234.00`, `2234`→`2,234.00` |
| `fmt_fixed.xlsx` | A1 literal `0.0000` (custom), A2 `=A1*2` `0.00` (built-in 2) | fixed-decimal | `12.5`→`12.5000`, `25`→`25.00` |
| `fmt_accounting.xlsx` | A2 `=A1-2234` — `$#,##0.00;($#,##0.00)` (custom, FORMULA-ONLY) | accounting (NEGATIVE) | `-1234`→`($1,234.00)` |

### Render-equivalence stressors (source `formatCode` ≠ FSA1's canonical export code)

These deliberately make the source code differ from FSA1's export code so the FORMAT gate proves
**rendered** equivalence, not code-string equality:

| File | Source `formatCode` | FSA1's export code | Both render |
|---|---|---|---|
| `stress_color_date.xlsx` | `[Blue]m/d/yyyy` (a COLOR bracket) | `m/d/yyyy` (color dropped, §4.2) | `5/15/2021` |
| `stress_builtin_date.xlsx` | `mm-dd-yy` (built-in id 14) | `mm-dd-yy` (built-in id 14) | `05-15-21` |
| the formula cells above | built-in ids 22 / 21 / 10 / 4 / 2 | the same built-in id | (per row) |

### A DECIDED scope note — the accounting padding stressor is a golden vector, not a fixture

Plan §8 names an "accounting formula (source `_(`/`*` padding vs FSA1's paren canonical)" as a
render-equivalence stressor. The phase-1 import classifier (`fsa1_model::Format::from_code`) accepts
the canonical **two-section** `$#,##0.00;($#,##0.00)` but **refuses** openpyxl's `_(`/`*`-padded
built-in accounting (four padded sections) — so a padded-accounting cell is a located CORE2 refusal, not
an importable accept fixture. The padded-vs-paren render-equivalence is therefore anchored where it can
be exercised: `../numfmt_render.py` normalizes `_x`/`*x` padding away, and the golden vector
`$#,##0.00;($#,##0.00)`, `-1234` → `($1,234.00)` (id 9 in `../golden_numfmt.json`) pins the paren
negative section against ECMA-376. The importable `fmt_accounting.xlsx` uses the canonical two-section
code.

## Scope boundary — GRID7 typed content is now IN scope (a DECIDED reversal of the plan-06 boundary)

Plan 06's `accept/` corpus was **DATE-FREE / General-format only** because GRID7 was unmodeled. Plan 07
closes GRID7: a date is a date (an ISO value + a `~<code>` marker), and the accepted numFmt catalog
(fixed / thousands / percent / currency / accounting / date / time / datetime) round-trips on value
literals AND formulas. The formatted family above exercises exactly that. The **exotic tail**
(conditional switches, digit masks, unknown custom codes) remains out of scope — refused, not modeled —
and lives in `../refuse/` (see `../refuse/PROVENANCE.md`).

## Declared blind spots

- **The `CellStyle` leg of the reopen comparison is presently VACUOUS here.** No fixture declares any
  presentation: `unpack --strict` over the General and formatted `.xlsx` writes 25 range files and **not one** carries an
  `@scope` block, so `Workbook::cell_style` returns `CellStyle::default()` at every covered coordinate
  and that leg separates only *covered* from *gap*. The narrowing is real but latent: the whole-file
  text comparison it replaced diffed an `@scope` block verbatim, whitespace and all, where a resolved
  `CellStyle` compares only what the block means. Add a fixture that declares one and the two stop being
  interchangeable. Presentation round-trips are graded where a corpus carries them:
  `conformance/presentation/`.

## Integrity

`../MANIFEST.sha256` fingerprints every `.xlsx` here (and in `../refuse/`); a `sha256sum -c` check in
`../` (wired into CI's `gates` job) fails on any silent byte change. The `.xlsx` are Git-LFS objects
(`*.xlsx` under `.gitattributes`), smudged by CI's `lfs: true` checkout. Regenerate the formatted
family: `../.venv/bin/python ../make_accept.py` (the venv is provisioned by `../run.sh`).
