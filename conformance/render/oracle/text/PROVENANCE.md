<!-- Concern: how the contacts-clean ground truth was computed, so it is auditable/reproducible | Non-concern: the sheet encoding itself (see artifacts/text/contacts-clean) and the format grammar (FORMAT.md) | IO: none -->
# PROVENANCE — oracle/text/contacts-clean

Ground truth for the `artifacts/text/contacts-clean` workbook (tab `Contacts`),
QA-ladder **tier 8 (text parse / clean, cell-exact)**.

## ORACLE-INPUT PURITY

The expected values were **NOT** produced by charlie (charlie cannot evaluate
yet, and grading the tool against itself is forbidden — `BRIEF.md`, experiment
README). They were computed by an **independent, hand-written Python
re-implementation** of the relevant text semantics: `compute_oracle.py` in this
directory. Python 3 only, standard library (`csv`, `json`, `os`) — no pandas,
no spreadsheet engine, no charlie.

## How to reproduce

```
python3 oracle/text/compute_oracle.py
```

Run from the experiment root. It regenerates, byte-for-byte:

- `contacts-clean.oracle.csv` — `cell,value` rows, sorted column-major then by
  row (stable diff).
- `contacts-clean.oracle.json` — `{ "<A1-address>": <rendered value>, ... }`.

Both are keyed by cell address and cover **every** cell of the used region
(A1:F13, 78 cells): the header row, the two literal input columns rendered as
their raw on-disk strings, and the four derived columns.

## What the script does

1. Reads **only the two literal INPUT columns off disk** — `A2:A13.range` (raw
   full names) and `B2:B13.range` (raw emails) — stripping line 1 (the `# `
   annotation) and the single trailing newline. These are the sheet's real
   inputs; everything else is derived from them here, independently.
2. Re-implements the charlie/Excel text semantics the sheet's formulas use:
   - `TRIM(s)` — strip leading/trailing spaces **and collapse each internal run
     of spaces to one** (Excel behaviour; `_trim`). This is why `"Bob  Jones"`
     and `" Carlos  Ramirez"` split cleanly.
   - `FIND(needle, s)` — 1-indexed first occurrence, case-sensitive.
   - `LEFT(s, n)`, `MID(s, start, n)` — `MID` past end returns the remainder.
   - `LEN(s)`, `LOWER(s)`.
   - `COUNTIF(range, crit)` — count of exact matches, text compared
     case-insensitively; modelled as an **expanding window** `$E$2:E{row}` with
     `crit = E{row}`, i.e. a running per-value counter. Count `== 1` ⇒ first
     occurrence ⇒ `"unique"`, else `"dup"`.

## Formula-to-oracle mapping (the four derived columns)

| Col | Formula (drag-fill, anchored at row 2)                    | Oracle rule implemented |
|-----|----------------------------------------------------------|-------------------------|
| C   | `=LEFT(TRIM(A2),FIND(" ",TRIM(A2))-1)`                    | first token of trimmed name |
| D   | `=MID(TRIM(A2),FIND(" ",TRIM(A2))+1,LEN(TRIM(A2)))`      | remainder after first space |
| E   | `=LOWER(TRIM(B2))`                                        | trimmed + lowercased email |
| F   | `=IF(COUNTIF($E$2:E2,E2)=1,"unique","dup")`              | expanding-window first-occurrence flag |

## Pinned / non-volatile

No dates, `TODAY`, `NOW`, `RAND`, or other volatile inputs are used, so the
ground truth is fully deterministic and reproducible. The only external inputs
are the two literal columns, read verbatim from the corpus files.

## Note on the "UNIQUE-style" column

v1 defers the `UNIQUE` dynamic-array spill (`scope.md`, "Deferred but
AST-RESERVED"). The dedupe requirement is therefore met with an in-scope
**expanding-window `COUNTIF` first-occurrence flag** (column F) rather than a
row-removing `UNIQUE()` — a flag, not a filtered result set. The oracle mirrors
exactly that: it flags, it does not drop rows.
