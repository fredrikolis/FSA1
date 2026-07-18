# Concern: AUTHOR the committed .ods test corpus for charlie-ingest — emit a small set of real OpenDocument spreadsheets (via odfpy) covering every value type, a SUM/formula chain, a cross-sheet reference, a date cell, and blank/repeated cells, each formula cell carrying BOTH its OpenFormula `of:=` text AND a cached computed value so calamine can read values and formulas without a live recalc engine | Non-concern: the Rust importer/translation under test (src/**) and running the tests (the .ods artifacts this writes are committed; the venv that runs this is gitignored) | IO: () -> writes tests/fixtures/*.ods next to this script's parent crate
"""Regenerate the committed .ods fixture corpus. Run from a venv with odfpy installed:

    python3 -m venv .fixture-venv && .fixture-venv/bin/pip install odfpy
    .fixture-venv/bin/python scripts/make_fixtures.py

The generated .ods files ARE committed (small), so the Rust tests need no python/odfpy.
"""

import os
from odf.opendocument import OpenDocumentSpreadsheet
from odf.table import Table, TableRow, TableCell
from odf.text import P

FIXTURES = os.path.join(os.path.dirname(__file__), "..", "tests", "fixtures")


def num(v):
    """A number literal cell."""
    c = TableCell(valuetype="float", value=repr(v))
    c.addElement(P(text=str(v)))
    return c


def text(s):
    """A text literal cell."""
    c = TableCell(valuetype="string")
    c.addElement(P(text=s))
    return c


def boolean(b):
    """A boolean literal cell."""
    c = TableCell(valuetype="boolean", booleanvalue="true" if b else "false")
    c.addElement(P(text="TRUE" if b else "FALSE"))
    return c


def date(iso, display):
    """A date-typed cell (ODS stores an ISO date; charlie maps it to an Excel serial)."""
    c = TableCell(valuetype="date", datevalue=iso)
    c.addElement(P(text=display))
    return c


def blank(repeat=1):
    """One (or `repeat`) empty cell(s) — exercises number-columns-repeated on read."""
    if repeat == 1:
        return TableCell()
    return TableCell(numbercolumnsrepeated=str(repeat))


def formula_num(of, cached):
    """A formula cell with its OpenFormula text and a cached float result."""
    c = TableCell(formula=of, valuetype="float", value=repr(cached))
    c.addElement(P(text=str(cached)))
    return c


def formula_text(of, cached):
    """A formula cell with its OpenFormula text and a cached string result."""
    c = TableCell(formula=of, valuetype="string")
    c.addElement(P(text=cached))
    return c


def row(cells):
    tr = TableRow()
    for c in cells:
        tr.addElement(c)
    return tr


def write(doc, name):
    os.makedirs(FIXTURES, exist_ok=True)
    path = os.path.join(FIXTURES, name)
    doc.save(path)
    print("wrote", os.path.normpath(path))


def make_smoke():
    """The batch smoke + cross-sheet fixture:
    Sheet1: A1=10, A2=20, A3=SUM(A1:A2)=30, B1=A3*2=60
    Sheet2: A1==Sheet1.A3=30
    """
    doc = OpenDocumentSpreadsheet()
    s1 = Table(name="Sheet1")
    s1.addElement(row([num(10), formula_num("of:=[.A3]*2", 60)]))       # A1, B1
    s1.addElement(row([num(20)]))                                        # A2
    s1.addElement(row([formula_num("of:=SUM([.A1:.A2])", 30)]))         # A3
    doc.spreadsheet.addElement(s1)
    s2 = Table(name="Sheet2")
    s2.addElement(row([formula_num("of:=[Sheet1.A3]", 30)]))           # A1
    doc.spreadsheet.addElement(s2)
    write(doc, "smoke.ods")


def make_literals():
    """Every literal value type + a date cell + interior/trailing blanks."""
    doc = OpenDocumentSpreadsheet()
    t = Table(name="Data")
    t.addElement(row([text("Name"), text("Qty"), text("Active"), text("When")]))
    # Widget row: text, number, bool, date. 2024-01-15 -> Excel serial 45306.
    t.addElement(row([text("Widget"), num(42), boolean(True), date("2024-01-15", "2024-01-15")]))
    # Gadget row with a negative float and an interior blank (no Active value).
    t.addElement(row([text("Gadget"), num(-3.5), blank(), date("2024-06-30", "2024-06-30")]))
    doc.spreadsheet.addElement(t)
    write(doc, "literals.ods")


def make_functions():
    """VLOOKUP + IF over a small table (';' arg separator, quoted string, nested FALSE())."""
    doc = OpenDocumentSpreadsheet()
    t = Table(name="Lookup")
    # Columns A,B are the table; column D holds the function cells (C left blank).
    t.addElement(row([
        text("apple"), num(1), blank(),
        formula_num('of:=VLOOKUP("banana";[.A1:.B3];2;FALSE())', 2),
    ]))
    t.addElement(row([
        text("banana"), num(2), blank(),
        formula_text('of:=IF([.B1]>0;"pos";"neg")', "pos"),
    ]))
    t.addElement(row([text("cherry"), num(3)]))
    doc.spreadsheet.addElement(t)
    write(doc, "functions.ods")


def make_blanks_repeats():
    """A sparse sheet: leading value, repeated interior blanks, a trailing value, a blank row."""
    doc = OpenDocumentSpreadsheet()
    t = Table(name="Sparse")
    # A1=1, B1,C1 blank (repeated), D1=4
    t.addElement(row([num(1), blank(2), num(4)]))
    # a fully blank row
    t.addElement(row([blank()]))
    # A3=7
    t.addElement(row([num(7)]))
    doc.spreadsheet.addElement(t)
    write(doc, "blanks_repeats.ods")


if __name__ == "__main__":
    make_smoke()
    make_literals()
    make_functions()
    make_blanks_repeats()
