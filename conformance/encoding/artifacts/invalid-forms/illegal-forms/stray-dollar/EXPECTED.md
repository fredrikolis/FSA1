<!-- Concern: the expected reject verdict for a filename carrying a stray $ absolute marker | Non-concern: the other illegal-forms cases | IO: output -->
# EXPECTED — illegal-forms: stray `$` in filename

**Fixture:** `Sheet/$A$1.cell`
**Rule under test:** FORMAT.md §2 (absolute markers `$`: none in filenames) + §11.

## Inputs
- Filename `$A$1.cell` carries `$` absolute/mixed markers. The body (`42`) is valid — the sole defect is the filename.

## Verdict: **REJECT — `$` is meaningless and rejected in a filename.**
A file's own address is intrinsically a fixed location, so `$` on the left of the dot has no meaning and is rejected. `$` exists **only inside formula bodies** (§4), where it governs relative-ref offsetting under fill (§5). The loader rejects at filename parse.

## Expected diagnostic (shape)
```
error[filename]: "$" is not allowed in a filename (absolute markers live only in formula bodies)
  Sheet/$A$1.cell
  fix: rename to  A1.cell
```

## Why (citation)
FORMAT.md §2: *"Absolute markers (`$`): none in filenames … `$` is meaningless on the left of the dot and is rejected."* Also §11: *"`$A$1.cell` — `$` in a filename → reject (`$` lives in formula bodies only)."*
