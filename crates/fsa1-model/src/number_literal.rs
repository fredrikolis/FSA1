// Concern: spells a number as a LITERAL that re-lexes to itself under a format | Non-concern: a viewport's display string (render.rs), the format catalog (format.rs) | IO: (f64, Format) -> a literal
//! The inverse leg of [`crate::format::lex_formatted_number`], kept beside it so the two cannot
//! drift. Both legs of an xlsx round-trip reach it: the reader deciding whether a value literal can
//! carry its format, and the serializer spelling that literal.

use crate::format::{Format, lex_formatted_number};

/// The exact inverse of [`lex_formatted_number`]. `Accounting` and the date family have no
/// number-literal spelling here.
pub fn display_number_literal(value: f64, format: Format) -> Option<String> {
    let (sign, mag) = if value.is_sign_negative() && value != 0.0 {
        ("-", -value)
    } else {
        ("", value)
    };
    match format {
        Format::Fixed { decimals } => Some(format!("{sign}{}", fixed(mag, decimals))),
        Format::Grouped { decimals } => Some(format!("{sign}{}", grouped(mag, decimals))),
        Format::Percent { decimals } => Some(format!("{sign}{}%", fixed(mag * 100.0, decimals))),
        Format::Currency {
            symbol,
            grouping,
            decimals,
        } => {
            let body = if grouping {
                grouped(mag, decimals)
            } else {
                fixed(mag, decimals)
            };
            Some(format!("{sign}{}{body}", symbol.glyph()))
        }
        Format::Accounting { .. } | Format::Date(_) | Format::Time(_) | Format::DateTime(_) => None,
    }
}

fn fixed(mag: f64, decimals: u8) -> String {
    format!("{:.*}", decimals as usize, mag)
}

fn grouped(mag: f64, decimals: u8) -> String {
    let s = fixed(mag, decimals);
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (s.as_str(), None),
    };
    let int = group_thousands(int);
    match frac {
        Some(f) => format!("{int}.{f}"),
        None => int,
    }
}

fn group_thousands(int: &str) -> String {
    let len = int.len();
    let mut out = String::with_capacity(len + len / 3);
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// A negative `Accounting` value is `None`: its parenthesized display cannot be recovered from a
/// displayed-form literal. The reader and the strict pre-flight share this ONE remap so they agree on
/// which value literals are carriable.
pub fn effective_literal_format(format: Format, value: f64) -> Option<Format> {
    match format {
        Format::Accounting { symbol, decimals } => (value >= 0.0).then_some(Format::Currency {
            symbol,
            grouping: true,
            decimals,
        }),
        other => Some(other),
    }
}

/// Whether spelling the value to `format`'s displayed precision and re-lexing recovers the identical
/// `(value, format)`. The caller passes the EFFECTIVE format, never a raw `Accounting`.
pub fn is_display_exact(value: f64, format: Format) -> bool {
    match format {
        Format::Date(_) | Format::Time(_) | Format::DateTime(_) => true,
        Format::Fixed { decimals: 0 } => value.fract() == 0.0,
        _ => match display_number_literal(value, format) {
            Some(s) => lex_formatted_number(&s) == Some((value, format)),
            None => false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::CurrencySymbol;

    #[test]
    fn display_exact_and_effective_format_pin_the_literal_boundary() {
        assert!(is_display_exact(12.5, Format::Fixed { decimals: 2 }));
        assert!(!is_display_exact(1234.5678, Format::Fixed { decimals: 2 }));
        assert!(is_display_exact(0.125, Format::Percent { decimals: 2 }));
        assert!(!is_display_exact(0.12345, Format::Percent { decimals: 2 }));
        assert!(is_display_exact(5.0, Format::Fixed { decimals: 0 }));
        assert!(!is_display_exact(5.5, Format::Fixed { decimals: 0 }));

        let acct = Format::Accounting {
            symbol: CurrencySymbol::Dollar,
            decimals: 2,
        };
        assert_eq!(
            effective_literal_format(acct, 1234.0),
            Some(Format::Currency {
                symbol: CurrencySymbol::Dollar,
                grouping: true,
                decimals: 2,
            })
        );
        assert_eq!(
            effective_literal_format(acct, -1234.0),
            None,
            "a negative accounting value has no displayed-form spelling"
        );
    }
}
