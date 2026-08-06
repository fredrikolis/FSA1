#!/usr/bin/env bash
# Concern: runs the pinned comment-budget linter over a path | Non-concern: the thresholds (.cargo-lint-extra.toml owns them) | IO: (path) -> diagnostics; exit 1 on a deny finding
set -euo pipefail

REV=7a232179e45414108d28047acd6315d9a2c4946b

# Resolve the target against the CALLER's cwd, before the cd to the repo root.
TARGET=$(cd -P -- "${1:-.}" >/dev/null 2>&1 && pwd) || {
	echo "comment-gate: no such directory: ${1:-.}" >&2
	exit 2
}

ROOT=$(cd -P -- "$(dirname -- "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd)
cd -P -- "$ROOT" >/dev/null

BIN="$ROOT/target/lint-tools/$REV/bin/cargo-lint-extra"
if [ ! -x "$BIN" ]; then
	echo "comment-gate: building cargo-lint-extra @ $REV (one-time, a few minutes)" >&2
	cargo install --git https://github.com/fredrikolis/cargo-lint-extra \
		--rev "$REV" --root "target/lint-tools/$REV" --locked --quiet cargo-lint-extra >&2
fi

# No -W: it exits 1 on ANY diagnostic including the warn-level rules (glob-imports, todo-comments),
# which would make this gate permanently red at zero blocking findings. Blocking comes from
# level = "deny" in .cargo-lint-extra.toml.
if "$BIN" lint-extra --config "$ROOT/.cargo-lint-extra.toml" "$TARGET"; then
	exit 0
fi

echo >&2
cat "$ROOT/docs/comment-standards.md" >&2
exit 1
