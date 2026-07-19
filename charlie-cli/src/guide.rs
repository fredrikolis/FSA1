// Concern: the `charlie-cli --guide` text — the ONE home of the terse, dense, scannable agent-facing tour of the on-disk model (structure, closed-range filenames, the TSV body grammar, the commands, and the start-here pointers) that replaces the retired format guide; printed verbatim by `--guide` | Non-concern: the spreadsheet logic it describes (charlie-model owns it), the authoritative contract (SPEC.md), and the live tutorial workbook (charlie-model `sample_workbook`, written by `charlie-cli sample`) | IO: () -> the guide text on stdout
//! The `--guide` text. One `const`, printed verbatim — per `agentic-communication.md`: dense,
//! scannable, facts not prose. Deeper rationale lives in the governing code; the contract is `SPEC.md`.

/// The terse guide, printed verbatim by `charlie-cli --guide`.
pub const GUIDE: &str = r#"charlie-cli — a spreadsheet that IS a filesystem. Terse guide; the contract is SPEC.md.

STRUCTURE
  workbook/ = a directory.  Tab = a sub-folder.  Cell/range = a file inside a tab.
  One folder level under the root (nested folders reserved, not sub-sheets).
  Cross-sheet reference: Tab!A1 (inside a formula body).

FILENAMES — a closed A1 range, no ending
  single cell A1 · row A1:D1 · block B2:D9 · column A2:A6
  top-left:bottom-right, uppercase, no $, no leading zero. A1:A1 illegal (write A1).
  Overlapping ranges in one tab reject; an unclaimed gap reads blank.

FILE BODY — a TSV grid, one cell per coordinate
  The whole file is the grid: no header/annotation line, line 1 is the first row.
  Rows = newlines, columns = tabs. An empty field = a blank cell.
  Each field is a literal (Product · 10 · TRUE · #REF!) or an =formula (=B2*C2).
  The grid must fill the declared range exactly (B2:D9 ⇒ 8×3 rows×cols, else dimension error).
  No drag-fill: write one explicit formula per cell (=B2*C2, =B3*C3, …).

AUTHORING — the filesystem IS the write surface; there is NO write command
  Author/edit by writing the A1-named cell files DIRECTLY with ordinary file tools. A cell is its own
  file: its name is its A1 range, its content is its grid. charlie-cli only READS (render/check/eval).
    mkdir -p budget/Sheet1
    printf '=SUM(A1:A2)' > budget/Sheet1/H3       # the file name IS the cell address
    charlie-cli check budget --cell Sheet1!H3      # scoped validation of just that cell
  Then render/check to verify. Repair a rejected cell by editing its file and re-checking.

COMMANDS
  charlie-cli render <dir> [--tab NAME] [--range A1:D9] [--values|--functions]
  charlie-cli check  <dir> [--tab NAME] [--range A1:B2] [--cell A1]   lint; non-zero on error
                                          scope to a tab/range to check ONLY the cells you authored
  charlie-cli eval   <dir> --formula '=SUM(A1:A5)'   evaluate an ad-hoc formula against the workbook
  charlie-cli sample <dir>                 write a live tutorial workbook to play with

START HERE
  charlie-cli sample ./demo && charlie-cli render ./demo
  Contract: SPEC.md
"#;
