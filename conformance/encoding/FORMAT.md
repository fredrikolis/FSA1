<!-- Concern: pointer to the authoritative on-disk-format contract now that this corpus is graded against the current spec | Non-concern: restating the format (SPEC.md + docs/format.md own it) | IO: none -->
# FORMAT.md — superseded

> **SUPERSEDED.** This file was the W1 *provisional* substrate spec (`.range`/`.cell` filename
> endings, a broadcast-conformance dimension rule, and a single-`=formula` drag-fill body). Those
> constructs have been **retired**. The authoritative on-disk-format contract is now:
>
> - **`SPEC.md`** (repo root) — the vocabulary and invariants (FT‑1 … FT‑14) the implementation
>   satisfies. In particular: a file's name **is** a closed range with **no ending** (FT‑3); its
>   content deserializes to an **explicit grid** that fills the range exactly (FT‑4/FT‑8), one cell per
>   coordinate; the current deserializer is **TSV** (FT‑5), an empty field a Blank cell; a cell's value
>   derives only from its own content, never its position (FT‑9) — there is no drag-fill anywhere.
> - **`docs/format.md`** — the format guide written against SPEC.md.
>
> The `EXPECTED.md` verdict ledgers in this corpus cite SPEC.md's FT‑invariants directly. This corpus
> was migrated **value-preservingly** to the current form (filenames dropped their endings; every
> former drag-fill body was expanded to the explicit per-cell grid it denoted, so rendered values are
> unchanged). See `PROVENANCE.md`.
