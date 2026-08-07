<!-- Concern: what enabling the FSA1 Claude plugin gives a user, and how it is released | Non-concern: the CLI's own surface (docs/cli-spec.md), the format (docs/format-spec.md) | IO: none -->
# FSA1 as a Claude plugin

This repo doubles as a [Claude Code / Cowork plugin](https://code.claude.com/docs/en/plugins-reference).
Enabling the plugin starts an MCP server whose tools drive a workbook, and teaches Claude — via a
skill — what it is and when to reach for it.

## What ships

```
.claude-plugin/marketplace.json      # the catalog users add, at the repo root where it must live
plugin/                              # the plugin itself, and everything a host fetches
plugin/.claude-plugin/plugin.json    # its manifest
plugin/.mcp.json                     # declares the one MCP server the host starts
plugin/launcher/fsa1-mcp             # cross-platform launcher (bash) -> execs the native server
plugin/skills/fsa1/SKILL.md          # tells Claude what the tools do and when to reach for them
.github/workflows/plugin-release.yml # cross-builds native binaries -> GitHub Release
```

## Installing it

```
/plugin marketplace add fredrikolis/FSA1
/plugin install fsa1@fredrikolis
```

The coordinate is `<plugin>@<marketplace>`: the plugin is `fsa1`, the marketplace is `fredrikolis`,
and the repo carries ONE plugin which is the repo root (`"source": "./"`). If the install summary
says `Run /reload-plugins to activate.`, run that.

A version bump in `plugin/.claude-plugin/plugin.json` is what offers users an update — pushing commits
without bumping it leaves `/plugin update` reporting they are already current. The `name` beside it
is the one field that can never change: a published plugin name is the slug every install holds, and
renaming it breaks all of them. Relabel with `displayName` instead.

`plugin/.mcp.json` = the capability (the host starts the server and its tools appear).
`plugin/skills/` = the instructions (a tool list alone doesn't teach Claude *when* to reach for one).

A claude.ai-hosted plugin may not ship a top-level `bin/`: those land on PATH without appearing on
the admin approval surface. An MCP server is declared, named and reviewable, so that is the door an
executable comes through. A Claude Code user who wants the command in their own shell installs it
with the one-liner at https://fsa1.sh.

## Why `plugin/launcher/fsa1-mcp` is a launcher, not the binary

A plugin's launcher is shared across every host the plugin runs on: **Cowork's Linux cloud sandbox**
and **Claude Code on the user's own macOS / Windows / Linux desktop**. A compiled Rust binary is
per-(os, arch), so `plugin/launcher/fsa1-mcp` is a small bash script that resolves the matching binary and caches
it under `${CLAUDE_PLUGIN_DATA}` (persistent across plugin updates — `${CLAUDE_PLUGIN_ROOT}` changes
every update, so nothing is cached there). Resolution order, all overridable:

1. `$FSA1_MCP_BIN` — explicit path (dev / power users)
2. cached binary in `${CLAUDE_PLUGIN_DATA}/bin`
3. bundled prebuilt `dist/<os>-<arch>/fsa1-mcp` (if you choose to commit binaries)
4. local dev build `../target/{release,debug}/fsa1-mcp` (contributors in the repo)
5. **GitHub Release asset** `fsa1-mcp-<os>-<arch>[.exe]` — the normal path for an installed plugin
6. build from source with `cargo -p fsa1-mcp` — last resort, needs a Rust toolchain

## Native binaries & platforms

`.github/workflows/plugin-release.yml` builds BOTH front ends on push of a `v*` tag, one pair per
platform: `fsa1-mcp-<slug>` is what the plugin's launcher fetches, `fsa1-cli-<slug>` is what
`https://fsa1.sh/install-cli` fetches for a user who wants the command in their own shell.

| Slug                    | Covers                          |
| ----------------------- | ------------------------------- |
| `linux-x86_64`          | **Cowork sandbox**, Linux CLI   |
| `linux-aarch64`         | ARM Linux, arm64 containers     |
| `macos-x86_64`          | Intel Macs                      |
| `macos-aarch64`         | Apple-silicon Macs              |
| `windows-x86_64.exe`    | Windows desktop (Claude Code)   |

Which runner builds which asset is the workflow's to say, and only its own: a table here restating
that goes stale the first time a runner is retired.

The launcher downloads the matching asset by name, so a release is what lets the plugin run on a
machine **without** a Rust toolchain. Every slug the launcher resolves has a row here; a host with no
matching asset falls through to building from source, which needs a Rust toolchain it may not have.

> **Windows correctness depends on the range-separator fix** in this repo: FSA1 names range files
> after A1 ranges (`A1:C1`), and `:` is illegal on NTFS (it silently becomes an alternate data
> stream). On Windows the on-disk name uses `-` (`A1-C1`); the logical `:` operator is unchanged
> everywhere else. See `FSA1-windows-build-issues.md`.

## Validate

```bash
claude plugin validate .
```

Checks `plugin.json`, skill frontmatter, and structure. (Structure and JSON/frontmatter were verified
by hand here; run the CLI check on a machine that has `claude` installed before submitting.)

## Publish & submit

1. Make the GitHub repo **public** (closed-source is not accepted for the public catalog).
2. Tag a release so the binaries get built and attached:
   ```bash
   git tag vX.Y.Z && git push origin vX.Y.Z
   ```
   Confirm the five assets appear on the Release and that it is marked **Latest** (the launcher pulls
   `releases/latest/download/...`).
3. Submit via the in-app form:
   - claude.ai — https://claude.ai/settings/plugins/submit
   - Console — https://platform.claude.com/plugins/submit
4. After it's listed, pushes to the repo are picked up automatically — no re-submit for updates.

### Staying private instead

You don't have to go public. You can direct-install the plugin for yourself/a small team, or host a
private [plugin marketplace](https://code.claude.com/docs/en/plugin-marketplaces) for internal
distribution. While the repo is private, the release-download step (5) won't authenticate, so rely on
the build-from-source step (needs Rust) or commit prebuilt binaries under `dist/<os>-<arch>/`.
