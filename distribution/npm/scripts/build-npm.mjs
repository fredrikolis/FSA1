// Concern: holds every npm manifest to the given version and drops each binary into its package | Non-concern: running `npm publish` | IO: (version, binaries) -> package directories, in publish order
//
// Usage:  node distribution/npm/scripts/build-npm.mjs <version> <binaries-dir>
//
// <binaries-dir> holds one binary per published target, at <binaries-dir>/<asset-name>, exactly as
// `gh release download` leaves them. Binaries are NEVER committed — they are injected here, so the
// repo carries the packaging and the release carries the bytes.

import { readFileSync, copyFileSync, existsSync, chmodSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// Single source of truth: front end <-> npm platform <-> release asset <-> the filename inside the
// package. `front` is also the front end's directory under distribution/npm/.
//
// ROW ORDER IS THE PUBLISH ORDER. The mcp rows come first deliberately: those package names are
// already established, and the publish loop runs under `set -e`. A first publish of a brand-new
// name can fail on a name that is unavailable or a trusted-publisher relationship not yet
// configured, and a failure there must not abort the tag before the working channel is updated.
const MATRIX = [
  { front: "mcp", plat: "linux-x64-musl", asset: "fsa1-mcp-linux-x86_64-musl", bin: "fsa1-mcp" },
  { front: "mcp", plat: "linux-arm64-musl", asset: "fsa1-mcp-linux-aarch64-musl", bin: "fsa1-mcp" },
  { front: "mcp", plat: "darwin-x64", asset: "fsa1-mcp-macos-x86_64", bin: "fsa1-mcp" },
  { front: "mcp", plat: "darwin-arm64", asset: "fsa1-mcp-macos-aarch64", bin: "fsa1-mcp" },
  { front: "mcp", plat: "windows-x64", asset: "fsa1-mcp-windows-x86_64.exe", bin: "fsa1-mcp.exe" },
  { front: "cli", plat: "linux-x64-musl", asset: "fsa1-cli-linux-x86_64-musl", bin: "fsa1-cli" },
  { front: "cli", plat: "linux-arm64-musl", asset: "fsa1-cli-linux-aarch64-musl", bin: "fsa1-cli" },
  { front: "cli", plat: "darwin-x64", asset: "fsa1-cli-macos-x86_64", bin: "fsa1-cli" },
  { front: "cli", plat: "darwin-arm64", asset: "fsa1-cli-macos-aarch64", bin: "fsa1-cli" },
  { front: "cli", plat: "windows-x64", asset: "fsa1-cli-windows-x86_64.exe", bin: "fsa1-cli.exe" },
];

const [version, binariesDir] = process.argv.slice(2);
if (!version || !binariesDir) {
  console.error("usage: node build-npm.mjs <version> <binaries-dir>");
  process.exit(2);
}

const npmDir = dirname(dirname(fileURLToPath(import.meta.url)));

// The committed number is the real one and nothing here writes it back: a value stamped at publish
// time is a value no reader and no gate can check. `pins` is a wrapper's EXPECTED
// optionalDependencies names, off the MATRIX; null for a platform package.
//
// The pin SET is held, not just the values of whichever pins exist: iterating the block checks
// nothing when the block is absent, and a wrapper published that way resolves no binary on any
// platform. A single missing key is quieter still — four platforms ship and the fifth is
// unreachable — so a missing key, an absent block and an unexpected extra each refuse by name. Each
// pin is this exact version: a range would let npm resolve a binary from a different release than
// the shim that reads it.
function disagreements(pkgPath, pins) {
  const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
  const found = pkg.optionalDependencies ?? {};
  const declared = [["version", pkg.version]];
  const unexpected = [];
  if (pins) {
    for (const name of pins) declared.push([`optionalDependencies.${name}`, found[name]]);
    for (const name of Object.keys(found)) {
      if (!pins.includes(name)) unexpected.push(`${pkgPath}: optionalDependencies.${name} is not a MATRIX platform package`);
    }
  }
  return declared
    .filter(([, got]) => got !== version)
    .map(([key, got]) => `${pkgPath}: ${key} is ${got ?? "<missing>"}, expected ${version}`)
    .concat(unexpected);
}

// The front ends come off the MATRIX rather than a list beside it, so adding a row is all it takes,
// and so does the platform-package name each wrapper must pin.
const fronts = [...new Set(MATRIX.map((r) => r.front))];
const pinsFor = (front) => MATRIX.filter((r) => r.front === front).map((r) => `fsa1-${front}-${r.plat}`);

// Every manifest is read before a single binary is copied, so a refusal leaves the tree untouched.
const wrong = [
  ...MATRIX.map((r) => disagreements(join(npmDir, r.front, "platforms", r.plat, "package.json"), null)),
  ...fronts.map((front) => disagreements(join(npmDir, front, "package.json"), pinsFor(front))),
].flat();
if (wrong.length) {
  console.error(wrong.join("\n"));
  process.exit(1);
}

const publishDirs = [];

for (const front of fronts) {
  for (const { plat, asset, bin } of MATRIX.filter((r) => r.front === front)) {
    const platDir = join(npmDir, front, "platforms", plat);
    const src = join(binariesDir, asset);
    if (!existsSync(src)) {
      console.error(`missing binary for ${front} ${plat}: ${src}`);
      process.exit(1);
    }
    const dest = join(platDir, bin);
    copyFileSync(src, dest);
    // The download loses the mode, and a binary npm ships without +x is one no host can spawn.
    chmodSync(dest, 0o755);
    publishDirs.push(platDir);
  }

  // The wrapper is pushed AFTER its own platform dirs, because a wrapper naming a package that is
  // not published yet resolves to nothing.
  publishDirs.push(join(npmDir, front));
}

console.log(publishDirs.join("\n"));
