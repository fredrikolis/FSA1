// Concern: resolves the CellStyle in force at one coordinate, and the one a cell declaring nothing wears | Non-concern: which blocks reach a coordinate (overlay.rs) | IO: (&Presentation, r, c) -> Style

use crate::declaration::{
    Border, Chars, Declaration, Edge, FontStyle, FontWeight, Points, Rgb, TextAlign,
    TextDecoration, VerticalAlign, WhiteSpace,
};
use crate::presentation::{Presentation, Rule, Target};

/// The face a cell declaring no `font-family` is drawn in, and the size one declaring no `font-size`
/// is drawn at. The format has no place to record a SOURCE workbook's own Normal font, so these are
/// what a writer restores — and therefore the only baseline a value may be left undeclared against.
pub const DEFAULT_FONT_FAMILY: &str = "Calibri";
pub const DEFAULT_FONT_SIZE: Points = Points(11.0);

/// Text 1 of the Office theme every FSA1 export carries, which is what a cell declaring no `color` is
/// drawn in.
const DEFAULT_COLOR: Rgb = Rgb { r: 0, g: 0, b: 0 };

/// Whether a look shows on a cell holding NO value: a fill covers the cell's area and an edge draws
/// its boundary, while a font, a size, a colour or an alignment needs text before it shows anything.
/// Both legs of the .xlsx crossing read this ONE answer — the read leg to call such a cell content,
/// the write leg to carry every cell that is — and [`BlankPaint::of`] is the only way to build one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlankPaint {
    pub filled: bool,
    pub edged: bool,
}

impl BlankPaint {
    /// The look `declarations` show on an empty cell. A [`Declaration`] is the ONLY input, which is
    /// what makes the two legs agree by construction: each feeds in what it can spell, so an
    /// appearance no declaration carries cannot answer `true` on either. Exhaustive over the
    /// vocabulary, so a property added later cannot reach a cell without answering this first.
    pub fn of(declarations: impl IntoIterator<Item = Declaration>) -> BlankPaint {
        let mut paint = BlankPaint::default();
        for declaration in declarations {
            match declaration {
                Declaration::BackgroundColor(_) => paint.filled = true,
                Declaration::Border { .. } => paint.edged = true,
                Declaration::Color(_)
                | Declaration::FontFamily(_)
                | Declaration::FontSize(_)
                | Declaration::FontStyle(_)
                | Declaration::FontWeight(_)
                | Declaration::Height(_)
                | Declaration::TextAlign(_)
                | Declaration::TextDecoration(_)
                | Declaration::VerticalAlign(_)
                | Declaration::WhiteSpace(_)
                | Declaration::Width(_) => {}
            }
        }
        paint
    }

    pub fn shows(self) -> bool {
        self.filled || self.edged
    }
}

/// One `Option` per property [`Declaration`] can carry. `None` is UNDECLARED — no rule named the
/// property — which is a different fact from a declared default like `font-weight: normal`, so a
/// consumer emits only what an author actually asked for.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CellStyle {
    pub background_color: Option<Rgb>,
    pub border_top: Option<Border>,
    pub border_bottom: Option<Border>,
    pub border_left: Option<Border>,
    pub border_right: Option<Border>,
    pub color: Option<Rgb>,
    pub font_family: Option<String>,
    pub font_size: Option<Points>,
    pub font_style: Option<FontStyle>,
    pub font_weight: Option<FontWeight>,
    /// The height of the ROW this coordinate sits in, and `width` the width of its COLUMN: an axis
    /// carries one size for every cell on it, so these two say the same thing at each of them.
    pub height: Option<Points>,
    pub text_align: Option<TextAlign>,
    pub text_decoration: Option<TextDecoration>,
    pub vertical_align: Option<VerticalAlign>,
    pub white_space: Option<WhiteSpace>,
    pub width: Option<Chars>,
}

impl CellStyle {
    pub fn blank_paint(&self) -> BlankPaint {
        BlankPaint::of(self.declarations())
    }

    /// The declarations in force, in the alphabetical-by-property order the format writes them in.
    /// [`CellStyle::apply`] read backward, so a consumer emitting a style cannot spell a property the
    /// parser would refuse.
    pub fn declarations(&self) -> Vec<Declaration> {
        let edge = |edge, border: Option<Border>| {
            border.map(|border| Declaration::Border { edge, border })
        };
        [
            self.background_color.map(Declaration::BackgroundColor),
            edge(Edge::Bottom, self.border_bottom),
            edge(Edge::Left, self.border_left),
            edge(Edge::Right, self.border_right),
            edge(Edge::Top, self.border_top),
            self.color.map(Declaration::Color),
            self.font_family.clone().map(Declaration::FontFamily),
            self.font_size.map(Declaration::FontSize),
            self.font_style.map(Declaration::FontStyle),
            self.font_weight.map(Declaration::FontWeight),
            self.height.map(Declaration::Height),
            self.text_align.map(Declaration::TextAlign),
            self.text_decoration.map(Declaration::TextDecoration),
            self.vertical_align.map(Declaration::VerticalAlign),
            self.white_space.map(Declaration::WhiteSpace),
            self.width.map(Declaration::Width),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// One block's match laid over what earlier blocks left, property by property: `over` wins every
    /// property it declares and takes back none it does not. Routed through the declaration
    /// vocabulary rather than field by field, so a property added later cannot miss the cascade.
    pub fn layer(&mut self, over: &CellStyle) {
        for declaration in over.declarations() {
            self.apply(&declaration);
        }
    }

    /// Exhaustive by construction, so a property added later cannot reach a rule without landing here.
    fn apply(&mut self, declaration: &Declaration) {
        match declaration {
            Declaration::BackgroundColor(rgb) => self.background_color = Some(*rgb),
            Declaration::Border { edge, border } => match edge {
                Edge::Top => self.border_top = Some(*border),
                Edge::Bottom => self.border_bottom = Some(*border),
                Edge::Left => self.border_left = Some(*border),
                Edge::Right => self.border_right = Some(*border),
            },
            Declaration::Color(rgb) => self.color = Some(*rgb),
            Declaration::FontFamily(name) => self.font_family = Some(name.clone()),
            Declaration::FontSize(points) => self.font_size = Some(*points),
            Declaration::FontStyle(style) => self.font_style = Some(*style),
            Declaration::FontWeight(weight) => self.font_weight = Some(*weight),
            Declaration::Height(points) => self.height = Some(*points),
            Declaration::TextAlign(align) => self.text_align = Some(*align),
            Declaration::TextDecoration(decoration) => self.text_decoration = Some(*decoration),
            Declaration::VerticalAlign(align) => self.vertical_align = Some(*align),
            Declaration::WhiteSpace(mode) => self.white_space = Some(*mode),
            Declaration::Width(chars) => self.width = Some(*chars),
        }
    }
}

/// The whole appearance a cell wearing NO declaration renders as. Every leg reads it: the one that
/// may leave a value undeclared measures against it, and the one that writes an .xlsx restores it.
/// A property left `None` here has none to restore — an undeclared fill, edge or alignment is simply
/// not drawn, and an undeclared width or height is the axis's own.
pub fn default_style() -> CellStyle {
    CellStyle {
        color: Some(DEFAULT_COLOR),
        font_family: Some(DEFAULT_FONT_FAMILY.to_string()),
        font_size: Some(DEFAULT_FONT_SIZE),
        font_style: Some(FontStyle::Normal),
        font_weight: Some(FontWeight::Normal),
        text_decoration: Some(TextDecoration::None),
        vertical_align: Some(VerticalAlign::Bottom),
        white_space: Some(WhiteSpace::Nowrap),
        ..CellStyle::default()
    }
}

/// CSS specificity, which is NOT [`Target`]'s `Ord`: `tr:nth-child(r) td` is two compound selectors
/// (0,1,2) where `td:nth-child(c)` is one (0,1,1), so a row rule outranks a column rule. A periodic
/// form TIES with its axis's literal, both being one pseudo-class, so CSS breaks that on source
/// order — and the writing order puts the periodic first, which giving it the lower slot IS.
fn specificity(target: Target) -> u8 {
    match target {
        Target::All => 0,
        Target::ColEvery { .. } => 1,
        Target::Col(_) => 2,
        Target::RowEvery { .. } => 3,
        Target::Row(_) => 4,
        Target::Cell { .. } => 5,
    }
}

/// `row` and `col` are 1-based and region-relative, the basis `:nth-child(k)` counts in. At most six
/// rules reach a coordinate, a duplicate target being refused at parse; they apply in ascending
/// [`specificity`], each overwriting property by property.
pub fn resolve(presentation: &Presentation, row: u32, col: u32) -> CellStyle {
    debug_assert!(
        presentation
            .rules
            .windows(2)
            .all(|w| w[0].target < w[1].target),
        "a parsed presentation's rules are strictly ascending by target",
    );
    // The slot IS the specificity, so the cascade order has one home rather than a second hand-kept ordering here.
    let mut matched: [Option<&Rule>; 6] = [None; 6];
    for target in [
        Target::All,
        Target::Row(row),
        Target::Col(col),
        Target::Cell { row, col },
    ] {
        if let Ok(i) = presentation
            .rules
            .binary_search_by(|rule| rule.target.cmp(&target))
        {
            matched[specificity(target) as usize] = Some(&presentation.rules[i]);
        }
    }
    // No search for THIS line finds a rule selecting many; the run sits after `All`, last wins.
    for rule in &presentation.rules {
        match rule.target {
            Target::All => {}
            Target::RowEvery { a, b } | Target::ColEvery { a, b } => {
                let line = if matches!(rule.target, Target::RowEvery { .. }) {
                    row
                } else {
                    col
                };
                if line % a == b {
                    matched[specificity(rule.target) as usize] = Some(rule);
                }
            }
            _ => break,
        }
    }
    let mut style = CellStyle::default();
    for rule in matched.into_iter().flatten() {
        for declaration in &rule.declarations {
            style.apply(declaration);
        }
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse_filename, parse_rules};

    fn presentation_over(root: &str, rules: &str) -> Presentation {
        let region = parse_filename(root).expect("a root names a range").region;
        parse_rules(&format!("Sheet1/{root}.css"), region, &format!("{rules}\n"))
            .unwrap_or_else(|d| panic!("{rules:?} should parse: {:?}", d[0]))
    }

    /// Four rows, so `2n` reaches TWO of them and stays periodic rather than folding to a literal.
    fn tall_presentation_of(rules: &str) -> Presentation {
        presentation_over("A1:C4", rules)
    }

    fn presentation_of(rules: &str) -> Presentation {
        presentation_over("A1:C3", rules)
    }

    /// The row-over-column cascade is CSS specificity and predates the periodic forms; widening the
    /// slot array must not quietly reorder it.
    #[test]
    fn a_row_rule_still_outranks_a_column_rule() {
        let p = presentation_of(
            "  tr:nth-child(2) td { color: #ff0000 }\n  td:last-child { color: #0000ff }",
        );
        assert_eq!(resolve(&p, 2, 3).color, Some(Rgb { r: 255, g: 0, b: 0 }));
    }

    /// A periodic index and a literal one are both a single pseudo-class, so CSS ties them and breaks
    /// the tie on source order — and the canonical writing order puts the periodic first.
    #[test]
    fn a_literal_index_wins_its_tie_with_the_periodic_it_overlaps() {
        let p = tall_presentation_of(
            "  td { color: #000000 }\n  tr:nth-child(2n) td { color: #00ff00 }\n  \
             tr:nth-child(2) td { color: #ff0000 }",
        );
        assert_eq!(resolve(&p, 2, 1).color, Some(Rgb { r: 255, g: 0, b: 0 }));
        assert_eq!(resolve(&p, 3, 1).color, Some(Rgb { r: 0, g: 0, b: 0 }));
    }

    /// The whole point of the form: one rule reaching every line it names, at any distance from it.
    #[test]
    fn a_periodic_rule_reaches_every_line_it_names() {
        let p = tall_presentation_of("  tr:nth-child(2n) td { font-weight: bold }");
        assert_eq!(resolve(&p, 2, 1).font_weight, Some(FontWeight::Bold));
        assert_eq!(resolve(&p, 4, 1).font_weight, Some(FontWeight::Bold));
        assert_eq!(resolve(&p, 1, 1).font_weight, None);
        assert_eq!(resolve(&p, 3, 1).font_weight, None);
    }

    /// A bare `td` is the whole scoping root, never its perimeter: what an author writing one rule for
    /// a region is promised.
    #[test]
    fn a_bare_td_styles_every_cell_of_the_range() {
        let p = presentation_of("  td { font-weight: bold }");
        for row in 1..=3 {
            for col in 1..=3 {
                assert_eq!(
                    resolve(&p, row, col).font_weight,
                    Some(FontWeight::Bold),
                    "({row},{col})"
                );
            }
        }
    }

    /// The path a presentation consumer walks — a sidecar read to rules, resolved, spelled back — over
    /// every property at once. The on-disk syntax IS CSS, so what an emitter writes is the text the
    /// author wrote, in the order the format canonicalizes.
    #[test]
    fn every_property_spells_back_to_the_text_it_was_written_from() {
        const DECLARATIONS: &str = "background-color: #ffffff; border-bottom: 1px solid #3f0421; \
             border-left: 2px dashed #3f0421; border-right: 3px double #3f0421; \
             border-top: 1px dotted #3f0421; color: #3f0421; font-family: Times New Roman; \
             font-size: 11pt; font-style: italic; font-weight: bold; height: 22.5pt; \
             text-align: right; text-decoration: line-through; vertical-align: middle; \
             white-space: nowrap; width: 14.5ch";
        let p = presentation_of(&format!("  td {{ {DECLARATIONS} }}"));
        let spelled: Vec<String> = resolve(&p, 1, 1)
            .declarations()
            .iter()
            .map(Declaration::spell)
            .collect();
        assert_eq!(spelled.join("; "), DECLARATIONS);
    }

    /// The CSS reading of the two selectors, which the canonical WRITING order reverses; a
    /// "simplification" back onto [`Target`]'s `Ord` fails here.
    #[test]
    fn a_row_rule_outranks_a_column_rule_where_both_match() {
        let p = presentation_of(
            "  tr:nth-child(2) td { color: #3f0421 }\n  td:nth-child(2) { color: #ffffff }",
        );
        let plum = Rgb {
            r: 0x3f,
            g: 0x04,
            b: 0x21,
        };
        let white = Rgb {
            r: 0xff,
            g: 0xff,
            b: 0xff,
        };
        assert_eq!(resolve(&p, 2, 2).color, Some(plum));
        assert_eq!(resolve(&p, 1, 2).color, Some(white));
    }
}
