#!/usr/bin/env bash
# Concern: the one-command entrypoint for the ENG6 differential oracle(s) — ensure charlie-cli is built, activate the local venv (creating it + pip-installing the PINNED reference stack from requirements.txt on first run), and run EITHER the per-formula oracle.py (default) OR the whole-workbook workbook_oracle.py (`--workbook`), forwarding its exit code | Non-concern: WHAT the oracles grade (oracle.py / workbook_oracle.py own the harnesses), the corpus content (corpus/*.json, corpus_workbooks/*.xlsx), and WHICH versions pin the reference (requirements.txt owns that) | IO: (in: the repo, requirements.txt, network on first-run pip install; args: `--workbook` selects the whole-workbook oracle) -> builds target/debug/charlie-cli if absent, prints the parity table, exits with the selected oracle's code
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$HERE/../.." && pwd)"
VENV="$HERE/.venv"

# Build the reference-under-test CLI if it is not already present.
if [[ ! -x "$REPO_ROOT/target/debug/charlie-cli" ]]; then
  ( cd "$REPO_ROOT" && cargo build -p charlie-cli )
fi

# First-run: create the venv and install the PINNED reference stack (gitignored, never committed).
# Versions are locked in requirements.txt so the documented reference (formulas v1.3.4) is the
# enforced reference — a rerun cannot silently pull a newer, semantically-different version.
if [[ ! -x "$VENV/bin/python" ]]; then
  python3 -m venv "$VENV"
  "$VENV/bin/pip" install --quiet --upgrade pip
  "$VENV/bin/pip" install --quiet -r "$HERE/requirements.txt"
fi

# Select the oracle: `--workbook` runs the whole-workbook harness (real .xlsx files); default is the
# per-formula corpus. Both share the venv, the reference stack, and oracle.py's comparison logic.
if [[ "${1:-}" == "--workbook" ]]; then
  shift
  exec "$VENV/bin/python" "$HERE/workbook_oracle.py" "$@"
fi
exec "$VENV/bin/python" "$HERE/oracle.py" "$@"
