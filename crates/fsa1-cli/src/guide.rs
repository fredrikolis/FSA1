// Concern: the --guide text — the on-disk model, filenames, body grammar | Non-concern: per-command help (main.rs owns it), filling the holes (main.rs) | IO: none

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

PRESENTATION — <tab>/<range>.css, one sidecar per styled region
  The FILENAME is the scoping root — A1:C9.css, a bare A1.css, or an OPEN range A:A.css /
  1:1.css whose open axis reaches as far as the tab's own content. The file holds rules and
  nothing else. The root is what the selectors count in: td · tr:first-child td ·
  td:nth-child(k) · tr:nth-child(k) td and the periodic An+B forms, all 1-based and relative
  to the root. NO selector names one cell — give that cell its own root, F9.css.
  A stem-less .css is the tab's own default layer, beneath every rooted sidecar.
  A sidecar needs no range file beside it: a .css over a rectangle nothing fills is a
  style-only region, which renders and packs.
  Sidecars may overlap; where two roots reach one coordinate the SMALLER area wins, property
  by property, ties settled by filename. check parses and validates the sheet and reports its
  faults; render --format html carries it into the document's CSS, and pack carries it into
  the .xlsx as fonts, fills, borders, alignment, column widths and row heights.

FIGURES — <tab>/<name>.vl.json, a Vega-Lite spec bound to the tab's ranges
  The STEM is a name, never a range, so it collides with no cell and takes no part in the
  cascade. Bind data through Vega-Lite's own named data: "data": {"name": "A1:D4"}, or
  "Orders!A1:D4" across tabs. A binding is a REFERENCE — one corner or two joined by ":",
  so A1:A1 is legal and a whole column A:A is refused. Every data.name in the spec binds,
  one per layer included, and each must name a rectangle the tab fills.
  The range resolves to a table whose FIRST ROW is the field names (no blank, no duplicate)
  and whose cells contribute VALUES, never formulas: blank is null, an error is its text.
  check lints the JSON and every binding; render --format html embeds the expanded spec and
  the Vega runtime and draws it; render --format ascii names it on stderr and draws the table.

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
  fsa1-cli check  <path>               lint; non-zero on error. A tab/region in the path (wb/Tab or
                                          wb/Tab/A1:B2) checks ONLY the cells you authored
  fsa1-cli eval   <wb>[/<tab>] --formula '=SUM(A1:A5)'   evaluate a formula against the workbook
                                          (unqualified refs bind to the path tab, else the first tab)
  fsa1-cli trace  <wb>/<tab>/<A1> [--dependents]   a cell's upstream deps / downstream consumers
  fsa1-cli tree   <path> [--mode combined|values|functions]   the whole workbook's structure (every
                                          tab/cell/name) as a nested view; default COMBINED (value +
                                          source); a wb/Tab path roots it at one tab, and a wb/Tab/A1:B9
                                          path shows that viewport's cells in full (cap overridden)
  fsa1-cli sample <dir>                 write a live tutorial workbook to play with
  fsa1-cli unpack [--strict] [--decompose <policy>] <src> [<dst>]   read a .ods/.xlsx into a workbook
                                          (<dst> derives to ./<src-stem>/; --strict refuses a file the
                                          skeleton cannot round-trip identically: a non-default number
                                          format, an out-of-scope package part, or a dropped size)
                                          --decompose ({DECOMPOSITIONS}) cuts a sheet into range
                                          files: occupancy at the widest fully-empty rows/columns,
                                          appearance over runs of one cell appearance. Default:
                                          occupancy for every source — appearance is only ever
                                          used when named, so an unflagged unpack never moves
  fsa1-cli pack <dir> [--target xlsx]   serialize a workbook to a fresh ./<basename>.xlsx (inverse of
                                          unpack)

START HERE
  fsa1-cli sample ./demo && fsa1-cli render ./demo
  Contract: docs/format-spec.md
"#;
