#!/usr/bin/env bash
# Concern: provisions and runs the SER2 round-trip oracle | Non-concern: what the oracle grades, the corpus, the pinned versions | IO: () -> a parity table
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$HERE/../.." && pwd)"
VENV="$HERE/.venv"

if [[ ! -x "$REPO_ROOT/target/debug/fsa1-cli" ]]; then
  ( cd "$REPO_ROOT" && cargo build -p fsa1-cli )
fi

if [[ ! -x "$VENV/bin/python" ]]; then
  python3 -m venv "$VENV"
  "$VENV/bin/pip" install --quiet --upgrade pip
  "$VENV/bin/pip" install --quiet -r "$HERE/requirements.txt"
fi

# roundtrip_oracle.py imports the sibling xl-oracle harness by bare module name.
export PYTHONPATH="$REPO_ROOT/conformance/xl-oracle${PYTHONPATH:+:$PYTHONPATH}"

echo "=== numfmt_render golden selftest ==="
"$VENV/bin/python" "$HERE/numfmt_render.py" --selftest

exec "$VENV/bin/python" "$HERE/roundtrip_oracle.py" "$@"
