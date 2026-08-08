<!-- Concern: the engineering principles every FSA1 change is judged against — universal, formula-AST, testing | Non-concern: the annotation format, the commit gate, prose style | IO: none -->
# Repo Standards

The rubric Gate A grades every FSA1 change against. It lives in the repo, not in a knowledge
base, because a reviewer runs in a fresh context that may have no home directory of ours — a
rubric it cannot open is a rubric it invents.

Three bodies of rule, one document: the universal principles, the ones specific to a formula
AST, and the ones governing what gets frozen in a test. Every principle appears in the
[summary table](#summary), and Gate A answers that table line by line.

---

## AUTO-REJECT (stop work immediately)

**Universal blockers** (-∞):

- **Circular imports**: module A imports B, B imports A → restructure
- **Failing tests**: `cargo test --workspace -- --include-ignored` is green before commit
- **Hardcoded secrets**: keys, passwords, tokens in code → environment variables
- **Force push to a protected branch**: never

**FSA1 blockers** (-∞):

- **A back-compat shim across a boundary we own both ends of.** Break the contract and fix every
  call site in the same commit. A shim is how a contract we still own becomes one we don't.
- **Raising a bound to pass a gate.** Not the 200-character annotation, not a comment ratio, not
  a run-length. The bound is the detector; a bigger number hides what it found.
- **A crate reaching across the firewall.** `fsa1-ast` learning about the filesystem,
  `fsa1-model` reaching into the AST's internals, or a format library's types (`zip`,
  `quick-xml`, `calamine`) escaping the format crate that confines it.

**Formula-AST blockers** (-∞):

- **Location as meaning**: a span/offset/source field on a node that participates in equality →
  a synthesized node no longer equals a parsed one, and every producer forks. Move it to a
  side-channel keyed by node id.
- **Poison error nodes**: `Node::Error` / `NotImplemented` variants consumers must defensively
  special-case, corrupting equality, traversal, or emit. The tree holds only well-formed nodes;
  a hole is a *located refusal* beside it.
- **Silent drop of input**: any construct the parser recognizes but discards without a located,
  named diagnostic. Coverage becomes unmeasurable and the tool lies about what it understood.
- **Semantics baked into syntax**: mandatory resolved-type/symbol-table fields with no `Unset`
  sentinel and no separable pass.
- **Twin hand-kept codec contracts**: two independent copies of a bidirectional encode/decode
  mapping. Define the mapping once; read it forward to decode and backward to encode.
- **Panic on untrusted input**: a parser that can panic on hostile text is not a boundary.
  Return diagnostics; never unwind.

---

## PART 1: Decision-making

### Evidence-based decisions

**Measure → decide. Not: opinion → decide.**

| Cargo cult | Evidence required |
| --- | --- |
| "This wants an arena" | Profile showing allocation or pointer-chasing dominates |
| "Cache it across runs" | Measured on every verb — ENG7 was built, measured net-negative, and removed |
| "Parallelize the walk" | Measured wall-clock on a realistic workbook |

**Default**: boring technology. Optimize when proven necessary.

### KISS

**Simplest working solution wins.**

| Justified complexity | Unjustified complexity |
| --- | --- |
| Benchmark proves the simple approach inadequate | Premature optimization |
| The current solution demonstrably fails | A framework for a single use case |
| An explicit requirement demands it | "Future-proofing" hypotheticals |

**Three duplicate lines > premature abstraction.**

### YAGNI

**Build for today. Not tomorrow.**

| Situation | Ship this | Not this |
| --- | --- | --- |
| One render target | That renderer | A pluggable render backend |
| One formula dialect | That grammar | A dialect plugin system |
| One import format path | The path we have | A format-agnostic abstraction over one impl |

**Exception**: extensibility explicitly required → design for extension, implement one.

---

## PART 2: Architecture and system design

### Separation of concerns

**Every unit does its job and stays out of every other unit's job.**

SoC is not a layering rule — it is the *ownership* rule, and it recurses at every scale. One
question, asked of a crate, a file, a type, a function, a variable:

**What is this thing's ONE job, and what is explicitly NOT its job?**

| Scale | Does its job | Stays out of others' jobs |
| --- | --- | --- |
| Crate | Owns one capability | No reaching into another crate's internals |
| File / module | Owns one concern | No neighbor's work (`annotated-tree --annotation-guide`) |
| Type | One reason to change | No knowledge of another type's internals |
| Function | One thing, one level of abstraction | No reaching across a call boundary to fix a caller's mistake |
| Variable | Holds one meaning | Not recycled for a second purpose |

**The crate firewall is SoC applied to FSA1's runtime dependencies**, and it points one way only:

```
fsa1-cli  →  fsa1-model  →  fsa1-ast
                  ↑
            fsa1-ingest        conformance (grades; depended on by nothing)
```

`fsa1-ast` knows nothing of the filesystem. `fsa1-model` consumes it through a narrow trait and
knows nothing of its internals. A FORMAT crate confines the libraries that read and write it, both
directions in one place — `fsa1-xlsx` for .xlsx — and the import pipeline asks it rather than
knowing itself. `fsa1-cli` is a thin shell over the layer below. `docs/architecture.md` holds the detail; a change that inverts
or short-circuits an arrow is an AUTO-REJECT.

**A charter is a routing instrument, not a whitelist.** A unit's charter states one job at its
own altitude, so it answers *does this change belong in here* — never *should this have been
built*. A change landing where the owning charter's `Non-concern:` denies it is a misrouting:
move the change, or move the charter. A `Concern:` that does not enumerate an addition is NOT a
finding — a charter names what its files have in common, never their sum. Whether a capability
should exist at all was decided before a plan existed, and is not a reviewer's question.

**SoC governs several principles here.** A violation surfaces downstream as defensive code (DbC
— doing a job you don't own), a leaking API (Minimal API — blast radius crossing a boundary), or
a multi-concern monolith (File Size — split by concern *first*). Fix it at the source.

#### Refactoring lens: keep vs move vs delete

Every refactor is an ownership audit. Ask in order:

1. **Intended** — what was this supposed to own? (its name, its reason to exist)
2. **Actual** — what does it handle now? (the drift from #1)
3. **Live** — does anyone still care about that concern?

The verdict falls out — argue the concern, never the code:

| Intended vs actual | Concern wanted? | Verdict |
| --- | --- | --- |
| Doing exactly its job | Yes | **Keep** |
| Its job **+ extra** | Yes | **Split** — extract the extra to its rightful owner |
| A **different** job than its name claims | Yes | **Move / rename** to where that concern lives |
| Its job fine, concern is **dead** | No | **Delete** |
| Its job, concern **already owned elsewhere** | Owned better elsewhere | **Delete, consolidate** (→ DRY) |

**"Should we just delete it?" is the most under-asked refactor question.** A unit can do its job
perfectly and still deserve deletion, because no one wants its concern anymore. ENG7's
cross-run cache did its job and was deleted; doing a dead job well is still waste.

### Dependency inversion

**Depend on abstractions, not concretions. Point dependencies at stability.**

High-level policy must not depend on low-level detail; both depend on an abstraction — the
volatile concrete depends on the stable abstract, never the reverse.

| Pattern | Score | Notes |
| --- | --- | --- |
| Policy depends on an abstraction; detail implements it | +10 | Stable core, swappable edges |
| Concrete injected behind a trait | +9 | Testable, decoupled |
| A high-level module importing a concrete low-level one | -8 | Volatile detail drags policy with it |
| An abstraction that leaks its single implementation | -6 | Not an abstraction — a rename |

*SOLID mapped to first principles: SRP → SoC (type scale) · LSP → DbC · ISP → Minimal API · OCP
and DIP → Dependency Inversion.*

### Minimal API surface

**Expose the minimum necessary interface.** Internal details private; the public surface minimal
and stable; the implementation changeable without breaking callers. From the consumer's side
this is interface segregation: a client depends only on the slice it uses.

### Agent UX — design for the agent as primary user

**`fsa1-cli`'s primary consumer is an AI agent, so agent UX IS the UX. Any commit touching the
invocation surface — subcommands, flags, defaults, output, diagnostics, exit codes, `--help`,
`--guide` — is an agent-UX change and carries a severity for whether an agent parses, trusts,
and acts on it more reliably.**

The human reading the same run is the *dual-render* of one structured object, never a second
code path. The test for every surface change: does it convert an act of inference into an act of
reading? The surface only ratchets forward — a regression in agent ergonomics is a blocker, not
a tradeoff.

**Core contract**:

- **Parseable** — structured data to stdout, progress/debug to stderr, never mixed
- **Unambiguous empties** — empty is a first-class value (`[]`, zero count), distinct from error
  and from not-found; one null convention
- **Stable dispatch keys** — agents branch on a namespaced `code`, never on message prose;
  prose may change, codes are an API
- **Syntax is an API too** — a flag rename, output reshape, or default flip breaks unattended
  callers; it lands with its `--help` / schema / docs update in the same commit
- **Located, fixable diagnostics** — `code` + location (the A1 coordinate and the file, or the
  byte span within a formula) + `fix`, one object per finding, never one opaque string that
  discards count, location, and remedy
- **Non-interactive** — nothing on the default path blocks
- **Deterministic** — same workbook → same output; meaningful, consistent exit codes
- **Verdict-driven exit** — status follows the verdict (workbook rejected or not), never "any
  diagnostics present"; a warning on an accepted workbook is not a failure
- **Token-economical** — dense, zero filler; context is the agent's scarcest resource
- **Self-correcting `--help`** — usage, examples, output schema, exit codes, so an agent repairs
  its own call without a human

| Pattern | Score | Notes |
| --- | --- | --- |
| New output path: structured, stdout-clean, dispatchable | +10 | Agent parses and branches reliably |
| Diagnostic carries code + location + fix | +9 | Agent applies, doesn't infer |
| One canonical object, dual-rendered to human and JSON | +8 | No second code path to drift |
| Flag renamed or default flipped without same-commit `--help`/docs | -9 | Breaks unattended callers mid-run |
| Human-only, unparseable output from an agent-first tool | -9 | Breaks the primary consumer |
| Agent forced to branch on message text | -8 | Prose drift breaks callers |
| Interactive prompt on the default path | -10 | Hangs autonomous execution |
| A warning flipping the exit code on an accepted workbook | -8 | Every warning halts automation |
| Empty, null, error and not-found conflated | -7 | Agent can't branch |
| Progress/debug on stdout, corrupting the parse | -8 | Poisons the data stream |
| The tool making the agent's semantic call | -7 | Non-deterministic; can't be branched on |

**Render, don't reason.** The tool's job is to make a workbook's state *observable* —
deterministically and cheaply — not to make the semantic judgments the agent exists to make.
Keep the tool simple and the intelligence in the agent: a zero-inference surface is more
trustworthy to branch on than a "smart" one that can be wrong.

### File size — agent-manageable modules

**Keep files at a size an agent can hold and edit confidently — split ONLY at a natural seam.**

Split when a file outgrows the budget **and** has a seam the code already has (phases,
construct-families, strands). A behavior-preserving split is gated like any change: a
contract/test proving equivalent behavior before and after.

**Heuristic (not a hard line)**: ~1.5–2k lines AND a clean seam → split; else leave it. A file a
sweep found to need a split, with the altitude attempt that failed, is recorded in
`docs/split-register.md`.

| Pattern | Score | Notes |
| --- | --- | --- |
| Cohesive module split at a natural seam when it outgrows the budget | +9 | Bounded, reviewable |
| Behavior-preserving split gated by a contract/test | +9 | Equivalence proven |
| Cohesive file left intact at size (no natural seam) | +5 | Correct — don't split for its own sake |
| Forced split fragmenting one concern across files | -9 | Worse than the monolith |
| Unbounded growth of a hot file | -7 | Compounding tax |

A multi-concern monolith is an SoC problem, not a file-size one — split by concern first.

### Refactoring: remove-then-replace

**Delete old → build new. Boundary tests are the spec.**

| Delete | Keep |
| --- | --- |
| The old implementation | Boundary / contract tests |
| Unit tests of internals | Integration tests at edges |
| Tests coupled to the old structure | Tests defining WHAT, not HOW |

Internal tests get deleted during a rewrite because they constrain the new implementation to
match the old structure. Preserving old code "for reference" shapes the new implementation (-8).

---

## PART 3: Code design and implementation

### Design by contract

**Own both sides → know the contract → fail fast. No defensive code for our own types.**

**Defensive code ONLY for**: the formula parser's untrusted input | a workbook read off disk |
an imported `.xlsx`/`.ods` | library boundaries.

**NOT for**: `fsa1-model` calling `fsa1-ast` | our own data structures | anything inside a crate.

The exemption is scoped, not absent. Across a boundary we own both ends of a shim is an
AUTO-REJECT rather than a tradeoff. The PUBLISHED surfaces are not such a boundary — the argv
contract and the on-disk format have readers we cannot fix in our own commit — so breaking one is
a versioning decision, never a free break.

**Red flags** (defensive ignorance):

| Pattern | Problem | Fix |
| --- | --- | --- |
| `x.or(y).or(z)` | Which is it? You control it. | Trace the producer, pick ONE |
| `value.map(...).unwrap_or(default)` on our own type | Contract uncertainty | Document the structure, access directly |
| `matches!(v, A(_) \| B(_) \| C(_))` for our own enum | Multiple types out of our own code? | Unify the contract |

**Subtypes too**: an impl must honor its trait's contract (Liskov) — one that quietly does a
different job is a broken contract, not a variant.

**DbC = DRY**: validate once at the boundary, trust internally.

### Canonical representation at boundaries

**One canonical internal form. Convert only at the edges.**

Pick a single representation for each quantity in the core and compute in that form
exclusively; convert to display or on-disk forms only at I/O boundaries. Never let two
representations coexist in the interior.

| Pattern | Score | Notes |
| --- | --- | --- |
| One canonical form, converted at the edge | +10 | No ambiguity internally |
| Display formatting applied at the render boundary only | +9 | A display format never computes |
| Two representations mixed in the core | -10 | Which is authoritative? |
| A boundary value stored without normalizing | -8 | Drift, comparison bugs |

### Fail fast

**Detect errors at the source, not downstream.** Explicit error > silent fallback > runtime
confusion. Every guard that cannot complete its check fails SAFE — an unverifiable state is
never a passing one.

### DRY

**Single source of truth for knowledge and logic.**

Eliminate duplication for the same knowledge, the same behavior, and where one change must
affect all. Duplication is fine for accidental similarity, where decoupling is wanted, and where
it is too early to know the right abstraction.

**Rule of three**: duplicate once, refactor at the third. **A wrong abstraction is worse than
duplication.**

### Agent instructions match the code

**A file that tells an agent what to CALL is graded against what the code accepts.**

`plugin/skills/fsa1/SKILL.md` and the `--guide` text are read by an agent deciding what to invoke.
Every command form, flag and argument name in them is a claim about `crates/fsa1-mcp/src/tools.rs`
and `fsa1-cli`'s argv. A stale claim is not a documentation nit: the agent makes the call, the call
is refused, and the refusal names a flag the agent was told to use. Nothing degrades gradually here
— it works or the user watches their agent fail.

So renaming a flag, adding or dropping an argument, or moving a verb between surfaces is not done
until every agent-facing file that spells it has been read against the change. The surfaces need not
agree with each other — MCP and the CLI do not today — but each must be described as it IS.

The same bar applies wherever the repo instructs rather than describes: a documented install path
must be the one that works, and a named default must be the code's default.

---

## PART 4: The formula AST

An AST is Separation of Concerns applied to a parse. Meaning lives in the tree; every other
concern — identity, location, formatting, errors, semantics — lives *beside* it, keyed by
identity. FSA1's AST is a contract shared by producers (the text parser, the importer, a
synthesizer) and consumers (the evaluator, the linter, the tracer, the exporter). Its quality is
how cleanly it holds that contract as both sets grow.

### Three layers

**Sort every fact about a node into exactly one layer. Mixing layers is the root defect.**

| Layer | Holds | Lives | Test |
| --- | --- | --- | --- |
| **Meaning** | The construct and its operands | In the node | Two nodes are equal iff they mean the same thing |
| **Identity** | A key attaching off-tree data to *this* node | In the node, **excluded from equality** | Removing it changes nothing a consumer sees |
| **Provenance** | Location, source text, trivia, errors, resolved semantics | In id-keyed **side-channels** | A synthesized node simply has no entry |

**The generative rule**: *if it isn't meaning, it doesn't belong in the node's value.*

### The source-free core

Nodes carry meaning only; illegal states unrepresentable. Enum for variants, struct for fixed
shapes, uniform metadata lifted into a wrapper. Own children; no back-pointers. Reference a
shared entity by stable id or name, never a positional index into a table that may be sorted.
Parse leaves once at the lowering boundary — never retain source text to re-parse later.

**Don't over-unify.** Two types admitting different operand grammars stay two types. A
god-enum that makes nonsense representable trades a real invariant for a cosmetic saving.

**Type the core; park the long tail as an opaque-but-located raw payload.** For a rarely-consumed
tail (an exotic function, a vendor construct), a typed variant carrying an owned raw tail beats
both a premature typed schema and a silent drop, and upgrades later without a schema break.

### Identity and provenance off the node

A node carries one id, minted per tree; it keys the side-channels and **must not participate in
equality or hashing**. Two consequences, and they are the whole payoff: a parsed node, an
imported node, and a synthesized node with the same meaning compare **equal**; and structural
equality that ignores identity is exactly what powers dedup, common-subexpression elimination,
and `emit == parse` round-trip tests.

**Compare values exactly.** Floats compare by bit pattern, so `-0.0 != 0.0` and NaN is
reflexive — a round-trip that flips a bit is a real difference, never smoothed over.

| Rule | Why |
| --- | --- |
| Store byte offsets, not line/col | Line/col derives; storing both invites drift |
| Slicing the source by a span must never panic | Spans land on char boundaries — an invariant, not a hope |
| Spans are optional, built lazily | Populate per-node spans only when a consumer diagnoses |
| Heterogeneous locations are fine | A formula wants a byte span; a workbook fault wants an A1 coordinate |
| Spans tile the source and stay sliceable | Assert it on **rejected** input too |

### Losslessness and round-trip fidelity

**Choose the round-trip bar deliberately. State it. Prove it as a test contract.**

| Bar | Meaning | When |
| --- | --- | --- |
| **Byte-identical** | `emit(parse(x)) == x` byte-for-byte | Never reformat the author's file |
| **Model-equality** | `parse(emit(x)) == parse(x)` | The abstract tree, through a canonicalizing emitter |
| **Reparse-clean** | `emit(parse(x))` parses without error | Weakest; necessary, never sufficient |

Losslessness comes from a retained source stream beside the tree, **not** from stuffing
whitespace and comments into every node. **Semantic trivia are nodes; cosmetic trivia are
re-derived** — a paren that changes evaluation is a real node; a paren that only expressed
precedence is dropped and re-emitted from tree shape and the binding-power ladder.

**Preserve what you don't understand**: a typed catch-all passthrough for unmodeled content;
store enough to rebuild each non-derivable representation exactly, never re-synthesizing one
from another; and **absent ≠ empty** — a missing part that serializes to nothing is not the same
document as an empty one.

**Reparse-clean hides wrong folds.** A miscomputed constant still reparses cleanly as a
*different* valid formula. A round-trip test asserts the **value survives**, not merely that the
output re-parses. An inverse emitter is *argued* correct from the parser's invariants and then
backed by adversarial-shape tests — never assumed.

### Errors — resilient parsing, located refusals

**Never panic. Accumulate and recover. The tree holds only well-formed nodes; what could not be
built is a located refusal beside it.**

```
Parser hits a construct it cannot build a well-formed node for
  ├─ Recognized but unimplemented?  → located refusal, warning severity, NAMED. Continue.
  ├─ Malformed input?               → located refusal, error severity. Recover to the next unit. Continue.
  ├─ Unmodeled but must survive?    → typed passthrough node. Continue.
  └─ Never                          → a poison error node, or a silent drop.
```

The parser is the one defended boundary; everything downstream trusts its contract and
re-validates nothing. Recovery is per-unit so one bad cell never kills the rest — an author
wants *every* fault at once, not the first.

Diagnostics are single-sourced data: one registry of stable codes with severity and summary,
derived from source and self-consistency tested. **Codes are the API consumers switch on;
wording is not frozen.** Severity is orthogonal to the verdict. **Category discipline**:
"construct not parsed" and "best-guess emitted" are different codes, and reusing one for the
other is a category lie.

### Syntax versus semantics

The parser produces syntax only — every type slot left `Unset`, every name left unresolved. A
resolver pass writes those reserved fields **in place**, changing field contents, never tree
shape. Symbol tables and scopes are external side structures, built and discarded during
resolution; the tree never gains a symbol-table field. An `Unset` sentinel plus a
resolved-invariant assertion at each consumer entry catches an unresolved tree reaching the
evaluator.

**Accept under uncertainty — the cardinal rule.** Type only what you are sure of; defer
everything ambiguous. Every semantic check fires only when operands are known-judgeable and the
combination is one the ground truth definitely rejects. **A false-reject is the cardinal sin;
every deferred gap must be a false-negative, never a false-positive.**

### Dialects, traversal, and mutation

One engine, N dialects **as data** — a small rules struct of named booleans instantiated per
dialect, never a forked parser. Keep the core grammar permissive and tag version- or
environment-dependent constructs via a queryable side analysis rather than rejecting them.

Match traversal to the consumer set: pattern-match directly for a small closed set; one
post-order, position-class-aware walker for a large or open one. Mutation, in order of
preference: **edit the text and re-parse** (sidesteps span invalidation entirely); a **surgical
located edit** proven confined; **in-place rewrite** only inside a transform pass that owns the
whole tree and re-establishes its invariants. Never hand-patch spans after a tree edit.

### Single-source the contract; grade against an oracle

One contract table read forward to decode and backward to encode, so the two directions cannot
drift. Generate the machine-readable schema from the types, eliding provenance fields, so the
spec cannot drift from the parser.

| Layer | Asserts |
| --- | --- |
| Round-trip / idempotence | The chosen bar holds, and the **value survives** |
| No-panic fuzz | Hostile and truncated input never panics; emit is exercised on every input |
| Bounded recursion | Deeply nested input yields a diagnostic, never a stack overflow |
| Oracle-diff conformance | Accept/reject and value parity against a **provenance-guarded** corpus the tool cannot itself generate — assert the corpus fingerprint so it cannot grade itself |
| Coverage ratchet | A monotonic count of modeled constructs with an honest denominator |
| No-leak diagnostics | No message leaks internal debug formatting to the author |

---

## PART 5: Testing — every assertion is a freeze decision

**When you write an assertion you are signalling: "I want this behavior FROZEN; a future agent
MUST update this test to change it."** Tests create an intentional refactor penalty. Freeze
deliberately.

### Before writing any assertion

| Question | If yes | If no |
| --- | --- | --- |
| External contract we can't coordinate? | **FREEZE** (commit the test) | Continue |
| Do we control both sides of this interface? | **DON'T FREEZE** (DbC) | Continue |
| Already caught by an integration or conformance test? | **DON'T FREEZE** (redundant) | Consider freezing |
| Would the test block a principled refactor? | **DELETE THE TEST** (-∞) | Proceed |

| Criteria | Score |
| --- | --- |
| External contract; a corpus/format others depend on | **+10** |
| Leaf-node stable abstraction, frozen at its interface | +9 |
| Edge case not caught end-to-end | +8 |
| We control both sides | **-10** |
| Already caught end-to-end | -8 |
| Glue / orchestration code | -9 |
| Implementation detail, not interface | -7 |
| Test blocks a principled refactor | **-∞** |

### Where FSA1 freezes

The conformance corpora and the round-trip oracle are the repo's real freeze points: they grade
behavior we have committed to (Excel parity, export fidelity) against inputs we cannot generate.
Crate-internal seams are not frozen — `fsa1-model` and `fsa1-ast` are updated together.

### Perverse behavior to avoid

| Anti-pattern | Symptom | Remedy |
| --- | --- | --- |
| Suboptimal refactor to pass a test | Awkward design because it was the quickest path to green | The test is wrong — delete or rewrite it |
| Tests driving architecture | Design compromised for the suite | Principles drive architecture |
| Testing implementation details | Freezes "how", not "what" | Test the interface |
| Redundant coverage | One failure, several tests | One test at the right level |
| Defensive tests for our own code | DbC violation | Assert at boundaries only |
| Tests without assertions | No governance value | Add assertions, or make it a walkthrough |

**NEVER compromise architecture for the test suite.**

### Representativeness

Tests approximate reality, and every divergence is a blind spot. Before taking a shortcut, ask
what this test will NOT catch — mocked dependencies hide contract drift, simplified fixtures
hide encoding and scale, clean state each run hides accumulation. Not a rule, a lens. Document
the blind spots you accept.

---

## Summary

Gate A answers every row.

| Principle | Essence | Violation signal |
| --- | --- | --- |
| **Evidence-based** | Measure → decide | "Best practice" without context |
| **KISS** | Simplest working solution | Complexity without justification |
| **YAGNI** | Build for today | Machinery for a hypothetical second case |
| **SoC** | Every unit owns one job; fractal crate→variable | A unit doing a neighbor's job |
| **Crate firewall** | `cli → model → ast`; a format crate confines its format | An arrow inverted or short-circuited |
| **Dependency inversion** | Depend on abstractions; point at stability | High-level module depends on low-level detail |
| **Minimal API** | Expose only what is necessary | Leaking implementation details |
| **Agent UX** | The agent is the primary user; the surface ratchets forward | A change making an agent's parse or self-repair worse |
| **File size** | Agent-manageable; split at natural seams | Multi-concern monolith |
| **Remove-then-replace** | Delete old, boundary tests are the spec | Keeping internal tests through a rewrite |
| **DbC** | Own both sides → know the contract | Defensive code for our own types |
| **Canonical representation** | One internal form, convert at edges | Two representations in the core |
| **Fail fast** | Errors at the source; guards fail SAFE | Silent fallback masking a problem |
| **DRY** | Single source of truth | Duplicated logic, or a doc restating a hook |
| **Agent instructions match the code** | A file telling an agent what to call is graded against what the code accepts | A flag, argument or command form in SKILL.md or `--guide` that the binary does not have |
| **AST: three layers** | Meaning in the node; provenance in side-channels | A non-meaning fact stored as a node value |
| **AST: source-free core** | Meaning only; illegal states unrepresentable | Over-unified enum; retained source text |
| **AST: identity ≠ meaning** | Node id excluded from equality | Synthesized node no longer equals a parsed one |
| **AST: round-trip bar** | Choose it, state it, prove the value survives | "Round-trips" with no stated or tested bar |
| **AST: preserve the unknown** | Typed passthrough; absent ≠ empty | Dropping unmodeled input on emit |
| **AST: located refusals** | Well-formed nodes only; holes are located data | Error nodes; silent drops |
| **AST: resilient parsing** | Never panic; accumulate; recover per unit | First-error abort; panic on hostile input |
| **AST: syntax vs semantics** | The resolver annotates reserved fields in place | Resolved types as mandatory node fields |
| **AST: accept under uncertainty** | Type what you're sure of; defer the rest | A false-reject of valid input (cardinal sin) |
| **AST: dialects as data** | One engine, N rules structs | A forked per-dialect parser |
| **AST: edit text, re-parse** | Mutation avoids span invalidation | Hand-patched spans after a tree edit |
| **AST: grade vs an oracle** | Round-trip + fuzz + provenance-guarded corpus | A corpus that can grade itself |
| **Testing: freeze decisions** | Every assertion freezes behavior; freeze at boundaries | A test on a seam we own both sides of |
| **Testing: representativeness** | Every divergence from reality is a blind spot | An undocumented shortcut in a fixture |

---

## References

- [Design by Contract vs Defensive Programming](https://softwareengineering.stackexchange.com/questions/125399/differences-between-design-by-contract-and-defensive-programming)
- [rust-analyzer architecture — keep semantic info out of the syntax tree](https://rust-analyzer.github.io/book/contributing/architecture.html)
- [Resilient LL Parsing Tutorial — Alex Kladov](https://matklad.github.io/2023/05/21/resilient-ll-parsing-tutorial.html)
- [The Lossless Syntax Tree Pattern — Oil](https://github.com/oilshell/oil/wiki/Lossless-Syntax-Tree-Pattern)
- [The Practical Test Pyramid — Martin Fowler](https://martinfowler.com/articles/practical-test-pyramid.html)
