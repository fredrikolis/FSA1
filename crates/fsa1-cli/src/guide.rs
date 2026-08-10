// Concern: the --guide text — the on-disk model, filenames, body grammar, the terse verb index | Non-concern: what a function DOES (fsa1-ast), a verb's FULL help and the holes (main.rs) | IO: none

/// `{DECOMPOSITIONS}` is filled by `main.rs::guide_text` from [`fsa1_ingest::Decomposition::ALL`], as
/// `unpack --help`'s same hole is: the terse index cannot name a different policy set than
/// `--decompose` accepts.
pub const GUIDE: &str = r#"fsa1-cli renders a spreadsheet that IS a filesystem. Terse guide; the contract is docs/format-spec.md.

STRUCTURE
  workbook/ = a directory.  Tab = a sub-folder.  Cell/range = a file inside a tab.
  One folder level under the root (nested folders reserved, not sub-sheets).
  Cross-sheet reference: Tab!A1 (inside a formula body).

FILENAMES — a closed A1 range, no ending
  single cell A1 · row A1:D1 · block B2:D9 · column A2:A6
  top-left:bottom-right, uppercase, no $, no leading zero. A1:A1 illegal (write A1).
  Overlapping ranges in one tab reject; an unclaimed gap reads blank.

FILE BODY — a TSV grid, one cell per coordinate
  The file is the grid, whole: no header/annotation line, line 1 is the first row, and no
  trailing block — presentation is a <range>.css sidecar, and a grid ending in a @scope
  block is a presentation-in-grid refusal.
  Rows = newlines, columns = tabs. An empty field = a blank cell.
  A field holds a tab/newline/backslash as \t \n \\ — only an UNESCAPED tab or newline
  delimits (so a cell can hold multi-line text); a stray backslash escape is a located error.
  Each field is a literal (Product · 10 · TRUE · #REF!) or an =formula (=B2*C2).
  The grid must fill the declared range exactly (B2:D9 ⇒ 8×3 rows×cols, else dimension error).
  No drag-fill: write one explicit formula per cell (=B2*C2, =B3*C3, …).

FUNCTIONS — the dynamic-array family, whose ARGUMENTS are the part you cannot guess
  There is NO spill beyond a declared range: name the file the range the result fills exactly, shape
  and orientation both. =SEQUENCE(3) is A1:A3 — A1:A4 is #SPILL!, A1:C1 is #SPILL!, and a bare A1
  keeps the first element only.
  [square brackets] mark an optional argument; an omitted one takes the default stated here.
  FILTER(array, include, [if_empty])   include is a 1-wide column (or 1-tall row) matching array's
                                          axis. NO match and NO if_empty is #CALC! — pass if_empty
                                          ("" for blank) whenever an empty result is legitimate.
  SORT(array, [index], [order], [by_col])   index=1 is 1-based along the sorted axis; order 1 asc
                                          / -1 desc and NOTHING else; by_col FALSE sorts rows,
                                          TRUE sorts columns. SORT keys on a column OF array.
  SORTBY(array, by1, [order1], by2, [order2], …)   sorts array by key vectors that need NOT be part
                                          of it, each matching array's axis; order defaults 1 asc.
                                          This, not SORT, is how you sort by a separate column.
  UNIQUE(array, [by_col], [exactly_once])   by_col FALSE compares rows; exactly_once TRUE keeps
                                          only the lines that appear exactly once.
  SEQUENCE(rows, [cols], [start], [step])   cols/start/step default 1.
  Others returning an array, NOT a closed list: TRANSPOSE · VSTACK · HSTACK · TAKE · DROP ·
                                          CHOOSEROWS · CHOOSECOLS · TEXTSPLIT · FREQUENCY, and
                                          INDEX/XLOOKUP in their whole-row and whole-column forms.
                                          Size the file from what the call RETURNS, not from here.
  Every other built-in is spelled and argued as in Excel; check names an unknown one.

PRESENTATION — <tab>/<range>.css, one sidecar per styled region
  The FILENAME is the scoping root — A1:C9.css, a bare A1.css, or an OPEN range A:A.css /
  1:1.css whose open axis reaches as far as the tab's own content. The file holds rules and
  nothing else. The root is what the selectors count in, and these are the ones FSA1 RESOLVES:
  fsa1-cell · fsa1-row:first-child fsa1-cell · fsa1-cell:nth-child(k) ·
  fsa1-row:nth-child(k) fsa1-cell and the periodic An+B forms, all 1-based and relative to the
  root. Any OTHER selector is written as you like — check takes it, the page paints it, and no
  other carrier does: check --xlsx and pack --strict name the whole rule as xlsx-not-carried.
  An index is a literal :nth-child(k), 1-based within the root — :first-child and :last-child
  are k=1 and k=the last line, one selector each — or a periodic :nth-child(An+B), A of 2 or more
  and B under A, whose +0 may be dropped (2n = 2n+0). odd, even, 0n+1 and any space inside the
  parens resolve to nothing; an index OUTSIDE the root is refused. ANY whitespace joins the row
  and the cell compound.
  Rules are written in ANY order and cascade as the browser does — specificity, then source
  order — so a row rule beats a column rule wherever both match, a selector written twice
  layers rather than being refused, and of two rules that tie the LAST one written wins.
  FSA1 resolves NO selector to one cell — give that cell its own root, F9.css.
  A stem-less .css is the tab's own default layer, beneath every rooted sidecar. Its indices
  count in the TAB's content, so once the tab holds one rooted sidecar every rule there is a
  bare fsa1-cell or declares nothing but width/height.
  A sidecar needs no range file beside it: a .css over a rectangle nothing fills is a
  style-only region, which renders and packs.
  Roots of one tab are DISJOINT, or nest with the inner root a SINGLE cell, which layers last
  and wins property by property. Crossing roots, and a multi-cell root inside another, are
  refused. check parses and validates the sheet and reports its
  faults; pack carries it into the .xlsx as fonts, fills, borders, alignment, column widths
  and row heights.
  A rule may declare ANY property but a SIZE the selector does not name the axis of: width is a
  column's and height a row's, so either on a selector naming neither — a periodic one, or the
  cross axis — is refused presentation-property. What FSA1 RESOLVES is the sixteen an .xlsx
  holds — the ones just named; anything else renders and nothing more, and pack writes the .xlsx
  and names it as xlsx-not-carried with its file and line, which check --xlsx says before you pack.
  render --format html writes the TRANSPARENT form (PRES2): every sidecar's own bytes reach
  the page unchanged, in a <style> scoped to the region its filename names and layered in the
  model's cascade order — so what the page paints is what check resolved and pack writes,
  plus the author's own bytes for every declaration the model does not carry.

FIGURES — <tab>/<stem>.json, a Vega-Lite spec bound to the tab's ranges
  Any .json entry of a tab with a non-empty stem is a figure, and a figure is the only JSON
  a workbook holds. Its stem states one of TWO forms.
  RANGE form — a stem spelling a canonical closed range inside Excel's grid (D2:K17, Q4) IS
  this form, so a chart named Q4 is placed at Q4 and not called Q4. The name is the
  placement: the figure FILLS that range and RESERVES it, colliding with any cell file or
  other figure reaching it, and it takes no .css — one beside it is figure-sidecar-clash.
  NAME form — any other stem (Chart1, sales1, notes2024). It reserves nothing and floats;
  an optional <name>.css states where it sits, in EMU, down to sub-cell offsets. An imported
  chart carries its source position in one; a position no anchor spells exactly is reported.
  Bind data through Vega-Lite's own named data: "data": {"name": "A1:D4"}, or
  "Orders!A1:D4" across tabs. A binding is a REFERENCE — one corner or two joined by ":",
  so A1:A1 is legal and a whole column A:A is refused. Every data.name in the spec binds,
  one per layer included, and each must name a rectangle the tab fills.
  The range resolves to a table whose FIRST ROW is the field names (no blank, no duplicate)
  and whose cells contribute VALUES, never formulas: blank is null, an error is its text.
  check lints the JSON and every binding; render --format html embeds the expanded spec and
  the Vega runtime and draws it; render --format ascii cannot draw one, so it names it on
  stderr and SHOWS it over the cells it covers: a figure named for its range (D2:F6.json)
  whose rectangle no other figure's cover reaches into is ONE block labelled over two lines,
  `<entry>` then `<mark>←<bindings>`; every other figure, that one included once another
  cover intersects it, MARKS each covered cell -- `fig` where the cell is empty, `fig! `
  before the cell's own text where it is not. tree marks nothing.

NAMES — a named cell/range/formula, by an identifier (not an A1 address)
  A name is an entry in its SCOPE folder: a tab folder = sheet-scoped, the workbook root = workbook-
  scoped (a sheet-scoped name shadows a workbook one of the same identifier). Reference it in a
  formula by its identifier: =SUM(Days), =total*2. A name is also ADDRESSABLE on the CLI as a path's
  final segment — render/check/trace <wb>/<tab>/<Name> resolves it to its target cell/range.
    single cell → a SYMLINK to the cell file:            ln -s B5 Sheet1/total
    a range     → two corner symlinks .begin / .end:     ln -s A2 Sheet1/Days.begin
                                                         ln -s A366 Sheet1/Days.end
    a formula/constant → a regular file holding =expr:    printf '=Base*1.05' > Sheet1/Rate
                                                         printf '3.14' > Sheet1/Pi
  A name's identifier must NOT parse as an A1 address (rename Q1); a range needs BOTH corners.
  Editing through a symlink writes its cell (CORE3). Distribution is a tar (it preserves symlinks).

AUTHORING — the filesystem IS the write surface; there is NO write command
  Author/edit by writing the A1-named cell files DIRECTLY with ordinary file tools. A cell is its own
  file: its name is its A1 range, its content is its grid. fsa1-cli only READS (render/check/eval).
    mkdir -p budget/Sheet1
    printf '=SUM(A1:A2)' > budget/Sheet1/H3       # the file name IS the cell address
    fsa1-cli check budget/Sheet1/H3             # scoped validation of just that cell
  Then render/check to verify. Repair a rejected cell by editing its file and re-checking.

COMMANDS  (the tab and A1 cell/range are PART OF THE PATH: <wb>[/<tab>[/<A1>]])
  fsa1-cli render <path> [--mode combined|values|functions] [--format ascii|html]
                                          <path>=<wb>[/<tab>[/<A1>]]; default COMBINED: a formula shows
                                          `<value> ← =<formula>`. render wb/Tab/A1:D9 draws that region.
                                          --format html writes one standalone styled document to stdout,
                                          always VALUES with each formula in its formula bar, and so
                                          takes no --mode
  fsa1-cli check  <path> [--xlsx]      lint; non-zero on error. A tab/region in the path (wb/Tab or
                                          wb/Tab/A1:B2) checks ONLY the cells you authored; --xlsx
                                          also names what an .xlsx export would not carry, writing
                                          no file
  fsa1-cli eval   <wb>[/<tab>] --formula '=SUM(A1:A5)'   evaluate a formula against the workbook
                                          (unqualified refs bind to the path tab, else the first tab)
  fsa1-cli trace  <wb>/<tab>/<A1> [--dependents]   a cell's upstream deps / downstream consumers
  fsa1-cli tree   <path> [--mode combined|values|functions]   the whole workbook's structure (every
                                          tab/cell/name/figure) as a nested view; default COMBINED
                                          (value + source); each figure is named with the mark it draws
                                          and the ranges it binds, `bar ← A1:B3`; a wb/Tab path roots it
                                          at one tab, and a wb/Tab/A1:B9 path shows that viewport's
                                          cells in full (cap overridden) and NO figure — a region is
                                          a rectangle of cells
  fsa1-cli sample <dir>                 write a live tutorial workbook to play with
  fsa1-cli unpack [--strict] [--decompose <policy>] <src> [<dst>]   read a .ods/.xlsx into a workbook
                                          (<dst> derives to ./<src-stem>/; --strict refuses a file the
                                          skeleton cannot round-trip identically: a non-default number
                                          format, an out-of-scope package part, a chart carrying no
                                          figure, or a dropped size)
                                          --decompose ({DECOMPOSITIONS}) cuts a sheet into range
                                          files: occupancy at the widest fully-empty rows/columns,
                                          appearance over runs of one cell appearance. Default:
                                          occupancy for every source — appearance is only ever
                                          used when named, so an unflagged unpack never moves
  fsa1-cli pack [--strict] [--force]    serialize a workbook to a single .xlsx (inverse of unpack).
      [--target xlsx] <dir> [<dst>]       <dst> is used verbatim and its parent must already exist;
                                          omitted, it derives to ./<basename>.xlsx. An existing file
                                          there is refused unless --force (-f, -y) is given, the one
                                          way pack overwrites; either way the write lands whole or
                                          not at all. A figure becomes
                                          a native Excel chart where Excel states one; one it does
                                          not is DROPPED and named, and --strict refuses instead of
                                          writing without it

START HERE
  fsa1-cli sample ./demo && fsa1-cli render ./demo
  Contract: docs/format-spec.md
"#;
