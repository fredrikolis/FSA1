#!/usr/bin/env python3
# Concern: independent ground-truth rendered cell values for the aggregation/sales_report workbook (input echo, SUM/SUMIFS/COUNTIFS/AVERAGEIFS rollups) | Non-concern: FSA1's own formula evaluation (that engine lives in the FSA1/ submodule) — recomputed here in pandas so FSA1 is never graded against its own output | IO: (in: input-cell records restated in-file, mirroring the .range inputs) -> writes expected_values.json + expected_derived.csv beside this script
"""Independent oracle for the aggregation/sales_report FSA1 workbook.

Provenance-pure: ground truth is computed HERE from the input data with pandas/
plain arithmetic. FSA1 never touches this — FSA1 cannot evaluate yet, and
grading the tool with its own output is forbidden (ORACLE-INPUT PURITY).

The input records below are a re-statement of the sheet's INPUT cells (the
hand-entered ledger + the region/product criteria keys). Every derived value is
the expected RENDERED result of evaluating the corresponding formula file."""
import json, pathlib
import pandas as pd

# Write outputs next to THIS script (portable: no hardcoded machine paths).
OUT = pathlib.Path(__file__).resolve().parent

# ---- INPUT (mirrors the .range input files; NOT read from FSA1) ----
rows = [
    (1001, "EMEA", "Widget",   10, 1200),
    (1002, "AMER", "Gadget",    5,  750),
    (1003, "APAC", "Widget",    8,  960),
    (1004, "EMEA", "Sprocket", 12, 1440),
    (1005, "AMER", "Widget",    6,  720),
    (1006, "EMEA", "Gadget",    4,  600),
    (1007, "APAC", "Sprocket", 15, 1800),
    (1008, "AMER", "Sprocket",  9, 1080),
    (1009, "EMEA", "Widget",    7,  840),
    (1010, "APAC", "Gadget",   11, 1650),
    (1011, "AMER", "Widget",    3,  360),
    (1012, "APAC", "Widget",    5,  600),
]
df = pd.DataFrame(rows, columns=["OrderID", "Region", "Product", "Units", "Revenue"])

regions  = ["EMEA", "AMER", "APAC"]     # Summary!A2:A4
products = ["Widget", "Gadget", "Sprocket"]  # Summary!A7:A9

def num(x):
    """Render as int when integral, else float — matches a spreadsheet display."""
    f = float(x)
    return int(f) if f == int(f) else f

cells = {}  # "Sheet!Addr" -> rendered value

# ===== Sales tab: input echo + totals row =====
addr_rows = range(2, 14)
for i, (oid, reg, prod, units, rev) in zip(addr_rows, rows):
    cells[f"Sales!A{i}"] = oid
    cells[f"Sales!B{i}"] = reg
    cells[f"Sales!C{i}"] = prod
    cells[f"Sales!D{i}"] = units
    cells[f"Sales!E{i}"] = rev
for col, name in zip("ABCDE", ["OrderID", "Region", "Product", "Units", "Revenue"]):
    cells[f"Sales!{col}1"] = name
cells["Sales!A14"] = "TOTAL"
cells["Sales!D14"] = num(df["Units"].sum())     # =SUM(D2:D13)
cells["Sales!E14"] = num(df["Revenue"].sum())   # =SUM(E2:E13)

# ===== Summary tab: by-Region rollup (rows 2-4) =====
for col, name in zip("ABCD", ["Region", "Revenue", "Orders", "Avg Revenue"]):
    cells[f"Summary!{col}1"] = name
for i, reg in zip(range(2, 5), regions):
    sub = df[df["Region"] == reg]
    cells[f"Summary!A{i}"] = reg
    cells[f"Summary!B{i}"] = num(sub["Revenue"].sum())              # SUMIFS
    cells[f"Summary!C{i}"] = num(len(sub))                          # COUNTIFS
    cells[f"Summary!D{i}"] = num(sub["Revenue"].mean())            # AVERAGEIFS

# ===== Summary tab: by-Product rollup (rows 7-9) =====
for col, name in zip("ABCD", ["Product", "Revenue", "Orders", "Avg Revenue"]):
    cells[f"Summary!{col}6"] = name
for i, prod in zip(range(7, 10), products):
    sub = df[df["Product"] == prod]
    cells[f"Summary!A{i}"] = prod
    cells[f"Summary!B{i}"] = num(sub["Revenue"].sum())
    cells[f"Summary!C{i}"] = num(len(sub))
    cells[f"Summary!D{i}"] = num(sub["Revenue"].mean())

# ===== Summary tab: scalar cells (rows 11-13) =====
cells["Summary!A11"] = "Overall Avg Revenue"
cells["Summary!B11"] = num(df["Revenue"].mean())                   # AVERAGE(Sales!E2:E13)
cells["Summary!A12"] = "EMEA Widget Revenue"
cells["Summary!B12"] = num(df[(df["Region"] == "EMEA") & (df["Product"] == "Widget")]["Revenue"].sum())  # 2-crit SUMIFS
cells["Summary!A13"] = "Total (check)"
cells["Summary!B13"] = num(sum(cells[f"Summary!B{i}"] for i in range(2, 5)))  # SUM(B2:B4)

# ---- write JSON (keyed by cell address) ----
with open(OUT / "expected_values.json", "w", encoding="utf-8", newline="\n") as f:
    json.dump({k: cells[k] for k in sorted(cells, key=lambda s: (s.split("!")[0], s))},
              f, indent=2, ensure_ascii=False)
    f.write("\n")

# ---- write CSV (diffable, only the DERIVED/output cells that matter for grading) ----
derived = [
    "Sales!D14", "Sales!E14",
    "Summary!B2", "Summary!C2", "Summary!D2",
    "Summary!B3", "Summary!C3", "Summary!D3",
    "Summary!B4", "Summary!C4", "Summary!D4",
    "Summary!B7", "Summary!C7", "Summary!D7",
    "Summary!B8", "Summary!C8", "Summary!D8",
    "Summary!B9", "Summary!C9", "Summary!D9",
    "Summary!B11", "Summary!B12", "Summary!B13",
]
with open(OUT / "expected_derived.csv", "w", encoding="utf-8", newline="\n") as f:
    f.write("cell,value\n")
    for a in derived:
        f.write(f"{a},{cells[a]}\n")

print("wrote", OUT / "expected_values.json", "and expected_derived.csv")
print("--- derived cells ---")
for a in derived:
    print(f"{a:14} = {cells[a]}")
