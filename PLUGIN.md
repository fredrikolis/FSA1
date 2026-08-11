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

The obligation is enforced, not merely stated: the `versions` job in `.github/workflows/plugin-release.yml`
runs before any build and fails the release when the version surfaces disagree, naming each one in the
log. A tag that forgot this bump therefore ships nothing, rather than assets that misreport which
build they are.

`plugin/.mcp.json` = the capability (the host starts the server and its tools appear).
`plugin/skills/` = the instructions (a tool list alone doesn't teach Claude *when* to reach for one).

A claude.ai-hosted plugin may not ship a top-level `bin/`: those land on PATH without appearing on
the admin approval surface. An MCP server is declared, named and reviewable, so that is the door an
executable comes through. A Claude Code user who wants the command in their own shell installs it
with the one-liner at https://fsa1.sh.

## Why both front ends come from npm

A plugin's files are shared across every host it runs on, and a compiled binary is per-(os, arch).
`npx -y fsa1-mcp` resolves the one binary for the machine from a cache that survives sessions, which
is how every local MCP server in the official plugin directory is reached. The command reaches a
machine the same way, as `npx -y fsa1-cli`.

So the channel carries two wrappers and ten platform packages: `distribution/npm/mcp/` publishes
`fsa1-mcp`, `distribution/npm/cli/` publishes `fsa1-cli`, and neither package root contains the
other's tree. Each wrapper names its own five per-platform packages as optional dependencies, each
carrying one binary behind an `os`/`cpu` constraint; npm installs exactly the matching one and skips
the rest.

Both front ends' Linux packages ship a STATIC musl build, so no glibc version has to agree with a
sandbox we never see.

## Native binaries & platforms

`.github/workflows/plugin-release.yml` builds BOTH front ends on push of a `v*` tag, one pair per
platform: the npm job packs both `fsa1-mcp-<slug>` and `fsa1-cli-<slug>` into the per-platform
packages `npx` resolves, and `https://fsa1.sh/install-cli` fetches an `fsa1-cli-<slug>` for a user
who wants the command in their own shell.

Two channels fetch from this list and they do not want the same builds, so the slug alone does not
say who gets it:

| Slug                    | Covers                          | Fetched by                    |
| ----------------------- | ------------------------------- | ----------------------------- |
| `linux-x86_64`          | Linux, glibc                    | `install-cli` (`fsa1-cli-`)   |
| `linux-x86_64-musl`     | **Cowork sandbox**, any glibc   | npm (`fsa1-cli-`, `fsa1-mcp-`)|
| `linux-aarch64`         | ARM Linux, glibc                | `install-cli` (`fsa1-cli-`)   |
| `linux-aarch64-musl`    | arm64 containers, any glibc     | npm (`fsa1-cli-`, `fsa1-mcp-`)|
| `macos-x86_64`          | Intel Macs                      | both                          |
| `macos-aarch64`         | Apple-silicon Macs              | both                          |
| `windows-x86_64.exe`    | Windows desktop (Claude Code)   | both                          |

Each glibc Linux row therefore publishes one asset nobody fetches — a `fsa1-mcp-linux-*`, since npm
takes the musl build of both front ends. The CI asset check names them in its log rather than
failing, because an unfetched build costs a runner and breaks no install.

Which runner builds which asset is the workflow's to say, and only its own: a table here restating
that goes stale the first time a runner is retired.

Both front ends come off the Release by asset name — the npm platform packages `npx` resolves are
built from the `fsa1-mcp-*` and `fsa1-cli-*` assets, and `install-cli` downloads an `fsa1-cli-*` one
directly — so a release is what lets FSA1 run on a machine **without** a Rust toolchain. Every slug
either front end resolves has a row here; a host with no matching asset is told to build from
source, which needs a Rust toolchain it may not have.

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
   Confirm the fourteen assets appear on the Release — seven per front end — and that it is
   marked **Latest**
   (`website/public/install-cli` pulls `releases/latest/download/...`).
3. Submit via the in-app form:
   - claude.ai — https://claude.ai/settings/plugins/submit
   - Console — https://platform.claude.com/plugins/submit
4. After it's listed, pushes to the repo are picked up automatically — no re-submit for updates.

### Staying private instead

You don't have to go public. You can direct-install the plugin for yourself/a small team, or host a
private [plugin marketplace](https://code.claude.com/docs/en/plugin-marketplaces) for internal
distribution. While the repo is private, the release-download step (5) won't authenticate, so rely on
the build-from-source step (needs Rust) or commit prebuilt binaries under `dist/<os>-<arch>/`.
