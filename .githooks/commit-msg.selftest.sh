#!/usr/bin/env bash
# commit-msg.selftest.sh — standalone calibration harness for the commit-msg hook. Adapted from
# fanuc's; the parser is identical, so these rows lock the same quirks (the ` vs ` delimiter, the
# keyword-anywhere count match, the severity gate, the state-gated Merge/Revert auto-skip).
# Most rows check the MESSAGE FILE only (the trailer checks need no staged diff): write a message to a
# temp file and invoke `"$HOOK" <msgfile>`, asserting the exit code. The Merge/Revert auto-skip is
# STATE-gated (it consults MERGE_HEAD/REVERT_HEAD, not the subject prefix alone), so those rows run
# against an ISOLATED temp git repo whose git-dir we control via GIT_DIR, letting us touch/remove
# MERGE_HEAD/REVERT_HEAD to simulate an operation in flight. Location-independent: finds the hook
# relative to its own path and touches only temp dirs. Run: bash .githooks/commit-msg.selftest.sh
set -uo pipefail

HOOK="$(cd "$(dirname "$0")" && pwd)/commit-msg"

pass=0
fail=0

TMP="$(mktemp -d)"
# Isolated repo whose git-dir the Merge/Revert rows drive via GIT_DIR (real `git init` so
# `git rev-parse --git-dir` resolves exactly this repo; GD is its git-dir).
REPO="$(mktemp -d)"
git init -q "$REPO"
git -C "$REPO" config user.email selftest@example.com
git -C "$REPO" config user.name selftest
GD="$REPO/.git"
trap 'rm -rf "$TMP" "$REPO"' EXIT

# check <PASS|BLOCK> <label> <message-body...>
# Writes the message to a temp file and runs the hook against it (ambient cwd); asserts the exit code.
check() {
	local want="$1" label="$2" msg="$3" got=PASS rc mf
	mf="$TMP/msg_${RANDOM}_${RANDOM}.txt"
	printf '%s\n' "$msg" >"$mf"
	"$HOOK" "$mf" >/dev/null 2>&1
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

# check_gitdir <PASS|BLOCK> <label> <message> — like `check`, but runs the hook with GIT_DIR pointed at
# the isolated repo $GD, so `git rev-parse --git-dir` (and thus the MERGE_HEAD/REVERT_HEAD probe) sees a
# state we control. Caller sets up/removes $GD/MERGE_HEAD or $GD/REVERT_HEAD around the call.
check_gitdir() {
	local want="$1" label="$2" msg="$3" got=PASS rc mf
	mf="$TMP/msg_${RANDOM}_${RANDOM}.txt"
	printf '%s\n' "$msg" >"$mf"
	GIT_DIR="$GD" "$HOOK" "$mf" >/dev/null 2>&1
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

# Fully-valid attestation blocks reused across rows.
A_OK='Reviewed: by wf_deadbeef vs ast+lang+testing — major=0 moderate=0 minor=0'
B_OK=$'Annotation-Reviewer: wf_annotate\nAnnotation-Issues: 0'

# --- both attestations present -> PASS -------------------------------------------------------
check PASS 'A (major=0 moderate=0) + B -> PASS' \
	"$(printf 'change subject\n\n%s\n%s' "$A_OK" "$B_OK")"

# --- ATTESTATION A: severity gate (B held valid) --------------------------------------------
check BLOCK 'no Reviewed line (A absent) -> BLOCK' \
	"$(printf 'change subject\n\n%s' "$B_OK")"

check BLOCK 'Reviewed malformed: no " vs <tag>" -> BLOCK' \
	"$(printf 'change subject\n\nReviewed: by wf_x — major=0 moderate=0 minor=0\n%s' "$B_OK")"

check BLOCK 'Reviewed missing severity counts -> BLOCK' \
	"$(printf 'change subject\n\nReviewed: by wf_x vs ast+lang+testing\n%s' "$B_OK")"

check BLOCK 'Reviewed partial counts (no minor) -> BLOCK' \
	"$(printf 'change subject\n\nReviewed: by wf_x vs ast+lang+testing — major=0 moderate=0\n%s' "$B_OK")"

check BLOCK 'major=1 unresolved -> BLOCK' \
	"$(printf 'change subject\n\nReviewed: by wf_x vs ast+lang+testing — major=1 moderate=0 minor=0\n%s' "$B_OK")"

check BLOCK 'moderate=2 unresolved -> BLOCK' \
	"$(printf 'change subject\n\nReviewed: by wf_x vs ast+lang+testing — major=0 moderate=2 minor=0\n%s' "$B_OK")"

check PASS 'minor=5 (discretionary, non-blocking) -> PASS' \
	"$(printf 'change subject\n\nReviewed: by wf_x vs ast+lang+testing — major=0 moderate=0 minor=5\n%s' "$B_OK")"

check PASS 'Review-skip: (A vent) -> PASS' \
	"$(printf 'change subject\n\nReview-skip: mechanical workflow-only change\n%s' "$B_OK")"

check PASS 'multi-word reviewer label -> PASS' \
	"$(printf 'change subject\n\nReviewed: by neutral review agent vs ast+lang+testing — major=0 moderate=0 minor=0\n%s' "$B_OK")"

# ` vs ` is the reviewer|tag delimiter. TWO frozen behaviours around it:
# (a) A reviewer label that embeds ` vs ` but still supplies all three counts mis-splits
#     (reviewer="a", tag="b vs ...") yet STILL PASSES, because counts parse anywhere. This documents
#     that the delimiter is greedy-left and the parser does not defend the absurd embedded-` vs ` edge.
check PASS 'reviewer label containing " vs " WITH counts (documented mis-split, still PASS)' \
	"$(printf 'change subject\n\nReviewed: by a vs b vs ast+lang — major=0 moderate=0 minor=0\n%s' "$B_OK")"
# (b) A reviewer label that embeds ` vs ` and supplies NO counts (author thought the whole phrase was
#     the reviewer and forgot the counts) BLOCKS — the ` vs ` split does not rescue a countless line.
check BLOCK 'reviewer label containing " vs " but MISSING counts -> BLOCK' \
	"$(printf 'change subject\n\nReviewed: by security vs perf audit\n%s' "$B_OK")"

# The malformed-flags reset per Reviewed: line: the LAST line's validity decides. A malformed line
# followed by a valid one PASSES (the earlier struct/counts failure does not stick). Fail-safe otherwise.
check PASS 'malformed Reviewed line then a valid Reviewed line -> PASS' \
	"$(printf 'change subject\n\nReviewed: by wf_x — major=0 moderate=0 minor=0\nReviewed: by wf_x vs ast+lang+testing — major=0 moderate=0 minor=0\n%s' "$B_OK")"

# --- ATTESTATION B (A held valid) ------------------------------------------------------------
check BLOCK 'missing Annotation-Reviewer -> BLOCK' \
	"$(printf 'change subject\n\n%s\nAnnotation-Issues: 0' "$A_OK")"

check BLOCK 'missing Annotation-Issues -> BLOCK' \
	"$(printf 'change subject\n\n%s\nAnnotation-Reviewer: wf_annotate' "$A_OK")"

check BLOCK 'Annotation-Issues: 2 (non-zero) -> BLOCK' \
	"$(printf 'change subject\n\n%s\nAnnotation-Reviewer: wf_annotate\nAnnotation-Issues: 2' "$A_OK")"

# Trailing text after the count is tolerated (regression guard for the relaxed regex).
check PASS 'Annotation-Issues: 0 with trailing text -> PASS' \
	"$(printf 'change subject\n\n%s\nAnnotation-Reviewer: wf_annotate\nAnnotation-Issues: 0  (all clean)' "$A_OK")"

check PASS 'Annotation-skip: (B vent) -> PASS' \
	"$(printf 'change subject\n\n%s\nAnnotation-skip: pure corpus/fixture data change, nothing annotatable' "$A_OK")"

# --- interaction: both required --------------------------------------------------------------
check BLOCK 'A present, B absent -> BLOCK' \
	"$(printf 'change subject\n\n%s' "$A_OK")"

check BLOCK 'B present, A absent -> BLOCK' \
	"$(printf 'change subject\n\n%s' "$B_OK")"

check BLOCK 'neither A nor B -> BLOCK' \
	"$(printf 'change subject\n\njust a body line, no trailers')"

# --- auto-skip subjects (exit 0 regardless of missing trailers) ------------------------------
# Merge/Revert auto-skip is STATE-gated: it fires ONLY when git records the operation in flight
# (MERGE_HEAD/REVERT_HEAD). A bare subject prefix with no such state is a real authored change -> BLOCK.
rm -f "$GD/MERGE_HEAD" "$GD/REVERT_HEAD"
check_gitdir BLOCK 'Merge subject WITHOUT MERGE_HEAD (no trailers) -> BLOCK' 'Merge branch feature into main'
touch "$GD/MERGE_HEAD"
check_gitdir PASS 'Merge subject WITH MERGE_HEAD -> PASS (auto-skip)' 'Merge branch feature into main'
check_gitdir PASS 'leading blank + Merge subject WITH MERGE_HEAD -> PASS (auto-skip)' "$(printf '\n\nMerge branch x')"
rm -f "$GD/MERGE_HEAD"

check_gitdir BLOCK 'Revert subject WITHOUT REVERT_HEAD (no trailers) -> BLOCK' "$(printf 'Revert "some earlier commit"\n\nThis reverts commit deadbeef.')"
touch "$GD/REVERT_HEAD"
check_gitdir PASS 'Revert subject WITH REVERT_HEAD -> PASS (auto-skip)' "$(printf 'Revert "some earlier commit"\n\nThis reverts commit deadbeef.')"
rm -f "$GD/REVERT_HEAD"

# fixup!/squash! have NO commit-time state signal -> trusted by subject PREFIX ALONE (ACCEPTED HOLE #2).
check PASS 'fixup! subject -> PASS (subject-only skip)' 'fixup! earlier subject'
check PASS 'squash! subject -> PASS (subject-only skip)' 'squash! earlier subject'

# --- both vents together -> PASS -------------------------------------------------------------
check PASS 'Review-skip + Annotation-skip -> PASS' \
	"$(printf 'regen artifact\n\nReview-skip: generated file only\nAnnotation-skip: generated fixture, nothing annotatable')"

# --- comment-only message: no real subject -> not auto-skipped -> BLOCK -----------------------
check BLOCK 'comment-only message (no subject) -> BLOCK' \
	"$(printf '# a comment only\n# another comment')"

printf '\n%s passed, %s failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
