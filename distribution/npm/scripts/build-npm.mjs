// Concern: stamps the release version across the wrapper and platform packages and drops each release binary into its platform directory | Non-concern: publishing (the workflow does that), building a binary | IO: (version, binaries-dir) -> an in-place distribution/npm tree + the publish order
//
// Usage:  node distribution/npm/scripts/build-npm.mjs <version> <binaries-dir>
//
// <binaries-dir> holds one binary per published target, at <binaries-dir>/<asset-name>, exactly as
// `gh release download` leaves them. Binaries are NEVER committed — they are injected here, so the
// repo carries the packaging and the release carries the bytes.

import { readFileSync, writeFileSync, copyFileSync, existsSync, chmodSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// Single source of truth: npm platform <-> release asset <-> the filename inside the package.
const MATRIX = [
  { plat: "linux-x64-musl", asset: "fsa1-mcp-linux-x86_64-musl", bin: "fsa1-mcp" },
  { plat: "linux-arm64-musl", asset: "fsa1-mcp-linux-aarch64-musl", bin: "fsa1-mcp" },
  { plat: "darwin-x64", asset: "fsa1-mcp-macos-x86_64", bin: "fsa1-mcp" },
  { plat: "darwin-arm64", asset: "fsa1-mcp-macos-aarch64", bin: "fsa1-mcp" },
  { plat: "win32-x64", asset: "fsa1-mcp-windows-x86_64.exe", bin: "fsa1-mcp.exe" },
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

for (const { plat, asset, bin } of MATRIX) {
  const platDir = join(npmDir, "platforms", plat);
  const src = join(binariesDir, asset);
  if (!existsSync(src)) {
    console.error(`missing binary for ${plat}: ${src}`);
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
// binary from a different release than the shim that reads it.
stampVersion(join(npmDir, "package.json"), (pkg) => {
  for (const key of Object.keys(pkg.optionalDependencies)) {
    pkg.optionalDependencies[key] = version;
  }
});
publishDirs.push(npmDir);

console.log(publishDirs.join("\n"));
