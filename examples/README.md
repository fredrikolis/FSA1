<!-- Concern: orients an agent to the runnable FSA1 examples in this folder | Non-concern: what any one example demonstrates (each file says so itself) | IO: none -->
# examples

Runnable references for driving FSA1 from a shell or an agent loop.

> **STUB.** Created during the FSA1 monorepo reorganization. `agent_demo.sh` runs today;
> `agent_python_demo.py` is a skeleton.

| entry | what it shows |
|---|---|
| `agent_demo.sh` | drive `fsa1-cli`: write a workbook, render it, edit a cell with a redirect, re-render |
| `agent_python_demo.py` | read the format with **no** library — a filename is the range, the body is TSV |
| `sample_fsa1_dir/` | a small two-tab workbook at rest, readable straight from the repo |

## The sample workbook

`sample_fsa1_dir/` is deliberately **not** a copy of the tutorial workbook in
`crates/fsa1-model/src/sample.rs` (the one `fsa1-cli sample <dir>` writes). Two copies of the same
workbook would drift, and the tests pin that one. This is a different, smaller sheet whose only job
is to be legible on GitHub without running anything:

```
Budget/A1-C1   Category<TAB>Budget<TAB>Actual
Budget/B6      =SUM(B2:B5)
Summary/B4     =ROUND(Budget!C6-Budget!B6,2)
```

Nothing enforces that it stays lint-clean yet — it is not in any test. Wiring it into one is a
sensible follow-up.
