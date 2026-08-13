#!/usr/bin/env bash
# Concern: runs every mechanical gate in one command | Non-concern: the thresholds each stage enforces (each stage's config owns those) | IO: (the tree) -> per-stage verdicts; exit 1 on any failure
set -uo pipefail

cd "$(git rev-parse --show-toplevel)" || exit 1

LOG_DIR="${TMPDIR:-/tmp}/fsa1-gate.$$"
mkdir -p "$LOG_DIR"
trap 'rm -rf "$LOG_DIR"' EXIT

failed=0

# fsa1-html's build.rs embeds the pinned runtime, so a fresh clone cannot compile without it. Fetched
# here rather than left to the developer: a gate that fails on a clean checkout teaches nothing.
if [ -z "${FSA1_VEGA_BUNDLE:-}" ] && [ ! -f crates/fsa1-html/vendor/vega-bundle.js ]; then
	printf 'vega bundle    '
	if bash scripts/fetch-vega.sh >"$LOG_DIR/vega.log" 2>&1; then
		printf 'fetched\n'
	else
		printf 'FAILED -- see %s\n' "$LOG_DIR/vega.log"
		exit 1
	fi
fi

fail() {
	printf 'FAILED\n'
	tail -30 "$1" >&2
	printf '  --- full log: %s ---\n\n' "$1" >&2
	trap - EXIT
	failed=1
}

stage() {
	local name="$1"; shift
	local log="$LOG_DIR/${name//[^a-zA-Z0-9]/_}.log"
	printf '%-24s ' "$name"
	if "$@" >"$log" 2>&1; then printf 'ok\n'; else fail "$log"; fi
}

echo "fsa1 gate"
echo

stage 'fmt' cargo fmt --all -- --check
stage 'clippy' cargo clippy --workspace --all-targets --locked -- -D warnings

# `cargo test` exits 0 when nothing matched, so an empty run is checked for explicitly.
printf '%-24s ' 'test'
test_log="$LOG_DIR/test.log"
if ! cargo test --workspace -- --include-ignored >"$test_log" 2>&1; then
	fail "$test_log"
elif ! grep -qE '^test result: ok\. [1-9][0-9]* passed' "$test_log"; then
	printf 'FAILED (zero tests ran)\n'
	trap - EXIT
	failed=1
else
	printf 'ok (%s passed)\n' "$(grep '^test result' "$test_log" | awk '{p+=$4} END {print p+0}')"
fi

stage 'annotations' ./scripts/annotation-gate.sh
stage 'comments' ./scripts/comment-gate.sh
stage 'site' ./scripts/render-site.sh --check
stage 'pre-commit selftest' bash .githooks/pre-commit.selftest.sh

echo
[ "$failed" -eq 0 ] || { echo 'gate: FAILED' >&2; exit 1; }
echo 'gate: OK'
