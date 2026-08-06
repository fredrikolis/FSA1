<!-- Concern: the expected reject verdict for a filename carrying a stray $ absolute marker | Non-concern: the other illegal-forms cases | IO: output -->
# EXPECTED — illegal-forms: stray `$` in filename

**Fixture:** `Sheet/$A$1`
**Rule under test:** SPEC.md FS2 (a file's name is a closed range) + the canonical-form policy
(no `$` in filenames).

## Inputs
- Filename `$A$1` carries `$` absolute/mixed markers. The body (`42`) is valid — the sole defect is the
  filename.

## Verdict: **REJECT — `$` is meaningless and rejected in a filename.**
A file's own address is intrinsically a fixed location, so `$` on a filename has no meaning and is
rejected. `$` lives only inside formula bodies, where it governs relative-vs-absolute references. The
loader rejects at filename parse.

## Expected diagnostic (verbatim)
```
error[dollar-in-filename]: $ is not allowed in a filename (it lives in formula bodies): "$A$1"
  $A$1 (byte 0)
```

## Why (citation)
SPEC.md FS2 + CORE1: A1 addressing uses `$` (`$C$7`) inside formula bodies; a filename is a bare
canonical closed range with no `$`.
