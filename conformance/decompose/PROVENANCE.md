<!-- Concern: the SOURCE of the sheet-decomposition corpus, and the correction rule its frozen expectations are held to | Non-concern: the grading verdicts | IO: none -->
# Provenance — the decompose (sheet-decomposition) corpus

This corpus is the acceptance oracle for the sheet-decomposition policy: the regions an author
expressed become the blocks `unpack` writes. Each fixture states one structural concern, labels the
regions a reader would name, and freezes the block list the policy must cut.

## The fixture bytes are authored by a third party, never by FSA1

Every `fixtures/*.xlsx` is written by **openpyxl** — a third-party writer with no knowledge of FSA1 —
from the committed `make_fixtures.py`. FSA1 never produces a byte of the corpus it is graded against.
This is the same discipline `conformance/presentation/` and `conformance/serde/` record: a tool that
generates its own oracle input grades itself, and a consistent-but-wrong implementation then passes.

Regenerate with any interpreter carrying the pinned openpyxl (`requirements.txt`; the corpus needs no
venv, because its grader is Rust):

    python3 conformance/decompose/make_fixtures.py

**Regeneration is not byte-stable** — openpyxl stamps `docProps/core.xml` with the run's timestamp, so
every fixture is a fresh LFS object even when nothing about it changed. Run the script when a
fixture's *content* must change, not to check that it still produces one.

### Why authored, and not downloaded

Four reasons, and the last one is the membership rule:

- **A tool that generates its own oracle input grades itself.** A third party writes every byte.
- **Revision-in-place.** Statistical publishers revise workbooks at the same URL, and a corpus whose
  truth can change under a stable filename is not a frozen expectation.
- **Licence and size.** Neither question arises for a file the repo authors.
- **Hand-derivability.** An authored fixture can be small enough that its expected block list is
  derivable by hand. A real 645-row sheet cannot.

> **A fixture whose block list cannot be derived by hand is too big for this corpus.**

That is what keeps the specification honest: every `block:` line here was executed by hand against the
fixture's own `s=` attributes — the seam, the seed cover, then each growth pass with the gains it
computed — and each derivation is written into the frozen file it produced.

## The correction rule — non-negotiable

> **A frozen expectation is corrected ONLY when the *reading* of the third-party-authored fixture was
> wrong — never edited to chase an FSA1 regression.**

That sentence is repeated verbatim in every assertion message the grader raises, so a failure carries
it without anyone opening this file. When FSA1 and an expectation disagree, the default verdict is an
FSA1 defect.

## The pressure that rule creates, and the one fixture re-authored under it

Property 1 is recall **1.000** with no slack, and a `region:` label may not be edited: it is a reading
of the authored structure, written down before `unpack` is ever run. So when a hand-derivation misses
a labelled start there is exactly one remaining freedom — **re-author the fixture until the policy
agrees**. A fixture changed for that reason is a fixture authored to pass, and it is declared here.

**`title_caption_table` was re-authored, once.** Its first shape was the obvious one: a one-cell title
in `A1`, a one-cell caption in `A2`, a bold header row `A3:C3`, a body `A4:C8`. The hand-derivation
said the title and the caption merge, and the run agreed: **`A1:A2` and `A3:C8`**. Two vertically
adjacent single-row regions of the same width always coalesce under this policy — the merged
rectangle spells the lower row with ONE row rule, which costs exactly the region term the merge
refunds, so the gain is `+1` — and the caption's own start row is then a boundary nothing writes.
Recall was 2 of 3.

The re-authored fixture states the title across `A1:B1` and the caption across `A2:A3`. Both changes
are width or depth, not style: they put the two regions at different widths, which is the one thing
that holds a boundary open here, because the merged rectangle then encloses an unoccupied coordinate
and loses its modal rule. Nothing else about the fixture moved, and no label was edited.

What this says about the policy is worth reading plainly: **a single-row region of the same width as
its neighbour is not addressable under today's default.** That is a real limit, not a corpus artifact,
and `contents_index` is authored three rows deep on each side precisely because the merge stops paying
at two.

## The frozen format

One directive per line; `#` comments and blank lines are ignored.

    sheet:  <tab name>
    region: <first row>-<last row> <the name a reader would give it>
    block:  <the A1 range of one block the default policy must write>
    policy: occupancy | appearance
    misses: <how many labelled region starts `--decompose occupancy` fails to emit>

`region:` is the **semantic label**, and the rule it is written against:

> A region is a maximal contiguous row-block a reader would give **one** name to. A caption is
> separate from the table it introduces. A table **includes** its header row, any subtotal row inside
> it, and any blank separator rows inside it. A prose block set off by blank rows and carrying its own
> heading is its own region.

`block:` is the **expected output**, hand-derived by executing the specification on the fixture — not
a transcript of a run. The derivation sits in the same file, above the directives.

`misses:` is derived the same way. All six fixtures are small enough that their occupancy bounding
box is under that policy's 256-cell floor, so `--decompose occupancy` writes ONE block per sheet and
misses every labelled start but the first — which on a fixture labelling exactly ONE region is none
at all, and that is why a `misses:` of 0 is a reading like any other rather than a fixture graded by
nothing.

## The six fixtures, one structural concern each

| fixture | asserts |
|---|---|
| `title_caption_table` | a title, a caption and a bold-header table under one bounding box split at the style change — the shape `occupancy` keeps as ONE block |
| `table_then_footnotes` | three separately-headed footnote blocks after a table, each set off by a SINGLE blank row, stay apart: the leap reaches every one of those gaps, and the cost of bridging one is what holds them open |
| `numfmt_only_series` | a workbook stating no font, fill or border anywhere splits at its series — the case that fails if the signature loses the number format |
| `contents_index` | two adjacent regions differing only in alignment and indent are two regions, because a `cellXfs` entry carries alignment |
| `banded_report` | a bold header row over an eight-row zebra body is ONE block and not one per band pair — the shape `appearance` used to shatter |
| `banded_subtotals` | that same body cut by three bold subtotal rows on the header's own fill, the banding running on global row parity straight through them, is still ONE block |

`numfmt_only_series` unpacks with sixteen `number formats coerced to plain` items in the fidelity
report. That is expected and is not this corpus's concern: the number format's own round trip is
SER2's, graded in `conformance/serde/`. What this corpus grades is that `numFmtId` reached the
**signature** the cut is computed over, which is a different claim from carrying the format across.

## What grades it

| leg | runs under | asserts |
|---|---|---|
| `conformance/tests/decompose_regions.rs` | `cargo test --workspace` — no venv, no network | all four properties below |

There is **no `run.sh` and no CI job**, because this corpus has no Python grading leg: the fixtures are
authored in Python and graded entirely in Rust, in-process, through `fsa1_ingest::import_file` and
`fsa1_model::Workbook`.

Per fixture, except where a property says otherwise:

1. **Every `region:` start row is the start row of some `block:`.** Recall is exactly 1.000 — no
   margin, no mean, no bar. Checked **twice**: statically on the frozen file, where a label and a
   block list that disagree are a corpus defect caught before any binary runs, and dynamically on what
   the run actually wrote.
2. **The tree the default policy writes is exactly the frozen `block:` set**, and the policy the
   source resolves to is the frozen `policy:`. The tree is then handed to `check`: `Workbook::load_dir`
   must accept it and `.lint()` must be empty, which is where `non-canonical-presentation` and
   `degenerate-range` would surface. Both are named in the failure message.
3. **`--decompose occupancy` misses exactly `misses:` of the labelled starts.** No bar is held over a
   single fixture's `misses:`. What is held instead is the corpus's **composition**, checked once over
   every frozen reading rather than per fixture: among the `policy: appearance` fixtures, at least one
   carries `misses:` of 1 or more, and at least one carries `misses: 0`. All six fixtures here are
   `.xlsx` and resolve to `appearance`, so that check ranges over the whole corpus.

   The per-fixture bar — `misses:` at least 1, on every fixture — is **gone**, and why it went is
   worth writing down so it is not re-derived. It read "`occupancy` cuts this the same way" as "this
   fixture grades nothing". That is true of a fixture testing **recall**, and false in general,
   because the harness binds the exact block list of EVERY fixture whatever its `misses:` is. A
   `misses: 0` fixture therefore still grades something — that `appearance` writes precisely that list
   and not a shattered one. The corpus now has to hold at least one of each kind: one that grades the
   recall `occupancy` cannot reach, and one that guards against fragmentation.
4. **A fixture with no `.expected` file fails the run**, exactly as the presentation corpus refuses a
   fixture with no reading. A fixture the graders skip is graded by nothing.

## What the corpus does and does not claim

**No block either policy writes is a semantic unit, and nothing here claims one is.** The defensible
claim is narrower and is the one the corpus measures: the structure an author expressed becomes
**addressable** far more often than under `occupancy` — 13 labelled starts across the six fixtures,
of which the default cut emits 13 and `occupancy` emits 6.

The corpus is authored small so its block lists are hand-derivable, and the frozen `block:` lists are
also what makes the per-fixture **file count** visible in a reviewable diff: a policy change that
splits a sheet into twice as many files shows up as twice as many `block:` lines, in the same review
that changes the policy.

## Adding a fixture

Author the `.xlsx` in `make_fixtures.py`. Then, in this order:

1. **Label the regions from the authored structure, before running `unpack` on anything.**
2. **Hand-derive the `block:` list** from the fixture's bytes and the specification, and write the
   derivation into the `.expected` file.
3. Only then run the binary. On a disagreement, work out which side is wrong: a derivation error is
   fixed, a policy defect is reported rather than papered over, and a fixture re-authored to make the
   policy agree is declared in this file with its derivation.

## Corpus history

- **All four fixtures added together, with the corpus.** Every derivation matched FSA1 on the first
  run: `title_caption_table` → `A1:B1`, `A2:A3`, `A4:C9`; `table_then_footnotes` → `A1:C5`, `A7:A9`,
  `A11:A13`, `A15:A17`; `numfmt_only_series` → `A1:B3`, `A4:B11`; `contents_index` → `A1:B3`,
  `A4:B6`. `--decompose occupancy` wrote one block per sheet, as derived. No expectation has ever been
  corrected.
- **`title_caption_table` re-authored before it was frozen**, for the reason and with the derivation
  recorded above. Its first shape's hand-derivation and the run agreed with each other; what they
  disagreed with was the reading, and the reading is what may not move.
- **`banded_report` and `banded_subtotals` added with the sibling join.** The shape the join was
  written for had no fixture: a banded table — a header row over a body whose rows alternate in
  appearance — which `appearance` fragmented into a number of files linear in the row count, and now
  cuts to one block. Both derivations matched FSA1 on the first run: `banded_report` → `A1:C9`;
  `banded_subtotals` → `A1:C10`. `--decompose occupancy` wrote that same single block on each, as
  derived, which is why both carry `misses: 0`. **No existing expectation was corrected**: the other
  four are byte-identical across the change and still grade green.

## Declared blind spots

- **The corpus is `.xlsx` only.** An `.ods` carries no appearance channel, so it resolves to
  `occupancy` and every cell would state the same signature. There is no `appearance` reading to
  freeze for one, and `--decompose appearance` on one is refused before anything is written.
- **The corpus is small and synthetic, by the membership rule.** It says nothing about a real
  workbook's scale, nor about how either policy's cost model behaves at one — the lambda, the
  two-strip leap and the waste budget are all swept at one size here. The measured basis for the
  constants lives in the plan that set them, not in this corpus.
- **Every fixture is one sheet whose regions are stacked in rows.** A region holding many style
  changes is swept — that is the whole of what the two banded fixtures are — but nothing here holds a
  sheet whose regions sit side by side across columns rather than stacked, so the column half of the
  growth rule is exercised only by `numfmt_only_series`'s two series columns merging into one block,
  and the join's horizontal phase is reached by no fixture at all.
- **Two column regions with an empty column between them are swept nowhere**, and the behaviour at
  that shape is written down here because it surprises. Data in columns A,B and D,E with a width
  authored on the empty column C cuts into `A1:B20` and `D1:E20`: both rectangles cost zero rules, so
  bridging C shows gain 0 and is never a candidate. Column C then sits in no range file, and its
  authored width is **dropped and NAMED** — `column width for C on sheet Gapped dropped: no range file
  covers column C` — which is the contract `unpack` owes for a loss. `--decompose occupancy` writes
  `A1:E20` and loses nothing. That pair is pinned by
  `crates/fsa1-ingest/tests/import_fidelity.rs`, at the crate level and not by this corpus.
- **The corpus grades WHICH cells a block holds, and not what the block carries.** The grid contents,
  the `@scope` block and the fidelity report are `conformance/presentation/`'s and
  `conformance/serde/`'s; a change that cut the same blocks and wrote the wrong TSV into them would
  pass here and fail there.
- **Recall is measured over labelled starts, never precision.** A policy that cut every sheet into
  one block per row would pass property 1 on every fixture and fail property 2 on all of them, which
  is what property 2 is for — but nothing here bounds the file count as such.
- **Precision is unbounded, and no bound on it exists anywhere in the suite either.** The join takes
  the worst shape out: a 20,000-row by 3-column sheet striped by row — one appearance on the odd rows,
  another on the even — now unpacks to **1** range file, the same **1** `--decompose occupancy`
  writes. But it takes it out case by case rather than as a bound: a union is taken only where `rules`
  says it pays, and nothing here says how many it will decline on a sheet no one has cut yet. Property
  2 makes each fixture's file count reviewable, and the membership rule keeps every fixture small, so
  nothing here would notice a policy change that multiplied the count on a real sheet. The two banded
  fixtures guard against fragmentation at the one shape each of them states, and the corpus as a whole
  still measures recall rather than precision.
- **Runtime is superlinear on one shape, and there is no bound on it — by decision, not by
  oversight.** `occupancy` stops splitting below a 16x16 whatever the occupancy; `appearance` seeds one
  region per contiguous run of one appearance and grows every one of them until two passes fall
  silent, under no size bound, no region-count bound and no fallback to another policy. Adding one
  would make WHICH blocks the policy cuts a function of how large the sheet is — a second,
  size-dependent policy hiding inside the first, and a sheet that decomposes one way at 700 rows and
  another at 900. The cost is taken instead.

  **The shape that triggers it is a DISTINCT appearance signature in a large fraction of the cells**
  — not the cell count, and not the region count. An authored workbook rarely reaches that shape,
  because a person styles by row, column and table and the signatures repeat. A GENERATED one can: a
  per-cell colour scale flattened into `cellXfs`, a per-row number format, an exporter stamping every
  cell with its own `xf`.

  **The timings this bullet used to carry are struck, deliberately and not by oversight.** They were
  taken against the PRE-JOIN policy over six scratch sheets that were never committed, and they were
  not re-measured when the sibling join landed. A struck row is the honest outcome here; a number
  nobody took on this tree would not be.

  Do not assume the direction. The policy now runs join rounds and re-enters `grow` after any round
  that applied a union, which is work it did not do before — but a join also destroys regions, and
  growth's cost rises faster than linearly in how many there are, so a round that folds most of a
  sheet away can leave less work behind than it added. Which of the two wins is a property of the
  SHAPE, it is not settled here, and nothing in this file should be read as settling it.

  The struck numbers existed so a later reader could tell the accepted cost from a regression, and
  until they are retaken that is exactly what is missing. **Re-measuring the joined policy across
  these six shapes, on one quiet machine with both builds built side by side, is outstanding work —
  the single most useful thing anyone can do to this file.**

  What HAS been measured on the current binary is the file count on the two sheets the join was
  written for. The `before` column is the struck table's own figure, not a re-measurement:

  | sheet | cells | signatures | `appearance` files | `occupancy` files | before |
  |---|---|---|---|---|---|
  | 20,000 x 3 | 60,000 | 2, by row | **1** | 1 | 9,999 |
  | 5,000 x 15 | 75,000 | 4, a banded report | **1** | 1 | 2,499 |

  The second row is the ordinary case and the one to read first — a bold header row, a body
  zebra-banded on two alternating fills by global row parity, a bold total row — and it now cuts to
  ONE block, the same one `occupancy` writes. The struck table's other four sheets (200x20, 400x20,
  800x20 and 1,500x20, one signature per cell) have no current figure at all, in either column.

  The sheets are authored in scratch, not committed — the membership rule keeps this corpus
  hand-derivable, and none of these is. Each is a full rectangle of numbers: "one per cell" gives
  every cell its own solid fill colour, and "2, by row" fills the odd rows and leaves the even ones
  bare. **Nothing here or in the suite measures any of it**, so no test would catch one of these
  numbers moving.
