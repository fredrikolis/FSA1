# .githooks — commit gate (wired in charlie-v1.md Phase 0)

Enable per clone: `git config core.hooksPath .githooks`

Planned hooks (adapted from `~/src/fanuc-tools-dev/fanuc-tools/.githooks`):

- **pre-commit** — `cargo fmt --check` · `cargo clippy -D warnings` ·
  `/home/olis/.cargo/bin/annotated-tree --strict-check` · coverage/backslide state-guard.
- **commit-msg** — require the neutral-reviewer score trailer and the annotation-drift
  acknowledgement trailer (see charlie-v1.md "Commit gate").

The full test gate (`cargo test --workspace -- --include-ignored`) stays a deliberate, observed
step — never chained into the commit action.
