#!/usr/bin/env python3
# Concern: INDEPENDENT ground-truth evaluator for the customer_tiers sheet (pure python, no FSA1) | Non-concern: parsing the on-disk .range grammar (values are transcribed by hand from the input files below) | IO: output
"""Independently compute the rendered values for artifacts/conditional/customer_tiers.

Ground truth is derived here in plain python, NOT by FSA1 (oracle-input purity).
The spend inputs are transcribed verbatim from Customers/B2:B13.range; the tier rule and
the count rule mirror the formulas in Customers/C2:C13.range and Customers/F2:F4.range but
are re-implemented independently below.
"""
import csv
import json
import math
import os

# Write outputs next to THIS script (portable: no cwd dependence, no hardcoded paths).
HERE = os.path.dirname(os.path.abspath(__file__))

# --- inputs transcribed from Customers/A2:A13.range and Customers/B2:B13.range ---
# ERROR sentinel models the #VALUE! error literal in B13.
class Err:
    def __init__(self, kind): self.kind = kind
    def __repr__(self): return self.kind

customers = [
    ("Acme Corp",          250000.0),
    ("Globex",             100001.0),
    ("Cyberdyne",          100000.5),
    ("Initech",            100000.0),
    ("Umbrella",            75000.0),
    ("Soylent",             50001.0),
    ("Massive Dynamic",     50000.5),
    ("Stark Industries",    50000.0),
    ("Wayne Enterprises",   42000.0),
    ("Wonka",                   0.0),
    ("Hooli",               -5000.0),
    ("Vandelay",           Err("#VALUE!")),
]

# --- tier rule (independent reimpl of IFERROR(IFS(...),"Bronze")) ---
def tier(spend):
    # An errored feed makes the > comparisons propagate the error; IFS returns the
    # error; IFERROR catches it and yields "Bronze". Model that directly.
    if isinstance(spend, Err):
        return "Bronze"
    if spend > 100000:      # strict: exactly 100000 is NOT Gold
        return "Gold"
    if spend > 50000:       # strict: exactly 50000 is NOT Silver
        return "Silver"
    return "Bronze"

# --- render column C (tiers) ---
cells = {}
for i, (name, spend) in enumerate(customers):
    row = 2 + i
    cells[f"A{row}"] = name
    cells[f"B{row}"] = spend.kind if isinstance(spend, Err) else spend
    cells[f"C{row}"] = tier(spend)

# --- header rows (literals) ---
cells["A1"], cells["B1"], cells["C1"] = "Customer", "Annual Spend", "Tier"
cells["E1"], cells["F1"] = "Tier", "Count"
cells["E2"], cells["E3"], cells["E4"] = "Gold", "Silver", "Bronze"
cells["E5"] = "Total"

# --- counts (independent reimpl of COUNTIF over column C) ---
tier_vals = [cells[f"C{2+i}"] for i in range(len(customers))]
cells["F2"] = tier_vals.count("Gold")
cells["F3"] = tier_vals.count("Silver")
cells["F4"] = tier_vals.count("Bronze")
cells["F5"] = cells["F2"] + cells["F3"] + cells["F4"]

def fmt(v):
    if isinstance(v, float):
        return str(int(v)) if v == math.floor(v) else str(v)
    return str(v)

# --- emit diffable renderings keyed by cell address ---
order = (["A1","B1","C1"]
         + [f"{c}{2+i}" for i in range(len(customers)) for c in "ABC"]
         + ["E1","F1","E2","F2","E3","F3","E4","F4","E5","F5"])

with open(os.path.join(HERE, "expected_values.csv"), "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["cell", "value"])
    for a in order:
        w.writerow([a, fmt(cells[a])])

with open(os.path.join(HERE, "expected_values.json"), "w") as f:
    json.dump({a: cells[a] for a in order}, f, indent=2)
    f.write("\n")

print("tier counts -> Gold:%d Silver:%d Bronze:%d Total:%d"
      % (cells["F2"], cells["F3"], cells["F4"], cells["F5"]))
