// Concern: the closed catalog of display formats, recovered from a code or a displayed literal | Non-concern: rendering a value under one, evaluation | IO: (&str) -> Option<Format>; (Format) -> code
//! Membership is deliberately CLOSED: [`Format::from_code`] returns `None` for anything outside the
//! catalog, and upstream a `None` is a located refusal. An under-populated catalog can therefore
//! only *refuse* a real code, never round-trip one wrongly.

/// Every payload is itself a closed enum, so a `Format` is always a catalog member — there is no
/// raw-string escape hatch. [`Format::code`] is the single source of each variant's spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Format {
    Fixed {
        decimals: u8,
    },
    Grouped {
        decimals: u8,
    },
    /// The value is a ratio; the display scales it by 100.
    Percent {
        decimals: u8,
    },
    Currency {
        symbol: CurrencySymbol,
        grouping: bool,
        decimals: u8,
    },
    /// Reachable only through an explicit `~<code>` marker on a FORMULA cell: recovering a negative
    /// value from a parenthesized display is not a value-literal read.
    Accounting {
        symbol: CurrencySymbol,
        decimals: u8,
    },
    Date(DatePattern),
    Time(TimePattern),
    DateTime(DateTimePattern),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CurrencySymbol {
    Dollar,
    Euro,
    Pound,
    Yen,
    Rupee,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DatePattern {
    Iso,
    Mdy,
    MmDdYy,
    DMmmYy,
    MmmYy,
    DMonthYyyy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TimePattern {
    Hms,
    Hm,
    Hm12,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DateTimePattern {
    MdyHm,
    IsoDateTime,
}

/// The sentinel for a format with no ECMA-376 built-in id. A pure `Format` cannot know the
/// per-workbook assignment, so `numfmt_id(f) >= CUSTOM_NUMFMT_ID` reports only *that* a custom
/// `<numFmts>` entry is needed; the export style builder assigns the concrete distinct ids.
pub const CUSTOM_NUMFMT_ID: u32 = 164;

impl CurrencySymbol {
    /// Also the canonical quote-free export spelling, which renders identically in a numFmt code.
    pub fn glyph(self) -> &'static str {
        match self {
            CurrencySymbol::Dollar => "$",
            CurrencySymbol::Euro => "€",
            CurrencySymbol::Pound => "£",
            CurrencySymbol::Yen => "¥",
            CurrencySymbol::Rupee => "Rs.",
        }
    }

    /// Longest glyphs first, so `Rs.` is not mistaken for text.
    fn strip_leading(s: &str) -> Option<(CurrencySymbol, &str)> {
        for sym in [
            CurrencySymbol::Rupee,
            CurrencySymbol::Dollar,
            CurrencySymbol::Euro,
            CurrencySymbol::Pound,
            CurrencySymbol::Yen,
        ] {
            if let Some(rest) = s.strip_prefix(sym.glyph()) {
                return Some((sym, rest));
            }
        }
        None
    }
}

impl DatePattern {
    fn code(self) -> &'static str {
        match self {
            DatePattern::Iso => "yyyy-mm-dd",
            DatePattern::Mdy => "m/d/yyyy",
            DatePattern::MmDdYy => "mm-dd-yy",
            DatePattern::DMmmYy => "d-mmm-yy",
            DatePattern::MmmYy => "mmm-yy",
            DatePattern::DMonthYyyy => "dd-mmm-yyyy",
        }
    }
    fn numfmt_id(self) -> u32 {
        match self {
            DatePattern::MmDdYy => 14,
            DatePattern::DMmmYy => 15,
            DatePattern::MmmYy => 17,
            DatePattern::Iso | DatePattern::Mdy | DatePattern::DMonthYyyy => CUSTOM_NUMFMT_ID,
        }
    }
}

impl TimePattern {
    fn code(self) -> &'static str {
        match self {
            TimePattern::Hms => "h:mm:ss",
            TimePattern::Hm => "h:mm",
            TimePattern::Hm12 => "h:mm AM/PM",
        }
    }
    fn numfmt_id(self) -> u32 {
        match self {
            TimePattern::Hms => 21,
            TimePattern::Hm => 20,
            TimePattern::Hm12 => 18,
        }
    }
}

impl DateTimePattern {
    fn code(self) -> &'static str {
        match self {
            DateTimePattern::MdyHm => "m/d/yy h:mm",
            DateTimePattern::IsoDateTime => "yyyy-mm-dd hh:mm:ss",
        }
    }
    fn numfmt_id(self) -> u32 {
        match self {
            DateTimePattern::MdyHm => 22,
            DateTimePattern::IsoDateTime => CUSTOM_NUMFMT_ID,
        }
    }
}

fn zeros(n: u8) -> String {
    "0".repeat(n as usize)
}

fn number_code_body(grouping: bool, decimals: u8) -> String {
    let mut s = String::from(if grouping { "#,##0" } else { "0" });
    if decimals > 0 {
        s.push('.');
        s.push_str(&zeros(decimals));
    }
    s
}

impl Format {
    /// The ONE spelling FSA1 writes as an on-disk `~code` marker and emits as an exported numFmt.
    /// It carries no `"`, `'`, `!`, `~`, colour, or condition bracket, which is what makes the
    /// marker split unambiguous.
    pub fn code(&self) -> String {
        match self {
            Format::Fixed { decimals } => {
                if *decimals == 0 {
                    "0".to_string()
                } else {
                    format!("0.{}", zeros(*decimals))
                }
            }
            Format::Grouped { decimals } => number_code_body(true, *decimals),
            Format::Percent { decimals } => {
                if *decimals == 0 {
                    "0%".to_string()
                } else {
                    format!("0.{}%", zeros(*decimals))
                }
            }
            Format::Currency {
                symbol,
                grouping,
                decimals,
            } => format!(
                "{}{}",
                symbol.glyph(),
                number_code_body(*grouping, *decimals)
            ),
            Format::Accounting { symbol, decimals } => {
                let body = format!("{}{}", symbol.glyph(), number_code_body(true, *decimals));
                format!("{body};({body})")
            }
            Format::Date(p) => p.code().to_string(),
            Format::Time(p) => p.code().to_string(),
            Format::DateTime(p) => p.code().to_string(),
        }
    }

    /// A built-in ECMA-376 id, else [`CUSTOM_NUMFMT_ID`].
    pub fn numfmt_id(&self) -> u32 {
        match self {
            Format::Fixed { decimals: 0 } => 1,
            Format::Fixed { decimals: 2 } => 2,
            Format::Grouped { decimals: 0 } => 3,
            Format::Grouped { decimals: 2 } => 4,
            Format::Percent { decimals: 0 } => 9,
            Format::Percent { decimals: 2 } => 10,
            Format::Fixed { .. }
            | Format::Grouped { .. }
            | Format::Percent { .. }
            | Format::Currency { .. }
            | Format::Accounting { .. } => CUSTOM_NUMFMT_ID,
            Format::Date(p) => p.numfmt_id(),
            Format::Time(p) => p.numfmt_id(),
            Format::DateTime(p) => p.numfmt_id(),
        }
    }

    /// The ONE classifier, shared by the import numFmt reader and the on-disk `~<code>` marker split
    /// ([`crate::grid::split_format_marker`]). A code carrying `"`, `'`, `!`, `~`, or a `[…]` bracket
    /// is refused outright, so the marker grammar stays exactly the bracket/quote/tilde-free set.
    pub fn from_code(code: &str) -> Option<Format> {
        if code.is_empty() || code.contains(['"', '\'', '!', '~', '[', ']']) {
            return None;
        }
        date_time_from_code(code).or_else(|| number_from_code(code))
    }
}

fn date_time_from_code(code: &str) -> Option<Format> {
    Some(match code {
        "yyyy-mm-dd" => Format::Date(DatePattern::Iso),
        "m/d/yyyy" => Format::Date(DatePattern::Mdy),
        "mm-dd-yy" => Format::Date(DatePattern::MmDdYy),
        "d-mmm-yy" => Format::Date(DatePattern::DMmmYy),
        "mmm-yy" => Format::Date(DatePattern::MmmYy),
        "dd-mmm-yyyy" => Format::Date(DatePattern::DMonthYyyy),
        "h:mm:ss" => Format::Time(TimePattern::Hms),
        "h:mm" => Format::Time(TimePattern::Hm),
        "h:mm AM/PM" => Format::Time(TimePattern::Hm12),
        "m/d/yy h:mm" => Format::DateTime(DateTimePattern::MdyHm),
        "yyyy-mm-dd hh:mm:ss" => Format::DateTime(DateTimePattern::IsoDateTime),
        _ => return None,
    })
}

/// Percent, then accounting, currency, grouped, fixed — the precedence is load-bearing.
fn number_from_code(code: &str) -> Option<Format> {
    if let Some(base) = code.strip_suffix('%') {
        return Some(Format::Percent {
            decimals: fixed_decimals(base)?,
        });
    }
    if code.matches(';').count() == 1 {
        let (pos, neg) = code.split_once(';')?;
        let (symbol, grouping, decimals) = currency_parts(pos)?;
        return (grouping && neg == format!("({pos})"))
            .then_some(Format::Accounting { symbol, decimals });
    }
    if let Some((symbol, grouping, decimals)) = currency_parts(code) {
        return Some(Format::Currency {
            symbol,
            grouping,
            decimals,
        });
    }
    if let Some(rest) = code.strip_prefix("#,##0") {
        let decimals = if rest.is_empty() {
            0
        } else {
            fixed_decimals(&format!("0{rest}"))?
        };
        return Some(Format::Grouped { decimals });
    }
    Some(Format::Fixed {
        decimals: fixed_decimals(code)?,
    })
}

fn number_body(rest: &str) -> Option<(bool, u8)> {
    if let Some(tail) = rest.strip_prefix("#,##0") {
        let d = if tail.is_empty() {
            0
        } else {
            fixed_decimals(&format!("0{tail}"))?
        };
        Some((true, d))
    } else {
        Some((false, fixed_decimals(rest)?))
    }
}

fn currency_parts(section: &str) -> Option<(CurrencySymbol, bool, u8)> {
    let (symbol, rest) = CurrencySymbol::strip_leading(section)?;
    let (grouping, decimals) = number_body(rest)?;
    Some((symbol, grouping, decimals))
}

/// The integer part must be exactly `0`, so a displayed VALUE like `12.50` is never read as a code.
fn fixed_decimals(base: &str) -> Option<u8> {
    match base.split_once('.') {
        None => (base == "0").then_some(0),
        Some((int, frac)) => {
            if int != "0" || frac.is_empty() || !frac.bytes().all(|b| b == b'0') {
                return None;
            }
            u8::try_from(frac.len()).ok()
        }
    }
}

/// Claims a field ONLY where it carries a format glyph the General lexer would not — a leading
/// currency symbol, a `,` group, a trailing `%`, or a trailing-zero decimal — so a bare `12`,
/// `12.5`, or `12.53` stays a General number. The date/time family is the grid deserializer's, and
/// a currency literal is always [`Format::Currency`], never [`Format::Accounting`].
pub fn lex_formatted_number(token: &str) -> Option<(f64, Format)> {
    let (neg, body) = match token.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, token),
    };
    let signed = |v: f64| if neg { -v } else { v };

    if let Some(base) = body.strip_suffix('%') {
        let (val, decimals) = parse_displayed_decimal(base, false)?;
        return Some((signed(val) / 100.0, Format::Percent { decimals }));
    }
    // One space after a multi-char symbol like `Rs.` is allowed.
    if let Some((symbol, rest)) = CurrencySymbol::strip_leading(body) {
        let rest = rest.strip_prefix(' ').unwrap_or(rest);
        let grouping = rest.contains(',');
        let (val, decimals) = parse_displayed_decimal(rest, true)?;
        return Some((
            signed(val),
            Format::Currency {
                symbol,
                grouping,
                decimals,
            },
        ));
    }
    if body.contains(',') {
        let (val, decimals) = parse_displayed_decimal(body, true)?;
        return Some((signed(val), Format::Grouped { decimals }));
    }
    // The TRAILING zero is what claims `12.50` while `12.5` falls through to the General path.
    if let Some((int, frac)) = body.split_once('.')
        && !frac.is_empty()
        && frac.ends_with('0')
        && !int.is_empty()
        && int.bytes().all(|b| b.is_ascii_digit())
        && frac.bytes().all(|b| b.is_ascii_digit())
    {
        let val: f64 = body.parse().ok()?;
        let decimals = u8::try_from(frac.len()).ok()?;
        return Some((signed(val), Format::Fixed { decimals }));
    }
    None
}

/// `None` for a non-numeric, mis-grouped, or non-finite field.
fn parse_displayed_decimal(s: &str, allow_grouping: bool) -> Option<(f64, u8)> {
    if s.is_empty() {
        return None;
    }
    let (int_part, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (s, None),
    };
    if int_part.contains(',') {
        if !allow_grouping || !valid_thousands(int_part) {
            return None;
        }
    } else if int_part.is_empty() || !int_part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let decimals = match frac_part {
        Some(f) => {
            if f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            u8::try_from(f.len()).ok()?
        }
        None => 0,
    };
    let cleaned: String = s.chars().filter(|c| *c != ',').collect();
    let val: f64 = cleaned.parse().ok()?;
    val.is_finite().then_some((val, decimals))
}

/// A first group of 1-3 digits, then one or more groups of exactly 3: `1,234`, `12,345,678`.
fn valid_thousands(int_part: &str) -> bool {
    let groups: Vec<&str> = int_part.split(',').collect();
    if groups.len() < 2 {
        return false;
    }
    let first_ok =
        (1..=3).contains(&groups[0].len()) && groups[0].bytes().all(|b| b.is_ascii_digit());
    let rest_ok = groups[1..]
        .iter()
        .all(|g| g.len() == 3 && g.bytes().all(|b| b.is_ascii_digit()));
    first_ok && rest_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_and_from_code_round_trip_every_catalog_member() {
        let members = [
            Format::Fixed { decimals: 0 },
            Format::Fixed { decimals: 2 },
            Format::Fixed { decimals: 4 },
            Format::Grouped { decimals: 0 },
            Format::Grouped { decimals: 2 },
            Format::Percent { decimals: 0 },
            Format::Percent { decimals: 2 },
            Format::Currency {
                symbol: CurrencySymbol::Dollar,
                grouping: true,
                decimals: 2,
            },
            Format::Currency {
                symbol: CurrencySymbol::Pound,
                grouping: true,
                decimals: 0,
            },
            Format::Currency {
                symbol: CurrencySymbol::Euro,
                grouping: false,
                decimals: 2,
            },
            Format::Accounting {
                symbol: CurrencySymbol::Dollar,
                decimals: 2,
            },
            Format::Date(DatePattern::Iso),
            Format::Date(DatePattern::Mdy),
            Format::Date(DatePattern::MmDdYy),
            Format::Date(DatePattern::DMmmYy),
            Format::Date(DatePattern::MmmYy),
            Format::Date(DatePattern::DMonthYyyy),
            Format::Time(TimePattern::Hms),
            Format::Time(TimePattern::Hm),
            Format::Time(TimePattern::Hm12),
            Format::DateTime(DateTimePattern::MdyHm),
            Format::DateTime(DateTimePattern::IsoDateTime),
        ];
        for f in members {
            let code = f.code();
            assert_eq!(
                Format::from_code(&code),
                Some(f),
                "code {code:?} did not round-trip through from_code for {f:?}"
            );
        }
    }

    #[test]
    fn from_code_pins_the_plan_catalog_codes() {
        assert_eq!(
            Format::from_code("0.00"),
            Some(Format::Fixed { decimals: 2 })
        );
        assert_eq!(
            Format::from_code("#,##0.00"),
            Some(Format::Grouped { decimals: 2 })
        );
        assert_eq!(
            Format::from_code("0.00%"),
            Some(Format::Percent { decimals: 2 })
        );
        assert_eq!(
            Format::from_code("$#,##0.00"),
            Some(Format::Currency {
                symbol: CurrencySymbol::Dollar,
                grouping: true,
                decimals: 2
            })
        );
        assert_eq!(
            Format::from_code("$#,##0.00;($#,##0.00)"),
            Some(Format::Accounting {
                symbol: CurrencySymbol::Dollar,
                decimals: 2
            })
        );
        assert_eq!(
            Format::from_code("m/d/yyyy"),
            Some(Format::Date(DatePattern::Mdy))
        );
    }

    #[test]
    fn from_code_refuses_the_exotic_and_ambiguity_hostile_tail() {
        for bad in [
            "",
            "[Blue]m/d/yyyy",
            "[<100]0;[>=100]0.0",
            "000000000",
            "\"$\"#,##0.00",
            "'Sheet'!A1",
            "0.00~0.00",
            "General",
            "0.0.0",
            "yyyy/mm/dd",
        ] {
            assert_eq!(Format::from_code(bad), None, "expected {bad:?} refused");
        }
    }

    #[test]
    fn lex_formatted_number_recovers_value_and_format() {
        assert_eq!(
            lex_formatted_number("12.50"),
            Some((12.5, Format::Fixed { decimals: 2 }))
        );
        assert_eq!(
            lex_formatted_number("0.0000"),
            Some((0.0, Format::Fixed { decimals: 4 }))
        );
        assert_eq!(
            lex_formatted_number("1,234.00"),
            Some((1234.0, Format::Grouped { decimals: 2 }))
        );
        assert_eq!(
            lex_formatted_number("1,234"),
            Some((1234.0, Format::Grouped { decimals: 0 }))
        );
        assert_eq!(
            lex_formatted_number("12.50%"),
            Some((0.125, Format::Percent { decimals: 2 }))
        );
        assert_eq!(
            lex_formatted_number("$1,234.00"),
            Some((
                1234.0,
                Format::Currency {
                    symbol: CurrencySymbol::Dollar,
                    grouping: true,
                    decimals: 2
                }
            ))
        );
        assert_eq!(
            lex_formatted_number("-$1,234.00"),
            Some((
                -1234.0,
                Format::Currency {
                    symbol: CurrencySymbol::Dollar,
                    grouping: true,
                    decimals: 2
                }
            ))
        );
        assert_eq!(
            lex_formatted_number("-12.50%"),
            Some((-0.125, Format::Percent { decimals: 2 }))
        );
        assert_eq!(lex_formatted_number("12"), None);
        assert_eq!(lex_formatted_number("12.5"), None);
        assert_eq!(lex_formatted_number("12.53"), None);
        assert_eq!(lex_formatted_number("hello"), None);
        assert_eq!(lex_formatted_number("1,2,3"), None);
    }

    #[test]
    fn numfmt_id_reports_builtins_and_the_custom_sentinel() {
        assert_eq!(Format::Fixed { decimals: 2 }.numfmt_id(), 2);
        assert_eq!(Format::Grouped { decimals: 2 }.numfmt_id(), 4);
        assert_eq!(Format::Percent { decimals: 0 }.numfmt_id(), 9);
        assert_eq!(Format::Date(DatePattern::DMmmYy).numfmt_id(), 15);
        assert_eq!(Format::Time(TimePattern::Hms).numfmt_id(), 21);
        assert_eq!(Format::DateTime(DateTimePattern::MdyHm).numfmt_id(), 22);
        assert!(Format::Fixed { decimals: 4 }.numfmt_id() >= CUSTOM_NUMFMT_ID);
        assert!(
            Format::Currency {
                symbol: CurrencySymbol::Dollar,
                grouping: true,
                decimals: 2
            }
            .numfmt_id()
                >= CUSTOM_NUMFMT_ID
        );
        assert!(
            Format::Accounting {
                symbol: CurrencySymbol::Dollar,
                decimals: 2
            }
            .numfmt_id()
                >= CUSTOM_NUMFMT_ID
        );
    }
}
