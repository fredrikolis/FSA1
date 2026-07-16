#!/usr/bin/env bash
# pre-commit.selftest.sh — calibration harness for the pre-commit hook's fast-check chain.
#
# W0 posture: the pre-commit hook runs fmt -> clippy -> annotated-tree (the conformance backslide
# guard is not wired until W3), so this harness isolates exactly the block/pass mapping of those
# three steps WITHOUT running the real toolchain or scanning this repo. It does so by shadowing
# `cargo` and `annotated-tree` with stubs on PATH: it points CARGO_HOME and npm_config_prefix at a
# temp dir whose `bin/` the hook prepends, and drops stub `cargo`/`annotated-tree` binaries there
# whose exit codes each case drives via env vars:
#   STUB_CARGO_FMT_RC     -> exit of `cargo fmt ...`
#   STUB_CARGO_CLIPPY_RC  -> exit of `cargo clippy ...`
#   STUB_AT_RC            -> exit of `annotated-tree ...`
# Because the stubs live in the exact dir the hook prepends ($CARGO_HOME/bin == $npm_config_prefix/bin),
# they win over any real toolchain on the ambient PATH.
# Run: bash .githooks/pre-commit.selftest.sh
set -uo pipefail

HOOK="$(cd "$(dirname "$0")" && pwd)/pre-commit"

pass=0
fail=0

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Stub toolchain in $TMP/bin — the hook prepends this dir (from CARGO_HOME and npm_config_prefix).
BIN="$TMP/bin"
mkdir -p "$BIN"
cat >"$BIN/cargo" <<'EOF'
#!/usr/bin/env bash
# stub cargo: exit code depends on the subcommand so each fast check can be failed independently.
case "${1:-}" in
	fmt)    exit "${STUB_CARGO_FMT_RC:-0}" ;;
	clippy) exit "${STUB_CARGO_CLIPPY_RC:-0}" ;;
	*)      exit 0 ;;
esac
EOF
cat >"$BIN/annotated-tree" <<'EOF'
#!/usr/bin/env bash
exit "${STUB_AT_RC:-0}"
EOF
chmod +x "$BIN/cargo" "$BIN/annotated-tree"

# check <PASS|BLOCK> <label> <fmt_rc> <clippy_rc> <at_rc>
check() {
	local want="$1" label="$2" got=PASS rc
	CARGO_HOME="$TMP" npm_config_prefix="$TMP" \
		STUB_CARGO_FMT_RC="$3" STUB_CARGO_CLIPPY_RC="$4" STUB_AT_RC="$5" \
		"$HOOK" >/dev/null 2>&1
	rc=$?
	[ "$rc" -eq 0 ] || got=BLOCK
	if [ "$got" = "$want" ]; then
		printf 'PASS  %s\n' "$label"
		pass=$((pass + 1))
	else
		printf 'FAIL  %s  (wanted %s, got %s, rc=%s)\n' "$label" "$want" "$got" "$rc"
		fail=$((fail + 1))
	fi
}

# 1. All fast checks clean -> the hook reaches its OK line.
check PASS 'fmt+clippy+annotated-tree all clean -> PASS' 0 0 0

# 2. fmt fails -> BLOCK (set -e aborts on the first failing check).
check BLOCK 'cargo fmt --check fails -> BLOCK' 1 0 0

# 3. clippy fails -> BLOCK.
check BLOCK 'cargo clippy -D warnings fails -> BLOCK' 0 1 0

# 4. annotated-tree fails -> BLOCK.
check BLOCK 'annotated-tree --strict-check fails -> BLOCK' 0 0 1

# 5. Multiple failures still BLOCK (fmt is first, so it fails fast).
check BLOCK 'all three fail -> BLOCK' 1 1 1

printf '\n%s passed, %s failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
