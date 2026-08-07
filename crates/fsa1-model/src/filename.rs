// Concern: parses a name into the A1 range it declares, closed or open, refusing non-canonical spellings | Non-concern: file contents, range overlap | IO: (&str) -> a FileName or a Root, else a refusal

use crate::diagnostic::{Applicability, ByteSpan, Code, Diagnostic, Fix, Loc};
use crate::overlap::Rect;
use fsa1_ast::Shape;
use fsa1_ast::a1::{A1Address, A1Error, parse_a1};

/// The two spellings of a closed range's separator IN A CELL FILE NAME. On Windows `:` opens an NTFS
/// alternate data stream, so `Data/A1:C1` writes a 0-byte `A1` carrying a hidden `C1` and loses the
/// grid. `-` is the portable spelling: like `:` it cannot occur in a single-cell address or a defined
/// name, so a name holding either is unambiguously a range and never one like `Tax_Rate`.
pub const RANGE_SEP_POSIX: char = ':';
pub const RANGE_SEP_WINDOWS: char = '-';

/// The separator the WRITER uses on this host: `:` on POSIX (where it is legal and natural), `-` on
/// Windows (where `:` is not a legal filename char). The reader accepts both regardless.
#[cfg(windows)]
pub const RANGE_SEP: char = RANGE_SEP_WINDOWS;
#[cfg(not(windows))]
pub const RANGE_SEP: char = RANGE_SEP_POSIX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileName {
    pub region: Rect,
    pub declared_shape: Shape,
}

/// A sidecar's scoping root. `Closed` is spelled whole; the other two name ONE axis and leave the
/// other open, binding it late to the tab's content — so `A:A` is column A over whatever the tab
/// reaches, and stays that after a row is appended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Root {
    Closed(Rect),
    Columns { first: u32, last: u32 },
    Rows { first: u32, last: u32 },
}

impl Root {
    /// `None` where the tab states no content: an open range is clamped to it by construction, so it
    /// can never be what makes a coordinate stated.
    pub fn resolve(self, content: Option<Rect>) -> Option<Rect> {
        match self {
            Root::Closed(region) => Some(region),
            Root::Columns { first, last } => content.map(|c| Rect {
                min_col: first,
                max_col: last,
                min_row: c.min_row,
                max_row: c.max_row,
            }),
            Root::Rows { first, last } => content.map(|c| Rect {
                min_col: c.min_col,
                max_col: c.max_col,
                min_row: first,
                max_row: last,
            }),
        }
    }
}

/// What a SIDECAR stem may name, which is a closed range or an open one. A range file's name is
/// [`parse_filename`], which still refuses the open forms: a grid fills its declared range exactly,
/// and a whole column is 1,048,576 rows of it.
pub fn parse_root(name: &str) -> Result<Root, Diagnostic> {
    let split = name
        .split_once(RANGE_SEP_POSIX)
        .or_else(|| dash_range_split(name))
        .or_else(|| open_dash_split(name));
    if let Some((left, right)) = split {
        if is_all_alpha(left) && is_all_alpha(right) {
            let (first, last) = (column_of(name, left)?, column_of(name, right)?);
            return ordered(name, first, last).map(|(first, last)| Root::Columns { first, last });
        }
        if is_all_digit(left) && is_all_digit(right) {
            let (first, last) = (row_of(name, left)?, row_of(name, right)?);
            return ordered(name, first, last).map(|(first, last)| Root::Rows { first, last });
        }
    }
    parse_filename(name).map(|parsed| Root::Closed(parsed.region))
}

/// `A-C` and `2-5` on a host that spells a range with `-`. Kept apart from [`dash_split`] because
/// that one demands two parseable A1 corners, which an open end is not.
fn open_dash_split(name: &str) -> Option<(&str, &str)> {
    let (left, right) = name.split_once(RANGE_SEP_WINDOWS)?;
    let open =
        (is_all_alpha(left) && is_all_alpha(right)) || (is_all_digit(left) && is_all_digit(right));
    open.then_some((left, right))
}

fn column_of(name: &str, letters: &str) -> Result<u32, Diagnostic> {
    parse_a1(&format!("{letters}1"))
        .map(|a| a.col)
        .map_err(|e| a1_diag(name, 0, letters.len(), e))
}

fn row_of(name: &str, digits: &str) -> Result<u32, Diagnostic> {
    let n: u32 = digits.parse().map_err(|_| {
        Diagnostic::new(
            Code::MalformedFilename,
            Loc::file(name),
            format!("{digits:?} is not a row number: {name:?}"),
        )
    })?;
    n.checked_sub(1).ok_or_else(|| {
        Diagnostic::new(
            Code::MalformedFilename,
            Loc::file(name),
            format!("row numbers start at 1: {name:?}"),
        )
    })
}

/// Top-left to bottom-right, as every closed range is spelled: `C:A` is the same region written
/// backwards, and one region with two spellings is what the cascade cannot order.
fn ordered(name: &str, first: u32, last: u32) -> Result<(u32, u32), Diagnostic> {
    if first > last {
        return Err(Diagnostic::new(
            Code::MalformedFilename,
            Loc::file(name),
            format!("an open range runs top-left to bottom-right: {name:?}"),
        ));
    }
    Ok((first, last))
}

pub fn parse_filename(name: &str) -> Result<FileName, Diagnostic> {
    if let Some((left, right)) = name.split_once(RANGE_SEP_POSIX) {
        return parse_range(name, RANGE_SEP_POSIX, left, right);
    }
    if let Some((left, right)) = dash_range_split(name) {
        return parse_range(name, RANGE_SEP_WINDOWS, left, right);
    }
    parse_single(name)
}

/// `Some((left, right))` iff `name` splits on `-` into two parseable A1 corners — the guard that
/// keeps a stray `-` file (or a name that merely contains one) out of the range parser.
fn dash_range_split(name: &str) -> Option<(&str, &str)> {
    let (left, right) = name.split_once(RANGE_SEP_WINDOWS)?;
    if parse_a1(left).is_ok() && !right.contains(RANGE_SEP_WINDOWS) && parse_a1(right).is_ok() {
        Some((left, right))
    } else {
        None
    }
}

/// Re-spell a closed-range file NAME with the `to` separator (`:` or `-`), parsing it under either
/// separator first. Returns `None` for a single cell, a defined name, a malformed range, or a name
/// already spelled with `to` — i.e. exactly the names the `convert` command should leave untouched.
pub fn reseparate_range_name(name: &str, to: char) -> Option<String> {
    let parsed = parse_filename(name).ok()?;
    if parsed.declared_shape.rows == 1 && parsed.declared_shape.cols == 1 {
        return None; // a single cell has no separator to convert
    }
    let r = parsed.region;
    let spelled = format!(
        "{}{to}{}",
        fsa1_ast::a1::format_cell(r.min_col, r.min_row),
        fsa1_ast::a1::format_cell(r.max_col, r.max_row),
    );
    (spelled != name).then_some(spelled)
}

fn parse_single(name: &str) -> Result<FileName, Diagnostic> {
    let a = parse_a1(name).map_err(|e| a1_diag(name, 0, name.len(), e))?;
    enforce_canonical(name, 0, name.len(), &a)?;
    Ok(FileName {
        region: Rect::cell(a.col, a.row),
        declared_shape: Shape { rows: 1, cols: 1 },
    })
}

fn parse_range(name: &str, sep: char, left: &str, right: &str) -> Result<FileName, Diagnostic> {
    // `split_once` consumed the separator here, so the right address starts at `at + 1`.
    let at = left.len();
    if right.contains(sep) {
        return Err(Diagnostic::new(
            Code::MalformedFilename,
            Loc::file_at(name, at, name.len() - at),
            format!("a closed range has exactly one `{sep}`; found more: {name:?}"),
        ));
    }

    // Before address parsing, so `A:A` earns its own refusal rather than a generic malformed one.
    if (is_all_alpha(left) && is_all_alpha(right)) || (is_all_digit(left) && is_all_digit(right)) {
        return Err(Diagnostic::new(
            Code::WholeColumnRowReserved,
            Loc::file(name),
            format!("whole-column/row ranges are not a closed range: {name:?}"),
        ));
    }

    let la = parse_a1(left).map_err(|e| a1_diag(name, 0, left.len(), e))?;
    let ra = parse_a1(right).map_err(|e| a1_diag(name, at + 1, right.len(), e))?;
    enforce_canonical(name, 0, left.len(), &la)?;
    enforce_canonical(name, at + 1, right.len(), &ra)?;

    if la.col > ra.col || la.row > ra.row {
        let canonical = format!(
            "{}{sep}{}",
            fsa1_ast::a1::format_cell(la.col.min(ra.col), la.row.min(ra.row)),
            fsa1_ast::a1::format_cell(la.col.max(ra.col), la.row.max(ra.row)),
        );
        return Err(Diagnostic::new(
            Code::NonCanonicalRange,
            Loc::file(name),
            format!("a range must be top-left:bottom-right; {name:?} should be {canonical}"),
        )
        .with_fix(rename_fix(name, canonical)));
    }
    if la.col == ra.col && la.row == ra.row {
        // A reject, not an accept-and-canonicalize: one file, one legal name.
        let canonical = fsa1_ast::a1::format_cell(la.col, la.row);
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

/// `offset`/`addr_len` locate the address WITHIN the whole filename, so each refusal's `fix`
/// overwrites exactly that token rather than the name.
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
            replacement: fsa1_ast::a1::format_cell(a.col, a.row),
        })
    };
    // A file's own address is intrinsically absolute: reject the marker rather than strip it.
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

/// Spans the WHOLE filename, unlike [`enforce_canonical`]'s per-address fix.
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
    // From the error byte to the end of the offending token, and never empty.
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

    /// Every assertion elsewhere reads names back canonically, which would still pass if the writer
    /// stopped choosing per host. This is the one that says which separator THIS host must write.
    #[test]
    fn the_writer_spells_a_range_the_way_this_filesystem_allows() {
        #[cfg(windows)]
        assert_eq!(RANGE_SEP, RANGE_SEP_WINDOWS);
        #[cfg(not(windows))]
        assert_eq!(RANGE_SEP, RANGE_SEP_POSIX);
    }

    #[test]
    fn reader_accepts_both_separators_on_every_platform() {
        assert_eq!(ok("A1-D1").declared_shape, Shape { rows: 1, cols: 4 });
        assert_eq!(ok("A1:D1").region, ok("A1-D1").region);
        assert_eq!(ok("B2-D9").declared_shape, Shape { rows: 8, cols: 3 });
        assert_eq!(err_code("A1-"), Code::MalformedFilename);
        assert_eq!(err_code("A1-B2-C3"), Code::MalformedFilename);
    }

    #[test]
    fn reseparate_converts_only_ranges() {
        assert_eq!(
            reseparate_range_name("A1:D1", '-').as_deref(),
            Some("A1-D1")
        );
        assert_eq!(
            reseparate_range_name("A1-D1", ':').as_deref(),
            Some("A1:D1")
        );
        assert_eq!(
            reseparate_range_name("B2:D9", '-').as_deref(),
            Some("B2-D9")
        );
        assert_eq!(reseparate_range_name("A1:D1", ':'), None); // already the target spelling
        assert_eq!(reseparate_range_name("A1-D1", '-'), None); // already the target spelling
        assert_eq!(reseparate_range_name("A1", '-'), None); // a single cell has no separator
        assert_eq!(reseparate_range_name("Tax_Rate", '-'), None); // a defined name, not a range
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
        assert_eq!(err_code("A1.cell"), Code::MalformedFilename);
        assert_eq!(err_code("A1.txt"), Code::MalformedFilename);
        assert_eq!(err_code(""), Code::MalformedFilename);
        assert_eq!(err_code("1"), Code::MalformedFilename);
        assert_eq!(err_code("A"), Code::MalformedFilename);
        assert_eq!(err_code("A1:"), Code::MalformedFilename);
        assert_eq!(err_code("A1:B2:C3"), Code::MalformedFilename);
    }

    #[test]
    fn hostile_names_never_panic() {
        for name in ["", ":", "::", &"A".repeat(100), "λ1", "A1A1", "A1:B2:"] {
            let _ = parse_filename(name);
        }
    }
}
