<!-- Concern: the prose standard for FSA1's reader-facing README — its voice, and that claims, examples and links match the code | Non-concern: the annotation format or code principles | IO: none -->
# Communication Style

Scored one changed line at a time. The first seven rules are voice, a judgment call per line. The last three are checkable, not
matters of taste: verify them against the binary and the headings.

| Rule | Flag a changed line when it... |
| --- | --- |
| **Matter-of-fact** | leans on metaphor, rhythm, or a soft pointer where a plain fact belongs. "the interesting part is further down" for a fact you could just state; "X is where it is spelled out" instead of "See X". Declarative, concrete, subject-verb-object. |
| **Don't announce the point** | opens with a setup clause that promises a point and colons into it ("The filesystem is the payoff, and it comes for free: once every cell..."). Delete the preamble, lead with the substance. A colon is for a definition or a list, not for clearing your throat. |
| **Scannable** | opens a paragraph that should be a bulleted list, states a key claim with nothing bolded, or leads with context instead of the point. |
| **Reader-first** | leads with what we built or how hard it was instead of a pain the reader has hit, or states a benefit before the problem is felt. Name the problem, then the fix, then the benefit. "What's in it for me" beats "what we did". |
| **Tight** | carries a windup, throat-clearing, or a sentence that explains our reasoning to ourselves rather than moving the reader. Every word earns its place. |
| **No em-dashes** | contains an em-dash in prose. Use commas, periods, colons, or parentheses. A quoted literal that itself contains one (`N/A — reason`) is not a finding. |
| **Plain, not hyped** | reaches for hype (superlatives, "blazingly", "seamless", "simply", "just") or stacks hedges ("we believe it might possibly"). Confident and direct. |
| **Claims match the code** | teaches a subcommand, flag, config key, default, exit code, or behavior the shipped `fsa1-cli` does not have or that behaves differently now. |
| **Examples run clean** | shows a command, workbook layout, or file name the tool would reject, or output it would not produce. |
| **Links resolve** | uses a `[...](#anchor)` whose heading does not exist, or a relative path to a file that is not there. |

**Checking the last three.** Spot-check every subcommand, `--flag`, exit code, example
invocation, example range-file name, and link in the changed lines against a binary built from
**this tree**:

```
cargo run -p fsa1-cli -- --help
cargo run -p fsa1-cli -- --guide
```

Never an older `fsa1-cli` on `PATH`, and never a cached build — a stale binary can embed content
you already changed and assert against text that no longer exists. A feature described but not
built is worse than one left out: it sends the reader down a path that is not there.

**Out of scope.** Whether a claim *should* be made is a scope question, settled before a plan
existed. This table judges only whether the line is written well and whether it is true.
