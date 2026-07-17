// Concern: the FILENAME grammar (FS2) — parse a bare filename that is a closed A1 range (`A1`, `F2:F11`, `B2:D9`; a single cell is the 0-D range `A1`) into a `FileName` (its `Rect` region + declared `Shape`), layering canonical-form POLICY on charlie-ast's A1 address grammar: reject `$`, lowercase, leading zeros, a reversed/degenerate(`A1:A1`)/whole-column range, each a named located diagnostic; never panics on a hostile name | Non-concern: the A1 address grammar itself (charlie-ast::a1 owns tokenizing an address), the file's grid (grid.rs), and whether the grid FILLS the range (GRID4, lib.rs) | IO: (a filename `&str`) -> `Result<FileName, Diagnostic>`
//! Filename parser: [`parse_filename`], [`FileName`].

use crate::diagnostic::{Applicability, ByteSpan, Code, Diagnostic, Fix, Loc};
use crate::overlap::Rect;
use charlie_ast::Shape;
use charlie_ast::a1::{A1Address, A1Error, parse_a1};

/// A well-formed, canonical filename: the closed A1 range it declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileName {
    /// The grid region the file claims (the input to the [`crate::overlap`] detector and the GRID4
    /// grid dimension check).
    pub region: Rect,
    /// `(rows, cols)` = `(maxRow-minRow+1, maxCol-minCol+1)`.
    pub declared_shape: Shape,
}

/// Parse a filename into its declared closed range, enforcing canonical form. A single address (`A1`)
/// is the 0-D range; `left:right` is a bounded rectangle. Never panics; a hostile name yields a
/// located [`Diagnostic`].
pub fn parse_filename(name: &str) -> Result<FileName, Diagnostic> {
    match name.split_once(':') {
        None => parse_single(name),
        Some((left, right)) => parse_range(name, left, right),
    }
}

/// A single-address filename (`A1`) — the 0-D closed range, one cell.
fn parse_single(name: &str) -> Result<FileName, Diagnostic> {
    let a = parse_a1(name).map_err(|e| a1_diag(name, 0, name.len(), e))?;
    enforce_canonical(name, 0, name.len(), &a)?;
    Ok(FileName {
        region: Rect::cell(a.col, a.row),
        declared_shape: Shape { rows: 1, cols: 1 },
    })
}

/// A `left:right` filename — a closed rectangle, canonical top-left`:`bottom-right.
fn parse_range(name: &str, left: &str, right: &str) -> Result<FileName, Diagnostic> {
    // The right address begins one byte past the `:` (`split_once` consumed it at `left.len()`), so a
    // diagnostic into the right endpoint offsets by this.
    let colon = left.len();
    if right.contains(':') {
        return Err(Diagnostic::new(
            Code::MalformedFilename,
            Loc::file_at(name, colon, name.len() - colon),
            format!("a closed range has exactly one `:`; found more: {name:?}"),
        ));
    }

    // Whole-column (`A:A`) / whole-row (`3:3`) ranges are not a closed range — they are reserved.
    // Detect them before address parsing so they get their own named refusal, not a generic malformed.
    if (is_all_alpha(left) && is_all_alpha(right)) || (is_all_digit(left) && is_all_digit(right)) {
        return Err(Diagnostic::new(
            Code::WholeColumnRowReserved,
            Loc::file(name),
            format!("whole-column/row ranges are not a closed range: {name:?}"),
        ));
    }

    let la = parse_a1(left).map_err(|e| a1_diag(name, 0, left.len(), e))?;
    let ra = parse_a1(right).map_err(|e| a1_diag(name, colon + 1, right.len(), e))?;
    enforce_canonical(name, 0, left.len(), &la)?;
    enforce_canonical(name, colon + 1, right.len(), &ra)?;

    // Canonical ordering: top-left is min column AND min row; bottom-right is max of each. This one
    // check rejects every reversed spelling (`G8:A3`, `A8:G3`, `G3:A8`) of the same rectangle. The
    // canonical rewrite of the whole filename is a deterministic machine `fix`.
    if la.col > ra.col || la.row > ra.row {
        let canonical = format!(
            "{}:{}",
            charlie_ast::a1::format_cell(la.col.min(ra.col), la.row.min(ra.row)),
            charlie_ast::a1::format_cell(la.col.max(ra.col), la.row.max(ra.row)),
        );
        return Err(Diagnostic::new(
            Code::NonCanonicalRange,
            Loc::file(name),
            format!("a range must be top-left:bottom-right; {name:?} should be {canonical}"),
        )
        .with_fix(rename_fix(name, canonical)));
    }
    if la.col == ra.col && la.row == ra.row {
        // A `1x1` range is a REJECT, not an accept-and-canonicalize to the bare address: one file,
        // one canonical name, and a single cell is always written as the address (`A1`), never `A1:A1`.
        let canonical = charlie_ast::a1::format_cell(la.col, la.row);
        return Err(Diagnostic::new(
            Code::DegenerateRange,
            Loc::file(name),
            format!("a 1x1 range is illegal; a single cell is written {canonical}"),
        )
        .with_fix(rename_fix(name, canonical)));
    }

    Ok(FileName {
        region: Rect {
            min_col: la.col,
            min_row: la.row,
            max_col: ra.col,
            max_row: ra.row,
        },
        declared_shape: Shape {
            rows: ra.row - la.row + 1,
            cols: ra.col - la.col + 1,
        },
    })
}

/// Apply the filename canonical-form policy to one parsed address: no `$`, uppercase only, no leading
/// zero (FS2). `offset`/`addr_len` are the address's byte span within the whole filename, so the
/// diagnostic points at (and its `fix` overwrites) exactly that token. Each refusal carries a
/// machine-applicable `fix`: the deterministic canonical spelling of the address ([`format_cell`],
/// which is uppercase, unpadded, and `$`-free by construction).
fn enforce_canonical(
    name: &str,
    offset: usize,
    addr_len: usize,
    a: &A1Address,
) -> Result<(), Diagnostic> {
    let canonical = |code: Code, message: String| {
        Diagnostic::new(code, Loc::file_at(name, offset, addr_len), message).with_fix(Fix {
            applicability: Applicability::MachineApplicable,
            span: ByteSpan {
                offset,
                len: addr_len,
            },
            replacement: charlie_ast::a1::format_cell(a.col, a.row),
        })
    };
    // A file's own address is intrinsically fixed, so a `$` absolute-marker is meaningless in a
    // filename — the `$` markers live only inside formula bodies. Reject rather than silently strip.
    if a.col_abs || a.row_abs {
        return Err(canonical(
            Code::DollarInFilename,
            format!("$ is not allowed in a filename (it lives in formula bodies): {name:?}"),
        ));
    }
    if a.col_had_lowercase {
        return Err(canonical(
            Code::LowercaseColumn,
            format!("column letters must be uppercase: {name:?}"),
        ));
    }
    if a.row_had_leading_zero {
        return Err(canonical(
            Code::LeadingZeroRow,
            format!("a row number must not have a leading zero: {name:?}"),
        ));
    }
    Ok(())
}

/// A machine-applicable rename of the WHOLE filename to `replacement` — the deterministic canonical
/// spelling of a reversed or degenerate range (which replaces the entire name, not one address token).
fn rename_fix(name: &str, replacement: String) -> Fix {
    Fix {
        applicability: Applicability::MachineApplicable,
        span: ByteSpan {
            offset: 0,
            len: name.len(),
        },
        replacement,
    }
}

fn a1_diag(name: &str, offset: usize, addr_len: usize, err: A1Error) -> Diagnostic {
    let (byte, detail) = match err {
        A1Error::Empty => (offset, "empty address".to_string()),
        A1Error::MissingColumn { at } => (offset + at, "missing column letters".to_string()),
        A1Error::MissingRow { at } => (offset + at, "missing row digits".to_string()),
        A1Error::UnexpectedChar { at } => {
            (offset + at, "unexpected trailing character".to_string())
        }
        A1Error::ColumnOverflow => (offset, "column index too large".to_string()),
        A1Error::RowOverflow => (offset, "row index too large".to_string()),
    };
    // A malformed address has no canonical spelling to suggest (no `fix`); the located span runs from
    // the error byte to the end of the offending address token (at least one byte).
    let len = (offset + addr_len).saturating_sub(byte).max(1);
    Diagnostic::new(
        Code::MalformedFilename,
        Loc::file_at(name, byte, len),
        format!("malformed address ({detail}): {name:?}"),
    )
}

fn is_all_alpha(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphabetic())
}

fn is_all_digit(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(name: &str) -> FileName {
        parse_filename(name).unwrap_or_else(|d| panic!("{name} should parse: {d}"))
    }

    fn err_code(name: &str) -> Code {
        parse_filename(name)
            .err()
            .unwrap_or_else(|| panic!("{name} should be rejected"))
            .code
    }

    #[test]
    fn canonical_cell_and_range_shapes() {
        // A single address is the 0-D range: a 1x1 grid.
        assert_eq!(ok("A1").declared_shape, Shape { rows: 1, cols: 1 });
        assert_eq!(ok("AA10").region, Rect::cell(26, 9));

        assert_eq!(ok("A1:D1").declared_shape, Shape { rows: 1, cols: 4 });
        assert_eq!(ok("A2:A6").declared_shape, Shape { rows: 5, cols: 1 });
        assert_eq!(ok("A3:G8").declared_shape, Shape { rows: 6, cols: 7 });
        assert_eq!(
            ok("A3:G8").region,
            Rect {
                min_col: 0,
                min_row: 2,
                max_col: 6,
                max_row: 7,
            }
        );
        assert_eq!(ok("B2:D9").declared_shape, Shape { rows: 8, cols: 3 });
    }

    #[test]
    fn rejects_non_canonical_names() {
        assert_eq!(err_code("a1"), Code::LowercaseColumn);
        assert_eq!(err_code("A01"), Code::LeadingZeroRow);
        assert_eq!(err_code("$A$1"), Code::DollarInFilename);
        assert_eq!(err_code("$A1"), Code::DollarInFilename);
        assert_eq!(err_code("G8:A3"), Code::NonCanonicalRange);
        assert_eq!(err_code("A8:G3"), Code::NonCanonicalRange);
        assert_eq!(err_code("G3:A8"), Code::NonCanonicalRange);
        assert_eq!(err_code("A1:A1"), Code::DegenerateRange);
        assert_eq!(err_code("A:A"), Code::WholeColumnRowReserved);
        assert_eq!(err_code("3:3"), Code::WholeColumnRowReserved);
    }

    #[test]
    fn rejects_malformed_names() {
        assert_eq!(err_code("A1.cell"), Code::MalformedFilename); // trailing `.cell` is not an address
        assert_eq!(err_code("A1.txt"), Code::MalformedFilename);
        assert_eq!(err_code(""), Code::MalformedFilename); // empty address
        assert_eq!(err_code("1"), Code::MalformedFilename); // no column
        assert_eq!(err_code("A"), Code::MalformedFilename); // no row
        assert_eq!(err_code("A1:"), Code::MalformedFilename); // no right address
        assert_eq!(err_code("A1:B2:C3"), Code::MalformedFilename); // too many ':'
    }

    #[test]
    fn hostile_names_never_panic() {
        // A grab-bag of adversarial names — the parser must return, never unwind.
        for name in ["", ":", "::", &"A".repeat(100), "λ1", "A1A1", "A1:B2:"] {
            let _ = parse_filename(name);
        }
    }
}
