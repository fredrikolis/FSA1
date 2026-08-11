#!/usr/bin/env node
// Concern: resolves the prebuilt fsa1-cli binary for this host and hands it stdin, stdout, stderr and its exit code | Non-concern: what the command does | IO: (argv, platform) -> its stdio + code
"use strict";

const path = require("path");
const { spawnSync } = require("child_process");

// Single source of truth mapping a host `${platform}-${arch}` to the npm platform package carrying
// its binary. Both Linux entries ship a STATIC musl build, so one covers glibc and musl hosts alike
// — which is what makes this survive a sandbox whose libc we do not get to know.
const PLATFORM_PACKAGES = {
  "linux-x64": "fsa1-cli-linux-x64-musl",
  "linux-arm64": "fsa1-cli-linux-arm64-musl",
  "darwin-x64": "fsa1-cli-darwin-x64",
  "darwin-arm64": "fsa1-cli-darwin-arm64",
  "win32-x64": "fsa1-cli-windows-x64",
};

function resolveBinary() {
  const key = `${process.platform}-${process.arch}`;
  const pkg = PLATFORM_PACKAGES[key];
  if (!pkg) {
    return { key, pkg: null, bin: null };
  }
  const exe = process.platform === "win32" ? "fsa1-cli.exe" : "fsa1-cli";
  try {
    const pkgDir = path.dirname(require.resolve(`${pkg}/package.json`));
    return { key, pkg, bin: path.join(pkgDir, exe) };
  } catch (_err) {
    return { key, pkg, bin: null };
  }
}

// stderr, not stdout: stdout is the command's data, and the caller parsing it must not have to
// filter this wrapper's diagnostics out of it.
//
// 127, the shell's "command not found", because every code the binary itself defines (1 I/O, 2
// invalid arguments, 3 validation, 4 conflict, 24 not found) is a verdict ABOUT the user's
// workbook. A wrapper that could not produce a binary at all must not answer inside that set: a
// caller branching on the code would retry the file instead of repairing its install.
const WRAPPER_FAILURE = 127;

function fail(message) {
  process.stderr.write(`fsa1-cli: ${message}\n`);
  process.exit(WRAPPER_FAILURE);
}

function main() {
  const { key, pkg, bin } = resolveBinary();

  if (!pkg) {
    fail(
      `unsupported platform ${key}. No prebuilt binary is published for it. ` +
        `Build from source instead: cargo install --git https://github.com/fredrikolis/FSA1 fsa1-cli`
    );
  }
  if (!bin) {
    fail(
      `the prebuilt binary for ${key} is missing (expected package "${pkg}"). ` +
        `Reinstall without skipping optional dependencies: npm install fsa1-cli ` +
        `(do not pass --no-optional), or install "${pkg}" directly.`
    );
  }

  // spawnSync over exec*: the command owns the inherited pipes for its whole run, and the wrapper
  // stays out of the way and then answers with its code.
  const r = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });
  if (r.error) {
    fail(r.error.message);
  }
  // A command killed by a signal DIES by that signal here too, rather than exiting with a number
  // that resembles one: the handler goes back to default and the signal is re-raised on this
  // process, so the parent shell sees a real signal death and reports it as it always does.
  if (r.signal) {
    process.removeAllListeners(r.signal);
    process.kill(process.pid, r.signal);
  }
  process.exit(r.status === null ? WRAPPER_FAILURE : r.status);
}

main();
