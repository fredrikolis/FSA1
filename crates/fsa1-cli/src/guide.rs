// Concern: the STATIC half of --guide — the on-disk model, filename and body grammar, the sidecar and figure spellings | Non-concern: the run transcript (live.rs), a verb's full help | IO: none

/// `{DECOMPOSITIONS}` is filled by `main.rs::guide_text` from [`fsa1_ingest::Decomposition::ALL`], as
/// `unpack --help`'s same hole is: the terse index cannot name a different policy set than
/// `--decompose` accepts.
pub const GUIDE: &str = r#"fsa1-cli renders a spreadsheet that IS a filesystem.

STRUCTURE
  workbook/ = dir. Tab = sub-folder, ONE level (nested reserved). Cell/range = a file in a tab.
  Cross-sheet reference inside a formula: Tab!A1

FILENAME — a closed A1 range, no extension
  A1 · A1:D1 · B2:D9 · A2:A6. Top-left:bottom-right, uppercase, no $, no leading zero.
  A1:A1 illegal (write A1). Overlap in one tab rejects; an unclaimed gap reads blank.

FILE BODY — TSV, the whole grid, no header line
  Rows = newlines, columns = tabs, empty field = blank cell. \t \n \\ escape a field;
  only an UNESCAPED tab or newline delimits, so a cell may hold multi-line text.
  Each field is a literal (Product · 10 · TRUE · #REF!) or an =formula (=B2*C2).
  The grid fills the declared range EXACTLY (B2:D9 => 8 rows x 3 cols) or it is a dimension error.
  No drag-fill: one explicit formula per cell.

ARRAYS — no spill beyond the declared range
  The filename states the range the result fills exactly, shape and orientation. =SEQUENCE(3) is
  A1:A3; A1:A4 and A1:C1 are #SPILL!; a bare A1 keeps the first element. Size the file from what
  the call RETURNS. [brackets] mark an optional argument.
  FILTER(array, include, [if_empty])   include is a 1-wide column or 1-tall row matching array's
                                        axis. No match and no if_empty is #CALC! — pass if_empty
                                        ("" for blank) whenever an empty result is legitimate.
  SORT(array, [index], [order], [by_col])   index is 1-based along the sorted axis; order is 1 asc
                                        or -1 desc and NOTHING else; by_col FALSE sorts rows.
                                        SORT keys on a column OF array.
  SORTBY(array, by1, [order1], …)     keys need NOT be part of array. This, not SORT, sorts by a
                                        separate column.
  UNIQUE(array, [by_col], [exactly_once])   exactly_once TRUE keeps only lines appearing once.
  SEQUENCE(rows, [cols], [start], [step])   cols/start/step default 1.
  Also array-returning: TRANSPOSE VSTACK HSTACK TAKE DROP CHOOSEROWS CHOOSECOLS TEXTSPLIT
                                        FREQUENCY, and INDEX/XLOOKUP whole-row/column forms.
  Every other built-in is Excel's; check names an unknown one.

PRESENTATION — <tab>/<range>.css, one sidecar per styled region
  The FILENAME is the scoping root: A1:C9.css, A1.css, or an open A:A.css / 1:1.css reaching as
  far as the tab's content. A stem-less .css is the tab's default layer, beneath every rooted one.
  RESOLVED selectors, 1-based and relative to the root:
    fsa1-cell · fsa1-row:first-child fsa1-cell · fsa1-cell:nth-child(k) ·
    fsa1-row:nth-child(k) fsa1-cell · the periodic An+B forms
  odd, even, 0n+1, a space inside the parens, and an index outside the root resolve to nothing.
  Any OTHER selector is yours: check takes it and the page paints it, but no other carrier does --
  check --xlsx and pack --strict name it xlsx-not-carried.
  Rules cascade as a browser's do. FSA1 resolves NO selector to one cell: give it its own root.
  Roots of one tab are DISJOINT, or nest with the inner root a SINGLE cell that wins property by
  property. A sidecar needs no range file beside it.

FIGURES — <tab>/<stem>.json, a Vega-Lite spec
  RANGE stem (D2:K17) IS the placement: the figure fills and RESERVES that range, collides like a
  cell file, and takes no .css.
  NAME stem (Chart1) floats and reserves nothing; an optional <name>.css states where it sits.
  Bind through Vega-Lite's named data: "data": {"name": "A1:D4"} or "Orders!A1:D4". A binding is a
  REFERENCE, so A1:A1 is legal and A:A is refused. The range resolves to a table whose FIRST ROW is
  the field names; cells contribute VALUES, never formulas.

NAMES — an identifier, not an A1 address
  An entry in its scope folder: a tab = sheet-scoped, the root = workbook-scoped. Use it in a
  formula (=SUM(Days)) or as a path's final segment. It must NOT parse as an A1 address.
    single cell   ln -s B5 Sheet1/total
    range         ln -s A2 Sheet1/Days.begin  &&  ln -s A366 Sheet1/Days.end
    formula       printf '=Base*1.05' > Sheet1/Rate

AUTHORING — the filesystem IS the write surface; there is NO write command
  Write the A1-named files directly. fsa1-cli only reads.
    printf '=SUM(A1:A2)' > budget/Sheet1/H3   &&   fsa1-cli check budget/Sheet1/H3

VERBS  render check eval trace tree sample unpack pack convert
  Surface, flags and exit codes: fsa1-cli --help, and <verb> --help.
  --decompose policies: {DECOMPOSITIONS}

"#;
