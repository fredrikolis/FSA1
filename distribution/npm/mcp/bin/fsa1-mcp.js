#!/usr/bin/env node
// Concern: resolves the prebuilt fsa1-mcp binary for this host and hands it stdin, stdout and its exit code | Non-concern: what the server exposes | IO: (argv, platform) -> the binary's stdio + code
"use strict";

const path = require("path");
const { spawnSync } = require("child_process");

// Single source of truth mapping a host `${platform}-${arch}` to the npm platform package carrying
// its binary. Both Linux entries ship a STATIC musl build, so one covers glibc and musl hosts alike
// — which is what makes this survive a sandbox whose libc we do not get to know.
const PLATFORM_PACKAGES = {
  "linux-x64": "fsa1-mcp-linux-x64-musl",
  "linux-arm64": "fsa1-mcp-linux-arm64-musl",
  "darwin-x64": "fsa1-mcp-darwin-x64",
  "darwin-arm64": "fsa1-mcp-darwin-arm64",
  "win32-x64": "fsa1-mcp-windows-x64",
};

function resolveBinary() {
  const key = `${process.platform}-${process.arch}`;
  const pkg = PLATFORM_PACKAGES[key];
  if (!pkg) {
    return { key, pkg: null, bin: null };
  }
  const exe = process.platform === "win32" ? "fsa1-mcp.exe" : "fsa1-mcp";
  try {
    const pkgDir = path.dirname(require.resolve(`${pkg}/package.json`));
    return { key, pkg, bin: path.join(pkgDir, exe) };
  } catch (_err) {
    return { key, pkg, bin: null };
  }
}

// Never stdout: this process's stdout IS the MCP transport, and one stray line on it is a protocol
// frame the client cannot parse.
function fail(message) {
  process.stderr.write(`fsa1-mcp: ${message}\n`);
  process.exit(1);
}

function main() {
  const { key, pkg, bin } = resolveBinary();

  if (!pkg) {
    fail(
      `unsupported platform ${key}. No prebuilt binary is published for it. ` +
        `Build from source instead: cargo install --git https://github.com/fredrikolis/FSA1 fsa1-mcp`
    );
  }
  if (!bin) {
    fail(
      `the prebuilt binary for ${key} is missing (expected package "${pkg}"). ` +
        `Reinstall without skipping optional dependencies: npm install fsa1-mcp ` +
        `(do not pass --no-optional), or install "${pkg}" directly.`
    );
  }

  // spawnSync over exec*: the server is long-lived and speaks JSON-RPC on the inherited pipes, so
  // the wrapper must stay out of the way for its whole life and then answer with its code.
  const r = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });
  if (r.error) {
    fail(r.error.message);
  }
  if (r.signal) {
    fail(`server terminated by signal ${r.signal}`);
  }
  process.exit(r.status === null ? 1 : r.status);
}

main();
