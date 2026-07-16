# Concern: independent ground-truth rendered values for the loan_amortization workbook (PMT payment, amortization schedule, and summary rollups) via the closed-form Excel PMT identity | Non-concern: charlie's own PMT/SUM/COUNTIF evaluation (that engine lives in the charlie/ submodule) — this must NEVER call charlie, preserving ORACLE-INPUT PURITY | IO: (in: pinned principal/rate/term constants restated in-file) -> writes oracle_values.json + oracle_values.csv beside this script
#
# Reproduces the rendered VALUES of every formula in
#   artifacts/model/loan_amortization/{Inputs,Amortization,Summary}
# using plain Python arithmetic + the closed-form Excel PMT identity. No spreadsheet
# engine, no charlie. Run:  python3 compute_oracle.py  -> writes oracle_values.json + oracle_values.csv
#
# Excel PMT(rate, nper, pv, fv=0, type=0) closed form (fv=0, ordinary annuity):
#   PMT = -pv * rate * (1+rate)**nper / ((1+rate)**nper - 1)
# The sheet stores a POSITIVE monthly payment as  =-PMT(r, n, P)  with P (pv) positive,
# i.e.  payment = +P * r * (1+r)**n / ((1+r)**n - 1).

import json
import csv
import os

# ---- pinned inputs (constants; no volatiles) -------------------------------
P = 20000.0        # Inputs!B2  principal
annual = 0.06      # Inputs!B3  annual nominal rate
n = 12             # Inputs!B4  term in months

r = annual / 12.0            # Inputs!B5  monthly rate
# closed-form positive payment = -PMT(r, n, P)
pmt_excel = -P * r * (1 + r) ** n / ((1 + r) ** n - 1)   # Excel PMT sign (negative)
payment = -pmt_excel                                     # Inputs!B6  (positive)

vals = {}
vals["Inputs!A1"] = "Parameter"
vals["Inputs!B1"] = "Value"
vals["Inputs!A2"] = "Principal"
vals["Inputs!A3"] = "Annual Rate"
vals["Inputs!A4"] = "Term (months)"
vals["Inputs!A5"] = "Monthly Rate"
vals["Inputs!A6"] = "Payment"
vals["Inputs!B2"] = P
vals["Inputs!B3"] = annual
vals["Inputs!B4"] = float(n)
vals["Inputs!B5"] = r
vals["Inputs!B6"] = payment

# ---- amortization schedule --------------------------------------------------
vals["Amortization!A1"] = "Month"
vals["Amortization!B1"] = "Begin Balance"
vals["Amortization!C1"] = "Payment"
vals["Amortization!D1"] = "Interest"
vals["Amortization!E1"] = "Principal Paid"
vals["Amortization!F1"] = "End Balance"

total_interest = 0.0
total_paid = 0.0
begin = P
for i in range(1, n + 1):
    row = i + 1  # sheet rows 2..13
    interest = begin * r
    principal_paid = payment - interest
    end = begin - principal_paid
    vals[f"Amortization!A{row}"] = float(i)
    vals[f"Amortization!B{row}"] = begin
    vals[f"Amortization!C{row}"] = payment
    vals[f"Amortization!D{row}"] = interest
    vals[f"Amortization!E{row}"] = principal_paid
    vals[f"Amortization!F{row}"] = end
    total_interest += interest
    total_paid += payment
    begin = end

# ---- summary ---------------------------------------------------------------
vals["Summary!A1"] = "Metric"
vals["Summary!B1"] = "Value"
vals["Summary!A2"] = "Total Interest"
vals["Summary!A3"] = "Total Paid"
vals["Summary!A4"] = "Payoff Month"
vals["Summary!A5"] = "Final Balance"
vals["Summary!B2"] = total_interest                    # =SUM(Amortization!D2:D13)
vals["Summary!B3"] = total_paid                        # =SUM(Amortization!C2:C13)
# =COUNTIF(Amortization!B2:B13, ">0") : count of periods whose begin balance is > 0
payoff_month = sum(1 for i in range(1, n + 1)
                   if vals[f"Amortization!B{i+1}"] > 0)
vals["Summary!B4"] = float(payoff_month)
vals["Summary!B5"] = vals["Amortization!F13"]          # =Amortization!F13 (final end balance)

# ---- emit ------------------------------------------------------------------
here = os.path.dirname(os.path.abspath(__file__))

def render(v):
    # near-zero float clean-up for display only (full precision kept in JSON)
    if isinstance(v, float):
        return round(v, 10)
    return v

out = {k: render(v) for k, v in vals.items()}
with open(os.path.join(here, "oracle_values.json"), "w") as f:
    json.dump(out, f, indent=2, sort_keys=True)
    f.write("\n")

with open(os.path.join(here, "oracle_values.csv"), "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["address", "value"])
    for k in sorted(out):
        w.writerow([k, out[k]])

print(f"payment (Inputs!B6)      = {payment:.10f}")
print(f"total interest (Sum!B2)  = {total_interest:.10f}")
print(f"total paid (Sum!B3)      = {total_paid:.10f}")
print(f"payoff month (Sum!B4)    = {payoff_month}")
print(f"final balance (Sum!B5)   = {vals['Amortization!F13']:.6e}")
