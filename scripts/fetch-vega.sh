#!/usr/bin/env bash
# Concern: fetches the pinned Vega runtime and verifies it against the checked-in manifest | Non-concern: what the page does with it | IO: (manifest) -> a gitignored bundle
#
# The runtime is a megabyte of somebody else's minified JavaScript. Committing it would put a blob
# nobody reviews under version control and make every `git log -p` unreadable, so the repo carries
# the PIN and the build carries the bytes. A hash mismatch is fatal: an unverifiable download is
# never a passing one.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

MANIFEST=vega-manifest.txt
BUNDLE=crates/fsa1-html/vendor/vega-bundle.js

# macOS ships `shasum`, GNU ships `sha256sum`; CI runs both hosts.
sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi; }

mkdir -p "$(dirname "$BUNDLE")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

: >"$work/bundle.js"
while read -r name version sha url; do
	case "$name" in ''|\#*) continue ;; esac
	printf 'fetching %s@%s\n' "$name" "$version" >&2
	curl -sfL "$url" -o "$work/$name.js"
	got="$(sha256 "$work/$name.js" | cut -d' ' -f1)"
	if [ "$got" != "$sha" ]; then
		printf '%s@%s does not match its pin\n  want %s\n  got  %s\n' \
			"$name" "$version" "$sha" "$got" >&2
		exit 1
	fi
	# A newline between parts: each build is minified and may not end in one.
	cat "$work/$name.js" >>"$work/bundle.js"
	printf '\n' >>"$work/bundle.js"
done <"$MANIFEST"

mv "$work/bundle.js" "$BUNDLE"
printf 'wrote %s (%s bytes)\n' "$BUNDLE" "$(wc -c <"$BUNDLE")" >&2
