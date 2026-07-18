# Concern: AUTHOR the committed .xlsx test corpus for charlie-ingest — emit a small set of real Excel (OOXML) spreadsheets (via openpyxl) MIRRORING the .ods corpus: every value type, a SUM/formula chain, a cross-sheet reference, date cells, blanks, and VLOOKUP/IF — so the same import->eval assertions hold for both formats. An xlsx formula is stored in Excel-A1 already (no `of:=`/`[.A1]`), and an openpyxl-written formula carries NO cached value (charlie re-evaluates), so these files exercise the reader's format-blind path and translate's near-noop-for-xlsx behaviour | Non-concern: the Rust importer/translation under test (src/**) and running the tests (the .xlsx artifacts this writes are committed; the venv that runs this is gitignored); the .ods corpus (make_fixtures.py owns that) | IO: () -> writes tests/fixtures/*.xlsx next to this script's parent crate
"""Regenerate the committed .xlsx fixture corpus. Run from a venv with openpyxl installed:

    python3 -m venv .fixture-venv && .fixture-venv/bin/pip install openpyxl
    .fixture-venv/bin/python scripts/make_xlsx_fixtures.py

The generated .xlsx files ARE committed (small), so the Rust tests need no python/openpyxl. These
mirror the .ods fixtures (make_fixtures.py) cell-for-cell so tests/import_xlsx.rs and the .ods
integration test assert the same Excel-correct values through the one format-blind engine.
"""

import datetime
import os

import openpyxl

FIXTURES = os.path.join(os.path.dirname(__file__), "..", "tests", "fixtures")


def write(wb, name):
    os.makedirs(FIXTURES, exist_ok=True)
    path = os.path.join(FIXTURES, name)
    wb.save(path)
    print("wrote", os.path.normpath(path))


def make_smoke():
    """The batch smoke + cross-sheet fixture:
    Sheet1: A1=10, A2=20, A3=SUM(A1:A2)=30, B1=A3*2=60
    Sheet2: A1==Sheet1!A3=30
    """
    wb = openpyxl.Workbook()
    s1 = wb.active
    s1.title = "Sheet1"
    s1["A1"] = 10
    s1["A2"] = 20
    s1["A3"] = "=SUM(A1:A2)"
    s1["B1"] = "=A3*2"
    s2 = wb.create_sheet("Sheet2")
    s2["A1"] = "=Sheet1!A3"
    write(wb, "smoke.xlsx")


def make_literals():
    """Every literal value type + date cells + an interior/trailing blank."""
    wb = openpyxl.Workbook()
    t = wb.active
    t.title = "Data"
    for col, head in enumerate(["Name", "Qty", "Active", "When"], start=1):
        t.cell(row=1, column=col, value=head)
    # Widget row: text, number, bool, date. 2024-01-15 -> Excel serial 45306.
    t["A2"] = "Widget"
    t["B2"] = 42
    t["C2"] = True
    t["D2"] = datetime.datetime(2024, 1, 15)
    # Gadget row: a negative float, an interior blank (no Active), another date (2024-06-30 -> 45473).
    t["A3"] = "Gadget"
    t["B3"] = -3.5
    t["D3"] = datetime.datetime(2024, 6, 30)
    write(wb, "literals.xlsx")


def make_functions():
    """VLOOKUP + IF over a small table (Excel-A1 args: ',' separators, quoted string, bare FALSE)."""
    wb = openpyxl.Workbook()
    t = wb.active
    t.title = "Lookup"
    # Columns A,B are the table; column D holds the function cells (C left blank).
    t["A1"] = "apple"
    t["B1"] = 1
    t["D1"] = '=VLOOKUP("banana",A1:B3,2,FALSE)'
    t["A2"] = "banana"
    t["B2"] = 2
    t["D2"] = '=IF(B1>0,"pos","neg")'
    t["A3"] = "cherry"
    t["B3"] = 3
    write(wb, "functions.xlsx")


def make_blanks_repeats():
    """A sparse sheet: leading value, interior blanks, a trailing value, a blank row. Used range A1:D3."""
    wb = openpyxl.Workbook()
    t = wb.active
    t.title = "Sparse"
    # A1=1, B1/C1 blank, D1=4; row 2 fully blank; A3=7.
    t["A1"] = 1
    t["D1"] = 4
    t["A3"] = 7
    write(wb, "blanks_repeats.xlsx")


if __name__ == "__main__":
    make_smoke()
    make_literals()
    make_functions()
    make_blanks_repeats()
