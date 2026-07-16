<!-- Concern: the expected reject verdict for a file whose body is both a literal line and an =formula line | Non-concern: the other illegal-forms cases | IO: output -->
# EXPECTED — illegal-forms: dual-body (literal + formula)

**Fixture:** `Sheet/A1.cell`
**Rule under test:** FORMAT.md §4 (body is *exactly one* of two forms) + §11.

## Inputs
- Filename & annotation are canonical/valid — the sole defect is the body.
- Body (lines 2..N):
  ```
  =B2*C2       (an =formula line)
  Hello        (a literal line)
  ```
  The body carries **both** body forms at once.

## Verdict: **REJECT — a body may be a single `=formula` OR a literal block, never both.**
§4 defines the body as *exactly one* of two mutually exclusive forms. This file is neither a lone formula (extra literal line) nor a pure literal block (a line begins with `=`). No precedence is defined; the loader rejects.

## Expected diagnostic (shape)
```
error[body]: file mixes an =formula line with a literal line (exactly one body form allowed)
  Sheet/A1.cell  line 2 is a formula, line 3 is a literal
```

## Why (citation)
FORMAT.md §4: *"The body … is exactly one of two forms"* (a single `=formula`, §4.1; or a literal block, §4.2). Also §11: *"A body that is both a literal line and an `=formula` line → reject (exactly one body form, §4)."*
