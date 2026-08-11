// Concern: stamps the version and drops each binary into its package, in publish order | Non-concern: running `npm publish` | IO: (version, binaries) -> a stamped tree + the order to publish it in
//
// Usage:  node distribution/npm/scripts/build-npm.mjs <version> <binaries-dir>
//
// <binaries-dir> holds one binary per published target, at <binaries-dir>/<asset-name>, exactly as
// `gh release download` leaves them. Binaries are NEVER committed — they are injected here, so the
// repo carries the packaging and the release carries the bytes.

import { readFileSync, writeFileSync, copyFileSync, existsSync, chmodSync } from "node:fs";
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

function stampVersion(pkgPath, mutate = (p) => p) {
  const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
  pkg.version = version;
  mutate(pkg);
  writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");
}

const publishDirs = [];

// The front ends come off the MATRIX rather than a list beside it, so adding a row is all it takes.
for (const front of [...new Set(MATRIX.map((r) => r.front))]) {
  for (const { plat, asset, bin } of MATRIX.filter((r) => r.front === front)) {
    const platDir = join(npmDir, front, "platforms", plat);
    const src = join(binariesDir, asset);
    if (!existsSync(src)) {
      console.error(`missing binary for ${front} ${plat}: ${src}`);
      process.exit(1);
    }
    stampVersion(join(platDir, "package.json"));
    const dest = join(platDir, bin);
    copyFileSync(src, dest);
    // The download loses the mode, and a binary npm ships without +x is one no host can spawn.
    chmodSync(dest, 0o755);
    publishDirs.push(platDir);
  }

  // The wrapper pins each optional dependency to this exact version: a range would let npm resolve a
  // binary from a different release than the shim that reads it. It is pushed AFTER its own platform
  // dirs, because a wrapper naming a package that is not published yet resolves to nothing.
  const frontDir = join(npmDir, front);
  stampVersion(join(frontDir, "package.json"), (pkg) => {
    for (const key of Object.keys(pkg.optionalDependencies)) {
      pkg.optionalDependencies[key] = version;
    }
  });
  publishDirs.push(frontDir);
}

console.log(publishDirs.join("\n"));
