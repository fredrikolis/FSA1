#!/usr/bin/env bash
# pre-commit.selftest.sh — calibration harness for the pre-commit hook's two-stage chain: the fast
# checks (fmt -> clippy -> annotated-tree) AND the conformance BACKSLIDE state-guard wired in W3.
#
# It shadows `cargo` and `annotated-tree` with stubs on PATH (pointing CARGO_HOME and npm_config_prefix
# at a temp dir whose `bin/` the hook prepends), so no real toolchain runs and this repo is never
# scanned. Two independent axes are driven by env vars:
#   STUB_CARGO_FMT_RC / STUB_CARGO_CLIPPY_RC -> exit of `cargo fmt` / `cargo clippy`
#   STUB_AT_RC                               -> exit of `annotated-tree`
#   STUB_BACKSLIDE_RC                        -> exit of the backslide guard, injected via the hook's
#                                               own $PRECOMMIT_BACKSLIDE_CMD test seam (so the real
#                                               `cargo run -p conformance -- backslide` is bypassed)
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
# stub cargo: fmt/clippy exit codes are driven per subcommand; a real `cargo run -p conformance --
# backslide` is bypassed by the hook's $PRECOMMIT_BACKSLIDE_CMD seam, so it never reaches here.
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
# The backslide stub the hook calls via $PRECOMMIT_BACKSLIDE_CMD; its exit code is $STUB_BACKSLIDE_RC.
BACKSLIDE_STUB="$TMP/backslide_stub.sh"
cat >"$BACKSLIDE_STUB" <<'EOF'
#!/usr/bin/env bash
exit "${STUB_BACKSLIDE_RC:-0}"
EOF
chmod +x "$BIN/cargo" "$BIN/annotated-tree" "$BACKSLIDE_STUB"

# check <PASS|BLOCK> <label> <fmt_rc> <clippy_rc> <at_rc> [backslide_rc]
# A missing backslide_rc leaves the seam UNSET (the hook then calls the stub `cargo run` -> exit 0).
check() {
	local want="$1" label="$2" got=PASS rc
	local seam=()
	if [ "$#" -ge 6 ]; then
		seam=(PRECOMMIT_BACKSLIDE_CMD="$BACKSLIDE_STUB" STUB_BACKSLIDE_RC="$6")
	fi
	env CARGO_HOME="$TMP" npm_config_prefix="$TMP" \
		STUB_CARGO_FMT_RC="$3" STUB_CARGO_CLIPPY_RC="$4" STUB_AT_RC="$5" \
		"${seam[@]}" \
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

# --- Fast-check chain (fmt -> clippy -> annotated-tree). With all clean, the hook now ALSO reaches
#     the backslide guard; the seam is unset here, so the stub `cargo run` returns 0 -> still PASS. ---
check PASS 'fmt+clippy+annotated-tree all clean (backslide via stub cargo=0) -> PASS' 0 0 0
check BLOCK 'cargo fmt --check fails -> BLOCK' 1 0 0
check BLOCK 'cargo clippy -D warnings fails -> BLOCK' 0 1 0
check BLOCK 'annotated-tree --strict-check fails -> BLOCK' 0 0 1
check BLOCK 'all three fast checks fail -> BLOCK' 1 1 1

# --- Backslide branch (fast checks all clean; exit code injected via the $PRECOMMIT_BACKSLIDE_CMD
#     seam). The W3 contract: 0 PASS / 1 BLOCK / 2 BLOCK (fail-safe) / other BLOCK (fail-safe). ---
check PASS  'backslide exit 0 (no fact lost) -> PASS' 0 0 0 0
check BLOCK 'backslide exit 1 (>=1 lost Match) -> BLOCK' 0 0 0 1
check BLOCK 'backslide exit 2 (anchor unreadable) -> BLOCK (fail-safe)' 0 0 0 2
check BLOCK 'backslide exit 3 (unexpected) -> BLOCK (fail-safe)' 0 0 0 3

# --- The fast checks gate the backslide guard: a fmt failure BLOCKs BEFORE backslide would pass. ---
check BLOCK 'fmt fails even though backslide would be 0 -> BLOCK (order: fast checks first)' 1 0 0 0

printf '\n%s passed, %s failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
