# .githooks — commit gate

Enable per clone: `git config core.hooksPath .githooks`

- **pre-commit** — the mechanical gates, each a script `../scripts/gate.sh` and CI also call. The
  leg list is NOT restated here: the hook is a flat list, short enough to read. The test suite is
  the observed gate, `bash ../scripts/gate.sh`, never chained into the commit.
- **commit-msg** — `git agent-verdict`, once per review this repo demands: standards, annotations,
  prose. The hook declares the gates and pins the tool version; the tool owns the trailer grammar,
  the severity ladder and the reviewer prompt it prints on failure. See its README.

`pre-commit.selftest.sh` is the pre-commit hook's regression table, and CI runs it in the `gates`
job of `../.github/workflows/ci.yml`.
