// Concern: the FORMAT-NEUTRAL REFERENCE RESOLVER (CORE3 import-time materialization) — the workbook's defined-name map and table geometry, plus the pure logic that turns a NAME token or a STRUCTURED/TABLE reference into a plain Excel-A1 string during formula translation, so the engine only ever sees A1 (HARD RULE 4 firewall). A [`Resolution`] holds only neutral data (names as `A1`/`Sheet!A1` target strings; each table's sheet, header names, full ref rectangle and header/totals extents) — no calamine/zip/xml types — built by the reader and consumed by `translate`. Name resolution respects scope (a sheet-local name shadows a workbook name on its sheet, Excel case-insensitive) and admits ONLY names whose target is a real cell/range ref: a constant (`{…}`), a formula-name (`MATCH(…)`), a union (`A1,B1`) or an external ref is DROPPED so its token stays verbatim and loads as a located GRID6 `#NAME?` (HARD RULE 5 — never a silently-wrong range). Structured-ref resolution is Excel-correct: `Table[Col]`→the column's DATA body; `[[#Headers],[Col]]`→the header cell; `[[#Totals],[Col]]`/`[[#All],[Col]]`→the totals/whole-column region; `Table[@Col]`/`[[#This Row],[Col]]`→the Col cell on the FORMULA's own row; a multi-column `[[A]:[B]]`→the spanning range; each qualified `Sheet!` only when the table lives on another sheet | Non-concern: READING the metadata off the file (reader.rs owns calamine, xlsx_meta.rs owns the zip/xml table+name parts), the token WALK that decides which identifiers are names/tables vs strings/functions (translate.rs owns boundaries), and whether the resulting A1 parses/evaluates (charlie-ast) | IO: none — pure data + string transforms over A1
//! Import-time reference resolution: [`Resolution`] maps defined-name tokens and structured/table
//! references to plain Excel-A1 strings so the engine stays A1-only. Pure logic + neutral data; the
//! reader fills it (from calamine + the xlsx name/table parts) and `translate` queries it per token.

use charlie_ast::a1::{format_cell, parse_a1};

/// One table's geometry, in 0-based ABSOLUTE grid coordinates, everything a structured reference needs.
/// The rectangle `(c0,r0)..=(c1,r1)` is the table's FULL `ref` (header rows + data body + totals rows);
/// `header_rows`/`totals_rows` slice it into regions. `columns` are the header names in left-to-right
/// order, so column `columns[i]` sits at absolute column `c0 + i`.
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

/// The workbook's neutral TABLE resolution context. Built by the reader, queried by `translate` to
/// resolve `Table[…]` structured references INLINE at import. Empty for a source with no tables — then
/// every lookup misses and a structured-ref token passes through verbatim (loading as `#NAME?` if truly
/// unresolvable). Defined NAMES are no longer inlined here — they are emitted as on-disk FS4 entries
/// (`names` module) and resolved at LOAD (HARD RULE 2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resolution {
    tables: Vec<TableGeom>,
}

/// Which row band of a table a structured reference selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Region {
    Headers,
    Data,
    Totals,
    All,
    ThisRow,
}

/// Which column(s) of a table a structured reference selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColSel {
    /// Every column (a bare `Table[#Data]` / `Table[#All]` with no column item).
    All,
    /// One column, by its index in `columns`.
    One(usize),
    /// A left..=right span of columns (`Table[[A]:[B]]`).
    Span(usize, usize),
}

impl Resolution {
    /// An empty resolution — every lookup misses (used for ODS and for context-free translation).
    pub fn empty() -> Self {
        Resolution::default()
    }

    /// True when there is nothing to resolve (lets the reader/translator skip work).
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }

    /// Register a table from its raw parts. A `ref` that does not parse as a rectangle is dropped
    /// (the table is simply not resolvable — its structured refs stay verbatim -> `#NAME?`).
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

    /// Is `ident` a known table name? (Excel table names are case-insensitive.) Lets `translate` decide
    /// that an `ident[` is a structured reference rather than something else.
    pub fn is_table(&self, ident: &str) -> bool {
        self.tables
            .iter()
            .any(|t| t.name.eq_ignore_ascii_case(ident))
    }

    /// Resolve a structured reference `table[inner]` to a plain A1 string (`Sheet!`-qualified only when
    /// the table is on another sheet than `cur_sheet`), given the FORMULA cell's own 0-based `cur_row`
    /// (for the relative `@`/`#This Row` forms). `None` when the table, column, or region cannot be
    /// resolved — the caller then keeps the token verbatim so it loads as a located `#NAME?`.
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
    /// The 0-based index of a column by header name (Excel case-insensitive).
    fn col_index(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.eq_ignore_ascii_case(name))
    }

    /// Parse the inner text of `Table[...]` into a (row-region, column-selection) pair. Handles the bare
    /// column (`Amount`), the bracketed forms (`[Amount]`, `[[#Headers],[Amount]]`, `[[#This Row],[Col]]`,
    /// `[[#All],[Col]]`, `[[#Totals],[Col]]`), the `@` this-row shorthand (`@Amount`, `@[Amount]`), a
    /// whole-region form (`#All`, `#Data`, `#Headers`, `#Totals`), and a multi-column span (`[A]:[B]`).
    fn parse_spec(&self, inner: &str) -> Option<(Region, ColSel)> {
        let s = inner.trim();
        // A leading `@` is the relative this-row selector; the remainder describes the column(s).
        let (this_row, rest) = match s.strip_prefix('@') {
            Some(r) => (true, r.trim()),
            None => (false, s),
        };
        let mut region: Option<Region> = None;
        let mut col_item: Option<&str> = None;
        for item in split_top(rest, ',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            if let Some(kw) = region_keyword(item) {
                region = Some(parse_region(kw)?);
            } else if col_item.is_some() {
                // Two column items is not a form we resolve — leave it verbatim (-> #NAME?).
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

    /// Parse the column portion of a structured ref into a [`ColSel`]. `None` -> a whole-table (all
    /// columns) selection; a bare/bracketed name -> one column; a `[A]:[B]` -> a span.
    fn parse_cols(&self, item: Option<&str>) -> Option<ColSel> {
        let Some(s) = item else {
            return Some(ColSel::All);
        };
        if let Some((l, r)) = split_top_once(s, ':') {
            let li = self.col_index(unbracket(l.trim()))?;
            let ri = self.col_index(unbracket(r.trim()))?;
            Some(ColSel::Span(li.min(ri), li.max(ri)))
        } else {
            Some(ColSel::One(self.col_index(unbracket(s.trim()))?))
        }
    }

    /// The absolute top/bottom row band for a region. `None` when the region does not exist for this
    /// table (no header rows, no totals row, an empty data body, or a `#This Row`/`@` whose formula
    /// row falls outside the data band) — the ref then stays verbatim -> a located `#NAME?`.
    fn rows_for(&self, region: Region, cur_row: u32) -> Option<(u32, u32)> {
        match region {
            Region::Headers => {
                // `header_rows` comes straight off the xlsx `headerRowCount` attr (unbounded u32); an
                // out-of-range value must not overshoot the table or overflow. CHECKED, and clamped to
                // the table's own bottom `r1` — an over-large header count is unresolvable, not wrong
                // (return None -> verbatim -> located #NAME?, honouring CORE2 no-panic + HARD RULE 5).
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
                // `totals_rows` comes straight off the xlsx `totalsRowCount` attr (unbounded u32); a
                // value exceeding the table's row span would underflow `r1 - totals_rows + 1`. CHECKED,
                // and clamped to the table's own top `r0` — an over-large totals count is unresolvable,
                // not a wrapped huge row number (return None -> verbatim -> located #NAME?).
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
            Region::ThisRow => {
                // `@Col` / `[#This Row]` is an implicit-intersection ref: it means the column cell on
                // the FORMULA's own row, and Excel considers it valid ONLY when that row lies inside the
                // table's DATA band (r0+header_rows ..= r1-totals_rows) — the calculated-column case.
                // Authored OUTSIDE the band (a cell above/below the table, or the header/totals row)
                // Excel yields `#VALUE!`; resolving it to `(cur_row, cur_row)` anyway would emit a
                // syntactically valid but semantically wrong cell (e.g. `=Sales[@Q1]` in Z50 -> B50, or
                // in the header row -> the header cell). So we return `None` and the token stays verbatim
                // -> a located `#NAME?`/error at load (HARD RULE 5 — never a silently-wrong range).
                // CHECKED like every sibling arm: `header_rows` is an unbounded u32 straight off the
                // xlsx part, so `r0 + header_rows` can overflow on a pathological geometry — on overflow
                // return None (unresolvable -> verbatim -> located #NAME?) rather than panic (CORE2).
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

    /// The absolute left/right column band for a column selection. `c0` is the table's 0-based column
    /// origin and can sit anywhere in the u32 grid — a corrupt xlsx table `ref` can carry a near-`u32::MAX`
    /// origin (`parse_a1` does not cap columns) — while `ColSel::One`/`Span` carry column offsets. CHECKED
    /// like every sibling band computation in `rows_for`: `c0 + offset` can overflow u32 on a pathological
    /// geometry, so on overflow return None (unresolvable -> the token stays verbatim -> a located #NAME?
    /// at load) rather than panic (CORE2 no-panic) or wrap to a silently-wrong column range (HARD RULE 5).
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

    /// Spell an absolute `(cl,rt)..=(cr,rb)` block as A1 — a bare cell when 1x1, else a range —
    /// qualified with `Sheet!` only when the table is on a different sheet than the formula.
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

/// Parse a table `ref` (`A1:B89`, or a single `A1`) into `(c0,r0,c1,r1)` 0-based absolute corners.
fn parse_ref(ref_str: &str) -> Option<(u32, u32, u32, u32)> {
    let (a, b) = match ref_str.trim().split_once(':') {
        Some((a, b)) => (a, b),
        None => (ref_str.trim(), ref_str.trim()),
    };
    let pa = parse_a1(a).ok()?;
    let pb = parse_a1(b).ok()?;
    // NORMALIZE corner ordering (min/max) so all band math can assume `c0<=c1` and `r0<=r1`. Excel
    // always writes a table `ref` as top-left:bottom-right, but a reversed/degenerate `ref` would
    // otherwise invert every region computation rather than being handled — this makes the geometry
    // orientation-agnostic instead of trusting the source's corner order.
    Some((
        pa.col.min(pb.col),
        pa.row.min(pb.row),
        pa.col.max(pb.col),
        pa.row.max(pb.row),
    ))
}

/// If `item` is a region keyword (`#Headers`, `[#Headers]`, …) return the text after the `#`.
fn region_keyword(item: &str) -> Option<&str> {
    if let Some(rest) = item.strip_prefix('#') {
        return Some(rest);
    }
    if let Some(inner) = item.strip_prefix("[#").and_then(|r| r.strip_suffix(']')) {
        return Some(inner);
    }
    None
}

/// Map a region keyword (case-insensitive, `#`-stripped) to a [`Region`]. Unknown -> `None`.
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

/// Strip one surrounding `[...]` from a column token (`[Amount]` -> `Amount`; `Amount` unchanged).
fn unbracket(s: &str) -> &str {
    s.strip_prefix('[')
        .and_then(|r| r.strip_suffix(']'))
        .unwrap_or(s)
}

/// Split `s` on `delim` at bracket-depth 0 (so a comma/colon inside `[...]` is not a separator).
fn split_top(s: &str, delim: char) -> Vec<&str> {
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

/// Split `s` on the FIRST bracket-depth-0 `delim` into `(left, right)`, or `None` if there is none.
fn split_top_once(s: &str, delim: char) -> Option<(&str, &str)> {
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

/// Spell a sheet name as a formula qualifier: bare when it is a simple identifier, else `'quoted'`
/// (with `''` escaping) so a name with spaces/punctuation still round-trips through charlie's lexer.
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

    /// A `Sales` table: A1:C4, 1 header row (row 1), data rows 2..=4, columns Region/Q1/Q2 at A/B/C.
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
        // Table[Col] -> that column's data body (header stripped).
        assert_eq!(
            r.resolve_structured("Sales", "Q1", "Data", 9).as_deref(),
            Some("B2:B4")
        );
        assert_eq!(
            r.resolve_structured("Sales", "[Q2]", "Data", 9).as_deref(),
            Some("C2:C4")
        );
        // Case-insensitive table + column names.
        assert_eq!(
            r.resolve_structured("sales", "q1", "Data", 9).as_deref(),
            Some("B2:B4")
        );
    }

    #[test]
    fn structured_headers_and_this_row_and_span() {
        let r = sales();
        // [[#Headers],[Col]] -> the header cell.
        assert_eq!(
            r.resolve_structured("Sales", "[#Headers],[Q1]", "Data", 9)
                .as_deref(),
            Some("B1")
        );
        // @Col / [#This Row],[Col] -> the Col cell on the FORMULA's own row (cur_row is 0-based).
        assert_eq!(
            r.resolve_structured("Sales", "@Q1", "Data", 2).as_deref(),
            Some("B3")
        );
        assert_eq!(
            r.resolve_structured("Sales", "[#This Row],[Q1]", "Data", 1)
                .as_deref(),
            Some("B2")
        );
        // Multi-column span -> the spanning range across the data body.
        assert_eq!(
            r.resolve_structured("Sales", "[Q1]:[Q2]", "Data", 9)
                .as_deref(),
            Some("B2:C4")
        );
    }

    #[test]
    fn structured_all_totals_and_cross_sheet() {
        let mut r = Resolution::empty();
        // A table WITH a totals row: A1:C5, header row 1, data 2..=4, totals row 5.
        r.add_table(
            "T",
            "Data",
            vec!["Region".into(), "Q1".into(), "Q2".into()],
            "A1:C5",
            1,
            1,
        );
        // [Col] excludes both header and totals.
        assert_eq!(
            r.resolve_structured("T", "Q1", "Data", 9).as_deref(),
            Some("B2:B4")
        );
        // [[#Totals],[Col]] -> the totals cell.
        assert_eq!(
            r.resolve_structured("T", "[#Totals],[Q1]", "Data", 9)
                .as_deref(),
            Some("B5")
        );
        // [[#All],[Col]] -> header+data+totals.
        assert_eq!(
            r.resolve_structured("T", "[#All],[Q1]", "Data", 9)
                .as_deref(),
            Some("B1:B5")
        );
        // Bare #All (no column) -> the whole rectangle.
        assert_eq!(
            r.resolve_structured("T", "#All", "Data", 9).as_deref(),
            Some("A1:C5")
        );
        // Referenced from ANOTHER sheet -> qualified with the table's sheet.
        assert_eq!(
            r.resolve_structured("T", "Q1", "Other", 9).as_deref(),
            Some("Data!B2:B4")
        );
    }

    #[test]
    fn a_totals_ref_on_a_table_without_totals_is_unresolvable_not_wrong() {
        // HARD RULE 5: [#Totals] on a table with no totals row is NOT silently mapped somewhere — it
        // stays unresolvable so the token loads as a located #NAME?.
        let r = sales();
        assert_eq!(
            r.resolve_structured("Sales", "[#Totals],[Q1]", "Data", 9),
            None
        );
        // An unknown column is likewise unresolvable.
        assert_eq!(r.resolve_structured("Sales", "Nope", "Data", 9), None);
    }

    #[test]
    fn a_this_row_ref_outside_the_data_band_is_unresolvable_not_wrong() {
        // HARD RULE 5: an `@Col` / `[#This Row]` authored OUTSIDE the table's data band is #VALUE! in
        // Excel — it must NOT resolve to a syntactically valid but semantically wrong cell. Sales is
        // A1:C4 (header row 0, data rows 1..=3, all 0-based), so only cur_row 1..=3 resolve.
        let r = sales();
        // In-band (the calculated-column case) still resolves to the Col cell on that row.
        assert_eq!(
            r.resolve_structured("Sales", "@Q1", "Data", 1).as_deref(),
            Some("B2")
        );
        assert_eq!(
            r.resolve_structured("Sales", "@Q1", "Data", 3).as_deref(),
            Some("B4")
        );
        // The header row (cur_row 0) is above the data band -> unresolvable (never the header cell).
        assert_eq!(r.resolve_structured("Sales", "@Q1", "Data", 0), None);
        // A row far below the table (Z50-style) is unresolvable (never a stray B-column cell).
        assert_eq!(r.resolve_structured("Sales", "@Q1", "Data", 49), None);
        assert_eq!(
            r.resolve_structured("Sales", "[#This Row],[Q1]", "Data", 0),
            None
        );

        // With a totals row (T is A1:C5, data rows 1..=3, totals row 4 all 0-based), the totals row
        // is BELOW the data band -> an `@` there is unresolvable, not the totals cell.
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
        assert_eq!(t.resolve_structured("T", "@Q1", "Data", 4), None); // totals row
    }

    #[test]
    fn out_of_range_header_and_totals_counts_are_unresolvable_not_a_panic() {
        // CORE2 (no panic) + HARD RULE 5: `headerRowCount`/`totalsRowCount` are unbounded u32 straight
        // off the xlsx part. A count exceeding the table's row span must NOT panic (debug underflow) or
        // wrap to a huge row (release) — it is unresolvable, so the token stays verbatim -> #NAME?.
        let mut r = Resolution::empty();
        // A1:C4 (rows 0..=3) but a corrupt totalsRowCount of 5 (> the 4-row span) and a huge headerCount.
        r.add_table(
            "Bad",
            "Data",
            vec!["Region".into(), "Q1".into(), "Q2".into()],
            "A1:C4",
            9, // headerRowCount overshoots the 4-row table
            5, // totalsRowCount exceeds the 4-row span (would underflow `r1 - totals + 1`)
        );
        // Totals: `r1 - totals_rows` underflows -> None (not a `1u32 - 5u32` panic / wrapped row).
        assert_eq!(
            r.resolve_structured("Bad", "[#Totals],[Q1]", "Data", 9),
            None
        );
        // Headers: `r0 + header_rows - 1` overshoots `r1` -> None (not a range past the table).
        assert_eq!(
            r.resolve_structured("Bad", "[#Headers],[Q1]", "Data", 9),
            None
        );
        // Data: first (r0+header_rows) > last (r1-totals_rows) -> None, as before.
        assert_eq!(r.resolve_structured("Bad", "Q1", "Data", 9), None);
    }

    #[test]
    fn a_degenerate_this_row_geometry_yields_none_not_a_panic() {
        // CORE2 (no panic): `@Col` / `[#This Row]` computes `r0 + header_rows`; `header_rows` is an
        // unbounded u32 straight off the xlsx part, so on a pathological geometry that add can overflow.
        // The ThisRow arm must use CHECKED arithmetic like its siblings -> None (unresolvable, so the
        // token stays verbatim -> located #NAME?), NEVER an overflow panic (debug) or a wrapped row.
        let mut r = Resolution::empty();
        // r0 = 1 (from A2) + a huge headerRowCount overflows u32 in `r0 + header_rows`.
        r.add_table(
            "Bad",
            "Data",
            vec!["Region".into(), "Q1".into(), "Q2".into()],
            "A2:C5",
            u32::MAX, // headerRowCount so large that `r0 + header_rows` overflows u32
            0,
        );
        // Must return None (no panic) for the this-row forms.
        assert_eq!(r.resolve_structured("Bad", "@Q1", "Data", 3), None);
        assert_eq!(
            r.resolve_structured("Bad", "[#This Row],[Q1]", "Data", 3),
            None
        );
    }

    #[test]
    fn a_near_max_column_origin_yields_none_not_a_panic() {
        // CORE2 (no panic) + HARD RULE 5: `cols_for` computes `c0 + column_offset`; `c0` is the table's
        // 0-based column origin taken straight from the xlsx table `ref`, and `parse_a1` does not cap
        // columns — a corrupt ref can carry a near-`u32::MAX` origin whose declared columns run past the
        // u32 grid edge. The column-band computation must use CHECKED arithmetic like `rows_for` -> None
        // (unresolvable, so the token stays verbatim -> located #NAME?), NEVER an overflow panic (debug)
        // or a wrapped, silently-wrong column range (release). Built directly so `c0` sits at the u32 edge
        // (the header/totals-count tests already cover the row-band siblings the same way).
        let mut r = Resolution::empty();
        r.tables.push(TableGeom {
            name: "Max".into(),
            sheet: "Data".into(),
            columns: vec!["Region".into(), "Q1".into(), "Q2".into()],
            c0: u32::MAX, // column origin at the very last u32 column
            r0: 0,
            c1: u32::MAX,
            r1: 3,
            header_rows: 1,
            totals_rows: 0,
        });
        // A single-column ref at a positive offset overflows `c0 + offset` -> None, no panic.
        assert_eq!(r.resolve_structured("Max", "Q1", "Data", 9), None);
        // A multi-column span likewise overflows on its right edge -> None, no panic.
        assert_eq!(
            r.resolve_structured("Max", "[Region]:[Q2]", "Data", 9),
            None
        );
        // The offset-0 column does NOT overflow, so it still resolves — its Data band strips the header
        // row (rows 1..=3, 0-based) at the origin column. This pins that the guard rejects ONLY the
        // genuinely-out-of-range offsets, never the resolvable ones (HARD RULE 5 — no false #NAME?).
        assert_eq!(
            r.resolve_structured("Max", "Region", "Data", 9).as_deref(),
            Some(format!("{}:{}", format_cell(u32::MAX, 1), format_cell(u32::MAX, 3)).as_str())
        );
    }

    #[test]
    fn a_reversed_table_ref_is_normalized_so_band_math_holds() {
        // parse_ref normalizes corner ordering (min/max), so a bottom-right:top-left `ref` yields the
        // same geometry as the canonical top-left:bottom-right — never an inverted region computation.
        let mut r = Resolution::empty();
        r.add_table(
            "Rev",
            "Data",
            vec!["Region".into(), "Q1".into(), "Q2".into()],
            "C4:A1", // reversed corners; must normalize to A1:C4
            1,
            0,
        );
        assert_eq!(
            r.resolve_structured("Rev", "Q1", "Data", 9).as_deref(),
            Some("B2:B4")
        );
    }
}
