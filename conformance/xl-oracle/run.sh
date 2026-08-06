#!/usr/bin/env bash
# Concern: provisions and runs the differential oracles | Non-concern: what the oracles grade, the corpora, the pinned versions | IO: (--workbook) -> a parity table
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

if [[ "${1:-}" == "--workbook" ]]; then
  shift
  exec "$VENV/bin/python" "$HERE/workbook_oracle.py" "$@"
fi
exec "$VENV/bin/python" "$HERE/oracle.py" "$@"
