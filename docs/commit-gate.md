<!-- Concern: the self-contained RATIONALE for charlie's commit gate — why it is two hooks + one out-of-band review, why the full test gate stays out of the hook, and why the parser quirks are selftest-locked; the in-clone home the gate's config/process files defer to for full design | Non-concern: the exact trailer grammar and parser quirks (single-sourced in the `commit-msg` hook + its `.selftest.sh`, never paraphrased here) and what the code does (see `docs/architecture.md`) | IO: none — a design document -->
# charlie — Commit Gate (rationale)

The mechanism lives in `.githooks/` and `.github/workflows/`; this document is the self-contained
*why* those files defer to. Enable per clone once: `git config core.hooksPath .githooks`.

**Load-bearing design honesty.** A git hook is bash and cannot invoke an LLM, so it can never
*perform* a review — it only verifies that a well-formed **attestation** is present. The semantic
review happens out-of-band (a spawned neutral agent); the hook records the tally. The gate is three
parts: two hooks plus one out-of-band review.

## Part 1 — `pre-commit` (mechanical, fail-fast)

Runs, in order (robustly prepending `$CARGO_HOME/bin` and the npm global bin to PATH — hooks run with
a minimal env):

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `annotated-tree --strict-check --include-tests .` — the annotation + architecture gate. The rules
   live in `.annotated-tree.toml`; corpus/fixture exclusion is done at the call site with `-I` globs
   (annotated-tree config has no `ignore` key), kept in sync between the hook and CI.
4. **Conformance backslide state-guard** — live and unconditional (the `conformance` crate exists and
   the guard is wired). It branches on `cargo run -p conformance -- backslide`'s exit code: `0` = no
   proven fact lost → PASS; `1` = ≥1 backslide → BLOCK; `2` = anchor missing/unreadable → BLOCK (fail
   SAFE).

The **full test gate** — `cargo test --workspace -- --include-ignored` — is deliberately **NOT** in
the hook. It stays a deliberate, observed step so it is never chained into the commit action.
`--include-ignored` is mandatory so an `#[ignore]` never silently skips a load-bearing test.

## Part 2 — `commit-msg` (attestation presence-check)

Requires **two independent attestation trailers** — the commit needs **(A or `Review-skip`) AND (B or
`Annotation-skip`)**. Severity-tiered, **not a numeric score**: a severity gate drives the weak
dimension to zero directly where a blended score buries it. Iterate fix → re-review until **no major
and no moderate** findings remain; minor is the author's discretion.

- **Attestation A — neutral standards review.** `Reviewed: by <reviewer> vs <tag> — major=<n>
  moderate=<n> minor=<n>`, passing iff `major=0 AND moderate=0`; or `Review-skip: <reason>`.
- **Attestation B — annotation-drift review.** `Annotation-Reviewer: <id>` + `Annotation-Issues: 0`;
  or `Annotation-skip: <reason>`.

The exact trailer grammar, the severity pass rule, and every parser quirk are **NOT restated here** —
they have one source of truth: the `commit-msg` hook itself and `commit-msg.selftest.sh`. Read those,
not a prose paraphrase that can drift.

## Part 3 — the neutral review, out-of-band

At every plan/milestone completion, spawn a fresh-context reviewer (a workflow / task agent, never the
author self-reviewing). Judge **blind** — never name the wanted verdict, which anchors the reviewer
into producing it. The reviewer loads the standards **by full path**
(`~/.knowledge-base/coding-standards/…`) and classifies findings per principle by severity plus the
AUTO-REJECT gate. Iterate fix → re-review until no major and no moderate remain; a rejected minor gets
a one-line rationale; any AUTO-REJECT trip fails outright.

## Selftests lock the parser quirks

Each hook ships a selftest enumerating PASS/BLOCK cases
(`.githooks/{pre-commit,commit-msg}.selftest.sh`), so the parser quirks (the ` vs ` delimiter,
keyword-anywhere count match, the `major=1`→BLOCK / `minor=5`→PASS rules) stay locked. CI
(`build.yml`) runs both selftests — their only mechanical entrypoint — so a hook-parser edit that
breaks a locked quirk fails CI rather than silently rotting.

## CI

Narrow workflows mirror the mechanical checks: `annotations.yml` (the annotation gate, `annotated-tree`
pinned) and `build.yml` (`--locked` fmt + clippy + the full `--include-ignored` test gate, then both
selftests). For any filtered `cargo test <filter>` in CI, add the **zero-tests-ran guard** — `cargo
test` exits 0 when the filter matches nothing, so also assert `test result: ok. [1-9][0-9]* passed`,
else filter drift silently green-lights. (The current `build.yml` runs unfiltered, so no filter can
drift; the guard is noted for whenever a filtered step lands.)
