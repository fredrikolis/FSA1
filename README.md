<!-- Concern: the repo's public orientation: what fsa1-cli is and how to get it | Non-concern: the contracts, the command surface, where each document lives (annotated-tree owns that) | IO: none -->
# FSA1

A spreadsheet that **is** a filesystem. Tabs are folders. Each file's name is the A1 range its
contents fill. Built for agents: greppable, diffable, and editable with `mv`, `grep` and a text
editor. Named for Charles Simonyi.

**[fsa1.sh](https://fsa1.sh)** — what it is, and why, with worked examples.

## Install

A prebuilt binary, into `~/.local/bin`:

```
curl -fsSL https://fsa1.sh/install-cli | sh
```

As a Claude Code plugin:

```
/plugin marketplace add fredrikolis/FSA1
/plugin install fsa1@fredrikolis
```

Or from source:

```
cargo run -p fsa1-cli -- sample ./demo && cargo run -p fsa1-cli -- tree ./demo
```

## Using it

```
fsa1-cli --guide     # the on-disk model plus authoring, in one screen
fsa1-cli --help      # commands, flags, exit codes (source-owned)
```

`fsa1-cli` renders (`render`, `tree`), lints (`check`), evaluates (`eval`), traces dependencies
(`trace`), unpacks (`unpack`) and packs (`pack`) a workbook. The authoritative surface lives in
`--help` and `--guide`, so this README cannot go stale about it.

## Working on it

Every file's first line says what it is for, so `annotated-tree` on any directory is the map. Run
every mechanical check with `bash scripts/gate.sh`.

## Licence

MIT — see [LICENSE](LICENSE).
