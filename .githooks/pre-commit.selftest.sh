#!/usr/bin/env bash
# pre-commit.selftest.sh — calibration harness for the pre-commit hook's full chain: the fast checks
# (fmt -> clippy -> comment budget -> annotated-tree).
#
# It shadows `cargo` and `annotated-tree` with stubs on PATH (pointing CARGO_HOME and npm_config_prefix
# at a temp dir whose `bin/` the hook prepends), so no real toolchain runs and THIS REPO IS NEVER
# SCANNED — a row's verdict can never depend on the state of the working tree. Every axis is
# env-driven:
#   STUB_CARGO_FMT_RC / STUB_CARGO_CLIPPY_RC -> exit of `cargo fmt` / `cargo clippy`
#   STUB_AT_RC                               -> exit of the annotation gate
#   COMMENT_RC (per row, defaults to 0)      -> exit of the comment budget, injected via the hook's
#                                               $PRECOMMIT_COMMENT_GATE_CMD test seam (so the real
#                                               scripts/comment-gate.sh never lints this repo)
#   AT_VERSION (per row, defaults to the gate's own pin) / NPX_RC -> which annotated-tree
#                                               scripts/annotation-gate.sh picks (installed binary vs
#                                               fetched pin) and how the fetched-pin fallback exits.
#                                               Those rows call the REAL annotation-gate.sh, since it
#                                               owns the pin logic; every other row stubs the stage.
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
case "${1:-}" in
	fmt)    exit "${STUB_CARGO_FMT_RC:-0}" ;;
	clippy) exit "${STUB_CARGO_CLIPPY_RC:-0}" ;;
	*)      exit 0 ;;
esac
EOF
# The hook USES a locally-installed annotated-tree only when its version equals the hook's pin, so the
# stub must answer --version. $STUB_AT_VERSION defaults to the pin READ OUT OF THE HOOK (never restated
# here — a second copy of the pin is the very drift the pin check exists to catch), and a row can set it
# to something else to exercise the mismatch branch.
GATE="$(cd "$(dirname "$0")/../scripts" && pwd)/annotation-gate.sh"
AT_MIN="$(sed -n 's/^MIN=//p' "$GATE")"
[ -n "$AT_MIN" ] || { echo "selftest: cannot read MIN from $GATE" >&2; exit 1; }
cat >"$BIN/annotated-tree" <<'EOF'
#!/usr/bin/env bash
if [ "${1:-}" = --version ]; then printf 'annotated-tree %s\n' "${STUB_AT_VERSION:?}"; exit 0; fi
exit "${STUB_AT_RC:-0}"
EOF
# npx stands in for the fetch-the-pin fallback the hook takes when no matching binary is installed.
# Stubbed so the mismatch branch is observable and deterministic instead of depending on the network.
cat >"$BIN/npx" <<'EOF'
#!/usr/bin/env bash
exit "${STUB_NPX_RC:-0}"
EOF
# The comment-budget stub the hook calls via $PRECOMMIT_COMMENT_GATE_CMD; its exit code is
# $STUB_COMMENT_RC. Stubbed rather than run for real so no row lints this repo — a selftest that
# depended on the repo's own comment volume would go red on an unrelated source edit.
COMMENT_STUB="$TMP/comment_gate_stub.sh"
cat >"$COMMENT_STUB" <<'EOF'
#!/usr/bin/env bash
exit "${STUB_COMMENT_RC:-0}"
EOF
# The annotation stub the hook calls via $PRECOMMIT_ANNOTATION_CMD, so no ordinary row scans this repo.
ANNOTATION_STUB="$TMP/annotation_gate_stub.sh"
cat >"$ANNOTATION_STUB" <<'EOF'
#!/usr/bin/env bash
exit "${STUB_AT_RC:-0}"
EOF

chmod +x "$BIN/cargo" "$BIN/annotated-tree" "$BIN/npx" "$COMMENT_STUB" "$ANNOTATION_STUB"

# check <PASS|BLOCK> <label> <fmt_rc> <clippy_rc> <at_rc>
# Prefix a row with COMMENT_RC=<n> to drive the comment-budget stage's exit code (defaults to 0/clean),
# or AT_VERSION=<v> / NPX_RC=<n> to drive which annotated-tree the hook picks and how the fetched-pin
# fallback exits.
run_row() { # $1..$3 fmt/clippy/at rc; $4 annotation seam ('' = call the REAL annotation-gate.sh)
	env CARGO_HOME="$TMP" npm_config_prefix="$TMP" \
		STUB_CARGO_FMT_RC="$1" STUB_CARGO_CLIPPY_RC="$2" STUB_AT_RC="$3" \
		PRECOMMIT_COMMENT_GATE_CMD="$COMMENT_STUB" STUB_COMMENT_RC="${COMMENT_RC:-0}" \
		PRECOMMIT_ANNOTATION_CMD="$4" \
		STUB_AT_VERSION="${AT_VERSION:-$AT_MIN}" STUB_NPX_RC="${NPX_RC:-0}" \
		"$HOOK" >/dev/null 2>&1
}

check() {
	local want="$1" label="$2" got=PASS rc
	run_row "$3" "$4" "$5" "$ANNOTATION_STUB"
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
check PASS 'fmt+clippy+annotated-tree all clean -> PASS' 0 0 0
check BLOCK 'cargo fmt --check fails -> BLOCK' 1 0 0
check BLOCK 'cargo clippy -D warnings fails -> BLOCK' 0 1 0
check BLOCK 'annotated-tree --strict-check fails -> BLOCK' 0 0 1
check BLOCK 'all three fast checks fail -> BLOCK' 1 1 1

# --- Comment BUDGET stage (exit code injected via the $PRECOMMIT_COMMENT_GATE_CMD seam, so no row
#     lints this repo). It blocks on its own, and it runs among the fast checks — BEFORE the

COMMENT_RC=1 \
	check BLOCK 'comment budget over a deny threshold -> BLOCK' 0 0 0
COMMENT_RC=1 \
	check BLOCK 'comment budget BLOCKs -> BLOCK' 0 0 0
COMMENT_RC=0 \
	check PASS  'comment budget clean -> PASS' 0 0 0

# check_pin — like `check`, but leaves the annotation seam UNSET so the hook calls the real
# scripts/annotation-gate.sh, which is where the pin logic now lives.
check_pin() {
	local want="$1" label="$2" got=PASS rc
	run_row "$3" "$4" "$5" ""
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

# --- annotated-tree VERSION PIN. The regression this locks: the hook used to run whatever
#     annotated-tree was installed, at any version. A 0.6.0 binary reported four over-length
#     annotations that the 0.5.0 pinned in CI passed, so the gate was green on one clone, red on
#     another, and CI agreed with neither. A load-bearing gate has to be ONE gate.
#     Matching version -> the local binary decides. Mismatched -> it is IGNORED and the pin is
#     fetched, so a stale local binary can neither pass nor fail the commit. ---
check_pin PASS  'installed annotated-tree AT the pin, clean -> PASS' 0 0 0
check_pin BLOCK 'installed annotated-tree AT the pin, failing -> BLOCK' 0 0 1
AT_VERSION=0.5.0 NPX_RC=0 \
	check_pin PASS  'installed annotated-tree OFF the pin: ignored, fetched pin is clean -> PASS' 0 0 1
AT_VERSION=0.5.0 NPX_RC=1 \
	check_pin BLOCK 'installed annotated-tree OFF the pin: ignored, fetched pin fails -> BLOCK' 0 0 0

check BLOCK 'fmt fails -> BLOCK before any later stage runs' 1 0 0

printf '\n%s passed, %s failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
