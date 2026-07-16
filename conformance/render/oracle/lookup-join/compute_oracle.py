#!/usr/bin/env python3
# Concern: independently compute the correct rendered values for the lookup-join sheet | Non-concern: charlie itself (this must NOT call charlie — oracle-input purity) | IO: output
#
# Ground truth for artifacts/lookup-join/. Models the join in pure pandas/python,
# reproducing exact-match XLOOKUP / INDEX-MATCH semantics (unmatched key -> #N/A),
# #N/A propagation through arithmetic and SUM, and IFNA fallback to 0.
# Run: python3 compute_oracle.py  -> writes oracle.csv and oracle.json next to it.

import csv
import json
import os

NA = "#N/A"  # sentinel matching FORMAT.md error-literal lexing

# --- Inputs, transcribed by hand from the artifact literal blocks (NOT read via charlie) ---
# Products/A2:C7  (product_id, product_name, unit_price)
products = {
    "P001": 2.50,
    "P002": 5.00,
    "P003": 1.25,
    "P004": 3.75,
    "P005": 8.20,
    "P006": 0.40,
}

# Orders/A2:C11  (order_id, product_id, qty)
orders = [
    ("O1001", "P001", 4),
    ("O1002", "P003", 10),
    ("O1003", "P002", 2),
    ("O1004", "P006", 25),
    ("O1005", "P004", 6),
    ("O1006", "P099", 3),   # <-- missing key: P099 is absent from Products
    ("O1007", "P005", 1),
    ("O1008", "P002", 7),
    ("O1009", "P001", 12),
    ("O1010", "P003", 8),
]


def xlookup(key):
    """Exact-match lookup; unmatched -> #N/A (XLOOKUP with no if_not_found)."""
    return products.get(key, NA)


def is_err(v):
    return isinstance(v, str) and v.startswith("#")


def mul(a, b):
    """Arithmetic with error propagation."""
    if is_err(a) or is_err(b):
        return NA
    return round(a * b, 2)


def ifna(v, fallback):
    return fallback if v == NA else v


cells = {}

# Header rows (literal, verbatim)
prod_hdr = ["product_id", "product_name", "unit_price"]
for j, h in enumerate(prod_hdr):
    cells[f"Products!{chr(ord('A')+j)}1"] = h

# Products data block A2:C7
for i, (pid, price) in enumerate(products.items()):
    r = 2 + i
    name = {"P001": "Widget", "P002": "Gadget", "P003": "Sprocket",
            "P004": "Cog", "P005": "Flange", "P006": "Bolt"}[pid]
    cells[f"Products!A{r}"] = pid
    cells[f"Products!B{r}"] = name
    cells[f"Products!C{r}"] = round(price, 2)

orders_hdr = ["order_id", "product_id", "qty", "unit_price_xlookup",
              "unit_price_index_match", "line_total", "unit_price_safe",
              "line_total_safe"]
for j, h in enumerate(orders_hdr):
    cells[f"Orders!{chr(ord('A')+j)}1"] = h

# Orders rows 2..11
sum_f = 0.0
sum_h = 0.0
f_has_err = False
for i, (oid, pid, qty) in enumerate(orders):
    r = 2 + i
    d = xlookup(pid)                 # D: XLOOKUP
    e = xlookup(pid)                 # E: INDEX/MATCH — identical exact-match result
    f = mul(qty, d)                  # F: line_total = qty * D
    g = ifna(xlookup(pid), 0.0)      # G: IFNA(XLOOKUP, 0)
    h = mul(qty, g)                  # H: safe line_total = qty * G
    cells[f"Orders!A{r}"] = oid
    cells[f"Orders!B{r}"] = pid
    cells[f"Orders!C{r}"] = qty
    cells[f"Orders!D{r}"] = d if is_err(d) else round(d, 2)
    cells[f"Orders!E{r}"] = e if is_err(e) else round(e, 2)
    cells[f"Orders!F{r}"] = f
    cells[f"Orders!G{r}"] = g if is_err(g) else round(g, 2)
    cells[f"Orders!H{r}"] = h
    if is_err(f):
        f_has_err = True
    else:
        sum_f += f
    if not is_err(h):
        sum_h += h

# Totals: F12 poisoned by #N/A in the range; H12 clean
cells["Orders!F12"] = NA if f_has_err else round(sum_f, 2)
cells["Orders!H12"] = round(sum_h, 2)

# --- Emit diffable renderings keyed by cell address ---
here = os.path.dirname(os.path.abspath(__file__))


def render(v):
    if isinstance(v, float):
        # fixed 2-dp for the money/price columns; keep ints for qty
        return f"{v:.2f}"
    return str(v)


ordered = sorted(cells.items(), key=lambda kv: (kv[0].split("!")[0],
                 int("".join(ch for ch in kv[0].split("!")[1] if ch.isdigit())),
                 kv[0].split("!")[1]))

with open(os.path.join(here, "oracle.csv"), "w", newline="") as fcsv:
    w = csv.writer(fcsv)
    w.writerow(["cell", "value"])
    for addr, v in ordered:
        w.writerow([addr, render(v)])

with open(os.path.join(here, "oracle.json"), "w") as fjson:
    json.dump({addr: (v if not isinstance(v, float) else round(v, 2))
               for addr, v in ordered}, fjson, indent=2)
    fjson.write("\n")

print("H12 (clean total) =", cells["Orders!H12"])
print("F12 (poisoned total) =", cells["Orders!F12"])
print("wrote oracle.csv and oracle.json")
