// Concern: reads the appearance an xlsx package states — its style table, theme colours and sheet geometry | Non-concern: cell values and their number formats (xlsx_meta.rs) | IO: (path) -> Styling

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use fsa1_ast::a1::parse_a1;
use fsa1_model::Rgb;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use zip::ZipArchive;

use crate::error::{ErrorKind, IngestError};
use crate::xlsx_meta::{attr, read_entry, sheet_name_by_part, xml_err};

/// A font as `xl/styles.xml` states it; a property the `<font>` omits stays `None`/`false`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct XlsxFont {
    pub name: Option<String>,
    pub size: Option<f64>,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub underline: Option<Underline>,
    pub color: Option<Rgb>,
    pub vert_align: Option<VertAlign>,
    /// The two legacy Mac face effects, which CSS's closed presentation set has no word for.
    pub outline: bool,
    pub shadow: bool,
}

/// A raised or lowered run, which CSS's closed presentation set has no word for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertAlign {
    Superscript,
    Subscript,
}

const VERT_ALIGNS: &[(&str, VertAlign)] = &[
    ("superscript", VertAlign::Superscript),
    ("subscript", VertAlign::Subscript),
];

impl VertAlign {
    pub fn spell(self) -> &'static str {
        spell_from(VERT_ALIGNS, self)
    }
}

/// The accounting underlines stay apart from the plain ones: they have no CSS spelling, so the leg
/// that narrows one can name what it narrowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Underline {
    Single,
    Double,
    SingleAccounting,
    DoubleAccounting,
}

/// Read forward to parse a `<u val>` and backward to name one, so the two directions cannot drift.
const UNDERLINES: &[(&str, Underline)] = &[
    ("single", Underline::Single),
    ("double", Underline::Double),
    ("singleAccounting", Underline::SingleAccounting),
    ("doubleAccounting", Underline::DoubleAccounting),
];

impl Underline {
    pub fn spell(self) -> &'static str {
        spell_from(UNDERLINES, self)
    }
}

/// A cell's fill. A SOLID fill paints `fg`; `bg` is the backdrop only a patterned fill shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct XlsxFill {
    pub pattern: FillPattern,
    pub fg: Option<Rgb>,
    pub bg: Option<Rgb>,
}

/// `Other` keeps the hatch's own name (`gray125`, `lightUp`, `gradient`) rather than flattening it to
/// a colour here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum FillPattern {
    #[default]
    None,
    Solid,
    Other(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XlsxBorder {
    pub style: BorderStyle,
    pub color: Option<Rgb>,
}

/// Excel's thirteen drawn edges. `dashDot` and its family have no CSS width-and-style pair, so they
/// are retained rather than coerced at the read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderStyle {
    Thin,
    Medium,
    Thick,
    Double,
    Hair,
    Dotted,
    Dashed,
    DashDot,
    DashDotDot,
    MediumDashed,
    MediumDashDot,
    MediumDashDotDot,
    SlantDashDot,
}

/// Read forward to parse a `<left style>` and backward to name one.
const BORDER_STYLES: &[(&str, BorderStyle)] = &[
    ("thin", BorderStyle::Thin),
    ("medium", BorderStyle::Medium),
    ("thick", BorderStyle::Thick),
    ("double", BorderStyle::Double),
    ("hair", BorderStyle::Hair),
    ("dotted", BorderStyle::Dotted),
    ("dashed", BorderStyle::Dashed),
    ("dashDot", BorderStyle::DashDot),
    ("dashDotDot", BorderStyle::DashDotDot),
    ("mediumDashed", BorderStyle::MediumDashed),
    ("mediumDashDot", BorderStyle::MediumDashDot),
    ("mediumDashDotDot", BorderStyle::MediumDashDotDot),
    ("slantDashDot", BorderStyle::SlantDashDot),
];

impl BorderStyle {
    pub fn spell(self) -> &'static str {
        spell_from(BORDER_STYLES, self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HorizontalAlign {
    General,
    Left,
    Center,
    Right,
    Fill,
    Justify,
    CenterContinuous,
    Distributed,
}

const HORIZONTAL_ALIGNS: &[(&str, HorizontalAlign)] = &[
    ("general", HorizontalAlign::General),
    ("left", HorizontalAlign::Left),
    ("center", HorizontalAlign::Center),
    ("right", HorizontalAlign::Right),
    ("fill", HorizontalAlign::Fill),
    ("justify", HorizontalAlign::Justify),
    ("centerContinuous", HorizontalAlign::CenterContinuous),
    ("distributed", HorizontalAlign::Distributed),
];

impl HorizontalAlign {
    pub fn spell(self) -> &'static str {
        spell_from(HORIZONTAL_ALIGNS, self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalAlign {
    Top,
    Center,
    Bottom,
    Justify,
    Distributed,
}

const VERTICAL_ALIGNS: &[(&str, VerticalAlign)] = &[
    ("top", VerticalAlign::Top),
    ("center", VerticalAlign::Center),
    ("bottom", VerticalAlign::Bottom),
    ("justify", VerticalAlign::Justify),
    ("distributed", VerticalAlign::Distributed),
];

impl VerticalAlign {
    pub fn spell(self) -> &'static str {
        spell_from(VERTICAL_ALIGNS, self)
    }
}

/// One resolved `cellXfs` entry: the whole appearance a cell's `s=` names, its font, fill and border
/// entries already followed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct XlsxStyle {
    pub font: XlsxFont,
    pub fill: XlsxFill,
    pub border_top: Option<XlsxBorder>,
    pub border_bottom: Option<XlsxBorder>,
    pub border_left: Option<XlsxBorder>,
    pub border_right: Option<XlsxBorder>,
    pub horizontal: Option<HorizontalAlign>,
    pub vertical: Option<VerticalAlign>,
    pub wrap_text: bool,
    pub indent: u32,
    /// A drawn `<diagonal>` edge, which no CSS border side is.
    pub diagonal: bool,
    /// `<alignment textRotation>` in degrees, 255 being Excel's vertical-stack sentinel.
    pub rotation: i32,
    /// `<alignment shrinkToFit>`: the face shrinks until the text fits, which no CSS declaration does.
    pub shrink_to_fit: bool,
    /// `<xf quotePrefix>`: the leading apostrophe that forced the entry to text. Not an appearance at
    /// all — it survives nowhere in the grid, so it is named as a loss.
    pub quote_prefix: bool,
}

/// The closed set of styles any cell can reference — `cellXfs` resolved — indexed by the `s=`
/// attribute value, plus the Normal style's font: the appearance a cell has by declaring nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleTable {
    styles: Vec<XlsxStyle>,
    normal_font: XlsxFont,
    default_text: Option<Rgb>,
}

impl StyleTable {
    pub fn get(&self, index: u32) -> Option<&XlsxStyle> {
        self.styles.get(index as usize)
    }

    pub fn normal_font(&self) -> &XlsxFont {
        &self.normal_font
    }

    /// The colour a cell's text has by naming none — Excel's Text 1, which a `<color theme="1">`
    /// names explicitly and 1,233,720 of the corpus's 1,234,236 theme-coloured cells restate. A
    /// writer emits no `color` for it.
    pub fn default_text_color(&self) -> Option<Rgb> {
        self.default_text
    }

    /// What the SOURCE draws over a cell holding no value: a fill of ANY pattern, or any drawn edge,
    /// `<diagonal>` included. Not the occupancy question ([`crate::scope_block::paints_blank`]) — it
    /// only keeps the coordinate inside the sheet's extent, so a look no rule can carry is still there
    /// to be NAMED as a loss instead of vanishing with its cell. An index outside the table draws none.
    pub fn draws_on_blank(&self, index: u32) -> bool {
        self.get(index).is_some_and(|style| {
            style.fill.pattern != FillPattern::None
                || style.diagonal
                || [
                    style.border_top,
                    style.border_bottom,
                    style.border_left,
                    style.border_right,
                ]
                .iter()
                .any(Option::is_some)
        })
    }
}

#[cfg(test)]
impl StyleTable {
    pub(crate) fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }

    /// A table with no package to read it from, for tests over the leg that WRITES a style.
    pub(crate) fn of(styles: Vec<XlsxStyle>, normal_font: XlsxFont) -> StyleTable {
        StyleTable {
            styles,
            normal_font,
            default_text: Some(Rgb { r: 0, g: 0, b: 0 }),
        }
    }
}

/// A merged region, 0-based and inclusive of its anchor; `cols` and `rows` are never zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MergedRegion {
    pub col: u32,
    pub row: u32,
    pub cols: u32,
    pub rows: u32,
}

/// One fact the sheet states over the contiguous axis run that states it — 0-based and inclusive at
/// both ends. A `<col>` run's legal `max` is the last addressable column and a `<cols>` may hold any
/// number of runs, so the run is what is READ: one entry per column here is the whole axis per run,
/// which a hostile `<cols>` turns into an allocation no machine has.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisRun<T> {
    pub first: u32,
    pub last: u32,
    pub value: T,
}

/// A size verbatim as .xlsx states it: a width in characters, a height in points.
pub type AxisSize = AxisRun<f64>;

/// The `s=` index a whole column or row states as the look its cells wear by stating none of their
/// own. It reaches every cell of that axis, written or not, so only the sheet's own extent resolves it.
pub type AxisStyle = AxisRun<u32>;

/// What one `xl/worksheets/sheetN.xml` says about appearance. `styled_cells` holds every `<c>`
/// carrying an `s=`, one with no `<v>` included — the only place a styled blank is stated.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SheetVisuals {
    /// `(col, row, style index)`, 0-based, in document order.
    pub styled_cells: Vec<(u32, u32, u32)>,
    /// One run per `<col>`, each width verbatim as .xlsx states it, in characters.
    pub col_widths: Vec<AxisSize>,
    /// One run per `<col style>`, in document order — how "format this whole column" is stated.
    pub col_styles: Vec<AxisStyle>,
    /// One run per `<row ht>`, each height verbatim as .xlsx states it, in points. A `<row>` states
    /// one row, so every run on this axis is one row wide.
    pub row_heights: Vec<AxisSize>,
    /// One entry per `<row s customFormat>`, the same fact on the row axis.
    pub row_styles: Vec<AxisStyle>,
    pub merges: Vec<MergedRegion>,
}

/// The whole appearance an xlsx package states: one style table for the workbook, and the geometry
/// each sheet carries.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Styling {
    pub styles: StyleTable,
    /// Keyed by sheet NAME, resolved through `workbook.xml` and its rels — never by part order.
    pub sheets: HashMap<String, SheetVisuals>,
}

/// Calamine exposes no font, fill or border and skips a cell that holds only a style, so the styling
/// is read off the parts here. A missing part yields no entries; a malformed one is a refusal.
pub fn read_styling(path: &Path) -> Result<Styling, IngestError> {
    let file = File::open(path).map_err(|e| {
        IngestError::io(
            ErrorKind::SourceIo,
            format!("cannot open {:?} for styling: {e}", path.display()),
        )
    })?;
    let mut zip = ZipArchive::new(BufReader::new(file)).map_err(|e| {
        IngestError::io(
            ErrorKind::SourceIo,
            format!("cannot read {:?} as a zip archive: {e}", path.display()),
        )
    })?;

    let theme_part = zip
        .file_names()
        .find(|n| n.starts_with("xl/theme/") && n.ends_with(".xml"))
        .map(str::to_string);
    let theme = match theme_part {
        Some(part) => match read_entry(&mut zip, &part)? {
            Some(xml) => parse_theme_colors(&xml)?,
            None => Vec::new(),
        },
        None => Vec::new(),
    };
    let styles = match read_entry(&mut zip, "xl/styles.xml")? {
        Some(xml) => resolve_styles(&parse_style_sheet(&xml, &theme)?, &theme),
        None => StyleTable::default(),
    };

    let name_by_part = sheet_name_by_part(&mut zip)?;
    let mut parts: Vec<String> = name_by_part.keys().cloned().collect();
    parts.sort();
    let mut sheets = HashMap::new();
    for part in parts {
        if let Some(xml) = read_entry(&mut zip, &part)? {
            sheets.insert(name_by_part[&part].clone(), parse_sheet_visuals(&xml)?);
        }
    }
    Ok(Styling { styles, sheets })
}

/// One `<xf>` as written: the ids it names, before they are followed. The number format is not one
/// of them — it is a value's spelling, which `xlsx_meta` reads.
#[derive(Clone, Debug, Default)]
struct Xf {
    font_id: Option<usize>,
    fill_id: Option<usize>,
    border_id: Option<usize>,
    quote_prefix: bool,
    alignment: Alignment,
}

#[derive(Clone, Copy, Debug, Default)]
struct Alignment {
    horizontal: Option<HorizontalAlign>,
    vertical: Option<VerticalAlign>,
    wrap_text: bool,
    indent: u32,
    rotation: i32,
    shrink_to_fit: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct BorderSet {
    top: Option<XlsxBorder>,
    bottom: Option<XlsxBorder>,
    left: Option<XlsxBorder>,
    right: Option<XlsxBorder>,
    diagonal: bool,
}

#[derive(Clone, Copy, Debug)]
enum BorderEdge {
    Top,
    Bottom,
    Left,
    Right,
}

/// `xl/styles.xml` as written: the indexed blocks an `<xf>` points into, none of them followed yet.
#[derive(Clone, Debug, Default)]
struct StyleSheet {
    fonts: Vec<XlsxFont>,
    fills: Vec<XlsxFill>,
    borders: Vec<BorderSet>,
    cell_xfs: Vec<Xf>,
    cell_style_xfs: Vec<Xf>,
    /// `(style name, index into `cell_style_xfs`)`.
    cell_styles: Vec<(String, usize)>,
}

type XmlReader<'a> = Reader<&'a [u8]>;

fn parse_style_sheet(xml: &str, theme: &[Option<Rgb>]) -> Result<StyleSheet, IngestError> {
    let mut reader = Reader::from_str(xml);
    let mut out = StyleSheet::default();
    loop {
        match reader.read_event().map_err(xml_err)? {
            Event::Start(e) => match e.local_name().as_ref() {
                b"fonts" => out.fonts = parse_fonts(&mut reader, theme)?,
                b"fills" => out.fills = parse_fills(&mut reader, theme)?,
                b"borders" => out.borders = parse_borders(&mut reader, theme)?,
                b"cellXfs" => out.cell_xfs = parse_xfs(&mut reader, b"cellXfs")?,
                b"cellStyleXfs" => out.cell_style_xfs = parse_xfs(&mut reader, b"cellStyleXfs")?,
                b"cellStyles" => out.cell_styles = parse_cell_styles(&mut reader)?,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

/// The `applyFont`/`applyFill`/`applyBorder` flags are not consulted: a producer writes the effective
/// ids into the `cellXfs` entry itself, so following them is what the entry already says.
fn resolve_styles(sheet: &StyleSheet, theme: &[Option<Rgb>]) -> StyleTable {
    StyleTable {
        styles: sheet
            .cell_xfs
            .iter()
            .map(|xf| resolve_xf(sheet, xf))
            .collect(),
        normal_font: normal_font(sheet),
        // Text 1 under no tint, which is what `<color theme="1"/>` resolves to.
        default_text: theme_color(theme, 1),
    }
}

fn resolve_xf(sheet: &StyleSheet, xf: &Xf) -> XlsxStyle {
    let borders = xf
        .border_id
        .and_then(|i| sheet.borders.get(i))
        .copied()
        .unwrap_or_default();
    XlsxStyle {
        font: xf
            .font_id
            .and_then(|i| sheet.fonts.get(i))
            .cloned()
            .unwrap_or_default(),
        fill: xf
            .fill_id
            .and_then(|i| sheet.fills.get(i))
            .cloned()
            .unwrap_or_default(),
        border_top: borders.top,
        border_bottom: borders.bottom,
        border_left: borders.left,
        border_right: borders.right,
        horizontal: xf.alignment.horizontal,
        vertical: xf.alignment.vertical,
        wrap_text: xf.alignment.wrap_text,
        indent: xf.alignment.indent,
        diagonal: borders.diagonal,
        rotation: xf.alignment.rotation,
        shrink_to_fit: xf.alignment.shrink_to_fit,
        quote_prefix: xf.quote_prefix,
    }
}

/// The named cell style "Normal" is the appearance every cell starts from; a workbook naming none
/// starts from its first font, the slot Excel writes Normal's into.
fn normal_font(sheet: &StyleSheet) -> XlsxFont {
    sheet
        .cell_styles
        .iter()
        .find(|(name, _)| name == "Normal")
        .and_then(|(_, xf_id)| sheet.cell_style_xfs.get(*xf_id))
        .and_then(|xf| xf.font_id)
        .and_then(|i| sheet.fonts.get(i))
        .or_else(|| sheet.fonts.first())
        .cloned()
        .unwrap_or_default()
}

fn parse_fonts(
    reader: &mut XmlReader<'_>,
    theme: &[Option<Rgb>],
) -> Result<Vec<XlsxFont>, IngestError> {
    let mut out = Vec::new();
    loop {
        match reader.read_event().map_err(xml_err)? {
            Event::Start(e) if e.local_name().as_ref() == b"font" => {
                out.push(parse_font(reader, theme)?);
            }
            Event::Empty(e) if e.local_name().as_ref() == b"font" => out.push(XlsxFont::default()),
            Event::End(e) if e.local_name().as_ref() == b"fonts" => break,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

fn parse_font(reader: &mut XmlReader<'_>, theme: &[Option<Rgb>]) -> Result<XlsxFont, IngestError> {
    let mut font = XlsxFont::default();
    loop {
        let e = match reader.read_event().map_err(xml_err)? {
            Event::Start(e) | Event::Empty(e) => e,
            Event::End(e) if e.local_name().as_ref() == b"font" => break,
            Event::Eof => break,
            _ => continue,
        };
        match e.local_name().as_ref() {
            b"b" => font.bold = flag(&e),
            b"i" => font.italic = flag(&e),
            b"strike" => font.strike = flag(&e),
            b"outline" => font.outline = flag(&e),
            b"shadow" => font.shadow = flag(&e),
            b"u" => font.underline = parse_underline(&e),
            b"sz" => font.size = attr(&e, b"val").and_then(|v| v.parse().ok()),
            b"name" | b"rFont" => font.name = attr(&e, b"val"),
            b"color" => font.color = parse_color(&e, theme),
            b"vertAlign" => {
                font.vert_align = attr(&e, b"val").and_then(|v| parse_from(VERT_ALIGNS, &v));
            }
            _ => {}
        }
    }
    Ok(font)
}

/// A bare `<u/>` is a single underline; `val="none"` is no underline at all.
fn parse_underline(e: &BytesStart) -> Option<Underline> {
    match attr(e, b"val") {
        None => Some(Underline::Single),
        Some(val) => parse_from(UNDERLINES, &val),
    }
}

fn parse_fills(
    reader: &mut XmlReader<'_>,
    theme: &[Option<Rgb>],
) -> Result<Vec<XlsxFill>, IngestError> {
    let mut out = Vec::new();
    let mut cur = XlsxFill::default();
    loop {
        match reader.read_event().map_err(xml_err)? {
            // A `<fill/>` states a whole entry, and it takes the index a later `fillId` counts on.
            Event::Empty(e) if e.local_name().as_ref() == b"fill" => {
                out.push(XlsxFill::default());
            }
            Event::Start(e) | Event::Empty(e) => match e.local_name().as_ref() {
                b"fill" => cur = XlsxFill::default(),
                b"patternFill" => cur.pattern = parse_pattern(attr(&e, b"patternType").as_deref()),
                b"gradientFill" => cur.pattern = FillPattern::Other("gradient".to_string()),
                b"fgColor" => cur.fg = parse_color(&e, theme),
                b"bgColor" => cur.bg = parse_color(&e, theme),
                _ => {}
            },
            Event::End(e) if e.local_name().as_ref() == b"fill" => {
                out.push(std::mem::take(&mut cur));
            }
            Event::End(e) if e.local_name().as_ref() == b"fills" => break,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

fn parse_pattern(pattern_type: Option<&str>) -> FillPattern {
    match pattern_type {
        None | Some("none") => FillPattern::None,
        Some("solid") => FillPattern::Solid,
        Some(other) => FillPattern::Other(other.to_string()),
    }
}

fn parse_borders(
    reader: &mut XmlReader<'_>,
    theme: &[Option<Rgb>],
) -> Result<Vec<BorderSet>, IngestError> {
    let mut out = Vec::new();
    let mut cur = BorderSet::default();
    let mut open: Option<BorderEdge> = None;
    loop {
        match reader.read_event().map_err(xml_err)? {
            // A `<border/>` states a whole entry, and it takes the index a later `borderId` counts on.
            Event::Empty(e) if e.local_name().as_ref() == b"border" => {
                out.push(BorderSet::default());
                open = None;
            }
            Event::Start(e) | Event::Empty(e) => match e.local_name().as_ref() {
                b"border" => cur = BorderSet::default(),
                // Which WAY it runs is the `diagonalUp`/`diagonalDown` pair on `<border>`, moot for an edge that is dropped.
                b"diagonal" => {
                    cur.diagonal = attr(&e, b"style").is_some();
                    open = None;
                }
                b"color" => {
                    if let Some(border) = open.and_then(|edge| edge_of(&mut cur, edge).as_mut()) {
                        border.color = parse_color(&e, theme);
                    }
                }
                name => {
                    let edge = parse_edge(name);
                    if let Some(edge) = edge {
                        let style = attr(&e, b"style").and_then(|s| parse_from(BORDER_STYLES, &s));
                        *edge_of(&mut cur, edge) =
                            style.map(|style| XlsxBorder { style, color: None });
                    }
                    open = edge;
                }
            },
            Event::End(e) if e.local_name().as_ref() == b"border" => {
                out.push(cur);
                open = None;
            }
            Event::End(e) if e.local_name().as_ref() == b"borders" => break,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

fn parse_edge(name: &[u8]) -> Option<BorderEdge> {
    Some(match name {
        b"top" => BorderEdge::Top,
        b"bottom" => BorderEdge::Bottom,
        b"left" => BorderEdge::Left,
        b"right" => BorderEdge::Right,
        _ => return None,
    })
}

fn edge_of(set: &mut BorderSet, edge: BorderEdge) -> &mut Option<XlsxBorder> {
    match edge {
        BorderEdge::Top => &mut set.top,
        BorderEdge::Bottom => &mut set.bottom,
        BorderEdge::Left => &mut set.left,
        BorderEdge::Right => &mut set.right,
    }
}

fn parse_xfs(reader: &mut XmlReader<'_>, section: &[u8]) -> Result<Vec<Xf>, IngestError> {
    let mut out = Vec::new();
    let mut cur: Option<Xf> = None;
    loop {
        match reader.read_event().map_err(xml_err)? {
            Event::Empty(e) if e.local_name().as_ref() == b"xf" => out.push(parse_xf_head(&e)),
            Event::Start(e) if e.local_name().as_ref() == b"xf" => cur = Some(parse_xf_head(&e)),
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"alignment" => {
                if let Some(xf) = cur.as_mut() {
                    xf.alignment = parse_alignment(&e);
                }
            }
            Event::End(e) if e.local_name().as_ref() == b"xf" => {
                if let Some(xf) = cur.take() {
                    out.push(xf);
                }
            }
            Event::End(e) if e.local_name().as_ref() == section => break,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

fn parse_xf_head(e: &BytesStart) -> Xf {
    let id = |key: &[u8]| attr(e, key).and_then(|v| v.parse::<usize>().ok());
    Xf {
        font_id: id(b"fontId"),
        fill_id: id(b"fillId"),
        border_id: id(b"borderId"),
        quote_prefix: attr(e, b"quotePrefix").as_deref().is_some_and(truthy),
        alignment: Alignment::default(),
    }
}

fn parse_alignment(e: &BytesStart) -> Alignment {
    Alignment {
        horizontal: attr(e, b"horizontal").and_then(|v| parse_from(HORIZONTAL_ALIGNS, &v)),
        vertical: attr(e, b"vertical").and_then(|v| parse_from(VERTICAL_ALIGNS, &v)),
        wrap_text: attr(e, b"wrapText").as_deref().is_some_and(truthy),
        indent: attr(e, b"indent")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0),
        rotation: attr(e, b"textRotation")
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0),
        shrink_to_fit: attr(e, b"shrinkToFit").as_deref().is_some_and(truthy),
    }
}

fn parse_cell_styles(reader: &mut XmlReader<'_>) -> Result<Vec<(String, usize)>, IngestError> {
    let mut out = Vec::new();
    loop {
        match reader.read_event().map_err(xml_err)? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"cellStyle" => {
                if let (Some(name), Some(xf_id)) = (
                    attr(&e, b"name"),
                    attr(&e, b"xfId").and_then(|v| v.parse::<usize>().ok()),
                ) {
                    out.push((name, xf_id));
                }
            }
            Event::End(e) if e.local_name().as_ref() == b"cellStyles" => break,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

/// The last column an .xlsx addresses, which is where a `<col>` run's stated `max` is clamped.
const MAX_COLUMNS: u32 = 16_384;

/// `<dimension>` is never read: it is Excel's cached claim, and a sheet holding one stray coordinate
/// declares a region tens of thousands of rows tall.
fn parse_sheet_visuals(xml: &str) -> Result<SheetVisuals, IngestError> {
    let mut reader = Reader::from_str(xml);
    let mut out = SheetVisuals::default();
    loop {
        match reader.read_event().map_err(xml_err)? {
            Event::Start(e) | Event::Empty(e) => match e.local_name().as_ref() {
                b"col" => {
                    out.col_widths.extend(parse_col_width(&e));
                    out.col_styles.extend(parse_col_style(&e));
                }
                b"row" => {
                    out.row_heights.extend(parse_row_height(&e));
                    out.row_styles.extend(parse_row_style(&e));
                }
                b"c" => out.styled_cells.extend(parse_styled_cell(&e)),
                b"mergeCell" => {
                    out.merges
                        .extend(attr(&e, b"ref").as_deref().and_then(parse_region));
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

/// The 0-based columns a `<col>` run covers, `min..=max` clamped to the addressable axis. The run is
/// what is kept — the sheet's own extent is what later clips it, and clipping cannot happen here
/// because this leg has not read the cells yet.
fn col_run(e: &BytesStart) -> Option<(u32, u32)> {
    let min = attr(e, b"min").and_then(|v| v.parse::<u32>().ok())?;
    let max = attr(e, b"max").and_then(|v| v.parse::<u32>().ok())?;
    let (first, last) = (min.max(1), max.min(MAX_COLUMNS));
    (first <= last).then_some((first - 1, last - 1))
}

/// Only a CUSTOM width is carried: a run without `customWidth` is the sheet's default width restated,
/// which no cell asked for.
fn parse_col_width(e: &BytesStart) -> Option<AxisSize> {
    if !attr(e, b"customWidth").as_deref().is_some_and(truthy) {
        return None;
    }
    let width = attr(e, b"width").and_then(|v| v.parse::<f64>().ok())?;
    let (first, last) = col_run(e)?;
    Some(AxisRun {
        first,
        last,
        value: width,
    })
}

/// `<col style>` gates on nothing: it is the whole statement "this column looks like xf N", and both
/// Excel and openpyxl write it with no `customWidth` beside it.
fn parse_col_style(e: &BytesStart) -> Option<AxisStyle> {
    let index = attr(e, b"style").and_then(|v| v.parse::<u32>().ok())?;
    let (first, last) = col_run(e)?;
    Some(AxisRun {
        first,
        last,
        value: index,
    })
}

fn parse_row_height(e: &BytesStart) -> Option<AxisSize> {
    if !attr(e, b"customHeight").as_deref().is_some_and(truthy) {
        return None;
    }
    let height = attr(e, b"ht").and_then(|v| v.parse::<f64>().ok())?;
    let row = row_of(e)?;
    Some(AxisRun {
        first: row,
        last: row,
        value: height,
    })
}

/// A `<row s>` counts only under `customFormat`; without it the index is Excel's cached restatement of
/// what the row's own `<c>` elements already carry.
fn parse_row_style(e: &BytesStart) -> Option<AxisStyle> {
    if !attr(e, b"customFormat").as_deref().is_some_and(truthy) {
        return None;
    }
    let index = attr(e, b"s").and_then(|v| v.parse::<u32>().ok())?;
    let row = row_of(e)?;
    Some(AxisRun {
        first: row,
        last: row,
        value: index,
    })
}

fn row_of(e: &BytesStart) -> Option<u32> {
    attr(e, b"r")
        .and_then(|v| v.parse::<u32>().ok())?
        .checked_sub(1)
}

fn parse_styled_cell(e: &BytesStart) -> Option<(u32, u32, u32)> {
    let index = attr(e, b"s").and_then(|v| v.parse::<u32>().ok())?;
    let at = parse_a1(&attr(e, b"r")?).ok()?;
    Some((at.col, at.row, index))
}

fn parse_region(text: &str) -> Option<MergedRegion> {
    let (start, end) = text.split_once(':').unwrap_or((text, text));
    let (a, b) = (parse_a1(start).ok()?, parse_a1(end).ok()?);
    Some(MergedRegion {
        col: a.col.min(b.col),
        row: a.row.min(b.row),
        cols: a.col.abs_diff(b.col) + 1,
        rows: a.row.abs_diff(b.row) + 1,
    })
}

/// `auto` names whatever colour the reading system uses, not a concrete one, so it resolves to none.
fn parse_color(e: &BytesStart, theme: &[Option<Rgb>]) -> Option<Rgb> {
    if attr(e, b"auto").as_deref().is_some_and(truthy) {
        return None;
    }
    let base = if let Some(rgb) = attr(e, b"rgb").and_then(|v| parse_argb(&v)) {
        rgb
    } else if let Some(index) = attr(e, b"indexed").and_then(|v| v.parse::<u32>().ok()) {
        indexed_color(index)?
    } else {
        theme_color(
            theme,
            attr(e, b"theme").and_then(|v| v.parse::<u32>().ok())?,
        )?
    };
    let tint = attr(e, b"tint")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0);
    Some(apply_tint(base, tint))
}

/// `AARRGGBB` or a bare `RRGGBB`; the alpha byte is dropped, an FSA1 colour having none.
fn parse_argb(text: &str) -> Option<Rgb> {
    let hex = match text.len() {
        8 => text.get(2..)?,
        6 => text,
        _ => return None,
    };
    Some(Rgb {
        r: u8::from_str_radix(hex.get(0..2)?, 16).ok()?,
        g: u8::from_str_radix(hex.get(2..4)?, 16).ok()?,
        b: u8::from_str_radix(hex.get(4..6)?, 16).ok()?,
    })
}

/// The scheme's own order, which a `theme=` attribute does NOT count in ([`theme_color`]).
const SCHEME_SLOTS: [&[u8]; 12] = [
    b"dk1",
    b"lt1",
    b"dk2",
    b"lt2",
    b"accent1",
    b"accent2",
    b"accent3",
    b"accent4",
    b"accent5",
    b"accent6",
    b"hlink",
    b"folHlink",
];

/// One slot per scheme colour whether or not the part states it, so a missing one cannot shift every
/// later index. Only the FIRST `<a:clrScheme>` counts; a theme may carry extra schemes after it.
fn parse_theme_colors(xml: &str) -> Result<Vec<Option<Rgb>>, IngestError> {
    let mut reader = Reader::from_str(xml);
    let mut out = vec![None; SCHEME_SLOTS.len()];
    let mut slot: Option<usize> = None;
    let mut in_scheme = false;
    loop {
        match reader.read_event().map_err(xml_err)? {
            Event::Start(e) if e.local_name().as_ref() == b"clrScheme" => in_scheme = true,
            Event::End(e) if e.local_name().as_ref() == b"clrScheme" => break,
            Event::Start(e) | Event::Empty(e) if in_scheme => {
                let name = e.local_name();
                if let Some(i) = SCHEME_SLOTS.iter().position(|s| *s == name.as_ref()) {
                    slot = Some(i);
                } else if let Some(i) = slot {
                    let color = match name.as_ref() {
                        b"srgbClr" => attr(&e, b"val").and_then(|v| parse_argb(&v)),
                        b"sysClr" => attr(&e, b"lastClr").and_then(|v| parse_argb(&v)),
                        _ => None,
                    };
                    if color.is_some() {
                        out[i] = color;
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

/// A `theme=` attribute indexes the scheme with each of its first two PAIRS swapped: Excel counts
/// Background 1 before Text 1, where `<a:clrScheme>` writes dk1 before lt1.
fn theme_color(theme: &[Option<Rgb>], index: u32) -> Option<Rgb> {
    let slot = match index {
        0 => 1,
        1 => 0,
        2 => 3,
        3 => 2,
        other => other as usize,
    };
    theme.get(slot).copied().flatten()
}

/// ECMA-376's tint, applied to the LUMINANCE in HLS space: a negative tint scales it toward black by
/// `L * (1 + tint)`, a positive one toward white by `L * (1 - tint) + tint`. Zero is the colour
/// itself, -1 is black and +1 is white.
fn apply_tint(color: Rgb, tint: f64) -> Rgb {
    if tint == 0.0 || !tint.is_finite() {
        return color;
    }
    let tint = tint.clamp(-1.0, 1.0);
    let (hue, saturation, lum) = to_hsl(color);
    let lum = if tint < 0.0 {
        lum * (1.0 + tint)
    } else {
        lum * (1.0 - tint) + tint
    };
    from_hsl(hue, saturation, lum)
}

fn to_hsl(color: Rgb) -> (f64, f64, f64) {
    let (r, g, b) = (
        f64::from(color.r) / 255.0,
        f64::from(color.g) / 255.0,
        f64::from(color.b) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let lum = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, lum);
    }
    let span = max - min;
    let saturation = if lum > 0.5 {
        span / (2.0 - max - min)
    } else {
        span / (max + min)
    };
    let hue = if max == r {
        (g - b) / span + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / span + 2.0
    } else {
        (r - g) / span + 4.0
    };
    (hue / 6.0, saturation, lum)
}

fn from_hsl(hue: f64, saturation: f64, lum: f64) -> Rgb {
    if saturation == 0.0 {
        let level = channel(lum);
        return Rgb {
            r: level,
            g: level,
            b: level,
        };
    }
    let q = if lum < 0.5 {
        lum * (1.0 + saturation)
    } else {
        lum + saturation - lum * saturation
    };
    let p = 2.0 * lum - q;
    Rgb {
        r: channel(hue_channel(p, q, hue + 1.0 / 3.0)),
        g: channel(hue_channel(p, q, hue)),
        b: channel(hue_channel(p, q, hue - 1.0 / 3.0)),
    }
}

fn hue_channel(p: f64, q: f64, at: f64) -> f64 {
    let at = at.rem_euclid(1.0);
    if at < 1.0 / 6.0 {
        p + (q - p) * 6.0 * at
    } else if at < 0.5 {
        q
    } else if at < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - at) * 6.0
    } else {
        p
    }
}

fn channel(level: f64) -> u8 {
    (level * 255.0).round().clamp(0.0, 255.0) as u8
}

/// The legacy indexed palette (ECMA-376 §18.8.27). 64 and 65 name the system's own foreground and
/// background, which have no fixed value and so resolve to nothing.
const INDEXED_PALETTE: [u32; 64] = [
    0x0000_0000,
    0x00ff_ffff,
    0x00ff_0000,
    0x0000_ff00,
    0x0000_00ff,
    0x00ff_ff00,
    0x00ff_00ff,
    0x0000_ffff,
    0x0000_0000,
    0x00ff_ffff,
    0x00ff_0000,
    0x0000_ff00,
    0x0000_00ff,
    0x00ff_ff00,
    0x00ff_00ff,
    0x0000_ffff,
    0x0080_0000,
    0x0000_8000,
    0x0000_0080,
    0x0080_8000,
    0x0080_0080,
    0x0000_8080,
    0x00c0_c0c0,
    0x0080_8080,
    0x0099_99ff,
    0x0099_3366,
    0x00ff_ffcc,
    0x00cc_ffff,
    0x0066_0066,
    0x00ff_8080,
    0x0000_66cc,
    0x00cc_ccff,
    0x0000_0080,
    0x00ff_00ff,
    0x00ff_ff00,
    0x0000_ffff,
    0x0080_0080,
    0x0080_0000,
    0x0000_8080,
    0x0000_00ff,
    0x0000_ccff,
    0x00cc_ffff,
    0x00cc_ffcc,
    0x00ff_ff99,
    0x0099_ccff,
    0x00ff_99cc,
    0x00cc_99ff,
    0x00ff_cc99,
    0x0033_66ff,
    0x0033_cccc,
    0x0099_cc00,
    0x00ff_cc00,
    0x00ff_9900,
    0x00ff_6600,
    0x0066_6699,
    0x0096_9696,
    0x0000_3366,
    0x0033_9966,
    0x0000_3300,
    0x0033_3300,
    0x0099_3300,
    0x0099_3366,
    0x0033_3399,
    0x0033_3333,
];

fn indexed_color(index: u32) -> Option<Rgb> {
    let packed = INDEXED_PALETTE.get(index as usize).copied()?;
    Some(Rgb {
        r: (packed >> 16) as u8,
        g: (packed >> 8) as u8,
        b: packed as u8,
    })
}

fn flag(e: &BytesStart) -> bool {
    attr(e, b"val").as_deref().is_none_or(truthy)
}

fn truthy(value: &str) -> bool {
    !matches!(value, "0" | "false")
}

/// The table read forward: the value a spelling parses to.
fn parse_from<T: Copy>(table: &[(&str, T)], text: &str) -> Option<T> {
    table.iter().find(|(k, _)| *k == text).map(|(_, v)| *v)
}

/// The same table read backward, so a spelling's two directions cannot drift.
fn spell_from<T: Copy + PartialEq>(table: &[(&'static str, T)], value: T) -> &'static str {
    table
        .iter()
        .find(|(_, v)| *v == value)
        .map(|(k, _)| *k)
        .expect("every variant comes from the table that parses it")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn rgb(hex: u32) -> Rgb {
        Rgb {
            r: (hex >> 16) as u8,
            g: (hex >> 8) as u8,
            b: hex as u8,
        }
    }

    /// The values Excel itself shows for these tints, so a "simplification" of the HLS walk that
    /// keeps the endpoints fails on the middle case.
    #[test]
    fn a_tint_moves_the_luminance_toward_white_or_black() {
        assert_eq!(apply_tint(rgb(0x4f81bd), 0.0), rgb(0x4f81bd));
        assert_eq!(apply_tint(rgb(0x4f81bd), 0.4), rgb(0x95b3d7));
        assert_eq!(apply_tint(rgb(0x4f81bd), -0.25), rgb(0x376092));
        assert_eq!(apply_tint(rgb(0x000000), 0.5), rgb(0x808080));
        assert_eq!(apply_tint(rgb(0xffffff), -0.5), rgb(0x808080));
        assert_eq!(apply_tint(rgb(0x4f81bd), 1.0), rgb(0xffffff), "+1 is white");
        assert_eq!(
            apply_tint(rgb(0x4f81bd), -1.0),
            rgb(0x000000),
            "-1 is black"
        );
    }

    #[test]
    fn a_theme_reference_indexes_the_scheme_with_its_first_two_pairs_swapped() {
        let theme = parse_theme_colors(concat!(
            r#"<a:theme><a:themeElements><a:clrScheme name="Office">"#,
            r#"<a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>"#,
            r#"<a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>"#,
            r#"<a:dk2><a:srgbClr val="1F497D"/></a:dk2>"#,
            r#"<a:lt2><a:srgbClr val="EEECE1"/></a:lt2>"#,
            r#"<a:accent1><a:srgbClr val="4F81BD"/></a:accent1>"#,
            r#"</a:clrScheme></a:themeElements></a:theme>"#
        ))
        .unwrap();
        assert_eq!(theme_color(&theme, 0), Some(rgb(0xffffff)), "0 is lt1");
        assert_eq!(theme_color(&theme, 1), Some(rgb(0x000000)), "1 is dk1");
        assert_eq!(theme_color(&theme, 2), Some(rgb(0xeeece1)), "2 is lt2");
        assert_eq!(theme_color(&theme, 3), Some(rgb(0x1f497d)), "3 is dk2");
        assert_eq!(theme_color(&theme, 4), Some(rgb(0x4f81bd)), "4 is accent1");
        assert_eq!(
            theme_color(&theme, 5),
            None,
            "an unstated slot is not a colour"
        );
    }

    #[test]
    fn a_color_resolves_from_rgb_indexed_or_theme_and_auto_from_none() {
        let theme = vec![None, Some(rgb(0x4f81bd))];
        let color = |xml: &str| {
            let mut reader = Reader::from_str(xml);
            let Event::Empty(e) = reader.read_event().unwrap() else {
                panic!("{xml} is one empty element");
            };
            parse_color(&e, &theme)
        };
        assert_eq!(color(r#"<color rgb="FFFF0000"/>"#), Some(rgb(0xff0000)));
        assert_eq!(color(r#"<color rgb="00B0F0"/>"#), Some(rgb(0x00b0f0)));
        assert_eq!(color(r#"<color indexed="10"/>"#), Some(rgb(0xff0000)));
        assert_eq!(color(r#"<color indexed="64"/>"#), None, "the system colour");
        assert_eq!(
            color(r#"<color theme="0" tint="0.4"/>"#),
            Some(rgb(0x95b3d7))
        );
        assert_eq!(color(r#"<color auto="1"/>"#), None);
    }

    /// The whole styling read over a real package: a fully styled cell, a cell whose only content is
    /// its fill, and the geometry around them.
    #[test]
    fn the_visuals_fixture_reads_its_styles_geometry_and_styled_blank() {
        let styling = read_styling(&fixture("visuals.xlsx")).unwrap();
        let sheet = styling.sheets.get("Visual").expect("the sheet by NAME");

        let at = |a1: &str| {
            let cell = parse_a1(a1).unwrap();
            sheet
                .styled_cells
                .iter()
                .find(|(col, row, _)| (*col, *row) == (cell.col, cell.row))
                .map(|(_, _, index)| *index)
        };
        let a1 = styling
            .styles
            .get(at("A1").expect("A1 carries a style"))
            .unwrap();
        assert_eq!(a1.font.name.as_deref(), Some("Times New Roman"));
        assert_eq!(a1.font.size, Some(14.0));
        assert!(a1.font.bold && a1.font.italic && a1.font.strike);
        assert_eq!(a1.font.underline, Some(Underline::Single));
        assert_eq!(
            a1.font.color,
            Some(rgb(0x95b3d7)),
            "accent1 under a 0.4 tint"
        );
        assert_eq!(a1.fill.pattern, FillPattern::Solid);
        assert_eq!(
            a1.fill.fg,
            Some(rgb(0xffc000)),
            "a solid fill paints its fg"
        );
        for edge in [
            a1.border_top,
            a1.border_bottom,
            a1.border_left,
            a1.border_right,
        ] {
            assert_eq!(
                edge,
                Some(XlsxBorder {
                    style: BorderStyle::Thin,
                    color: Some(rgb(0xff0000))
                })
            );
        }
        assert_eq!(a1.horizontal, Some(HorizontalAlign::Center));
        assert_eq!(a1.vertical, Some(VerticalAlign::Top));
        assert!(a1.wrap_text);
        assert_eq!(a1.indent, 2);

        let b2 = at("B2").expect("a cell holding only a fill still states its style");
        assert_eq!(styling.styles.get(b2).unwrap().fill.fg, Some(rgb(0x00b0f0)));
        assert!(
            styling.styles.draws_on_blank(b2),
            "a filled blank is drawn over"
        );
        assert_eq!(at("A4"), None, "a plain value cell states no style");

        assert_eq!(
            sheet.col_widths,
            vec![AxisSize {
                first: 2,
                last: 2,
                value: 14.5
            }],
            "column C, verbatim"
        );
        assert_eq!(
            sheet.row_heights,
            vec![AxisSize {
                first: 2,
                last: 2,
                value: 22.5
            }],
            "row 3, verbatim"
        );
        assert_eq!(
            sheet.merges,
            vec![MergedRegion {
                col: 3,
                row: 0,
                cols: 2,
                rows: 1
            }]
        );
    }

    #[test]
    fn the_normal_style_font_is_what_a_cell_declaring_nothing_wears() {
        let styles = read_styling(&fixture("visuals.xlsx")).unwrap().styles;
        assert_eq!(styles.normal_font().name.as_deref(), Some("Calibri"));
        assert_eq!(styles.normal_font().size, Some(11.0));
        assert_eq!(
            styles.default_text_color(),
            Some(rgb(0x000000)),
            "Text 1 under no tint, which `<color theme=\"1\"/>` names"
        );
        assert!(
            !styles.draws_on_blank(0),
            "xf 0 is the Normal style itself, so it draws nothing"
        );
        assert!(
            !styles.draws_on_blank(99),
            "an index outside the table names no style"
        );
    }

    /// The extent reading, which is deliberately WIDER than occupancy: a hatch and a `<diagonal>` cover
    /// an empty cell in Excel and no rule can carry either, so the coordinate is kept for
    /// `name_losses` to find rather than vanishing with the cell.
    #[test]
    fn a_look_that_needs_a_glyph_is_not_drawn_over_an_empty_cell() {
        let arial_9 = XlsxFont {
            name: Some("Arial".to_string()),
            size: Some(9.0),
            ..Default::default()
        };
        let shows_nothing = XlsxStyle {
            font: arial_9,
            horizontal: Some(HorizontalAlign::Center),
            vertical: Some(VerticalAlign::Top),
            wrap_text: true,
            indent: 2,
            rotation: 90,
            shrink_to_fit: true,
            quote_prefix: true,
            ..Default::default()
        };
        let table = StyleTable::of(
            vec![
                shows_nothing,
                fill(FillPattern::Solid, Some(rgb(0x00b0f0))),
                fill(FillPattern::Other("gray125".to_string()), None),
                XlsxStyle {
                    border_bottom: Some(XlsxBorder {
                        style: BorderStyle::Thin,
                        color: Some(rgb(0x000000)),
                    }),
                    ..Default::default()
                },
                XlsxStyle {
                    diagonal: true,
                    ..Default::default()
                },
            ],
            XlsxFont::default(),
        );
        assert!(
            !table.draws_on_blank(0),
            "a font, an alignment, a rotation and a quote prefix all need text before they show"
        );
        for (index, what) in [
            (1, "a solid fill"),
            (2, "a hatch"),
            (3, "an edge"),
            (4, "a diagonal"),
        ] {
            assert!(table.draws_on_blank(index), "{what} covers an empty cell");
        }
    }

    fn fill(pattern: FillPattern, fg: Option<Rgb>) -> XlsxStyle {
        XlsxStyle {
            fill: XlsxFill {
                pattern,
                fg,
                bg: None,
            },
            ..Default::default()
        }
    }

    /// The tail .xlsx states and CSS has no word for. Each is kept, never coerced at the read, so the
    /// write leg can say what it narrowed.
    #[test]
    fn the_untranslatable_tail_of_a_style_survives_the_read() {
        let xml = concat!(
            r#"<styleSheet><fonts count="1"><font><u val="singleAccounting"/>"#,
            r#"<vertAlign val="superscript"/><outline/><shadow/></font></fonts>"#,
            r#"<fills count="1"><fill><patternFill patternType="gray125"/></fill></fills>"#,
            r#"<borders count="1"><border><left style="dashDot"><color rgb="FF00FF00"/></left>"#,
            r#"<diagonal style="thin"><color rgb="FF0000FF"/></diagonal></border></borders>"#,
            r#"<cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" quotePrefix="1">"#,
            r#"<alignment textRotation="90" shrinkToFit="1"/></xf></cellXfs></styleSheet>"#
        );
        let table = resolve_styles(&parse_style_sheet(xml, &[]).unwrap(), &[]);
        let style = table.get(0).unwrap();
        assert_eq!(style.font.underline, Some(Underline::SingleAccounting));
        assert_eq!(style.font.underline.unwrap().spell(), "singleAccounting");
        assert_eq!(style.font.vert_align, Some(VertAlign::Superscript));
        assert_eq!(style.font.vert_align.unwrap().spell(), "superscript");
        assert!(style.font.outline && style.font.shadow);
        assert!(style.diagonal, "a drawn diagonal edge is read");
        assert_eq!(style.rotation, 90);
        assert!(style.shrink_to_fit);
        assert!(style.quote_prefix);
        assert_eq!(
            style.fill.pattern,
            FillPattern::Other("gray125".to_string())
        );
        assert_eq!(
            style.border_left,
            Some(XlsxBorder {
                style: BorderStyle::DashDot,
                color: Some(rgb(0x00ff00))
            })
        );
        assert_eq!(style.border_left.unwrap().style.spell(), "dashDot");
    }

    /// An `<xf>` names its fill and its border by INDEX, so an entry written self-closing has to take
    /// one: skipping it shifts every later entry and the style silently wears its neighbour's look.
    #[test]
    fn a_self_closing_fill_or_border_still_occupies_its_index() {
        let xml = concat!(
            r#"<styleSheet><fills count="3"><fill/><fill><patternFill patternType="none"/></fill>"#,
            r#"<fill><patternFill patternType="solid"><fgColor rgb="FFFF0000"/></patternFill></fill></fills>"#,
            r#"<borders count="2"><border/><border><top style="thin"/></border></borders>"#,
            r#"<cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="2" borderId="1"/></cellXfs>"#,
            r#"</styleSheet>"#
        );
        let table = resolve_styles(&parse_style_sheet(xml, &[]).unwrap(), &[]);
        let style = table.get(0).expect("cellXfs states one entry");
        assert_eq!(style.fill.pattern, FillPattern::Solid);
        assert_eq!(
            style.fill.fg,
            Some(rgb(0xff0000)),
            "fillId 2 is the red one"
        );
        assert_eq!(
            style.border_top,
            Some(XlsxBorder {
                style: BorderStyle::Thin,
                color: None
            }),
            "borderId 1 is the one with a top edge"
        );
    }

    /// `<dimension>` claims A1:E4 on this fixture, which is 20 coordinates; the sheet states four.
    #[test]
    fn the_cached_dimension_is_never_read_as_a_bound() {
        let sheet = &read_styling(&fixture("visuals.xlsx")).unwrap().sheets["Visual"];
        assert_eq!(sheet.styled_cells.len(), 2, "{:?}", sheet.styled_cells);
    }

    #[test]
    fn a_default_width_or_height_is_not_a_stated_one() {
        let visuals = parse_sheet_visuals(concat!(
            r#"<worksheet><cols><col min="1" max="3" width="8.43"/>"#,
            r#"<col min="5" max="6" width="20" customWidth="1"/></cols>"#,
            r#"<sheetData><row r="1" ht="15"/><row r="2" ht="30" customHeight="1"/>"#,
            r#"</sheetData></worksheet>"#
        ))
        .unwrap();
        assert_eq!(
            visuals.col_widths,
            vec![AxisSize {
                first: 4,
                last: 5,
                value: 20.0
            }],
            "a run is read as the run it is, not as one entry per column it spans"
        );
        assert_eq!(
            visuals.row_heights,
            vec![AxisSize {
                first: 1,
                last: 1,
                value: 30.0
            }]
        );
    }

    /// The other place a style is stated: not on a `<c>` at all, but on the axis, which is how Excel
    /// and openpyxl both write "format this whole column/row". A `<col style>` needs no `customWidth`
    /// beside it, and a `<row s>` counts only under `customFormat` — without it the index is Excel's
    /// cached restatement of what the row's own cells already carry.
    #[test]
    fn a_column_and_a_row_state_a_default_style_no_cell_of_them_restates() {
        let visuals = parse_sheet_visuals(concat!(
            r#"<worksheet><cols><col min="2" max="2" style="1"/>"#,
            r#"<col min="4" max="5" width="13" customWidth="1" style="7"/>"#,
            r#"<col min="7" max="7" width="9" customWidth="1"/></cols><sheetData>"#,
            r#"<row r="1" s="3"/><row r="2" customFormat="1" s="3"/>"#,
            r#"<row r="4" customFormat="1"/></sheetData></worksheet>"#
        ))
        .unwrap();
        let run = |first, last, value| AxisStyle { first, last, value };
        assert_eq!(
            visuals.col_styles,
            vec![run(1, 1, 1), run(3, 4, 7)],
            "a bare `<col style>` states one, and a sized one states one too",
        );
        assert_eq!(
            visuals.row_styles,
            vec![run(1, 1, 3)],
            "only the row under `customFormat` states one",
        );
        assert_eq!(
            visuals.col_widths,
            vec![
                AxisSize {
                    first: 3,
                    last: 4,
                    value: 13.0
                },
                AxisSize {
                    first: 6,
                    last: 6,
                    value: 9.0
                }
            ],
            "and the width leg reads the same runs unchanged",
        );
    }

    /// The whole point of keeping the run: `max` may legally be the last addressable column, and a
    /// `<cols>` may state that run as often as it likes. Expanding one entry per column here cost
    /// 327 million entries on this input — a 4 GB allocation, and the process died on SIGABRT before
    /// the sheet's own extent ever got the chance to clip it.
    #[test]
    fn a_cols_block_restating_the_whole_axis_costs_one_entry_per_run() {
        let runs = r#"<col min="1" max="16384" width="20" customWidth="1"/>"#.repeat(20_000);
        let visuals =
            parse_sheet_visuals(&format!("<worksheet><cols>{runs}</cols></worksheet>")).unwrap();
        assert_eq!(visuals.col_widths.len(), 20_000);
        assert_eq!(
            visuals.col_widths[0],
            AxisSize {
                first: 0,
                last: 16_383,
                value: 20.0
            },
        );
    }
}
