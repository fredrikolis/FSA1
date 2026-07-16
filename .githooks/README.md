# .githooks — commit gate

Enable per clone: `git config core.hooksPath .githooks`

Adapted from `~/src/fanuc-tools-dev/fanuc-tools/.githooks`. Full rationale:
`../docs/commit-gate.md`.

- **pre-commit** — robust PATH repair, then `cargo fmt --all -- --check` ·
  `cargo clippy --workspace --all-targets -- -D warnings` ·
  `annotated-tree --strict-check --include-tests` (PATH-resolved via `command -v` after prepending
  `$CARGO_HOME/bin` and the npm global bin, with an `npx --yes annotated-tree@0.2.1` fallback). The conformance backslide state-guard is
  wired here in **W3** (no conformance crate exists yet); the marked block in the hook holds its
  place. The full test gate stays a deliberate, observed step — never chained into the commit.
- **commit-msg** — a mechanical presence-check for two attestation trailers: **(A standards-review
  or `Review-skip`) AND (B annotation-drift review or `Annotation-skip`)**. It verifies only that
  the trailers are present and well-formed; the reviews themselves are performed out-of-band by an
  independent reviewer. The exact trailer grammar, the severity pass rule, and every parser quirk
  are NOT restated here — they have one source of truth: the `commit-msg` hook itself and
  `commit-msg.selftest.sh`, with full rationale in `../docs/commit-gate.md`. Read those,
  not a prose paraphrase that can drift.

Regression tables (both must pass, and CI runs them via `build.yml`): `pre-commit.selftest.sh`,
`commit-msg.selftest.sh`.
