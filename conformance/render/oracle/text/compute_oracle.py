#!/usr/bin/env python3
# Concern: independent ground-truth computation for the contacts-clean sheet | Non-concern: FSA1 itself (must NEVER produce this truth) | IO: output
"""
Independent oracle for artifacts/text/contacts-clean.

ORACLE-INPUT PURITY: this script is a hand-written re-implementation of the
relevant Excel/FSA1 text semantics in plain Python. It does NOT call FSA1.
It reads ONLY the two literal INPUT columns (A2:A13 raw names, B2:B13 raw
emails) straight off disk and derives C/D/E/F from scratch, so the expected
values are computed independently of the engine under test.

Excel/FSA1 semantics re-implemented here:
  TRIM(s)  -> strip leading/trailing spaces AND collapse each internal run of
              spaces to a single space (Excel behaviour).
  FIND(needle, s) -> 1-indexed position of first occurrence (case-sensitive).
  LEFT(s, n)      -> first n characters.
  MID(s, start, n)-> up to n characters starting at 1-indexed `start`; if n
                     runs past the end, returns the remainder (Excel behaviour).
  LEN(s)          -> character count.
  LOWER(s)        -> lowercase.
  COUNTIF(range, crit) -> count of cells in `range` equal to `crit`; text
                     comparison is case-insensitive (Excel). Here the range is
                     the expanding window $E$2:E{row}, and crit is E{row}.
"""
import csv
import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
CONTACTS = os.path.normpath(os.path.join(
    HERE, "..", "..", "artifacts", "text", "contacts-clean", "Contacts"))

FIRST_ROW = 2
LAST_ROW = 13

HEADERS = ["Full Name", "Email", "First", "Last", "Email Clean", "Unique?"]


def read_literal_column(path):
    """Return the body lines (annotation stripped) of a single-column .range."""
    with open(path, "r", encoding="utf-8") as fh:
        lines = fh.read().split("\n")
    # line 0 is the '# ' annotation; drop it. Drop a single trailing empty line
    # produced by the final newline, but preserve genuinely-blank interior cells.
    body = lines[1:]
    if body and body[-1] == "":
        body = body[:-1]
    return body


def _trim(s):
    # split on spaces only (Excel TRIM targets the space char), collapse runs,
    # strip ends.
    parts = [p for p in s.split(" ") if p != ""]
    return " ".join(parts)


def excel_find(needle, s):
    idx = s.find(needle)
    if idx < 0:
        return None  # #VALUE! in Excel; not expected for this data
    return idx + 1  # 1-indexed


def excel_left(s, n):
    if n < 0:
        return None
    return s[:n]


def excel_mid(s, start, n):
    if start < 1 or n < 0:
        return None
    return s[start - 1:start - 1 + n]


def main():
    names = read_literal_column(os.path.join(CONTACTS, "A2:A13.range"))
    emails = read_literal_column(os.path.join(CONTACTS, "B2:B13.range"))
    n = LAST_ROW - FIRST_ROW + 1
    assert len(names) == n, f"expected {n} names, got {len(names)}: {names!r}"
    assert len(emails) == n, f"expected {n} emails, got {len(emails)}: {emails!r}"

    oracle = {}

    # Header row A1:F1
    for col, h in zip("ABCDEF", HEADERS):
        oracle[f"{col}1"] = h

    seen_counts = {}  # normalized-email(lowercased) -> running count, for COUNTIF window
    for i in range(n):
        row = FIRST_ROW + i
        raw_name = names[i]
        raw_email = emails[i]

        # A/B are literal inputs: rendered value == the raw stored string.
        oracle[f"A{row}"] = raw_name
        oracle[f"B{row}"] = raw_email

        # C: first name
        t = _trim(raw_name)
        sp = excel_find(" ", t)
        first = excel_left(t, sp - 1)
        # D: last name (remainder after first space)
        last = excel_mid(t, sp + 1, len(t))
        # E: normalized email
        clean = _trim(raw_email).lower()
        # F: expanding-window COUNTIF first-occurrence flag (case-insensitive)
        key = clean.lower()
        cnt = seen_counts.get(key, 0) + 1
        seen_counts[key] = cnt
        flag = "unique" if cnt == 1 else "dup"

        oracle[f"C{row}"] = first
        oracle[f"D{row}"] = last
        oracle[f"E{row}"] = clean
        oracle[f"F{row}"] = flag

    # Emit CSV (cell,value) sorted by column then row for a stable diff.
    def sort_key(cell):
        col = "".join(ch for ch in cell if ch.isalpha())
        rownum = int("".join(ch for ch in cell if ch.isdigit()))
        return (col, rownum)

    csv_path = os.path.join(HERE, "contacts-clean.oracle.csv")
    with open(csv_path, "w", encoding="utf-8", newline="") as fh:
        w = csv.writer(fh)
        w.writerow(["cell", "value"])
        for cell in sorted(oracle, key=sort_key):
            w.writerow([cell, oracle[cell]])

    json_path = os.path.join(HERE, "contacts-clean.oracle.json")
    with open(json_path, "w", encoding="utf-8") as fh:
        json.dump(oracle, fh, indent=2, ensure_ascii=False, sort_keys=True)
        fh.write("\n")

    print(f"wrote {csv_path}")
    print(f"wrote {json_path}")
    print(f"{len(oracle)} cells")


if __name__ == "__main__":
    main()
