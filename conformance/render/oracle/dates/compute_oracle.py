# Concern: independent ground-truth day-counts and aging-bucket rollups for the dates/invoice-aging workbook, cross-checked two ways (if-ladder vs COUNTIFS/SUMIFS criteria) | Non-concern: charlie's DATE/DATEDIF/IFS implementation (that engine lives in the charlie/ submodule) — computed here with the stdlib datetime so the engine is never its own grader | IO: (in: pinned invoice rows + reference date restated in-file) -> prints per-row serials/day-counts/buckets and reconciling rollups to stdout
from datetime import date

# Excel/Sheets 1900 date system: serial(d) = (d - 1899-12-30).days for modern dates.
EPOCH = date(1899, 12, 30)
def serial(d): return (d - EPOCH).days

REF = date(2026, 3, 31)          # pinned reference constant (NOT volatile TODAY)
ref_serial = serial(REF)

# (id, customer, invoice_date, amount)
rows = [
    ("INV-001", "Acme Corp",  date(2026,3,15), 1200.00),
    ("INV-002", "Globex",     date(2026,3,31),  500.00),
    ("INV-003", "Initech",    date(2026,3,1),  3400.50),
    ("INV-004", "Umbrella",   date(2026,2,28), 2750.00),
    ("INV-005", "Soylent",    date(2026,2,15),  900.00),
    ("INV-006", "Hooli",      date(2026,1,30), 5000.00),
    ("INV-007", "Stark Ind",  date(2026,1,29), 1500.00),
    ("INV-008", "Wayne Ent",  date(2026,1,15), 4200.75),
    ("INV-009", "Wonka",      date(2025,12,31), 620.00),
    ("INV-010", "Cyberdyne",  date(2025,12,30),3300.00),
    ("INV-011", "Tyrell",     date(2025,12,1), 7800.00),
    ("INV-012", "Oscorp",     date(2025,10,15), 250.00),
]

def bucket(days):
    if days <= 30: return "0-30"
    if days <= 60: return "31-60"
    if days <= 90: return "61-90"
    return "90+"

print("ref_serial(2026-03-31) =", ref_serial)
print()
print("row | id | date | serial | days=DATEDIF | bucket")
recs=[]
for i,(iid,cust,d,amt) in enumerate(rows):
    r = i+2
    s = serial(d)
    days = ref_serial - s
    b = bucket(days)
    recs.append((r,iid,cust,d,s,amt,days,b))
    print(f"E{r} C{r}={s} ({d})  days={days}  bucket={b}")

print()
buckets = ["0-30","31-60","61-90","90+"]
for bi,bname in enumerate(buckets):
    cnt = sum(1 for x in recs if x[7]==bname)
    amt = sum(x[5] for x in recs if x[7]==bname)
    print(f"{bname}: count={cnt} amount={amt:.2f}")
print("TOTAL count=", len(recs), "amount=", round(sum(x[5] for x in recs),2))

# criteria-based recompute (mirrors COUNTIFS/SUMIFS) as an independent cross-check
def countifs_days(lo,hi):
    return sum(1 for x in recs if lo<=x[6]<=hi)
def countif_gt(v):
    return sum(1 for x in recs if x[6]>v)
def sumifs_days(lo,hi):
    return round(sum(x[5] for x in recs if lo<=x[6]<=hi),2)
def sumif_gt(v):
    return round(sum(x[5] for x in recs if x[6]>v),2)
print()
print("CROSS-CHECK via criteria:")
print("B17 COUNTIFS 0..30 =", countifs_days(0,30), " C17 SUMIFS=", sumifs_days(0,30))
print("B18 COUNTIFS 31..60 =", countifs_days(31,60), " C18 SUMIFS=", sumifs_days(31,60))
print("B19 COUNTIFS 61..90 =", countifs_days(61,90), " C19 SUMIFS=", sumifs_days(61,90))
print("B20 COUNTIF >90 =", countif_gt(90), " C20 SUMIF=", sumif_gt(90))
