# Concern: authors the formatted accept/ fixtures — one category per file, plus stressors | Non-concern: computing or diffing values, the refuse/ probes | IO: () -> accept/fmt_*.xlsx + stress_*.xlsx
"""Regenerate the FORMATTED accept/ fixtures for the serde round-trip conformance corpus (plan 07 §8).

Run via the oracle venv (created by run.sh):

    conformance/serde/.venv/bin/python conformance/serde/make_accept.py

The generated .xlsx ARE committed (small, LFS-tracked); the venv is gitignored. Provenance is honest
by construction: openpyxl (a third-party writer) authors the bytes, never FSA1. Each fixture is a
single accepted-catalog category so a grader failure names the category. The General (unformatted)
accept fixtures were graduated separately from xl-oracle (see accept/PROVENANCE.md) and are NOT
re-authored here.

Design notes anchored to the actual phase-1..3 implementation:
  * A date/time LITERAL is authored as a bare SERIAL number under a date/time numFmt (not an openpyxl
    ``datetime``), so calamine hands FSA1 the exact serial the catalog re-spells as ``<ISO>~<code>``.
  * Every number-family LITERAL value is DISPLAY-EXACT under its format (formatting to the format's
    decimals and re-parsing yields the identical f64), the precondition ``unpack --strict`` requires
    (§4.1); a sub-display-precision literal would be a located refusal, not an accept fixture.
  * ACCOUNTING is FORMULA-ONLY (§4.3): a value literal in the currency family always maps to Currency,
    and a negative accounting value cannot be recovered from a parenthesized display literal. Its
    source numFmt is the canonical two-section ``$#,##0.00;($#,##0.00)`` (which the phase-1 classifier
    accepts) rather than openpyxl's ``_(``/``*``-padded built-in accounting (which the classifier
    refuses); the padded-vs-paren stressor is therefore realized in the golden vectors + numfmt_render,
    not as an importable fixture (recorded in accept/PROVENANCE.md).
"""

import os

import openpyxl

HERE = os.path.dirname(__file__)
OUT = os.path.join(HERE, "accept")

# The Excel 1900 serial for 2021-05-15 and the 13:30:00 day-fraction — the corpus-wide date/time anchors
# (identical to golden_numfmt.json's ECMA-376 vectors), so a reviewer can cross-read the two.
DATE_SERIAL = 44331          # 2021-05-15
TIME_FRAC = 0.5625           # 13:30:00
DATETIME_SERIAL = 44331.5625  # 2021-05-15 13:30:00


def _save(wb, name):
    os.makedirs(OUT, exist_ok=True)
    path = os.path.join(OUT, name)
    wb.save(path)
    print(f"wrote accept/{name}")


def _wb(title="S"):
    wb = openpyxl.Workbook()
    wb.active.title = title
    return wb, wb.active


def make_date():
    """A date LITERAL (serial under ``m/d/yyyy``) + a date FORMULA (``=A1+1`` under ``m/d/yyyy``). The
    formula computes on the pure serial (ENG1) and the marker is presentation only (ENG5)."""
    wb, s = _wb()
    s["A1"] = DATE_SERIAL
    s["A1"].number_format = "m/d/yyyy"          # -> 5/15/2021
    s["A2"] = "=A1+1"
    s["A2"].number_format = "m/d/yyyy"          # -> 5/16/2021
    _save(wb, "fmt_date.xlsx")


def make_datetime():
    """A datetime LITERAL + FORMULA under the built-in ``m/d/yy h:mm`` (numFmtId 22)."""
    wb, s = _wb()
    s["A1"] = DATETIME_SERIAL
    s["A1"].number_format = "m/d/yy h:mm"       # -> 5/15/21 13:30
    s["A2"] = "=A1+1"
    s["A2"].number_format = "m/d/yy h:mm"       # -> 5/16/21 13:30
    _save(wb, "fmt_datetime.xlsx")


def make_time():
    """A time LITERAL (day-fraction under ``h:mm:ss``) + FORMULA (``=A1-0.25`` -> 07:30:00)."""
    wb, s = _wb()
    s["A1"] = TIME_FRAC
    s["A1"].number_format = "h:mm:ss"           # -> 13:30:00
    s["A2"] = "=A1-0.25"
    s["A2"].number_format = "h:mm:ss"           # -> 07:30:00
    _save(wb, "fmt_time.xlsx")


def make_percent():
    """A percent LITERAL + FORMULA under the built-in ``0.00%`` (numFmtId 10) — a formatted formula
    whose source numFmt is a BUILT-IN id (a §8 render-equivalence stressor)."""
    wb, s = _wb()
    s["A1"] = 0.125
    s["A1"].number_format = "0.00%"             # -> 12.50%
    s["A2"] = "=A1/2"
    s["A2"].number_format = "0.00%"             # -> 6.25%
    _save(wb, "fmt_percent.xlsx")


def make_currency():
    """A currency LITERAL + FORMULA under a custom quote-free ``$#,##0.00`` (numFmtId >=164)."""
    wb, s = _wb()
    s["A1"] = 1234.5
    s["A1"].number_format = "$#,##0.00"         # -> $1,234.50
    s["A2"] = "=A1*2"
    s["A2"].number_format = "$#,##0.00"         # -> $2,469.00
    _save(wb, "fmt_currency.xlsx")


def make_thousands():
    """A thousands-grouped LITERAL + FORMULA under the built-in ``#,##0.00`` (numFmtId 4)."""
    wb, s = _wb()
    s["A1"] = 1234
    s["A1"].number_format = "#,##0.00"          # -> 1,234.00
    s["A2"] = "=A1+1000"
    s["A2"].number_format = "#,##0.00"          # -> 2,234.00
    _save(wb, "fmt_thousands.xlsx")


def make_fixed():
    """A fixed-decimal LITERAL (custom ``0.0000``) + FORMULA (built-in ``0.00``, numFmtId 2)."""
    wb, s = _wb()
    s["A1"] = 12.5
    s["A1"].number_format = "0.0000"            # -> 12.5000
    s["A2"] = "=A1*2"
    s["A2"].number_format = "0.00"              # -> 25.00
    _save(wb, "fmt_fixed.xlsx")


def make_accounting():
    """ACCOUNTING is FORMULA-ONLY (§4.3). A negative-valued formula under the canonical two-section
    ``$#,##0.00;($#,##0.00)`` so the parenthesized NEGATIVE section renders (``($1,234.00)``)."""
    wb, s = _wb()
    s["A1"] = 1000
    s["A2"] = "=A1-2234"                        # -> -1234
    s["A2"].number_format = "$#,##0.00;($#,##0.00)"  # -> ($1,234.00)
    _save(wb, "fmt_accounting.xlsx")


def make_stress_color_date():
    """RENDER-EQUIVALENCE STRESSOR: a COLOR-prefixed date ``[Blue]m/d/yyyy``. FSA1's import strips
    the color bracket (cosmetic loss, §4.2) and re-emits the quote-free ``m/d/yyyy``; source and export
    render the SAME ``5/15/2021`` — a divergence in code-string that must NOT be a SER2 divergence."""
    wb, s = _wb()
    s["A1"] = DATE_SERIAL
    s["A1"].number_format = "[Blue]m/d/yyyy"    # source carries the color; FSA1 drops it
    _save(wb, "stress_color_date.xlsx")


def make_stress_builtin_date():
    """RENDER-EQUIVALENCE STRESSOR: a BUILT-IN-id date (``mm-dd-yy``, numFmtId 14). openpyxl resolves
    the id to its code; FSA1 re-emits the same built-in id. Both render ``05-15-21``."""
    wb, s = _wb()
    s["A1"] = DATE_SERIAL
    s["A1"].number_format = "mm-dd-yy"          # built-in numFmtId 14
    _save(wb, "stress_builtin_date.xlsx")


def main():
    make_date()
    make_datetime()
    make_time()
    make_percent()
    make_currency()
    make_thousands()
    make_fixed()
    make_accounting()
    make_stress_color_date()
    make_stress_builtin_date()


if __name__ == "__main__":
    main()
