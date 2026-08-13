# .githooks — commit gate

Enable per clone: `git config core.hooksPath .githooks`

- **pre-commit** — THE definition of every mechanical gate, and the one CI runs too. On a commit it
  runs the fast stages; `bash .githooks/pre-commit --full` adds the test suite (and fetches the Vega
  bundle when it is missing). The stage list is NOT restated here: the hook is a flat table, short
  enough to read. The suite stays a deliberate, observed step, never chained into the commit.
- **commit-msg** — `git agent-verdict`, once per review this repo demands: standards, annotations,
  prose. The hook declares the gates and pins the tool version; the tool owns the trailer grammar,
  the severity ladder and the reviewer prompt it prints on failure. See its README.
