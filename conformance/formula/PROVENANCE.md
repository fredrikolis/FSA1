# Formula-conformance corpus — provenance and the oracle contract

This directory is the **formula-level** conformance corpus for `charlie-ast`. Each fixture is a
value probe: an input **context** (named cells with literal values), a **formula** string, and the
**EXPECTED** value the formula must evaluate to. The `conformance` runner grades charlie by parsing
and evaluating the formula through the shipping `charlie_ast::parse` + `charlie_ast::eval` path,
against a deterministic stub `Resolver` built from the context, and comparing the produced `Value`
to the expected value **bit-exactly** (`Value`'s own `Eq`: `-0.0 ≠ 0.0`, `NaN == NaN`).

## The oracle contract (ORACLE-INPUT PURITY)

**The corpus is the oracle. Its EXPECTED values are truth — authoritative, never questioned by the
gate or an agent.** A divergence is always a defect in charlie, never in the corpus. Correcting an
expected value is a deliberate maintainer act at the corpus-production layer, never an agent
rationalizing a red fixture (a Diverge means *fix charlie*, never *edit the expectation to match
it*).

**Every EXPECTED value was authored INDEPENDENTLY of charlie** — derived by hand from the documented
Excel/spreadsheet semantics that `~/.knowledge-base/coding-standards/ast-standards.md` and
`docs/architecture.md` §3–§4 specify, and cross-checked against Excel's published behavior. **No
expected value was produced by running charlie** (that would make the engine its own oracle —
circular; the thing under test can never be its own judge). This purity is an INPUT-side property of
how the corpus was authored; the `MANIFEST.sha256` fingerprint below is its tamper-evidence.

The interesting behaviors the seed pins, all from Excel semantics:

- **Direct-vs-in-range coercion asymmetry** — a boolean / numeric-text datum coerces when passed
  *directly* as an argument (`SUM(1,TRUE,"2") = 4`) but is ignored *inside* a range; `COUNT` never
  yields an error from its data.
- **Laziness** — `IF(TRUE,1,1/0) = 1` (the unreached `1/0` never surfaces); `IFERROR` evaluates its
  fallback only on an error.
- **Error propagation** — leftmost-first (`#REF!+#DIV/0! = #REF!`); `/0 → #DIV/0!`; a complex or
  overflowing power → `#NUM!`; a non-numeric text in an arithmetic position → `#VALUE!`.
- **The operator ladder** — unary minus binds tighter than `^`; `^` is left-associative
  (`2^3^2 = 64`); `&` concatenation stringifies operands; comparisons are cross-type ranked
  (Number < Text < Bool) and text-equality is case-insensitive.
- **ROUND** — half away from zero (`2.5 → 3`, `-2.5 → -3`), negative digits to the left of the point.

## What is NOT here (and why)

- **charlie-produced anything.** `facts-snapshot.tsv` (the backslide anchor) IS charlie-derived —
  it records what charlie currently evaluates — so it is **not** an oracle input and is **not**
  fingerprinted here. It is the ratchet's memory, not ground truth.
- **Parse-refusal probes.** This corpus grades VALUES. A formula that charlie refuses to parse
  (e.g. a cross-sheet ref) grades as a Diverge here (a value was expected, a refusal is not one);
  the located-refusal contract is charlie-ast's own `diag`/parser test surface, a different axis.

## How the seed was produced, and how it grows (the W3b grind)

The seed covers the foundational functions implemented in W3 (`SUM AVERAGE COUNT · IF IFERROR AND
OR · ABS ROUND`) plus the operator and error-propagation core. The W3b function grind EXTENDS this
per function: add fixtures (context + formula + hand-derived expected), regenerate the anchor with
`cargo run -p conformance -- resnapshot`, and commit. The coverage ratchet (`modeled / target=70`)
counts a function as *modeled* only once it is in the live registry AND has a Matching fixture, so
growth is monotonic under the backslide guard.

## Fixture file grammar (line-based)

```
[fixture-name]           # unique within the file; the key is "<file-stem>/<name>"
funcs: SUM, AVERAGE      # functions exercised (for the coverage ratchet; optional)
cell A1: 1               # zero or more context cells; canonical A1 only (no $/lowercase/leading-zero)
cell A2: "text"          # literals: number | "text" | TRUE/FALSE | #ERR! | <blank> | {r,c;r,c}
formula: =SUM(A1:A2)     # required; leading = optional
expect: 3                # required; the EXTERNALLY-derived oracle value
```

## Integrity

`MANIFEST.sha256` fingerprints every `.fixtures` file and this `PROVENANCE.md`. Verify with
`sha256sum -c MANIFEST.sha256` (run from this directory), or via the gated
`conformance/tests/corpus_integrity.rs`, which recomputes each digest with the crate's vendored
SHA-256 and fails loudly on any tamper or on a `.fixtures` file missing from the manifest. After a
DELIBERATE corpus edit, regenerate the manifest (see the header comment in `MANIFEST.sha256`).
