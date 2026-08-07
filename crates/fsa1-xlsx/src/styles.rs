// Concern: xl/styles.xml — its fonts, fills, borders, alignments and numFmts, one <cellXfs> entry per look | Non-concern: axis geometry, stamping a cell's s= | IO: (a Workbook) -> bytes + an index map

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use fsa1_model::{
    BorderLine, CUSTOM_NUMFMT_ID, Cell, CellStyle, DEFAULT_FONT_FAMILY, DEFAULT_FONT_SIZE,
    FontStyle, FontWeight, Format, Rgb, TextAlign, TextDecoration, VerticalAlign, WhiteSpace,
    Workbook,
};

const HEADER: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#;

/// The default face, and the `family` hint that belongs to it alone: Calibri IS Windows family 2, so
/// the hint is a fact here and a guess anywhere else — an author-named face's family class is
/// something no declaration carries, and stating one would put in the export what the source never
/// said. The ONE spelling of the pair, so the two places that write the default face cannot drift.
fn default_face() -> String {
    format!(
        r#"<name val="{}"/><family val="2"/>"#,
        escape_attr(DEFAULT_FONT_FAMILY)
    )
}

/// The `<font>` a cell takes by carrying no `s=` at all, spelled from the one default appearance
/// `fsa1-model` states — the same fact the READ leg leaves a matching value undeclared against.
fn default_font() -> String {
    format!(
        r#"<font><sz val="{}"/>{}</font>"#,
        DEFAULT_FONT_SIZE.0,
        default_face()
    )
}

/// Excel reserves the first two fills — `none` then `gray125` — and reads a declared solid one only
/// from index 2 up, so the two are emitted whether or not anything indexes them.
const DEFAULT_FILLS: [&str; 2] = [
    r#"<fill><patternFill patternType="none"/></fill>"#,
    r#"<fill><patternFill patternType="gray125"/></fill>"#,
];

const DEFAULT_BORDER: &str = r#"<border><left/><right/><top/><bottom/><diagonal/></border>"#;

const CELL_STYLE_XFS: &str = r#"<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#;

const CELL_STYLES: &str = r#"<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#;

const GENERAL_NUMFMT_ID: u32 = 0;
const GENERAL_XF: &str = r#"<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>"#;

/// A cell's VISUAL projection: the four style elements it emits, plus its number format. Holding the
/// elements themselves is what keeps the key and the entry one fact — a property no element carries,
/// a column width or a row height, cannot mint an entry, so two cells alike but for the size of the
/// axis they sit on share one `<xf>`.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct XfKey {
    format: Option<Format>,
    font: String,
    fill: String,
    border: String,
    alignment: String,
}

pub(crate) struct StyleTable {
    pub bytes: Vec<u8>,
    index: BTreeMap<XfKey, u32>,
}

impl StyleTable {
    /// `None` is the default look under General — `<xf>` 0, which a cell takes by carrying no `s=`.
    pub(crate) fn index_of(&self, style: &CellStyle, cell: &Cell) -> Option<u32> {
        self.index.get(&xf_key(style, cell_format(cell))).copied()
    }
}

pub(crate) fn build(wb: &Workbook) -> StyleTable {
    emit(&collect(wb))
}

fn collect(wb: &Workbook) -> BTreeSet<XfKey> {
    let mut keys = BTreeSet::new();
    for sheet in 0..wb.sheet_names().len() as u32 {
        let Some(region) = wb.used_region(sheet) else {
            continue;
        };
        for row in region.min_row..=region.max_row {
            for col in region.min_col..=region.max_col {
                // Keyed off the STYLE, so a coordinate a scope root covers and no file does mints its `<xf>` too; it has no value and so no number format.
                let Some(style) = wb.cell_style(sheet, col, row) else {
                    continue;
                };
                let format = wb
                    .source_at(sheet, col, row)
                    .and_then(|cs| cell_format(cs.cell));
                let key = xf_key(&style, format);
                if key != XfKey::default() {
                    keys.insert(key);
                }
            }
        }
    }
    keys
}

pub(crate) fn cell_format(cell: &Cell) -> Option<Format> {
    match cell {
        Cell::Value { format, .. } | Cell::Formula { format, .. } => *format,
        Cell::LoadError { .. } => None,
    }
}

fn xf_key(style: &CellStyle, format: Option<Format>) -> XfKey {
    XfKey {
        format,
        font: font(style),
        fill: fill(style),
        border: border(style),
        alignment: alignment(style),
    }
}

fn emit(keys: &BTreeSet<XfKey>) -> StyleTable {
    let (numfmt_ids, custom) = assign_numfmt_ids(keys);
    let mut fonts = vec![default_font()];
    let mut fills: Vec<String> = DEFAULT_FILLS.iter().map(|f| (*f).to_string()).collect();
    let mut borders = vec![DEFAULT_BORDER.to_string()];

    let mut index = BTreeMap::new();
    let mut xfs = String::from(GENERAL_XF);
    for key in keys {
        let font_id = intern(&mut fonts, &key.font);
        let fill_id = intern(&mut fills, &key.fill);
        let border_id = intern(&mut borders, &key.border);
        let numfmt_id = key.format.map_or(GENERAL_NUMFMT_ID, |f| numfmt_ids[&f]);
        let _ = write!(
            xfs,
            r#"<xf numFmtId="{numfmt_id}" fontId="{font_id}" fillId="{fill_id}" borderId="{border_id}" xfId="0""#
        );
        for (applied, attr) in [
            (key.format.is_some(), "applyNumberFormat"),
            (font_id != 0, "applyFont"),
            (fill_id != 0, "applyFill"),
            (border_id != 0, "applyBorder"),
            (!key.alignment.is_empty(), "applyAlignment"),
        ] {
            if applied {
                let _ = write!(xfs, r#" {attr}="1""#);
            }
        }
        if key.alignment.is_empty() {
            xfs.push_str("/>");
        } else {
            let _ = write!(xfs, ">{}</xf>", key.alignment);
        }
        index.insert(key.clone(), index.len() as u32 + 1);
    }

    let mut xml = String::from(HEADER);
    if !custom.is_empty() {
        let _ = write!(xml, r#"<numFmts count="{}">"#, custom.len());
        for (id, code) in &custom {
            let _ = write!(
                xml,
                r#"<numFmt numFmtId="{id}" formatCode="{}"/>"#,
                escape_attr(code)
            );
        }
        xml.push_str("</numFmts>");
    }
    push_table(&mut xml, "fonts", &fonts);
    push_table(&mut xml, "fills", &fills);
    push_table(&mut xml, "borders", &borders);
    xml.push_str(CELL_STYLE_XFS);
    let _ = write!(xml, r#"<cellXfs count="{}">"#, keys.len() + 1);
    xml.push_str(&xfs);
    xml.push_str("</cellXfs>");
    xml.push_str(CELL_STYLES);

    StyleTable {
        bytes: xml.into_bytes(),
        index,
    }
}

/// A custom code takes ONE id however many looks share it, and the ids run in [`Format`] order, so
/// adding a bold heading over a currency cell cannot renumber the `<numFmts>` block.
fn assign_numfmt_ids(keys: &BTreeSet<XfKey>) -> (BTreeMap<Format, u32>, Vec<(u32, String)>) {
    let mut ids = BTreeMap::new();
    let mut custom = Vec::new();
    let mut next_custom_id = CUSTOM_NUMFMT_ID;
    for format in keys
        .iter()
        .filter_map(|k| k.format)
        .collect::<BTreeSet<_>>()
    {
        let id = if format.numfmt_id() >= CUSTOM_NUMFMT_ID {
            let id = next_custom_id;
            next_custom_id += 1;
            custom.push((id, format.code()));
            id
        } else {
            format.numfmt_id()
        };
        ids.insert(format, id);
    }
    (ids, custom)
}

/// An empty fragment is the DEFAULT entry, which every table already holds at index 0.
fn intern(table: &mut Vec<String>, xml: &str) -> u32 {
    if xml.is_empty() {
        return 0;
    }
    match table.iter().position(|x| x == xml) {
        Some(at) => at as u32,
        None => {
            table.push(xml.to_string());
            table.len() as u32 - 1
        }
    }
}

fn push_table(xml: &mut String, name: &str, entries: &[String]) {
    let _ = write!(xml, r#"<{name} count="{}">"#, entries.len());
    for entry in entries {
        xml.push_str(entry);
    }
    let _ = write!(xml, "</{name}>");
}

/// Empty where nothing a `<font>` carries was declared. The size and the FACE are spelled whether or
/// not the author named them: a `<font>` is read whole, so one carrying `<b/>` alone would take
/// Excel's own default face rather than the workbook's. The `family` hint rides with the default face
/// alone — see [`default_face`].
fn font(style: &CellStyle) -> String {
    let underline = matches!(style.text_decoration, Some(TextDecoration::Underline));
    let strike = matches!(style.text_decoration, Some(TextDecoration::LineThrough));
    let bold = matches!(style.font_weight, Some(FontWeight::Bold));
    let italic = matches!(style.font_style, Some(FontStyle::Italic));
    let plain = !(bold || italic || strike || underline)
        && style.color.is_none()
        && style.font_size.is_none()
        && style.font_family.is_none();
    if plain {
        return String::new();
    }
    let mut out = String::from("<font>");
    for (set, tag) in [
        (bold, "b"),
        (italic, "i"),
        (strike, "strike"),
        (underline, "u"),
    ] {
        if set {
            let _ = write!(out, "<{tag}/>");
        }
    }
    let _ = write!(
        out,
        r#"<sz val="{}"/>"#,
        style.font_size.unwrap_or(DEFAULT_FONT_SIZE).0
    );
    if let Some(rgb) = style.color {
        let _ = write!(out, r#"<color rgb="{}"/>"#, argb(rgb));
    }
    // The one author-written text in the table, and `font-family` admits `<` and `&`.
    match &style.font_family {
        Some(name) => {
            let _ = write!(out, r#"<name val="{}"/>"#, escape_attr(name));
        }
        None => out.push_str(&default_face()),
    }
    out.push_str("</font>");
    out
}

fn fill(style: &CellStyle) -> String {
    match style.background_color {
        None => String::new(),
        Some(rgb) => format!(
            r#"<fill><patternFill patternType="solid"><fgColor rgb="{}"/><bgColor indexed="64"/></patternFill></fill>"#,
            argb(rgb)
        ),
    }
}

/// ECMA-376 fixes the child order left, right, top, bottom, diagonal, and an undeclared edge is
/// present-but-empty rather than absent.
fn border(style: &CellStyle) -> String {
    let edges = [
        ("left", style.border_left),
        ("right", style.border_right),
        ("top", style.border_top),
        ("bottom", style.border_bottom),
    ];
    if edges.iter().all(|(_, edge)| edge.is_none()) {
        return String::new();
    }
    let mut out = String::from("<border>");
    for (tag, edge) in edges {
        match edge {
            None => {
                let _ = write!(out, "<{tag}/>");
            }
            Some(b) => {
                let _ = write!(
                    out,
                    r#"<{tag} style="{}"><color rgb="{}"/></{tag}>"#,
                    line_style(b.line),
                    argb(b.color)
                );
            }
        }
    }
    out.push_str("<diagonal/></border>");
    out
}

/// The `ST_BorderStyle` name of each pair the model admits; the CSS width and style it was written
/// from live in `fsa1_model::declaration`, and no third spelling exists.
fn line_style(line: BorderLine) -> &'static str {
    match line {
        BorderLine::ThinSolid => "thin",
        BorderLine::MediumSolid => "medium",
        BorderLine::ThickSolid => "thick",
        BorderLine::ThinDashed => "dashed",
        BorderLine::MediumDashed => "mediumDashed",
        BorderLine::ThinDotted => "dotted",
        BorderLine::ThickDouble => "double",
    }
}

fn alignment(style: &CellStyle) -> String {
    let mut attrs = String::new();
    if let Some(align) = style.text_align {
        let horizontal = match align {
            TextAlign::Left => "left",
            TextAlign::Center => "center",
            TextAlign::Right => "right",
            TextAlign::Justify => "justify",
        };
        let _ = write!(attrs, r#" horizontal="{horizontal}""#);
    }
    if let Some(align) = style.vertical_align {
        let vertical = match align {
            VerticalAlign::Top => "top",
            VerticalAlign::Middle => "center",
            VerticalAlign::Bottom => "bottom",
        };
        let _ = write!(attrs, r#" vertical="{vertical}""#);
    }
    // A cell wraps only where asked to, so `nowrap` IS the unstyled cell and carries no attribute.
    if matches!(style.white_space, Some(WhiteSpace::Normal)) {
        attrs.push_str(r#" wrapText="1""#);
    }
    if attrs.is_empty() {
        String::new()
    } else {
        format!("<alignment{attrs}/>")
    }
}

/// OOXML colours are ARGB and the model carries no alpha, so every one is opaque.
fn argb(rgb: Rgb) -> String {
    format!("FF{:02X}{:02X}{:02X}", rgb.r, rgb.g, rgb.b)
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fsa1_model::{Chars, CurrencySymbol, DatePattern, Format, Points, Workbook};

    /// The Office default stylesheet: what a workbook declaring no style and no format must emit, byte
    /// for byte, since `xl/styles.xml` is a graded part of the round-trip corpus.
    const GENERAL_ONLY_XML: &str = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        "\n",
        r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="1"><font><sz val="11"/><name val="Calibri"/><family val="2"/></font></fonts><fills count="2"><fill>"#,
        r#"<patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>"#,
        r#"<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>"#,
        r#"<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#,
    );

    fn table(styles: &[(CellStyle, Option<Format>)]) -> (StyleTable, String) {
        let keys: BTreeSet<XfKey> = styles
            .iter()
            .map(|(style, format)| xf_key(style, *format))
            .filter(|k| *k != XfKey::default())
            .collect();
        let table = emit(&keys);
        let xml = String::from_utf8(table.bytes.clone()).expect("the part is UTF-8");
        (table, xml)
    }

    fn styled(rule: &str) -> CellStyle {
        let root = fsa1_model::parse_filename("A1:B2").expect("a root").region;
        let presentation = fsa1_model::parse_rules("S/A1:B2.css", root, &format!("  {rule}\n"))
            .unwrap_or_else(|d| panic!("{rule:?} should parse: {:?}", d[0]));
        fsa1_model::style::resolve(&presentation, 1, 1)
    }

    #[test]
    fn an_unstyled_workbook_is_byte_identical_to_the_office_default() {
        let (table, xml) = table(&[]);
        assert_eq!(xml, GENERAL_ONLY_XML);
        assert!(table.index.is_empty());
    }

    #[test]
    fn a_builtin_format_reuses_its_id_and_emits_no_numfmt() {
        let percent = Format::Percent { decimals: 2 };
        let (table, xml) = table(&[(CellStyle::default(), Some(percent))]);
        assert_eq!(
            table.index_of(&CellStyle::default(), &numeric(Some(percent))),
            Some(1)
        );
        assert!(
            !xml.contains("<numFmts"),
            "a built-in needs no custom numFmt"
        );
        assert!(
            xml.contains(r#"<xf numFmtId="10" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>"#),
            "percent 0.00% reuses built-in id 10 and still takes a cellXfs entry: {xml}"
        );
    }

    #[test]
    fn a_custom_format_gets_a_164_numfmt_and_a_cellxfs_entry() {
        let currency = Format::Currency {
            symbol: CurrencySymbol::Dollar,
            grouping: true,
            decimals: 2,
        };
        let (table, xml) = table(&[(CellStyle::default(), Some(currency))]);
        assert_eq!(
            table.index_of(&CellStyle::default(), &numeric(Some(currency))),
            Some(1)
        );
        assert!(xml.contains(r#"<numFmts count="1">"#));
        assert!(xml.contains(r#"<numFmt numFmtId="164" formatCode="$#,##0.00"/>"#));
        assert!(xml.contains(
            r#"<xf numFmtId="164" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>"#
        ));
    }

    #[test]
    fn custom_ids_are_assigned_in_deterministic_format_order() {
        let formats = [
            Format::Currency {
                symbol: CurrencySymbol::Dollar,
                grouping: true,
                decimals: 2,
            },
            Format::Fixed { decimals: 4 },
            Format::Date(DatePattern::MmDdYy),
        ];
        let styles: Vec<(CellStyle, Option<Format>)> = formats
            .iter()
            .map(|f| (CellStyle::default(), Some(*f)))
            .collect();
        let (_, xml) = table(&styles);
        let (_, again) = table(&styles);
        assert_eq!(xml, again, "a second emit is byte-identical");
        assert!(
            xml.contains(r#"numFmtId="164""#),
            "the first custom takes id 164, in Format order"
        );
        assert!(
            xml.contains(r#"numFmtId="165""#),
            "the second custom takes id 165, in Format order"
        );
        assert!(
            xml.contains(r#"<xf numFmtId="14""#),
            "the interleaved built-in date reuses id 14 and takes no custom slot"
        );
    }

    /// A custom code is a workbook-level entry, so the two looks sharing it must not mint it twice.
    #[test]
    fn two_looks_over_one_custom_format_share_its_numfmt() {
        let currency = Format::Currency {
            symbol: CurrencySymbol::Dollar,
            grouping: true,
            decimals: 2,
        };
        let (_, xml) = table(&[
            (CellStyle::default(), Some(currency)),
            (styled("td { font-weight: bold }"), Some(currency)),
        ]);
        assert_eq!(xml.matches(r#"formatCode="$#,##0.00""#).count(), 1, "{xml}");
        assert_eq!(xml.matches(r#"<xf numFmtId="164""#).count(), 2, "{xml}");
    }

    /// The load-bearing exclusion: a width and a height are axis geometry, not a cell's look, and an
    /// `<xf>` per column width would mint an entry per column and unpin the part's bytes.
    #[test]
    fn two_cells_differing_only_in_their_axis_sizes_share_one_xf() {
        let narrow = CellStyle {
            width: Some(Chars(4.0)),
            height: Some(Points(15.0)),
            ..styled("td { font-weight: bold }")
        };
        let wide = CellStyle {
            width: Some(Chars(40.0)),
            height: Some(Points(30.0)),
            ..styled("td { font-weight: bold }")
        };
        assert_ne!(narrow, wide, "the two styles really do differ");
        let (table, xml) = table(&[(narrow.clone(), None), (wide.clone(), None)]);
        assert!(
            xml.contains(r#"<cellXfs count="2">"#),
            "one <xf> beside General: {xml}"
        );
        assert_eq!(
            table.index_of(&narrow, &numeric(None)),
            table.index_of(&wide, &numeric(None))
        );
    }

    #[test]
    fn each_declared_property_reaches_its_own_element() {
        let (_, xml) = table(&[(
            styled(
                "td { background-color: #ffffff; border-bottom: 1px solid #3f0421; color: #3f0421; \
                 font-family: Times New Roman; font-size: 14pt; font-style: italic; \
                 font-weight: bold; text-align: center; text-decoration: underline; \
                 vertical-align: middle; white-space: normal }",
            ),
            None,
        )]);
        assert!(
            xml.contains(
                r#"<font><b/><i/><u/><sz val="14"/><color rgb="FF3F0421"/><name val="Times New Roman"/></font>"#
            ),
            "{xml}"
        );
        assert!(
            xml.contains(
                r#"<fill><patternFill patternType="solid"><fgColor rgb="FFFFFFFF"/><bgColor indexed="64"/></patternFill></fill>"#
            ),
            "{xml}"
        );
        assert!(
            xml.contains(
                r#"<border><left/><right/><top/><bottom style="thin"><color rgb="FF3F0421"/></bottom><diagonal/></border>"#
            ),
            "{xml}"
        );
        assert!(
            xml.contains(
                r#"<xf numFmtId="0" fontId="1" fillId="2" borderId="1" xfId="0" applyFont="1" applyFill="1" applyBorder="1" applyAlignment="1"><alignment horizontal="center" vertical="center" wrapText="1"/></xf>"#
            ),
            "{xml}"
        );
    }

    /// One element per distinct value however many looks carry it, which is what keeps a heading
    /// colour from re-emitting a font per cell.
    #[test]
    fn a_shared_font_is_interned_once_across_the_looks_that_carry_it() {
        let bold = styled("td { font-weight: bold }");
        let (_, xml) = table(&[
            (bold.clone(), None),
            (bold, Some(Format::Percent { decimals: 2 })),
        ]);
        assert_eq!(xml.matches("<font>").count(), 2, "the default and one bold");
        assert!(xml.contains(r#"<fonts count="2">"#), "{xml}");
    }

    #[test]
    fn a_font_family_holding_markup_is_escaped_into_the_attribute() {
        let style = CellStyle {
            font_family: Some("A<B&C".to_string()),
            ..CellStyle::default()
        };
        let (_, xml) = table(&[(style, None)]);
        assert!(xml.contains(r#"<name val="A&lt;B&amp;C"/>"#), "{xml}");
    }

    #[test]
    fn every_border_line_spells_its_own_ooxml_style() {
        for (css, want) in [
            ("1px solid", "thin"),
            ("2px solid", "medium"),
            ("3px solid", "thick"),
            ("1px dashed", "dashed"),
            ("2px dashed", "mediumDashed"),
            ("1px dotted", "dotted"),
            ("3px double", "double"),
        ] {
            let style = styled(&format!("td {{ border-top: {css} #3f0421 }}"));
            let (_, xml) = table(&[(style, None)]);
            assert!(
                xml.contains(&format!(r#"<top style="{want}">"#)),
                "{css} -> {want}: {xml}"
            );
        }
    }

    /// The walk a real workbook takes: every distinct look in the tab reaches the table, and the
    /// index a cell reads back is the one its own style and format were interned under.
    #[test]
    fn a_loaded_workbook_interns_the_look_each_cell_reads_back() {
        let wb = Workbook::from_tabs(&[(
            "Sheet1",
            &[
                ("A1:A2", "Total\n12.50%"),
                ("A1:A2.css", "  tr:first-child td { font-weight: bold }\n"),
            ],
        )]);
        let wb = wb.unwrap_or_else(|d| panic!("the workbook loads: {:?}", d[0]));
        let table = build(&wb);
        let heading = wb.cell_style(0, 0, 0).expect("A1 is covered");
        let body = wb.cell_style(0, 0, 1).expect("A2 is covered");
        let heading_cell = wb.source_at(0, 0, 0).expect("A1 is covered").cell;
        let body_cell = wb.source_at(0, 0, 1).expect("A2 is covered").cell;
        assert_eq!(table.index_of(&heading, heading_cell), Some(1));
        assert_eq!(table.index_of(&body, body_cell), Some(2));
        let xml = String::from_utf8(table.bytes).expect("the part is UTF-8");
        assert!(xml.contains("<b/>"), "{xml}");
        assert!(xml.contains(r#"numFmtId="10""#), "{xml}");
    }

    fn numeric(format: Option<Format>) -> Cell {
        Cell::Value {
            value: fsa1_ast::Value::Number(1.0),
            format,
        }
    }
}
