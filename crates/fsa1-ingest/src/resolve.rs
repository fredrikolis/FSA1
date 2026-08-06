// Concern: resolves a Table[...] structured reference to plain A1 | Non-concern: reading the table metadata, the formula token walk | IO: (table, inner, sheet, row) -> Option<A1>

use fsa1_ast::a1::{format_cell, parse_a1};

/// `(c0,r0)..=(c1,r1)` is the table's FULL rectangle in 0-based absolute grid coordinates — header
/// rows, data body and totals rows together. Column `columns[i]` sits at absolute column `c0 + i`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TableGeom {
    name: String,
    sheet: String,
    columns: Vec<String>,
    c0: u32,
    r0: u32,
    c1: u32,
    r1: u32,
    header_rows: u32,
    totals_rows: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resolution {
    tables: Vec<TableGeom>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Region {
    Headers,
    Data,
    Totals,
    All,
    ThisRow,
}

/// `One`/`Span` carry indices into [`TableGeom::columns`], not grid columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColSel {
    All,
    One(usize),
    Span(usize, usize),
}

impl Resolution {
    pub fn empty() -> Self {
        Resolution::default()
    }

    /// A `ref_str` that is not a rectangle is DROPPED — the table is simply never resolvable.
    pub fn add_table(
        &mut self,
        name: &str,
        sheet: &str,
        columns: Vec<String>,
        ref_str: &str,
        header_rows: u32,
        totals_rows: u32,
    ) {
        if let Some((c0, r0, c1, r1)) = parse_ref(ref_str) {
            self.tables.push(TableGeom {
                name: name.to_string(),
                sheet: sheet.to_string(),
                columns,
                c0,
                r0,
                c1,
                r1,
                header_rows,
                totals_rows,
            });
        }
    }

    pub fn is_table(&self, ident: &str) -> bool {
        self.tables
            .iter()
            .any(|t| t.name.eq_ignore_ascii_case(ident))
    }

    /// `cur_row` is the FORMULA cell's own 0-based row, which the `@`/`#This Row` forms select on.
    /// `None` means unresolvable — the caller keeps the token verbatim so it loads as a `#NAME?`.
    pub fn resolve_structured(
        &self,
        table: &str,
        inner: &str,
        cur_sheet: &str,
        cur_row: u32,
    ) -> Option<String> {
        let g = self
            .tables
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(table))?;
        let (region, colsel) = g.parse_spec(inner)?;
        let (rt, rb) = g.rows_for(region, cur_row)?;
        let (cl, cr) = g.cols_for(colsel)?;
        Some(g.qualify(cur_sheet, cl, rt, cr, rb))
    }
}

impl TableGeom {
    fn col_index(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.eq_ignore_ascii_case(name))
    }

    fn parse_spec(&self, inner: &str) -> Option<(Region, ColSel)> {
        let s = inner.trim();
        let (this_row, rest) = match s.strip_prefix('@') {
            Some(r) => (true, r.trim()),
            None => (false, s),
        };
        let mut region: Option<Region> = None;
        let mut col_item: Option<&str> = None;
        for item in split_outside_brackets(rest, ',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            if let Some(kw) = region_keyword(item) {
                region = Some(parse_region(kw)?);
            } else if col_item.is_some() {
                return None;
            } else {
                col_item = Some(item);
            }
        }
        let region = if this_row {
            Region::ThisRow
        } else {
            region.unwrap_or(Region::Data)
        };
        let cols = self.parse_cols(col_item)?;
        Some((region, cols))
    }

    fn parse_cols(&self, item: Option<&str>) -> Option<ColSel> {
        let Some(s) = item else {
            return Some(ColSel::All);
        };
        if let Some((l, r)) = split_once_outside_brackets(s, ':') {
            let li = self.col_index(unbracket(l.trim()))?;
            let ri = self.col_index(unbracket(r.trim()))?;
            Some(ColSel::Span(li.min(ri), li.max(ri)))
        } else {
            Some(ColSel::One(self.col_index(unbracket(s.trim()))?))
        }
    }

    /// `header_rows`/`totals_rows` are unbounded u32 straight off the xlsx part, so every band is
    /// computed with checked arithmetic: out of range is `None`, never a panic or a wrapped row.
    fn rows_for(&self, region: Region, cur_row: u32) -> Option<(u32, u32)> {
        match region {
            Region::Headers => {
                if self.header_rows == 0 {
                    return None;
                }
                let last = self.r0.checked_add(self.header_rows)?.checked_sub(1)?;
                if last > self.r1 {
                    None
                } else {
                    Some((self.r0, last))
                }
            }
            Region::Data => {
                let first = self.r0.checked_add(self.header_rows)?;
                let last = self.r1.checked_sub(self.totals_rows)?;
                if first > last {
                    None
                } else {
                    Some((first, last))
                }
            }
            Region::Totals => {
                if self.totals_rows == 0 {
                    return None;
                }
                let top = self.r1.checked_sub(self.totals_rows)?.checked_add(1)?;
                if top < self.r0 {
                    None
                } else {
                    Some((top, self.r1))
                }
            }
            Region::All => Some((self.r0, self.r1)),
            // Excel answers #VALUE! for an `@Col` outside the data band, so it is unresolvable here.
            Region::ThisRow => {
                let first = self.r0.checked_add(self.header_rows)?;
                let last = self.r1.checked_sub(self.totals_rows)?;
                if cur_row < first || cur_row > last {
                    None
                } else {
                    Some((cur_row, cur_row))
                }
            }
        }
    }

    /// `c0` is unbounded (`parse_a1` does not cap columns), so `c0 + offset` is checked like `rows_for`.
    fn cols_for(&self, cols: ColSel) -> Option<(u32, u32)> {
        match cols {
            ColSel::All => Some((self.c0, self.c1)),
            ColSel::One(i) => {
                let c = self.c0.checked_add(u32::try_from(i).ok()?)?;
                Some((c, c))
            }
            ColSel::Span(a, b) => Some((
                self.c0.checked_add(u32::try_from(a).ok()?)?,
                self.c0.checked_add(u32::try_from(b).ok()?)?,
            )),
        }
    }

    fn qualify(&self, cur_sheet: &str, cl: u32, rt: u32, cr: u32, rb: u32) -> String {
        let a1 = if cl == cr && rt == rb {
            format_cell(cl, rt)
        } else {
            format!("{}:{}", format_cell(cl, rt), format_cell(cr, rb))
        };
        if self.sheet == cur_sheet {
            a1
        } else {
            format!("{}!{a1}", sheet_ref(&self.sheet))
        }
    }
}

/// Corner order is NORMALIZED, so every band computation may assume `c0<=c1` and `r0<=r1`.
fn parse_ref(ref_str: &str) -> Option<(u32, u32, u32, u32)> {
    let (a, b) = match ref_str.trim().split_once(':') {
        Some((a, b)) => (a, b),
        None => (ref_str.trim(), ref_str.trim()),
    };
    let pa = parse_a1(a).ok()?;
    let pb = parse_a1(b).ok()?;
    Some((
        pa.col.min(pb.col),
        pa.row.min(pb.row),
        pa.col.max(pb.col),
        pa.row.max(pb.row),
    ))
}

fn region_keyword(item: &str) -> Option<&str> {
    if let Some(rest) = item.strip_prefix('#') {
        return Some(rest);
    }
    if let Some(inner) = item.strip_prefix("[#").and_then(|r| r.strip_suffix(']')) {
        return Some(inner);
    }
    None
}

fn parse_region(kw: &str) -> Option<Region> {
    match kw.trim().to_ascii_lowercase().as_str() {
        "headers" => Some(Region::Headers),
        "data" => Some(Region::Data),
        "totals" => Some(Region::Totals),
        "all" => Some(Region::All),
        "this row" => Some(Region::ThisRow),
        _ => None,
    }
}

fn unbracket(s: &str) -> &str {
    s.strip_prefix('[')
        .and_then(|r| r.strip_suffix(']'))
        .unwrap_or(s)
}

fn split_outside_brackets(s: &str, delim: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            _ if c == delim && depth == 0 => {
                out.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

fn split_once_outside_brackets(s: &str, delim: char) -> Option<(&str, &str)> {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            _ if c == delim && depth == 0 => return Some((&s[..i], &s[i + c.len_utf8()..])),
            _ => {}
        }
    }
    None
}

fn sheet_ref(name: &str) -> String {
    let simple = !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if simple {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Sales`: A1:C4, 1 header row, data rows 2..=4, columns Region/Q1/Q2 at A/B/C.
    fn sales() -> Resolution {
        let mut r = Resolution::empty();
        r.add_table(
            "Sales",
            "Data",
            vec!["Region".into(), "Q1".into(), "Q2".into()],
            "A1:C4",
            1,
            0,
        );
        r
    }

    #[test]
    fn structured_column_is_the_data_body() {
        let r = sales();
        assert_eq!(
            r.resolve_structured("Sales", "Q1", "Data", 9).as_deref(),
            Some("B2:B4")
        );
        assert_eq!(
            r.resolve_structured("Sales", "[Q2]", "Data", 9).as_deref(),
            Some("C2:C4")
        );
        assert_eq!(
            r.resolve_structured("sales", "q1", "Data", 9).as_deref(),
            Some("B2:B4"),
            "table and column names are case-insensitive"
        );
    }

    #[test]
    fn structured_headers_and_this_row_and_span() {
        let r = sales();
        assert_eq!(
            r.resolve_structured("Sales", "[#Headers],[Q1]", "Data", 9)
                .as_deref(),
            Some("B1")
        );
        assert_eq!(
            r.resolve_structured("Sales", "@Q1", "Data", 2).as_deref(),
            Some("B3"),
            "@Col selects the formula's own 0-based row"
        );
        assert_eq!(
            r.resolve_structured("Sales", "[#This Row],[Q1]", "Data", 1)
                .as_deref(),
            Some("B2")
        );
        assert_eq!(
            r.resolve_structured("Sales", "[Q1]:[Q2]", "Data", 9)
                .as_deref(),
            Some("B2:C4")
        );
    }

    #[test]
    fn structured_all_totals_and_cross_sheet() {
        let mut r = Resolution::empty();
        // A1:C5, header row 1, data 2..=4, totals row 5.
        r.add_table(
            "T",
            "Data",
            vec!["Region".into(), "Q1".into(), "Q2".into()],
            "A1:C5",
            1,
            1,
        );
        assert_eq!(
            r.resolve_structured("T", "Q1", "Data", 9).as_deref(),
            Some("B2:B4"),
            "a bare column excludes both header and totals"
        );
        assert_eq!(
            r.resolve_structured("T", "[#Totals],[Q1]", "Data", 9)
                .as_deref(),
            Some("B5")
        );
        assert_eq!(
            r.resolve_structured("T", "[#All],[Q1]", "Data", 9)
                .as_deref(),
            Some("B1:B5")
        );
        assert_eq!(
            r.resolve_structured("T", "#All", "Data", 9).as_deref(),
            Some("A1:C5"),
            "a region with no column item spans the whole rectangle"
        );
        assert_eq!(
            r.resolve_structured("T", "Q1", "Other", 9).as_deref(),
            Some("Data!B2:B4"),
            "referenced from another sheet, the table's sheet qualifies it"
        );
    }

    #[test]
    fn a_totals_ref_on_a_table_without_totals_is_unresolvable_not_wrong() {
        let r = sales();
        assert_eq!(
            r.resolve_structured("Sales", "[#Totals],[Q1]", "Data", 9),
            None
        );
        assert_eq!(
            r.resolve_structured("Sales", "Nope", "Data", 9),
            None,
            "an unknown column is likewise unresolvable"
        );
    }

    #[test]
    fn a_this_row_ref_outside_the_data_band_is_unresolvable_not_wrong() {
        // Sales is A1:C4: header row 0, data rows 1..=3, all 0-based.
        let r = sales();
        assert_eq!(
            r.resolve_structured("Sales", "@Q1", "Data", 1).as_deref(),
            Some("B2")
        );
        assert_eq!(
            r.resolve_structured("Sales", "@Q1", "Data", 3).as_deref(),
            Some("B4")
        );
        assert_eq!(
            r.resolve_structured("Sales", "@Q1", "Data", 0),
            None,
            "the header row is above the data band, never the header cell"
        );
        assert_eq!(
            r.resolve_structured("Sales", "@Q1", "Data", 49),
            None,
            "a row far below the table never becomes a stray B-column cell"
        );
        assert_eq!(
            r.resolve_structured("Sales", "[#This Row],[Q1]", "Data", 0),
            None
        );

        // T is A1:C5: data rows 1..=3, totals row 4, all 0-based.
        let mut t = Resolution::empty();
        t.add_table(
            "T",
            "Data",
            vec!["Region".into(), "Q1".into(), "Q2".into()],
            "A1:C5",
            1,
            1,
        );
        assert_eq!(
            t.resolve_structured("T", "@Q1", "Data", 3).as_deref(),
            Some("B4")
        );
        assert_eq!(
            t.resolve_structured("T", "@Q1", "Data", 4),
            None,
            "the totals row is below the data band"
        );
    }

    #[test]
    fn out_of_range_header_and_totals_counts_are_unresolvable_not_a_panic() {
        let mut r = Resolution::empty();
        r.add_table(
            "Bad",
            "Data",
            vec!["Region".into(), "Q1".into(), "Q2".into()],
            "A1:C4",
            9,
            5,
        );
        assert_eq!(
            r.resolve_structured("Bad", "[#Totals],[Q1]", "Data", 9),
            None,
            "`r1 - totals_rows` underflows: neither a panic nor a wrapped row"
        );
        assert_eq!(
            r.resolve_structured("Bad", "[#Headers],[Q1]", "Data", 9),
            None,
            "the header band overshoots r1: never a range past the table"
        );
        assert_eq!(r.resolve_structured("Bad", "Q1", "Data", 9), None);
    }

    #[test]
    fn a_degenerate_this_row_geometry_yields_none_not_a_panic() {
        let mut r = Resolution::empty();
        // r0 = 1 (from A2) plus a header count that overflows u32 in `r0 + header_rows`.
        r.add_table(
            "Bad",
            "Data",
            vec!["Region".into(), "Q1".into(), "Q2".into()],
            "A2:C5",
            u32::MAX,
            0,
        );
        assert_eq!(r.resolve_structured("Bad", "@Q1", "Data", 3), None);
        assert_eq!(
            r.resolve_structured("Bad", "[#This Row],[Q1]", "Data", 3),
            None
        );
    }

    #[test]
    fn a_near_max_column_origin_yields_none_not_a_panic() {
        // Built directly so `c0` sits at the u32 edge, which no parseable table `ref` reaches.
        let mut r = Resolution::empty();
        r.tables.push(TableGeom {
            name: "Max".into(),
            sheet: "Data".into(),
            columns: vec!["Region".into(), "Q1".into(), "Q2".into()],
            c0: u32::MAX,
            r0: 0,
            c1: u32::MAX,
            r1: 3,
            header_rows: 1,
            totals_rows: 0,
        });
        assert_eq!(
            r.resolve_structured("Max", "Q1", "Data", 9),
            None,
            "`c0 + offset` overflows: neither a panic nor a wrapped column"
        );
        assert_eq!(
            r.resolve_structured("Max", "[Region]:[Q2]", "Data", 9),
            None
        );
        assert_eq!(
            r.resolve_structured("Max", "Region", "Data", 9).as_deref(),
            Some(format!("{}:{}", format_cell(u32::MAX, 1), format_cell(u32::MAX, 3)).as_str()),
            "the offset-0 column still resolves: the guard rejects only out-of-range offsets"
        );
    }

    #[test]
    fn a_reversed_table_ref_is_normalized_so_band_math_holds() {
        let mut r = Resolution::empty();
        r.add_table(
            "Rev",
            "Data",
            vec!["Region".into(), "Q1".into(), "Q2".into()],
            "C4:A1",
            1,
            0,
        );
        assert_eq!(
            r.resolve_structured("Rev", "Q1", "Data", 9).as_deref(),
            Some("B2:B4")
        );
    }
}
