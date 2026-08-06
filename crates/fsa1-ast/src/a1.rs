// Concern: parses and renders one A1 address | Non-concern: canonical-form policy, range syntax, the formula grammar | IO: (&str) -> A1Address; (col, row) -> String
//! `parse_a1` is deliberately lenient: it REPORTS canonical-form deviations and judges none, so
//! each consumer can layer its own policy (the filename layer rejects, the formula layer normalizes).

/// Zero-based `col`/`row`; the `*_abs` flags record `$`-anchoring, the `*_had_*` flags record
/// canonical-form deviations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct A1Address {
    pub col: u32,
    pub row: u32,
    pub col_abs: bool,
    pub row_abs: bool,
    pub col_had_lowercase: bool,
    pub row_had_leading_zero: bool,
}

/// A located refusal; `at` is a byte offset into the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum A1Error {
    Empty,
    MissingColumn { at: usize },
    MissingRow { at: usize },
    UnexpectedChar { at: usize },
    ColumnOverflow,
    RowOverflow,
}

/// Grammar `[$] LETTER+ [$] DIGIT+`, ASCII only; hostile input is a located [`A1Error`], never a panic.
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
    // At least one letter was consumed, so `col_acc >= 1` and this cannot underflow.
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

    // Row `0` has no 1-indexed form; the caller rejects it via `row_had_leading_zero`.
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

/// Bijective base-26: `0` -> `"A"`, `26` -> `"AA"`.
pub fn format_column(col: u32) -> String {
    let mut n = u64::from(col) + 1;
    let mut out = String::new();
    while n > 0 {
        let rem = ((n - 1) % 26) as u8;
        out.insert(0, (b'A' + rem) as char);
        n = (n - 1) / 26;
    }
    out
}

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
        assert_eq!(parse_a1(&"A".repeat(64)), Err(A1Error::ColumnOverflow));
        assert!(parse_a1("Aλ1").is_err(), "non-ASCII must refuse, not panic");
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
        for s in ["A1", "D7", "AA10", "G8", "ZZ100"] {
            let a = parsed(s);
            assert_eq!(format_cell(a.col, a.row), s);
        }
    }
}
