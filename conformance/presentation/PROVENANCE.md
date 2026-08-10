<!-- Concern: the SOURCE of the visual-fidelity corpus, and the correction rule its frozen expectations are held to | Non-concern: the grading verdicts | IO: none -->
# Provenance — the presentation (visual-fidelity) corpus

This corpus is the standing proof that an .xlsx which goes through `fsa1-cli unpack` and back through
`fsa1-cli pack` **no longer comes out visually blank**. Before plan 01, every font, fill, border and
alignment was lost while the tool printed `unpack fidelity: nothing lost`.

## The fixture bytes are authored by a third party, never by FSA1

Every `fixtures/*.xlsx` is written by **openpyxl** — a third-party writer with no knowledge of FSA1 —
from the committed `make_fixtures.py`. FSA1 never produces a byte of the corpus it is graded against.
This is the same discipline `conformance/serde/` records: a tool that generates its own oracle input
grades itself, and a consistent-but-wrong implementation then passes.

Regenerate with the oracle venv (`run.sh` provisions it):

    conformance/presentation/.venv/bin/python conformance/presentation/make_fixtures.py

The `.xlsx` are committed (small, Git-LFS-tracked via the repo's `.gitattributes`); the `.venv/` and
`.artifacts/` are gitignored.

**Regeneration is not byte-stable** — openpyxl stamps `docProps/core.xml` with the run's timestamp, so
every fixture is a fresh LFS object even when nothing about it changed. Run the script when a fixture's
*content* must change, not to check that it still produces one.

## The correction rule — non-negotiable

> **A frozen expectation is corrected ONLY when the *reading* of the openpyxl-authored fixture was
> wrong — never edited to chase an FSA1 regression.**

An `expected/<fixture>.expected` file states what `unpack` must write for that fixture: the range
files, their contents, the presentation sidecars, and the SER3 warnings the run must report. It is
a **reading** of the fixture's own `xl/styles.xml`, `xl/worksheets/sheetN.xml` and `xl/theme/theme1.xml`
against the encoder rules in plan 01 — not a transcript of FSA1 output. When FSA1 and an expectation
disagree, the default verdict is an FSA1 defect. The sentence above is repeated in the assertion
message of both graders, so a failure carries it without anyone opening this file.

## The expectation format

One directive per line; `#` comments and blank lines are ignored.

    warning: <the exact SER3 line the run must report>
    file: <tab>/<entry-name>
    |<a line of that entry's contents, verbatim>

A `file:` entry is a range file, a `<range>.css` sidecar, a `<name>.json` figure or the `<name>.css`
stating where that figure sits; all four are listed alike, in the `<tab>/<name>` order the grader
sorts by, which puts a range file directly ahead of its own sidecar.
Every content line is `|`-prefixed, which is what makes an empty grid line distinguishable from
nothing at all. A sidecar ends in a newline, so its last content line is a bare `|`. A fixture with
no `warning:` line must print `unpack fidelity: nothing lost`; a fixture with one must not. A fixture
with no `file:` line must write no entry at all.

That unconditional `nothing lost` is expected here because every fixture is an `.xlsx` — the one source
format whose readers open all eight categories the report tracks. A run that inspected fewer prints a
line naming what it did not look at instead, which is why an `.ods` cannot be graded by this format
(see the blind spots).

### The sidecar's layout, and where it comes from

A sidecar's **filename** names its scoping root in the tab's own absolute A1 coordinates, and the
encoder states one root per SHEET — its used region, the bounding box of its occupancy — so a tab cut
into several blocks still carries one sidecar, and the root is wider than any one range file whenever
it was.

The sidecar's **content** — which rules, which selectors, which declarations, which values — is
derived from the fixture's own bytes against plan 01 §D2/§D3/§D4 read over that root, and
`fsa1-model`'s parser is what makes the selector spelling and the rule order canonical rather than a
matter of taste. The selector SPELLING is no longer among them, nor is the DECLARATION order: the
parser reads any spelling of the target a selector names, and a rule's declarations in any order, so
the canonical selector and the alphabetical order every expectation shows are the ENCODER's own,
which the parser accepts rather than demands.

The sidecar's **whitespace** is fixed by no contract — the parser accepts a rule on one line or five.
The corpus freezes the form the repo's own hand-written examples already use, so the expectation is
not a transcript of the writer's formatting choice:

      <selector> { <property>: <value>; <property>: <value> }

Two-space indent, one rule per line, `; `-separated declarations, a space inside each brace, no
trailing `;`, and a newline after the last rule.

## What grades it, and where each half runs

| leg | runs under | asserts |
|---|---|---|
| `conformance/tests/presentation_scope.rs` | `cargo test --workspace` — no venv, no network | the frozen stylesheet and warnings; `check` accepts what `unpack` wrote; `unpack(pack(unpack(x))) == unpack(x)` |
| `presentation_oracle.py` (via `run.sh`) | the CI `presentation-roundtrip` job | the same frozen expectation through the CLI, plus a **third-party reopen**: openpyxl must find the source's own look on the packed export |

`scripts/gate.sh` does not run the Python leg (it needs a network `pip install`), so the Rust half is
what the workspace gate picks up. The Python half is the one that can say FSA1's export is *visually*
right, because openpyxl — not FSA1 — is what reads it back.

`presentation_oracle.py` imports the sibling `conformance/xl-oracle/oracle.py` for the one definition
of the fsa1-cli location and the parity-table clipper, so `requirements.txt` pins the same reference
closure as `conformance/serde/` and `conformance/xl-oracle/`. Duplicating those two helpers here
instead would be a second copy free to drift.

## Adding a fixture

A fixture with no reading is graded by nothing, so both legs refuse a corpus that has one: the Rust
leg fails, and `run.sh` exits 2. Author the `.xlsx` in `make_fixtures.py`, then derive its
`.expected` **before** running `unpack` — a reading written after seeing the output is an
anchored transcript, whatever it says at the top of the file.

## Declared readings

Two normalizations the graders apply, stated here because they are judgments about the fixtures rather
than mechanism:

- **No fixture carries a non-`FF` alpha.** `fsa1-xlsx` writes every colour opaque, so a source colour
  with another alpha could never survive a pack. openpyxl pads a 6-digit fill to `00`, which is an
  authoring artifact and not something a real `.xlsx` states, so the corpus spells fills 8-digit.
- **`theme=1, tint=0` is NO colour.** It is the document's own default text colour (`dk1`), a visual
  no-op that the encoder deliberately never declares. The Python leg's reopen comparison reads it as
  absent on both sides.
- **The baseline a value may be left undeclared against is the FORMAT's default, not the source
  workbook's `Normal` style.** `fsa1_model::default_style` states that default once — Calibri 11pt,
  `dk1` text, upright, unbolded, undecorated, bottom-aligned, `nowrap` — and both legs read it: the
  writer restores exactly it for every property no rule named, so exactly it may be dropped on the way
  out. A source whose `Normal` style differs declares the difference on its cells.

  This used to read *"the Normal font is Calibri 11pt"*, as an observation about openpyxl. That was
  load-bearing rather than benign: every fixture happened to inherit a Calibri-11 `Normal`, so the
  corpus could not tell the two baselines apart, and a workbook with any other one lost its typeface
  in silence under `unpack fidelity: nothing lost`. `normal_font_arial_9` is the fixture that can now
  tell them apart.
- **A cell wears its axis's default where it states none of its own.** `<col style>` and
  `<row s customFormat>` are how Excel and openpyxl both write "format this whole column/row", and
  openpyxl's reader resolves `cell.font` through `cellXfs` alone — so the Python leg follows the axis
  itself (`presentation_oracle.effective`) on BOTH the source and the export. Without it the leg
  under-reads the source and calls a faithful export a divergence. Stamping the style onto the cells
  instead, the remedy the `xfId` reading below uses, is not available here: a fixture whose cells state
  the style is not a fixture for an axis default at all.
- **openpyxl resolves a cell's font through `cellXfs` alone**, never following the entry's `xfId` to
  the named cell style behind it. So on a cell that states no font of its own the reopen leg would
  compare against openpyxl's seeded Calibri 11 whatever the workbook's `Normal` style says.
  `normal_font_arial_9` therefore stamps `Normal` onto its cells, which puts the same one fact where
  both readers look; a fixture leaving them bare would grade the reader, not the export.

## The fixtures, one concern each

| fixture | asserts |
|---|---|
| `styled_header_row` | a bold/filled/centred header row crosses as ONE row rule |
| `formatted_column` | a column uniform in italic + right alignment crosses as ONE column rule |
| `total_row_top_border` | a total row's `border-top` crosses |
| `banded_body` | a zebra body whose shaded lines are EXACTLY the even ones crosses as ONE periodic rule, not three row rules |
| `body_8pt_vs_normal_11pt` | an 8pt body against the 11pt Normal font crosses as ONE `fsa1-cell` rule |
| `normal_font_arial_9` | a workbook whose `Normal` style is Arial 9 declares it, rather than treating its own Normal as the baseline |
| `sparse_blocks_normal_font_arial_9` | the same non-default Normal over SPARSE occupancy: the blanks inside each block are not content, so the two blocks survive a re-pack |
| `column_and_row_default_style` | a whole-column and a whole-row format, stated on the AXIS and on no cell, cross as a column rule and a row rule |
| `theme1_color_noop` | a theme-1 no-op emits NO `color` declaration |
| `axis_width_and_height` | a 14.5 column and a 22.5 row appear as `width: 14.5ch` and `height: 22.5pt` |
| `title_over_table` | a title over a table writes ONE file and carries column A's width |
| `stray_cell_sheet` | `A1`, `B1`, `Z50000` write TWO files totalling THREE TSV fields |
| `single_cell_sheet` | a lone cell writes a file named `A1`, never `A1:A1`, and `check` accepts it |
| `style_only_blank` | a cell whose only content is its fill is occupancy, and survives the pack |
| `hatch_and_diagonal_blanks` | of three blanks that all draw something in Excel, only the solid-filled one is occupancy; the hatch and the diagonal are named as losses instead |
| `empty_sheet` | a sheet with no occupancy writes NO file |
| `warn_merged_region` | exactly one SER3 warning, and no `nothing lost` |
| `warn_indent` | as above, for an indent level |
| `warn_dash_dot_border` | as above, for a `dashDot` edge |
| `warn_underline_double` | as above, for `u="double"` |
| `warn_center_continuous` | as above, for `horizontal="centerContinuous"` |
| `unstyled_nothing_lost` | a workbook stating no style anywhere still prints `nothing lost` |
| `chart_bar_one_series` | a one-series bar chart crosses as a `bar` figure binding the bounding rectangle of its two references, extended up to the header row |
| `chart_line_two_series` | two series sharing one category column cross as a TWO-layer spec with two `data` objects |
| `chart_radar_unsupported` | a radar has no Vega-Lite mark, so it yields NO figure and one named loss — and the unpack still completes |

### What the pack leg grades

The second unpack — `unpack(pack(unpack(x)))` — is compared over EVERY entry, figures included.
`presentation_oracle.py`'s `read_tree` excludes nothing, and it has not needed to since plan 15 gave
`pack` a chart to write: `chart_bar_one_series` reproduces its figure identically on the second leg.
This paragraph previously said the pack leg abstained from figures, which was true while a figure
had nothing on the way out to survive as, and false from the moment one did.

## Corpus history

Every entry records a fixture ADDED, an expectation CORRECTED, or the whole corpus RE-DERIVED under a
changed encoding, and why. No expectation here has been corrected: the corrections column stays empty
until a reading is shown to have been wrong.

- **Every selector re-spelled under a changed vocabulary** — the element token moved off HTML's own:
  `td` became `fsa1-cell` and `tr` became `fsa1-row`, so a sidecar's selectors name custom elements
  no `<table>` is needed to hold (plan 39). This is not a correction and no fixture was regenerated:
  every reading is the identity but for that one token, and the 24 selector lines in 16 of the 26
  `.expected` files were re-spelled in place, along with the six derivation notes naming the old
  token. No `file:` line, no ordering, and no declaration
  changed — what a rule SELECTS and what it SAYS are both untouched.

- **The two chart readings re-derived a second time** — a figure now carries the POSITION its source
  chart stated, as a `.css` beside its `.json`, where `unpack` previously kept the spec and dropped
  the placement (plan 37). This is not a correction: no reading was wrong, and both re-derivations are
  the identity on every line already frozen. Each fixture's `xl/drawings/drawing1.xml` holds exactly
  one `<oneCellAnchor>` whose `<from>` is `<col>3</col>` (`chart_bar_one_series`) or `<col>4</col>`
  (`chart_line_two_series`), `<row>1</row>`, both offsets `0`, and whose size is
  `<ext cx="5400000" cy="2700000"/>` — which is D2 and E2, a zero `left`/`top`, and exactly the 15cm
  by 7.5cm box `placement.rs` defaults to, so every property but the anchor is omitted and each new
  entry reads `  figure { anchor: D2 }` / `  figure { anchor: E2 }`. Neither fixture gains a
  `warning:` line: a `oneCellAnchor` IS what a fixed box means, so nothing is approximated. No fixture
  was added or regenerated — the approximating anchor forms are unit-tested where they parse instead,
  because regenerating churns all 25 LFS objects.

- **The two chart readings re-derived** — a figure's entry name lost its `.vl`: `FIGURE_SUFFIX`
  became `.json`, so ANY `.json` entry of a tab is a figure and the format names no vendor in a
  filename (plan 23). This is not a correction: no reading was wrong. Both re-derivations are the
  identity on their CONTENT — every `|`-prefixed line of `chart_bar_one_series.expected` and
  `chart_line_two_series.expected` is unchanged, character for character. What changed in the frozen
  text is the two `file:` lines alone: each drops the `.vl` ahead of its `.json`. The `file:`
  ordering is unaffected: the stems are unchanged and each tab holds one figure.

- **All 15 scoped readings re-derived a second time** — the scoping root moved out of the file's body
  into its NAME: the tab's one `@presentation.css` became one `<root>.css` sidecar, holding the same
  rules with the `@scope (<root>) { … }` frame dropped (plan 09). This is not a correction: no reading
  was wrong, and every one of the fifteen re-derivations is the identity on its rules — the root each
  prelude stated is now the stem its filename states, character for character. What DID change in the
  frozen text is the `file:` ordering: a sidecar sorts directly after the range file it shares a stem
  with, where `@presentation.css` sorted ahead of every range name.

- **All 15 scoped readings re-derived** — presentation moved out of the range file's tail into the
  tab's `@presentation.css`, addressed in absolute A1 (plan 08). This is not a correction: no reading
  was wrong. Fourteen of the fifteen re-derivations are the identity, because their tab holds ONE
  range file and the sheet's used region is exactly that file's range, so the rules over the new root
  are the rules over the old one, relocated. The fifteenth, `sparse_blocks_normal_font_arial_9`, is
  the one tab holding TWO blocks: its root is now the used region A1:E60 rather than A1:E5 and
  A56:E60 separately, so its two identical `fsa1-cell` rules re-derive to ONE over the whole
  rectangle. That fixture is therefore also the only one whose reading could have gone wrong here,
  and it is derived in full at the head of its own file.

- **`column_and_row_default_style` added** — the read leg took a cell's style off `<c s=>` and nowhere
  else, so the two other places .xlsx states one — `<col style=>` and `<row s= customFormat=>`, which
  is exactly how Excel and openpyxl encode "format this whole column/row" — reached no cell at all.
  A bold column unpacked to a tree with no `font-weight` in it, a bold-and-red row to a tree with no
  `@scope` block at all, and both runs printed `unpack fidelity: nothing lost` over the Styling
  category they had just lost. Every fixture the corpus held put its appearance on a `<c>`, which is
  why none of them could see it: no `<col style>` or `<row customFormat>` existed anywhere in the
  repo's .xlsx. **No expectation was corrected**: the fix carries what the old code dropped, and all
  20 existing readings stayed byte-for-byte true, because none of their fixtures states an axis
  default. This fixture's own expectation was derived from its `xl/worksheets/sheetN.xml` and
  `xl/styles.xml` — `<col style="1">` → `cellXfs[1]` → `fontId 1` → Calibri 11 bold, and
  `<row s="2" customFormat="1">` → `cellXfs[2]` → `fontId 2` → Calibri 11 italic — before `unpack`
  was run, and matched FSA1 on the first run; on the pre-fix reader sheet 1 comes back carrying only
  its width and sheet 2 carries no `@scope` block at all.
  The statement is resolved onto the cells INSIDE the sheet's extent and nowhere else: it dresses an
  unbounded set of cells the source never wrote, and materializing those is the occupancy inflation
  `BlankPaint::of` closed, arriving through a second door. So it reaches a blank only where
  `paints_blank` says the look SHOWS on one — a bold column leaves its blanks bare, exactly as the
  write leg would — and what a drawn look still covers past the extent is named
  (`AxisDefaultStyleClipped`) rather than vouched for. Both halves are pinned in
  `crates/fsa1-ingest/tests/import_fidelity.rs`, which is where a blank's half of the story lives;
  this fixture's two sheets are fully occupied so the corpus reading grades the carrying alone.
- **`normal_font_arial_9` added** — a review found the two legs held two independent notions of "the
  default a cell wears": the read leg took the SOURCE workbook's `Normal` style, the write leg a
  hardcoded Calibri 11. A workbook whose `Normal` style was anything else therefore unpacked to a tree
  with no `@scope` block at all, under `unpack fidelity: nothing lost`, and packed back in Calibri 11.
  Every fixture the corpus held was openpyxl-authored with a Calibri-11 `Normal`, which is exactly why
  none of them could see it. **No expectation was corrected**: the fix declares what the old code
  dropped, and all 17 existing readings stayed byte-for-byte true, because on a Calibri-11 `Normal`
  the two baselines coincide. This fixture's own expectation was derived from its `xl/styles.xml` —
  `cellStyles` → `cellStyleXfs[0]` → `fontId 1` → Arial 9 — and matched FSA1 on the first run; it fails
  on the pre-fix encoder, which writes the range file with no `@scope` block at all.
- **`hatch_and_diagonal_blanks` added** — the THIRD instance of the class the two entries below record,
  and the reason the relation is now structural rather than remembered. The read leg called a blank
  *filled* whenever its fill stated any pattern and *edged* whenever a `<diagonal>` was drawn, but the
  encoder spells a `background-color` only for a SOLID fill and has no declaration for a diagonal at
  all — so both were dropped and named, while the coordinate they had occupied went out through `pack`
  and never came back. `style_only_blank`'s blank carries a solid fill and every `warn_*` fixture puts
  its unspellable attribute on a cell that also holds a VALUE, so no fixture held a blank whose only
  look is one no rule can spell. **No expectation was corrected**: the fix narrows only what the READ
  leg calls content, and all 19 existing readings stayed byte-for-byte true. This fixture's own
  expectation was derived from its `xl/styles.xml` and `xl/worksheets/sheet1.xml` before `unpack` was
  run and matched FSA1 on the first run; on the pre-fix reader its 2-field `A1:B1` comes back as a
  25-field `A1:E5`. What closes the class rather than this one instance: `BlankPaint` is now built
  ONLY by `fsa1_model::BlankPaint::of`, whose sole input is a `Declaration` and whose match over that
  vocabulary is exhaustive. The write leg feeds it the declarations a cell wears; the read leg feeds it
  the ones its encoder emits for a source style. An appearance no declaration carries can therefore no
  longer answer `true` on either leg, and a droppable attribute added later reaches neither.
- **`sparse_blocks_normal_font_arial_9` added** — a review found the two legs held two independent
  notions of "a look that makes a valueless cell content": the write leg carried a blank whose look
  differed from the format default AT ALL, and the read leg counted such a blank as occupancy whenever
  its font differed from the workbook's `Normal`. On a workbook whose `Normal` is not Calibri 11 —
  which modern Excel defaults away from — a block-level `font-family` gave every blank inside a block
  a non-default look, so `pack` wrote it and the next `unpack` read it as content: a sparse sheet's
  blocks fused into one and its blank fields multiplied on every re-pack, under `unpack fidelity:
  nothing lost`. `normal_font_arial_9` is 2x2 and fully occupied, so it holds no blank inside a block
  at all, and `style_only_blank`'s blank carries a fill — so neither could see it. **No expectation
  was corrected**: the fix narrows only what the READ leg calls content, and all 18 existing readings
  stayed byte-for-byte true. This fixture's own expectation was derived from its bytes and the
  partition budget before `unpack` was run, and matched FSA1 on the first run; on the pre-fix reader
  it fails at the `unpack(pack(unpack(x)))` leg, its two 25-field files coming back as one 300-field
  `A1:E60`.
- **`style_only_blank` added** — a review found the write leg dropped a blank cell's `<c>` even when
  it carried an `s=`, so a fill-only cell went out through `pack` and never came back. Every fixture
  the corpus held put its appearance on a cell that also held a value, so nothing graded the one case
  where the appearance IS the content. Its expectation was derived from the fixture's bytes and
  matched FSA1 on the first run; it fails on the pre-fix writer at the `unpack(pack(unpack(x)))` leg.

## Declared blind spots

- **The corpus is `.xlsx` only — declared here because it was the one blind spot nothing declared.**
  `unpack` reads an `.ods` for its values and formulas alone: no styles, no widths, no heights, by
  design. So an `.ods` fixture here would freeze a reading with no `@scope` block in it, and the second
  leg could not grade it at all — `pack` writes `.xlsx`, and the reopen compares against a source
  openpyxl cannot open. The `.ods` path is bound instead by
  `crates/fsa1-ingest/tests/fixtures/styled.ods`, an odfpy-authored workbook whose bold, red-on-yellow
  `A1` sits in a 2.5cm column: `a_styled_ods_loses_its_look_and_the_report_vouches_for_no_category_it_never_read`
  asserts the tree carries no `@scope`, and the CLI leg asserts the run says which categories it never
  inspected rather than printing `nothing lost` over the loss. Both of those failed before that fix.
- The corpus is **small and synthetic**. It fixes the encoder's shape one concern at a time; it says
  nothing about a real workbook's scale, and nothing about how the partition policy behaves on one.
  The measured basis for the policy lives in plan 01 §D2, not here.
- **What the corpus samples thinly is the SHAPE of a sheet's occupancy, not its size.** The round-trip
  bar is asserted on every fixture that writes a file, by both legs — it was never a spot check. What
  was thin is the set of shapes it ran on: until `sparse_blocks_normal_font_arial_9` no fixture put a
  blank INSIDE a block whose look the block itself declared, and until `hatch_and_diagonal_blanks` no
  fixture put a blank's whole look in an attribute the encoder DROPS. Both times nothing could see the
  two legs disagree about whether such a blank is content, and the bar passed everywhere while the
  property it stands for was false. Those two fixtures now sit at one crossing point of the partition's
  waste budget and at the fill/edge vocabulary's edge. Two of the three instances of that class were
  found by a reader rather than by the corpus, which is why the third was closed at the type level —
  `BlankPaint::of` takes a `Declaration` and nothing else — rather than by adding a fixture per
  attribute. The corpus still sweeps neither the waste budget nor the block geometry, so a
  disagreement of some OTHER class at another shape would still be found by a reader.
- The Rust leg's round-trip bar is **model-equality through FSA1's own reader**
  (`unpack(pack(unpack(x))) == unpack(x)`), which cannot catch a loss FSA1 makes symmetrically on both
  legs. Catching that is exactly the Python leg's job, and it is why the corpus has two.
- No fixture carries a number format. Those are SER2's, and `conformance/serde/` already grades them.
