// Concern: the shared A1 ADDRESS grammar — parse an A1 address string (`$`-anchor flags, bijective base-26 column, 1-indexed row) into its raw components, and render a zero-based `(col,row)` back to A1 text; the single home for A1 syntax, reused by the filename parser (charlie-model) now and the formula parser later | Non-concern: the FILENAME grammar and its canonical-form policy (charlie-model owns the bare closed-range filename, `:`-ranges, and the rejection of lowercase/leading-zero/`$`) and resolving an address to a value | IO: (an A1 address `&str`) -> `A1Address`, and `(col,row)` -> `String`
//! Shared A1 address grammar: [`parse_a1`], [`format_cell`], [`format_column`].
//!
//! A1 is the syntax two later consumers share: the *filename* parser in `charlie-model` (a file
//! named `A1` / `A3:G8` is a closed range of A1 addresses) and the *formula* parser (`=B2*C2`). Putting
//! the grammar here keeps it single-sourced; each consumer layers its own **policy** on the raw
//! parse — the filename layer enforces canonical form (no `$`, uppercase, no leading zero), the
//! formula layer will normalize instead. So [`parse_a1`] is deliberately lenient and *reports* the
//! deviations (`col_had_lowercase`, `row_had_leading_zero`, the `$` flags) rather than judging them.

/// The raw components of a parsed A1 address, zero-based, before any layer's policy is applied.
///
/// The `*_abs` flags record `$`-anchoring; the `*_had_*` flags record canonical-form deviations a
/// downstream layer may reject (filename) or normalize (formula) — this type judges neither.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct A1Address {
    /// Zero-based column index (`A` -> 0, `Z` -> 25, `AA` -> 26).
    pub col: u32,
    /// Zero-based row index (`1` -> 0).
    pub row: u32,
    /// A `$` preceded the column letters (`$A1`, `$A$1`).
    pub col_abs: bool,
    /// A `$` preceded the row digits (`A$1`, `$A$1`).
    pub row_abs: bool,
    /// The column contained at least one lowercase letter (`a1`) — a canonical-form deviation.
    pub col_had_lowercase: bool,
    /// The row began with `0` (`A01`, `A0`) — a canonical-form deviation.
    pub row_had_leading_zero: bool,
}

/// A located reason an A1 address string is not well-formed. `at` is a byte offset into the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum A1Error {
    /// The input was empty.
    Empty,
    /// No column letters where a column was required.
    MissingColumn { at: usize },
    /// No row digits where a row was required.
    MissingRow { at: usize },
    /// Trailing bytes after a complete address.
    UnexpectedChar { at: usize },
    /// The column index overflowed `u32` (an absurdly long column).
    ColumnOverflow,
    /// The row index overflowed `u32`.
    RowOverflow,
}

/// Parse one A1 address. Never panics on hostile input — returns a located [`A1Error`].
///
/// Grammar: `[$] LETTER+ [$] DIGIT+`, ASCII only. Leniency is deliberate (see the module note):
/// lowercase letters and a leading-zero row parse successfully but are flagged for the caller.
pub fn parse_a1(s: &str) -> Result<A1Address, A1Error> {
    let b = s.as_bytes();
    if b.is_empty() {
        return Err(A1Error::Empty);
    }
    let mut i = 0usize;

    let col_abs = b[i] == b'$';
    if col_abs {
        i += 1;
    }

    let col_start = i;
    let mut col_acc: u32 = 0;
    let mut col_had_lowercase = false;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        let ch = b[i];
        if ch.is_ascii_lowercase() {
            col_had_lowercase = true;
        }
        let digit = u32::from(ch.to_ascii_uppercase() - b'A' + 1);
        col_acc = match col_acc.checked_mul(26).and_then(|v| v.checked_add(digit)) {
            Some(v) => v,
            None => return Err(A1Error::ColumnOverflow),
        };
        i += 1;
    }
    if i == col_start {
        return Err(A1Error::MissingColumn { at: col_start });
    }
    // `col_acc >= 1` here (at least one letter consumed), so the subtraction cannot underflow.
    let col = col_acc - 1;

    let row_abs = i < b.len() && b[i] == b'$';
    if row_abs {
        i += 1;
    }

    let row_start = i;
    let mut row_acc: u32 = 0;
    let mut row_had_leading_zero = false;
    let mut first_digit = true;
    while i < b.len() && b[i].is_ascii_digit() {
        if first_digit && b[i] == b'0' {
            row_had_leading_zero = true;
        }
        first_digit = false;
        let digit = u32::from(b[i] - b'0');
        row_acc = match row_acc.checked_mul(10).and_then(|v| v.checked_add(digit)) {
            Some(v) => v,
            None => return Err(A1Error::RowOverflow),
        };
        i += 1;
    }
    if i == row_start {
        return Err(A1Error::MissingRow { at: row_start });
    }
    if i != b.len() {
        return Err(A1Error::UnexpectedChar { at: i });
    }

    // A leading-zero row can be `0`, which has no valid 1-indexed form; `saturating_sub` avoids the
    // underflow and the caller rejects on `row_had_leading_zero` anyway.
    let row = row_acc.saturating_sub(1);
    Ok(A1Address {
        col,
        row,
        col_abs,
        row_abs,
        col_had_lowercase,
        row_had_leading_zero,
    })
}

/// Render a zero-based column index to canonical uppercase letters (`0` -> `"A"`, `26` -> `"AA"`).
pub fn format_column(col: u32) -> String {
    // Work in `u64` so `col + 1` cannot overflow at `u32::MAX`.
    let mut n = u64::from(col) + 1;
    let mut out = String::new();
    while n > 0 {
        let rem = ((n - 1) % 26) as u8;
        out.insert(0, (b'A' + rem) as char);
        n = (n - 1) / 26;
    }
    out
}

/// Render a zero-based `(col,row)` to a canonical A1 address (`(0,0)` -> `"A1"`).
pub fn format_cell(col: u32, row: u32) -> String {
    let mut out = format_column(col);
    out.push_str(&(u64::from(row) + 1).to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(s: &str) -> A1Address {
        parse_a1(s).expect("should parse")
    }

    #[test]
    fn plain_addresses() {
        assert_eq!(parsed("A1").col, 0);
        assert_eq!(parsed("A1").row, 0);
        assert_eq!(parsed("D7").col, 3);
        assert_eq!(parsed("D7").row, 6);
        // Bijective base-26: AA is column 27 -> index 26.
        assert_eq!(parsed("AA10").col, 26);
        assert_eq!(parsed("AA10").row, 9);
        assert_eq!(parsed("Z1").col, 25);
    }

    #[test]
    fn absolute_flags() {
        let a = parsed("$A$1");
        assert!(a.col_abs && a.row_abs);
        let b = parsed("A$1");
        assert!(!b.col_abs && b.row_abs);
        let c = parsed("$A1");
        assert!(c.col_abs && !c.row_abs);
    }

    #[test]
    fn deviation_flags_are_reported_not_judged() {
        assert!(parsed("a1").col_had_lowercase);
        assert!(!parsed("A1").col_had_lowercase);
        assert!(parsed("A01").row_had_leading_zero);
        assert!(!parsed("A10").row_had_leading_zero);
    }

    #[test]
    fn malformed_inputs_are_located_never_panic() {
        assert_eq!(parse_a1(""), Err(A1Error::Empty));
        assert_eq!(parse_a1("1"), Err(A1Error::MissingColumn { at: 0 }));
        assert_eq!(parse_a1("A"), Err(A1Error::MissingRow { at: 1 }));
        assert_eq!(parse_a1("A1B"), Err(A1Error::UnexpectedChar { at: 2 }));
        assert_eq!(parse_a1("$$A1"), Err(A1Error::MissingColumn { at: 1 }));
        // A hostile, absurdly long column must overflow-guard, never panic.
        assert_eq!(parse_a1(&"A".repeat(64)), Err(A1Error::ColumnOverflow));
        // Non-ASCII bytes are simply unexpected, never a panic.
        assert!(parse_a1("Aλ1").is_err());
    }

    #[test]
    fn round_trip_index_to_text() {
        assert_eq!(format_column(0), "A");
        assert_eq!(format_column(25), "Z");
        assert_eq!(format_column(26), "AA");
        assert_eq!(format_column(701), "ZZ");
        assert_eq!(format_column(702), "AAA");
        assert_eq!(format_cell(0, 0), "A1");
        assert_eq!(format_cell(6, 7), "G8");
        // parse . format is the identity on a canonical address.
        for s in ["A1", "D7", "AA10", "G8", "ZZ100"] {
            let a = parsed(s);
            assert_eq!(format_cell(a.col, a.row), s);
        }
    }
}
