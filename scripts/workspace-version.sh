#!/usr/bin/env bash
# Concern: prints the version [workspace.package] declares | Non-concern: which surfaces must agree with it (.github/workflows/) | IO: (the root Cargo.toml) -> one version; exit 1 if none is declared
set -uo pipefail

# ONE definition, called by .github/workflows/{ci,plugin-release}.yml. A hand-kept second copy of
# this parser in a workflow is the drift the gates that call it exist to prevent.
#
# The value comes from the workspace MANIFEST, never from a resolved PACKAGE version: `cargo
# metadata`/`cargo pkgid` report for fsa1-cli the very number env!("CARGO_PKG_VERSION") compiled
# into the binary, so holding the binary to it is a tautology that passes by construction -- and
# passes hardest in the case that matters, a crate that stopped inheriting. v0.2.3 shipped a
# fsa1-cli reporting 0.1.0 in exactly that shape.
#
# The root is found from this script's own location rather than from cargo, so the check runs in a
# job that holds a checkout and no toolchain (plugin-release.yml's `versions`).
ROOT=$(cd -P -- "$(dirname -- "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd) || exit 1

v=$(awk '/^\[workspace\.package\]/{f=1;next} /^\[/{f=0}
         f && /^version *= *"/{sub(/^version *= *"/,"");sub(/".*/,"");print;exit}' "$ROOT/Cargo.toml")
[ -n "$v" ] || {
	echo "workspace-version: [workspace.package] in $ROOT/Cargo.toml declares no version" >&2
	exit 1
}
printf '%s\n' "$v"
