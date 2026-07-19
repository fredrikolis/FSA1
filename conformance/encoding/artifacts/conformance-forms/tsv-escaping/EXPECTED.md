<!-- Concern: the expected deserialization of a tab holding the four TSV field-escaping cases (embedded newline, embedded tab, literal backslash, and a malformed escape) under the current spec | Non-concern: the value oracles for the six category workbooks (conformance/render) | IO: output -->
# EXPECTED — TSV field escaping (`\t`, `\n`, `\\`) and a malformed escape

**Fixture:** `tsv-escaping/Cells/{A1,A2,A3,A4}`
**Rule under test:** SPEC.md GRID2 "current deserializer" (field escaping) + GRID6 (error locality) +
CORE2 (a malformed cell is a located refusal, never a silent literal).

A field may contain a tab, newline, or backslash, written as the escapes `\t`, `\n`, and `\\`; a
literal backslash is always `\\`. The deserializer fixes field boundaries on **unescaped** tab/newline
first, then decodes each field by this one rule. A backslash before anything else — or a trailing
backslash — is a **malformed cell** and deserializes to a located error value (GRID6), never a silent
literal and never a whole-file refusal.

## Inputs (bytes on disk) and expected values

| File | On-disk bytes | Decoded cell value | Renders (`--values`) |
|------|---------------|--------------------|----------------------|
| `A1` | `line1\nline2` | text `line1`⏎`line2` (an embedded newline) | the two lines of one cell |
| `A2` | `col1\tcol2`   | text `col1`⇥`col2` (an embedded tab)       | `col1<TAB>col2` |
| `A3` | `path\\to`     | text `path\to` (one literal backslash)     | `path\to` |
| `A4` | `bad\x`        | **GRID6 located error** `#VALUE!`          | `#VALUE!` (raw `bad\x` under `--functions`) |

`A1`, `A2`, `A3` load cleanly — the escaped tab/newline/backslash is **content**, not a delimiter, so
each stays a 1×1 grid holding its exact text.

## Verdict: **`check` REJECTS (exit non-zero), locating only `A4`.**

`A4`'s backslash begins no valid escape (`\x`), so the cell is a located `malformed-escape` error value
(`#VALUE!` class); every other cell still loads and renders (GRID6 locality).

## Expected diagnostic (verbatim wording not frozen; code + location are)
```
error[malformed-escape]: malformed escape in field "bad\\x" at byte 3: a backslash must begin \t, \n, or \\ (write a literal backslash as \\)
  A4:1:4
```

## Why (citation)
SPEC.md GRID2 (the current deserializer): *"A backslash followed by anything else — or a trailing
backslash — is a malformed cell and deserializes to a located error value (GRID6), never a silent
literal or a file-level refusal."* SPEC.md GRID6: the error is **located** and **per-cell** — `check`
reports it with a non-zero exit and its location while every unrelated cell still yields its value. The
fix is to write the intended byte as an escape: a literal backslash is `\\`.
