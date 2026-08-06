<!-- Concern: pointer to the authoritative on-disk-format contract now that this corpus is graded against the current spec | Non-concern: restating the format (SPEC.md owns the contract; `fsa1-cli --guide` + the exported `fsa1-cli sample` workbook are the agent-facing tour) | IO: none -->
# FORMAT.md — superseded

> **SUPERSEDED.** This file was the W1 *provisional* substrate spec (`.range`/`.cell` filename
> endings, a broadcast-conformance dimension rule, and a single-`=formula` drag-fill body). Those
> constructs have been **retired**. The authoritative on-disk-format contract is now:
>
> - **`SPEC.md`** (repo root) — the invariants, at need level, the implementation
>   satisfies. In particular: a file's name **is** a closed range with **no ending** (FS2); its
>   content deserializes to an **explicit grid** that fills the range exactly (GRID1/GRID4), one cell per
>   coordinate; the current deserializer is **TSV** (GRID2), an empty field a Blank cell; a cell's value
>   derives only from its own content, never its position (VAL1) — there is no drag-fill anywhere.
> - **`fsa1-cli --guide`** — the terse, agent-facing tour of the on-disk model, plus
>   **`fsa1-cli sample <dir>`**, which writes a live tutorial workbook you can render, check, and edit.
>   (These replace the former `docs/format.md` format guide, whose rationale now lives in the governing
>   `fsa1-model` / `fsa1-ast` code itself.)
>
> The `EXPECTED.md` verdict ledgers in this corpus cite SPEC.md's subsystem-scoped invariants directly. This corpus
> was migrated **value-preservingly** to the current form (filenames dropped their endings; every
> former drag-fill body was expanded to the explicit per-cell grid it denoted; and each file's former
> line-1 `# Concern…` annotation was removed so the content is exactly the grid — the first line is the
> first row (GRID1) — so rendered values are unchanged). See `PROVENANCE.md`.
