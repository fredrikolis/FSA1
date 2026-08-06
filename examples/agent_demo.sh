#!/usr/bin/env bash
# Concern: shows an agent creating, reading and editing an FSA1 workbook with plain shell tools | Non-concern: the full verb surface (`fsa1-cli --help` owns it) | IO: () -> stdout
#
# The point of FSA1: a workbook is a directory, so the tools you already have ARE the editor.
# No library, no API — `ls`, `cat`, and a redirect.
set -euo pipefail

CLI="${FSA1_CLI:-fsa1-cli}"
WB="$(mktemp -d)/demo"
trap 'rm -rf "$(dirname "$WB")"' EXIT

echo "== 1. write a workbook =="
"$CLI" sample "$WB"
find "$WB" -type f | sort | sed "s|$WB|<wb>|"

echo
echo "== 2. a file's NAME is the range it fills; its CONTENT is TSV =="
cat "$WB/Orders/A1:D1"
echo "--- D2:D4 (formulas, one per row) ---"
cat "$WB/Orders/D2:D4"

echo
echo "== 3. render it =="
"$CLI" render "$WB/Orders"

echo
echo "== 4. edit a cell with a redirect, then re-render =="
printf '99\n15\n4\n' > "$WB/Orders/B2:B4"
"$CLI" render "$WB/Orders"

echo
echo "== 5. lint =="
"$CLI" check "$WB"
