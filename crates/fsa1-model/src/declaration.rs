// Concern: declaration vocabulary — a property's type, unit, range and the punctuation it bars | Non-concern: selectors, rule order | IO: (text) -> Declaration; (f64) -> a measure; (Declaration) -> CSS

use crate::diagnostic::Code;

/// One decimal-and-unit value: what [`measure`] reads, and the wordings it leaves a value uncarried
/// with.
struct Measure {
    what: &'static str,
    unit: &'static str,
    plural: &'static str,
    example: &'static str,
    lo: f64,
    hi: f64,
}

/// Each range is Excel's own, which is what the value has to round-trip through as a `sz`, a `<row
/// ht>` or a `<col width>`. An axis may measure zero, a collapsed one having no extent at all; a
/// font may not.
const FONT_SIZE: Measure = Measure {
    what: "font size",
    unit: "pt",
    plural: "points",
    example: "11pt",
    lo: 1.0,
    hi: 409.0,
};

const ROW_HEIGHT: Measure = Measure {
    what: "row height",
    unit: "pt",
    plural: "points",
    example: "15pt",
    lo: 0.0,
    hi: 409.5,
};

const COLUMN_WIDTH: Measure = Measure {
    what: "column width",
    unit: "ch",
    plural: "characters",
    example: "10ch",
    lo: 0.0,
    hi: 255.0,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    /// The single home for a colour's canonical on-disk spelling, so a rewrite suggestion and a
    /// future writer cannot disagree.
    pub fn spell(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

/// A font size in points. Compared by bit pattern, so a round-trip that changes a bit is a real
/// difference rather than one smoothed over.
#[derive(Clone, Copy, Debug)]
pub struct Points(pub f64);

impl Points {
    /// The single home for a font size's canonical on-disk spelling, as [`Rgb::spell`] is a colour's.
    /// Rust's shortest round-tripping form, so `parse(spell(p)) == p` for every `p` reachable here.
    pub fn spell(self) -> String {
        format!("{}pt", self.0)
    }

    /// The canonical `font-size` a raw number takes, or `None` where it has no spelling at all —
    /// [`FONT_SIZE`] read by a WRITER, so the leg that emits a declaration cannot admit a value the
    /// leg that parses one refuses.
    pub fn font_size(n: f64) -> Option<Points> {
        canonical(n, &FONT_SIZE).map(Points)
    }

    /// [`ROW_HEIGHT`] read by a writer, as [`Points::font_size`] reads [`FONT_SIZE`].
    pub fn row_height(n: f64) -> Option<Points> {
        canonical(n, &ROW_HEIGHT).map(Points)
    }
}

impl PartialEq for Points {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for Points {}

/// A column width in characters — the `0` digit's width in the workbook's own font, which is the unit
/// an .xlsx `<col width>` already carries, so the write leg needs no font metrics of its own.
#[derive(Clone, Copy, Debug)]
pub struct Chars(pub f64);

impl Chars {
    /// The single home for a width's canonical on-disk spelling, as [`Points::spell`] is a size's.
    pub fn spell(self) -> String {
        format!("{}ch", self.0)
    }

    /// [`COLUMN_WIDTH`] read by a writer, as [`Points::font_size`] reads [`FONT_SIZE`].
    pub fn column_width(n: f64) -> Option<Chars> {
        canonical(n, &COLUMN_WIDTH).map(Chars)
    }
}

impl PartialEq for Chars {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for Chars {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontWeight {
    Normal,
    Bold,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontStyle {
    Normal,
    Italic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextDecoration {
    None,
    Underline,
    LineThrough,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalAlign {
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhiteSpace {
    Normal,
    Nowrap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

/// The seven `<width> <style>` pairs a border edge may take. A pair outside them (`2px dotted`) is
/// valid CSS with no edge to become, and a width alone renders nothing at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderLine {
    ThinSolid,
    MediumSolid,
    ThickSolid,
    ThinDashed,
    MediumDashed,
    ThinDotted,
    ThickDouble,
}

impl BorderLine {
    /// The CSS `border-style` word of this pair — [`BORDER_LINES`] read backward, as [`Border::spell`]
    /// reads it for the whole shorthand. What a producer naming an approximation says it drew.
    pub fn style_word(self) -> &'static str {
        BORDER_LINES
            .iter()
            .find(|(line, _, _)| *line == self)
            .map(|(_, _, style)| *style)
            .expect("every BorderLine comes from BORDER_LINES")
    }
}

/// Read forward to parse a border and backward to spell one, so the two directions cannot drift.
pub(crate) const BORDER_LINES: &[(BorderLine, &str, &str)] = &[
    (BorderLine::ThinSolid, "1px", "solid"),
    (BorderLine::MediumSolid, "2px", "solid"),
    (BorderLine::ThickSolid, "3px", "solid"),
    (BorderLine::ThinDashed, "1px", "dashed"),
    (BorderLine::MediumDashed, "2px", "dashed"),
    (BorderLine::ThinDotted, "1px", "dotted"),
    (BorderLine::ThickDouble, "3px", "double"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Border {
    pub line: BorderLine,
    pub color: Rgb,
}

impl Border {
    /// [`BORDER_LINES`] read backward — the direction its forward read already promises.
    pub fn spell(self) -> String {
        let (_, width, style) = BORDER_LINES
            .iter()
            .find(|(line, _, _)| *line == self.line)
            .expect("every BorderLine comes from BORDER_LINES");
        format!("{width} {style} {}", self.color.spell())
    }
}

/// Every character a declaration VALUE may not hold without ending the segment the cursor is reading
/// or the rule around it — the FRAME the reader needs named, `!important` moving a cell off the
/// track its own coordinates name and a second `:` being ambiguous with a missing `;`.
const FRAME_FORBIDDEN: &[(char, Code, &str)] = &[
    (
        '!',
        Code::PresentationSyntax,
        "no presentation value carries `!important`",
    ),
    (
        ':',
        Code::PresentationSyntax,
        "declarations are separated by `;`",
    ),
    (
        ';',
        Code::PresentationSyntax,
        "declarations are separated by `;`",
    ),
    (
        '{',
        Code::PresentationSyntax,
        "a declaration holds no `{`; a rule ends with `}`",
    ),
    (
        '}',
        Code::PresentationSyntax,
        "a declaration holds no `}`; it is what closes the rule",
    ),
];

/// Every character a family NAME may not hold: the WRITE leg's own list, read by
/// [`Declaration::font_family`] alone, so the encoder's grammar is stated once. The read leg leaves
/// a face outside it UNCARRIED rather than refusing it.
const FAMILY_FORBIDDEN: &[char] = &['!', ':', ';', '{', '}', '(', ')', ',', '"', '\''];

/// The one read of [`FRAME_FORBIDDEN`] the reader gets, so it cannot grow a list of its own.
fn frame_fault(value: &str) -> Option<(Code, &'static str)> {
    FRAME_FORBIDDEN
        .iter()
        .find(|(c, _, _)| value.contains(*c))
        .map(|(_, code, why)| (*code, *why))
}

/// What one declaration's text broke. A `Frame` is the reader's own — the segment or the rule ended
/// where it stands — and stays a located refusal; every other fault leaves the declaration out of
/// the [`crate::Rule`] entirely, saying why, and the sidecar LOADS.
pub(crate) enum DeclFault {
    Frame(Code, String),
    Uncarried(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Declaration {
    BackgroundColor(Rgb),
    Border { edge: Edge, border: Border },
    Color(Rgb),
    FontFamily(String),
    FontSize(Points),
    FontStyle(FontStyle),
    FontWeight(FontWeight),
    Height(Points),
    TextAlign(TextAlign),
    TextDecoration(TextDecoration),
    VerticalAlign(VerticalAlign),
    WhiteSpace(WhiteSpace),
    Width(Chars),
}

impl Declaration {
    /// One unquoted name whose words are separated by single spaces: CSS family names are unquoted
    /// space-separated identifiers, which is also why declarations need their `;`.
    /// [`FAMILY_FORBIDDEN`] read by a WRITER, as [`Points::font_size`] reads [`FONT_SIZE`], so the
    /// leg that emits a declaration only ever spells a name it can read back.
    pub fn font_family(name: &str) -> Option<Declaration> {
        let spellable = !name.is_empty()
            && !name.contains(FAMILY_FORBIDDEN)
            && name.split(' ').all(|word| {
                !word.is_empty() && !word.chars().any(|c| c.is_whitespace() || c.is_control())
            });
        spellable.then(|| Declaration::FontFamily(name.to_string()))
    }

    /// The canonical CSS text of this declaration, `<property>: <value>`.
    pub fn spell(&self) -> String {
        format!("{}: {}", self.property(), self.value_text())
    }

    /// The canonical CSS text of this declaration's VALUE, spelled by reading the table that parsed
    /// it backward, so a consumer emitting a stylesheet cannot drift from what the parser accepts.
    pub fn value_text(&self) -> String {
        match self {
            Declaration::BackgroundColor(rgb) | Declaration::Color(rgb) => rgb.spell(),
            Declaration::Border { border, .. } => border.spell(),
            Declaration::FontFamily(name) => name.clone(),
            Declaration::FontSize(points) => points.spell(),
            Declaration::FontStyle(s) => spell_keyword(*s, FONT_STYLES).to_string(),
            Declaration::FontWeight(w) => spell_keyword(*w, FONT_WEIGHTS).to_string(),
            Declaration::Height(points) => points.spell(),
            Declaration::TextAlign(a) => spell_keyword(*a, TEXT_ALIGNS).to_string(),
            Declaration::TextDecoration(d) => spell_keyword(*d, TEXT_DECORATIONS).to_string(),
            Declaration::VerticalAlign(a) => spell_keyword(*a, VERTICAL_ALIGNS).to_string(),
            Declaration::WhiteSpace(w) => spell_keyword(*w, WHITE_SPACES).to_string(),
            Declaration::Width(chars) => chars.spell(),
        }
    }

    pub fn property(&self) -> &'static str {
        match self {
            Declaration::BackgroundColor(_) => "background-color",
            Declaration::Border { edge, .. } => match edge {
                Edge::Top => "border-top",
                Edge::Bottom => "border-bottom",
                Edge::Left => "border-left",
                Edge::Right => "border-right",
            },
            Declaration::Color(_) => "color",
            Declaration::FontFamily(_) => "font-family",
            Declaration::FontSize(_) => "font-size",
            Declaration::FontStyle(_) => "font-style",
            Declaration::FontWeight(_) => "font-weight",
            Declaration::Height(_) => "height",
            Declaration::TextAlign(_) => "text-align",
            Declaration::TextDecoration(_) => "text-decoration",
            Declaration::VerticalAlign(_) => "vertical-align",
            Declaration::WhiteSpace(_) => "white-space",
            Declaration::Width(_) => "width",
        }
    }
}

pub(crate) fn parse_declaration(text: &str) -> Result<Declaration, DeclFault> {
    let Some((property, value)) = text.split_once(':') else {
        return Err(frame(&format!(
            "a declaration is `<property>: <value>`; found {text:?}"
        )));
    };
    let (property, value) = (property.trim_end(), value.trim());
    // A rewrite must be a text the reader can receive back: the caller trims each segment, so an empty half would name one ending in whitespace, which trims to the refused text again.
    match (property.is_empty(), value.is_empty()) {
        (true, true) => {
            return Err(frame(
                "a declaration is `<property>: <value>`; found a bare `:`",
            ));
        }
        (true, false) => {
            return Err(frame(&format!(
                "a declaration is `<property>: <value>`; the value {value:?} is given no property"
            )));
        }
        (false, true) => {
            return Err(frame(&format!(
                "a declaration is `<property>: <value>`; the property {property:?} is given no value"
            )));
        }
        (false, false) => {}
    }
    // Kept now that `(` is no longer barred, where it only changes the WORDING: a computed value otherwise reaches the property table below and is named a malformed colour, or size, or keyword — every branch describing the type it is not, and none the one thing wrong with it.
    if value.contains("calc(") {
        return Err(DeclFault::Uncarried(format!(
            "a presentation value is a literal, never computed: {text:?}"
        )));
    }
    if let Some((code, why)) = frame_fault(value) {
        return Err(DeclFault::Frame(code, format!("{why}: {text:?}")));
    }
    let carried = match property {
        "background" => background_shorthand(value),
        "background-color" => color(value).map(Declaration::BackgroundColor),
        "border-top" => border(Edge::Top, value),
        "border-bottom" => border(Edge::Bottom, value),
        "border-left" => border(Edge::Left, value),
        "border-right" => border(Edge::Right, value),
        "color" => color(value).map(Declaration::Color),
        "font-family" => Declaration::font_family(value).ok_or_else(|| {
            format!("a font family is one unquoted name, e.g. `Times New Roman`; found {value:?}")
        }),
        "font-size" => measure(value, &FONT_SIZE).map(|n| Declaration::FontSize(Points(n))),
        "font-style" => keyword(value, FONT_STYLES, "font style").map(Declaration::FontStyle),
        "font-weight" => keyword(value, FONT_WEIGHTS, "font weight").map(Declaration::FontWeight),
        "height" => measure(value, &ROW_HEIGHT).map(|n| Declaration::Height(Points(n))),
        "text-align" => keyword(value, TEXT_ALIGNS, "text alignment").map(Declaration::TextAlign),
        "text-decoration" => {
            keyword(value, TEXT_DECORATIONS, "text decoration").map(Declaration::TextDecoration)
        }
        "vertical-align" => {
            keyword(value, VERTICAL_ALIGNS, "vertical alignment").map(Declaration::VerticalAlign)
        }
        "white-space" => {
            keyword(value, WHITE_SPACES, "white-space mode").map(Declaration::WhiteSpace)
        }
        "width" => measure(value, &COLUMN_WIDTH).map(|n| Declaration::Width(Chars(n))),
        _ => Err(format!(
            "{property:?} is not a supported presentation property"
        )),
    };
    carried.map_err(DeclFault::Uncarried)
}

const FONT_STYLES: &[(&str, FontStyle)] =
    &[("normal", FontStyle::Normal), ("italic", FontStyle::Italic)];

/// The canonical word first: [`spell_keyword`] reads this table backward and takes the first row a
/// variant appears in, so the numeric spellings sit after the words they mean.
const FONT_WEIGHTS: &[(&str, FontWeight)] = &[
    ("normal", FontWeight::Normal),
    ("bold", FontWeight::Bold),
    ("400", FontWeight::Normal),
    ("700", FontWeight::Bold),
];

const TEXT_DECORATIONS: &[(&str, TextDecoration)] = &[
    ("none", TextDecoration::None),
    ("underline", TextDecoration::Underline),
    ("line-through", TextDecoration::LineThrough),
];

const TEXT_ALIGNS: &[(&str, TextAlign)] = &[
    ("left", TextAlign::Left),
    ("center", TextAlign::Center),
    ("right", TextAlign::Right),
    ("justify", TextAlign::Justify),
];

const VERTICAL_ALIGNS: &[(&str, VerticalAlign)] = &[
    ("top", VerticalAlign::Top),
    ("middle", VerticalAlign::Middle),
    ("bottom", VerticalAlign::Bottom),
];

const WHITE_SPACES: &[(&str, WhiteSpace)] = &[
    ("normal", WhiteSpace::Normal),
    ("nowrap", WhiteSpace::Nowrap),
];

/// The admissible spellings come from the same table the value is read with, so the reason a value
/// is uncarried can never name a set the parser does not accept.
fn keyword<T: Copy>(value: &str, table: &[(&str, T)], what: &str) -> Result<T, String> {
    match table.iter().find(|(k, _)| *k == value) {
        Some((_, v)) => Ok(*v),
        None => Err(format!(
            "{value:?} is not a {what}; write one of {}",
            table.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// The same table read backward, so a keyword's two directions cannot drift.
fn spell_keyword<T: Copy + PartialEq>(value: T, table: &[(&'static str, T)]) -> &'static str {
    table
        .iter()
        .find(|(_, v)| *v == value)
        .map(|(k, _)| *k)
        .expect("every keyword variant comes from the table that spells it")
}

/// The one number a measure admits, or `None` where it admits none — read by the parser below AND by
/// a writer through [`Points::font_size`], so a value the write leg emits can never be one the read
/// leg refuses. An axis may measure zero, so `-0` is in range where `-1` is not, and a surviving sign
/// bit would be a SECOND canonical zero to types comparing by bit pattern. NaN and infinity fail.
fn canonical(n: f64, m: &Measure) -> Option<f64> {
    let n = if n == 0.0 { 0.0 } else { n };
    (m.lo..=m.hi).contains(&n).then_some(n)
}

fn measure(value: &str, m: &Measure) -> Result<f64, String> {
    let (what, unit, plural) = (m.what, m.unit, m.plural);
    let bad = || {
        format!(
            "a {what} is a number of {plural}, e.g. `{}`; found {value:?}",
            m.example
        )
    };
    let digits = value.strip_suffix(unit).ok_or_else(bad)?;
    let Ok(n) = digits.parse::<f64>() else {
        return Err(bad());
    };
    canonical(n, m).ok_or_else(|| {
        format!(
            "a {what} is between {}{unit} and {}{unit}; found {value:?}",
            m.lo, m.hi
        )
    })
}

fn border(edge: Edge, value: &str) -> Result<Declaration, String> {
    let parts: Vec<&str> = value.split_whitespace().collect();
    let [width, style, tint] = parts.as_slice() else {
        return Err(format!(
            "a border takes all three of width, style and colour, e.g. `1px solid #3f0421`; found {value:?}"
        ));
    };
    let Some((line, _, _)) = BORDER_LINES
        .iter()
        .find(|(_, w, s)| w == width && s == style)
    else {
        return Err(format!("no border edge is `{width} {style}`"));
    };
    Ok(Declaration::Border {
        edge,
        border: Border {
            line: *line,
            color: color(tint)?,
        },
    })
}

fn color(value: &str) -> Result<Rgb, String> {
    let Some(hex) = value.strip_prefix('#') else {
        return Err(not_a_colour(value));
    };
    match hex.len() {
        6 => parse_hex6(hex).ok_or_else(|| not_a_colour(value)),
        3 => expand_hex3(hex).ok_or_else(|| not_a_colour(value)),
        _ => Err(not_a_colour(value)),
    }
}

fn parse_hex6(hex: &str) -> Option<Rgb> {
    Some(Rgb {
        r: u8::from_str_radix(hex.get(0..2)?, 16).ok()?,
        g: u8::from_str_radix(hex.get(2..4)?, 16).ok()?,
        b: u8::from_str_radix(hex.get(4..6)?, 16).ok()?,
    })
}

fn expand_hex3(hex: &str) -> Option<Rgb> {
    let doubled: String = hex.chars().flat_map(|c| [c, c]).collect();
    parse_hex6(&doubled)
}

fn not_a_colour(value: &str) -> String {
    format!("{value:?} is not a colour; a colour is written `#rrggbb` in lowercase hex")
}

/// `background` is only a spelling of `background-color` when it carries a bare colour; carrying a
/// gradient or an image it is a property with no counterpart at all.
fn background_shorthand(value: &str) -> Result<Declaration, String> {
    match color(value) {
        Ok(rgb) => Ok(Declaration::BackgroundColor(rgb)),
        Err(why) => Err(format!("`background` carries a colour alone: {why}")),
    }
}

fn frame(message: &str) -> DeclFault {
    DeclFault::Frame(Code::PresentationSyntax, message.to_string())
}

pub(crate) fn syntax(message: &str) -> (Code, String) {
    (Code::PresentationSyntax, message.to_string())
}
