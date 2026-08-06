# Concern: authors the committed .ods fixture corpus | Non-concern: the Rust importer, the .xlsx corpus | IO: () -> tests/fixtures/*.ods
"""Run from a venv with odfpy installed; the generated .ods files are committed, so the Rust
tests need neither.

    python3 -m venv .fixture-venv && .fixture-venv/bin/pip install odfpy
    .fixture-venv/bin/python scripts/make_fixtures.py
"""

import os
from odf.opendocument import OpenDocumentSpreadsheet
from odf.style import Style, TableCellProperties, TableColumnProperties, TextProperties
from odf.table import Table, TableColumn, TableRow, TableCell
from odf.text import P

FIXTURES = os.path.join(os.path.dirname(__file__), "..", "tests", "fixtures")


def num(v):
    c = TableCell(valuetype="float", value=repr(v))
    c.addElement(P(text=str(v)))
    return c


def text(s):
    c = TableCell(valuetype="string")
    c.addElement(P(text=s))
    return c


def boolean(b):
    c = TableCell(valuetype="boolean", booleanvalue="true" if b else "false")
    c.addElement(P(text="TRUE" if b else "FALSE"))
    return c


def date(iso, display):
    """ODS stores an ISO date; FSA1 maps it to an Excel serial."""
    c = TableCell(valuetype="date", datevalue=iso)
    c.addElement(P(text=display))
    return c


def blank(repeat=1):
    """A `repeat` above 1 exercises number-columns-repeated on read."""
    if repeat == 1:
        return TableCell()
    return TableCell(numbercolumnsrepeated=str(repeat))


def formula_num(of, cached):
    c = TableCell(formula=of, valuetype="float", value=repr(cached))
    c.addElement(P(text=str(cached)))
    return c


def formula_text(of, cached):
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
    doc = OpenDocumentSpreadsheet()
    t = Table(name="Data")
    t.addElement(row([text("Name"), text("Qty"), text("Active"), text("When")]))
    # 2024-01-15 is Excel serial 45306.
    t.addElement(row([text("Widget"), num(42), boolean(True), date("2024-01-15", "2024-01-15")]))
    t.addElement(row([text("Gadget"), num(-3.5), blank(), date("2024-06-30", "2024-06-30")]))
    doc.spreadsheet.addElement(t)
    write(doc, "literals.ods")


def make_functions():
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


def make_fidelity():
    """A1's inline array is untranslatable, B1 is plain, so the report must name exactly A1."""
    doc = OpenDocumentSpreadsheet()
    t = Table(name="Calc")
    t.addElement(row([formula_num("of:=SUM({1;2;3})", 6), formula_num("of:=[.A1]*2", 12)]))
    doc.spreadsheet.addElement(t)
    write(doc, "fidelity.ods")


def make_blanks_repeats():
    doc = OpenDocumentSpreadsheet()
    t = Table(name="Sparse")
    t.addElement(row([num(1), blank(2), num(4)]))   # A1=1, B1/C1 blank, D1=4
    t.addElement(row([blank()]))                    # a fully blank row
    t.addElement(row([num(7)]))                     # A3=7
    doc.spreadsheet.addElement(t)
    write(doc, "blanks_repeats.ods")


def make_styled():
    """Every appearance an .ods can state, on a path that reads none of it: bold, a text colour, a
    fill and a column width. The importer carries the VALUE and nothing else, so this fixture is what
    holds the report to saying so instead of vouching for appearance and geometry it never read."""
    doc = OpenDocumentSpreadsheet()
    col = Style(name="co1", family="table-column")
    col.addElement(TableColumnProperties(columnwidth="2.5cm"))
    doc.automaticstyles.addElement(col)
    cell = Style(name="ce1", family="table-cell")
    cell.addElement(TableCellProperties(backgroundcolor="#ffff00"))
    cell.addElement(TextProperties(fontweight="bold", color="#ff0000"))
    doc.automaticstyles.addElement(cell)

    t = Table(name="Styled")
    t.addElement(TableColumn(stylename=col))
    c = TableCell(stylename=cell, valuetype="string")
    c.addElement(P(text="Total"))
    t.addElement(row([c]))
    t.addElement(row([num(42)]))
    doc.spreadsheet.addElement(t)
    write(doc, "styled.ods")


if __name__ == "__main__":
    make_smoke()
    make_literals()
    make_functions()
    make_fidelity()
    make_blanks_repeats()
    make_styled()
