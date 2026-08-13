#!/usr/bin/env bash
# Concern: runs the floored annotated-tree strict-check over the repo | Non-concern: the charter rules (.annotated-tree.toml owns them) | IO: (the tree) -> diagnostics; exit 1 on any finding
#
# THE single call site. A duplicated exclusion list once made CI check 149 files where the hook
# checked 156, so seven serde graders were gated locally and not in CI.
#
# A FLOOR, not an exact version: a newer tool finds MORE, never fewer, so a clone above it fails
# safe. npx installs the floor exactly, keeping CI reproducible.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

MIN=0.7.0

# The conformance corpora are graded oracle INPUTS, not authored source. Their data files carry no
# recognized code extension, but their .md/.py ledgers would otherwise be governed. The GRADERS are
# NOT excluded and stay annotated (the Python harness, and every tests/*.rs).
ARGS=(--strict-check --include-tests
	-I 'conformance/encoding/**'
	-I 'conformance/formula/**'
	-I 'conformance/render/**'
	-I 'conformance/serde/accept/**'
	-I 'conformance/serde/refuse/**'
	# The landing page's fixture workbook is CONTENT the page renders, not authored source: a
	# <name>.annotation sidecar would join the workbook and show up in the `ls -R` the page prints,
	# and a comment would be rendered as a cell's bytes. Its directory charter is the whole map.
	-I 'website/fixture/**'
	# The npm manifests describe a PUBLISHING channel, not a crate in this workspace's graph, so
	# the orphan rule has nothing real to say about them. Their scripts and shim stay governed.
	-I 'distribution/npm/**/package.json'
	.)

npm_bin="${npm_config_prefix:-$HOME/.npm-global}/bin"
case ":$PATH:" in *":$npm_bin:"*) ;; *) PATH="$npm_bin:$PATH" ;; esac
export PATH

# `|| true` because the probe must not decide the run: under `set -euo pipefail` an absent binary
# (127) or one too broken to answer `--version` takes the script down with it, and the npx fallback
# below — the only path a machine without the tool has, CI included — never runs.
have="$(annotated-tree --version 2>/dev/null | tr -dc '0-9.' || true)"
if [ -n "$have" ] && [ "$(printf '%s\n%s\n' "$MIN" "$have" | sort -V | head -1)" = "$MIN" ]; then
	exec annotated-tree "${ARGS[@]}"
elif command -v npx >/dev/null 2>&1; then
	exec npx --yes "annotated-tree@$MIN" "${ARGS[@]}"
fi

echo "annotation-gate: need annotated-tree >= $MIN (have ${have:-none}); install: 'npm i -g annotated-tree', or ensure npx is on PATH" >&2
exit 1
