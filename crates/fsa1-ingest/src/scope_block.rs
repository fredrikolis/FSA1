// Concern: encodes what a block's styles cross as, names what they cannot, and assigns each axis to one block | Non-concern: cutting the blocks | IO: (sheet, blocks) -> geometry, rules, warnings

use std::collections::{BTreeMap, HashMap};

use fsa1_ast::a1::{format_cell, format_column};
use fsa1_model::{
    BlankPaint, Border, BorderLine, Chars, Declaration, Edge, FontStyle, FontWeight, Points,
    Presentation, Rgb, Rule, Target, TextAlign, TextDecoration, VerticalAlign, WhiteSpace,
};

use crate::decompose::Block;
use crate::source::SheetSource;
use crate::warnings::{Axis, UnpackWarning, unowned};
use crate::xlsx_style::{
    BorderStyle, FillPattern, HorizontalAlign, StyleTable, Underline,
    VerticalAlign as XlsxVerticalAlign, XlsxBorder, XlsxFont, XlsxStyle,
};

/// A block's canonical order, `r0, c0, r1, c1` ascending.
pub fn key(block: &Block) -> (u32, u32, u32, u32) {
    (
        block.row,
        block.col,
        block.row + block.rows,
        block.col + block.cols,
    )
}

/// The sheet axes ONE block sizes, block-relative and 1-based. Every file whose own range contains
/// an axis would state the same number, so which of them states it is a canonicity question.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockGeometry {
    pub widths: Vec<(u32, Chars)>,
    pub heights: Vec<(u32, Points)>,
}

/// Hands each authored width and height to the one block that will write it, and names the axes no
/// block can — an axis lying outside the sheet's whole occupancy. `blocks` come in canonical order,
/// which is the tie-break when two of them carry equally much of the axis. The unowned axes go to
/// [`unowned`] whole: a height over a row range no block reaches is ONE authored fact, not one a row.
pub fn assign_geometry(
    sheet: &SheetSource,
    blocks: &[Block],
    warnings: &mut Vec<UnpackWarning>,
) -> Vec<BlockGeometry> {
    debug_assert!(
        blocks.windows(2).all(|w| key(&w[0]) < key(&w[1])),
        "assign_geometry breaks its ties by the order it is given",
    );
    let mut out = vec![BlockGeometry::default(); blocks.len()];
    let mut unowned_columns = Vec::new();
    for (&col, &width) in &sheet.col_widths {
        let Some(size) = Chars::column_width(width) else {
            warnings.push(UnpackWarning::ColumnWidthUnspellable {
                sheet: sheet.name.clone(),
                column: format_column(col),
                width: width.to_string(),
            });
            continue;
        };
        let owner = owner(
            blocks,
            |b| holds(b.col, b.cols, col),
            |b| {
                (b.row..b.row + b.rows)
                    .filter(|r| sheet.is_occupied(col, r - 1))
                    .count()
            },
        );
        match owner {
            Some(i) => out[i].widths.push((col + 2 - blocks[i].col, size)),
            None => unowned_columns.push((col, col)),
        }
    }
    warnings.extend(unowned(Axis::Column, &sheet.name, &unowned_columns));
    let mut unowned_rows = Vec::new();
    for (&row, &height) in &sheet.row_heights {
        let Some(size) = Points::row_height(height) else {
            warnings.push(UnpackWarning::RowHeightUnspellable {
                sheet: sheet.name.clone(),
                row: row + 1,
                height: height.to_string(),
            });
            continue;
        };
        let owner = owner(
            blocks,
            |b| holds(b.row, b.rows, row),
            |b| {
                (b.col..b.col + b.cols)
                    .filter(|c| sheet.is_occupied(c - 1, row))
                    .count()
            },
        );
        match owner {
            Some(i) => out[i].heights.push((row + 2 - blocks[i].row, size)),
            None => unowned_rows.push((row, row)),
        }
    }
    warnings.extend(unowned(Axis::Row, &sheet.name, &unowned_rows));
    out
}

/// The 0-based sheet axis `axis` against a block's 1-based anchor and extent.
fn holds(anchor: u32, extent: u32, axis: u32) -> bool {
    (anchor..anchor + extent).contains(&(axis + 1))
}

/// The containing block carrying most of the axis's occupancy; ties keep the earliest, `min_by_key`
/// returning the first of equal keys where `max_by_key` returns the last.
fn owner(
    blocks: &[Block],
    holds: impl Fn(&Block) -> bool,
    carries: impl Fn(&Block) -> usize,
) -> Option<usize> {
    (0..blocks.len())
        .filter(|&i| holds(&blocks[i]))
        .min_by_key(|&i| std::cmp::Reverse(carries(&blocks[i])))
}

/// The rules a block's own cells and its owned axes earn. `None` where they earn none: an empty
/// block is not written.
pub fn encode(sheet: &SheetSource, block: Block, geometry: &BlockGeometry) -> Option<Presentation> {
    let ctx = Ctx::of(&sheet.styles);
    let restored = fsa1_model::default_style().declarations();
    let mut rules: BTreeMap<Target, Vec<Declaration>> = BTreeMap::new();
    for property in PROPERTIES {
        encode_property(sheet, block, &ctx, &restored, property, &mut rules);
    }
    match modal_size(block.cols, &geometry.widths) {
        Some(size) => place(&mut rules, Target::All, Declaration::Width(size)),
        None => {
            debug_assert!(
                block.cols > 1 || geometry.widths.is_empty(),
                "one column is the whole block, so its width is always modal",
            );
            for &(col, size) in &geometry.widths {
                place(&mut rules, Target::Col(col), Declaration::Width(size));
            }
        }
    }
    match modal_size(block.rows, &geometry.heights) {
        Some(size) => place(&mut rules, Target::All, Declaration::Height(size)),
        None => {
            debug_assert!(
                block.rows > 1 || geometry.heights.is_empty(),
                "one row is the whole block, so its height is always modal",
            );
            for &(row, size) in &geometry.heights {
                place(&mut rules, Target::Row(row), Declaration::Height(size));
            }
        }
    }
    if rules.is_empty() {
        return None;
    }
    Some(Presentation {
        rules: rules
            .into_iter()
            .map(|(target, mut declarations)| {
                declarations.sort_by_key(Declaration::property);
                Rule {
                    target,
                    declarations,
                }
            })
            .collect(),
    })
}

/// The one size a bare `td` may state for the whole block: EVERY axis of it sized, and all sizes
/// agreeing. A `td` over a block holding one unsized axis would fabricate a size for that axis, which
/// no finer rule can take back — the same hazard that keeps the modal rule off an unspellable default.
/// It also subsumes the extent-1 case, where the block's one axis carries no selector of its own.
fn modal_size<T: Copy + PartialEq>(extent: u32, sizes: &[(u32, T)]) -> Option<T> {
    let (first, rest) = sizes.split_first()?;
    (sizes.len() == extent as usize && rest.iter().all(|(_, size)| *size == first.1))
        .then_some(first.1)
}

fn place(rules: &mut BTreeMap<Target, Vec<Declaration>>, target: Target, declaration: Declaration) {
    rules.entry(target).or_default().push(declaration);
}

/// The two workbook-wide facts a cell's own style is read against: the Normal font, which is what the
/// SOURCE says a cell carrying no style wears, and the document's own default text colour. Neither is
/// the baseline a value may be left undeclared against — that is [`fsa1_model::default_style`], which
/// the write leg restores and this leg therefore measures against.
struct Ctx<'a> {
    normal: &'a XlsxFont,
    default_text: Option<Rgb>,
}

impl<'a> Ctx<'a> {
    fn of(styles: &'a StyleTable) -> Ctx<'a> {
        Ctx {
            normal: styles.normal_font(),
            default_text: styles.default_text_color(),
        }
    }
}

/// Whether the style at `index` shows on a cell holding no value — the whole of what makes such a cell
/// content. Read off the declarations THIS encoder spells for the style, never off the raw
/// [`XlsxStyle`]: an appearance no [`Property`] carries — a hatch, a `<diagonal>`, a colourless solid
/// fill — is dropped and named, so it must not mint occupancy the export has no way to carry back.
pub fn paints_blank(styles: &StyleTable, index: u32) -> bool {
    let ctx = Ctx::of(styles);
    styles
        .get(index)
        .is_some_and(|style| blank_paint(&ctx, style).shows())
}

fn blank_paint(ctx: &Ctx<'_>, style: &XlsxStyle) -> BlankPaint {
    BlankPaint::of(PROPERTIES.iter().filter_map(|p| (p.of)(ctx, style)))
}

/// One property read off the source two ways: what a cell stating a style wears, and what a cell
/// stating none wears. A `bare` of `None` is a property an unstyled cell has NO CSS spelling for, so
/// a coarser rule must never assert over a cell that has it.
struct Property {
    of: fn(&Ctx<'_>, &XlsxStyle) -> Option<Declaration>,
    bare: fn(&Ctx<'_>) -> Option<Declaration>,
}

/// In measured prevalence order — the first buys the most visible fidelity — which is NOT the order
/// the rules are written in; that is [`Target`]'s own.
const PROPERTIES: &[Property] = &[
    Property {
        of: font_size,
        bare: normal_font_size,
    },
    Property {
        of: font_family,
        bare: normal_font_family,
    },
    Property {
        of: text_align,
        bare: |_| None,
    },
    Property {
        of: |_, style| Some(italic(style.font.italic)),
        bare: |ctx| Some(italic(ctx.normal.italic)),
    },
    Property {
        of: vertical_align,
        bare: |_| Some(Declaration::VerticalAlign(VerticalAlign::Bottom)),
    },
    Property {
        of: |_, style| Some(bold(style.font.bold)),
        bare: |ctx| Some(bold(ctx.normal.bold)),
    },
    Property {
        of: white_space,
        bare: |_| Some(Declaration::WhiteSpace(WhiteSpace::Nowrap)),
    },
    Property {
        of: background_color,
        bare: |_| None,
    },
    Property {
        of: |_, style| edge(Edge::Bottom, style.border_bottom),
        bare: |_| None,
    },
    Property {
        of: |_, style| edge(Edge::Left, style.border_left),
        bare: |_| None,
    },
    Property {
        of: |_, style| edge(Edge::Right, style.border_right),
        bare: |_| None,
    },
    Property {
        of: |_, style| edge(Edge::Top, style.border_top),
        bare: |_| None,
    },
    Property {
        of: color,
        bare: normal_color,
    },
    Property {
        of: |_, style| Some(decoration(&style.font)),
        bare: |ctx| Some(decoration(ctx.normal)),
    },
];

/// The declaration the write leg puts back where `like`'s property is declared nowhere, read off the
/// ONE default appearance [`fsa1_model::default_style`] states. A value equal to it is the only value
/// a rule may leave unwritten; every other one is lost unless some rule carries it.
fn restored<'a>(default: &'a [Declaration], like: &Declaration) -> Option<&'a Declaration> {
    default.iter().find(|d| d.property() == like.property())
}

/// Resolution cascades All -> Col -> Row -> Cell, each rule overwriting property by property, so
/// each property is encoded over the block ALONE: the modal value, a column rule per column uniform
/// in another, a row rule per row uniform and not already right — rows applying after columns — and
/// a cell rule for each coordinate those three leave wrong.
fn encode_property(
    sheet: &SheetSource,
    block: Block,
    ctx: &Ctx<'_>,
    default: &[Declaration],
    property: &Property,
    rules: &mut BTreeMap<Target, Vec<Declaration>>,
) {
    let (rows, cols) = (block.rows as usize, block.cols as usize);
    let bare = (property.bare)(ctx);
    let values: Vec<Option<Declaration>> = (0..rows * cols)
        .map(|i| {
            sheet
                .style_at(
                    block.col - 1 + (i % cols) as u32,
                    block.row - 1 + (i / cols) as u32,
                )
                .and_then(|style| (property.of)(ctx, style))
                .or_else(|| bare.clone())
        })
        .collect();
    // With an unspellable default in play a modal value would assert over cells no finer rule could then take back.
    let modal = if values.contains(&None) {
        None
    } else {
        most_common(&values)
    };

    let col_rules: Vec<Option<Declaration>> = (0..cols)
        .map(
            |c| match uniform(&values, (0..rows).map(|r| r * cols + c)) {
                Some(value) if value != modal => value,
                _ => None,
            },
        )
        .collect();
    let after_columns = |c: usize| col_rules[c].as_ref().or(modal.as_ref());
    let row_rules: Vec<Option<Declaration>> = (0..rows)
        .map(
            |r| match uniform(&values, (0..cols).map(|c| r * cols + c)) {
                Some(value) if (0..cols).any(|c| after_columns(c) != value.as_ref()) => value,
                _ => None,
            },
        )
        .collect();

    if let Some(declaration) = modal.clone()
        && restored(default, &declaration) != Some(&declaration)
    {
        place(rules, Target::All, declaration);
    }
    let col_periodic = periodic_runs(&col_rules);
    for &(a, b, ref declaration) in &col_periodic.runs {
        place(rules, Target::ColEvery { a, b }, declaration.clone());
    }
    for (c, value) in col_rules.iter().enumerate() {
        if col_periodic.covered[c] {
            continue;
        }
        if let Some(declaration) = value.clone() {
            debug_assert!(cols > 1, "one column is the whole block, never a Col rule");
            place(rules, Target::Col(c as u32 + 1), declaration);
        }
    }
    let row_periodic = periodic_runs(&row_rules);
    for &(a, b, ref declaration) in &row_periodic.runs {
        place(rules, Target::RowEvery { a, b }, declaration.clone());
    }
    for (r, value) in row_rules.iter().enumerate() {
        if row_periodic.covered[r] {
            continue;
        }
        if let Some(declaration) = value.clone() {
            debug_assert!(rows > 1, "one row is the whole block, never a Row rule");
            place(rules, Target::Row(r as u32 + 1), declaration);
        }
    }
    for r in 0..rows {
        for c in 0..cols {
            let effective = row_rules[r]
                .as_ref()
                .or(col_rules[c].as_ref())
                .or(modal.as_ref());
            let want = &values[r * cols + c];
            if effective == want.as_ref() {
                continue;
            }
            // Unset here needs an unspellable default, which forces the modal unset, which no uniform row or column over the cell can then have overridden.
            debug_assert!(want.is_some(), "no rule restores an unspellable default");
            debug_assert!(
                rows > 1 && cols > 1,
                "an axis of extent 1 spells no cell rule"
            );
            if let Some(declaration) = want.clone() {
                place(
                    rules,
                    Target::Cell {
                        row: r as u32 + 1,
                        col: c as u32 + 1,
                    },
                    declaration,
                );
            }
        }
    }
}

/// Which lines a periodic rule takes over, and which are left to spell themselves.
struct Periodic {
    runs: Vec<(u32, u32, Declaration)>,
    covered: Vec<bool>,
}

/// Collapses ONE property's per-line rules into `An+B`. Admitted only where the lines carrying a
/// declaration are EXACTLY a congruence class, never merely inside one, since a periodic rule
/// reaches every line it names. A size is skipped: it is refused on a periodic selector, so
/// collapsing one would write what `check` then rejects.
fn periodic_runs(values: &[Option<Declaration>]) -> Periodic {
    let extent = values.len() as u32;
    let mut runs = Vec::new();
    let mut covered = vec![false; values.len()];
    let mut examined = vec![false; values.len()];
    for start in 0..values.len() {
        let Some(declaration) = values[start].clone() else {
            continue;
        };
        if examined[start] || matches!(declaration, Declaration::Width(_) | Declaration::Height(_))
        {
            continue;
        }
        let lines: Vec<u32> = (0..values.len())
            .filter(|&j| values[j].as_ref() == Some(&declaration))
            .map(|j| j as u32 + 1)
            .collect();
        for line in &lines {
            examined[(line - 1) as usize] = true;
        }
        if lines.len() < 3 {
            continue;
        }
        // An exact class is evenly spaced, so its gap IS the only period that can fit.
        let a = lines[1] - lines[0];
        let b = lines[0] % a;
        if (1..=extent)
            .filter(|line| line % a == b)
            .eq(lines.iter().copied())
        {
            for line in &lines {
                covered[(line - 1) as usize] = true;
            }
            runs.push((a, b, declaration));
        }
    }
    Periodic { runs, covered }
}

/// The one value every named cell shares, or `None` where they do not all share one.
fn uniform(
    values: &[Option<Declaration>],
    mut cells: impl Iterator<Item = usize>,
) -> Option<Option<Declaration>> {
    let first = &values[cells.next()?];
    cells.all(|i| &values[i] == first).then(|| first.clone())
}

/// The value the most cells carry; ties keep the one appearing first, so the block's own reading
/// order decides and the output is deterministic. Keyed by the canonical CSS spelling, the one
/// identity every value already has.
fn most_common(values: &[Option<Declaration>]) -> Option<Declaration> {
    let mut counts: HashMap<String, (usize, usize)> = HashMap::new();
    for (i, value) in values.iter().enumerate() {
        let key = value.as_ref().map(Declaration::spell).unwrap_or_default();
        counts.entry(key).or_insert((0, i)).0 += 1;
    }
    let (_, (_, first)) = counts
        .iter()
        .max_by_key(|(_, (count, first))| (*count, std::cmp::Reverse(*first)))?;
    values[*first].clone()
}

fn font_size(ctx: &Ctx<'_>, style: &XlsxStyle) -> Option<Declaration> {
    points(style.font.size.or(ctx.normal.size)?)
}

fn normal_font_size(ctx: &Ctx<'_>) -> Option<Declaration> {
    points(ctx.normal.size?)
}

fn points(size: f64) -> Option<Declaration> {
    Points::font_size(size).map(Declaration::FontSize)
}

fn font_family(ctx: &Ctx<'_>, style: &XlsxStyle) -> Option<Declaration> {
    family(style.font.name.as_deref().or(ctx.normal.name.as_deref())?)
}

fn normal_font_family(ctx: &Ctx<'_>) -> Option<Declaration> {
    family(ctx.normal.name.as_deref()?)
}

/// The family grammar read from fsa1-model, as [`points`] reads [`Points::font_size`]: a face holding
/// any character a declaration value may not — a quote, a comma, a `:`, a `!`, a line break — has no
/// declaration at all, on either leg, and crosses as a named loss instead.
fn family(name: &str) -> Option<Declaration> {
    Declaration::font_family(name)
}

fn text_align(_: &Ctx<'_>, style: &XlsxStyle) -> Option<Declaration> {
    Some(Declaration::TextAlign(horizontal_of(style.horizontal?)?))
}

/// `None` for the four .xlsx states with no CSS word: `general` is the default, and the other three
/// are named as losses.
fn horizontal_of(align: HorizontalAlign) -> Option<TextAlign> {
    Some(match align {
        HorizontalAlign::Left => TextAlign::Left,
        HorizontalAlign::Center => TextAlign::Center,
        HorizontalAlign::Right => TextAlign::Right,
        HorizontalAlign::Justify => TextAlign::Justify,
        HorizontalAlign::General
        | HorizontalAlign::Fill
        | HorizontalAlign::CenterContinuous
        | HorizontalAlign::Distributed => return None,
    })
}

fn vertical_align(_: &Ctx<'_>, style: &XlsxStyle) -> Option<Declaration> {
    Some(Declaration::VerticalAlign(vertical_of(style.vertical?)?))
}

fn vertical_of(align: XlsxVerticalAlign) -> Option<VerticalAlign> {
    Some(match align {
        XlsxVerticalAlign::Top => VerticalAlign::Top,
        XlsxVerticalAlign::Center => VerticalAlign::Middle,
        XlsxVerticalAlign::Bottom => VerticalAlign::Bottom,
        XlsxVerticalAlign::Justify | XlsxVerticalAlign::Distributed => return None,
    })
}

fn italic(set: bool) -> Declaration {
    Declaration::FontStyle(if set {
        FontStyle::Italic
    } else {
        FontStyle::Normal
    })
}

fn bold(set: bool) -> Declaration {
    Declaration::FontWeight(if set {
        FontWeight::Bold
    } else {
        FontWeight::Normal
    })
}

/// A cell wraps only where asked to, so `nowrap` is what an unstyled cell already is.
fn white_space(_: &Ctx<'_>, style: &XlsxStyle) -> Option<Declaration> {
    Some(Declaration::WhiteSpace(if style.wrap_text {
        WhiteSpace::Normal
    } else {
        WhiteSpace::Nowrap
    }))
}

/// Only a SOLID fill paints a colour CSS can carry; a hatch or a gradient is named as a loss and
/// left unpainted rather than flattened to one of its two colours.
fn background_color(_: &Ctx<'_>, style: &XlsxStyle) -> Option<Declaration> {
    (style.fill.pattern == FillPattern::Solid)
        .then_some(style.fill.fg)
        .flatten()
        .map(Declaration::BackgroundColor)
}

/// CSS takes all three of width, style and colour, so an edge whose colour is `auto` — the reading
/// system's own, which resolves to none — is drawn in the black Excel draws it in.
fn edge(edge: Edge, border: Option<XlsxBorder>) -> Option<Declaration> {
    let border = border?;
    Some(Declaration::Border {
        edge,
        border: Border {
            line: line_of(border.style),
            color: border.color.unwrap_or(Rgb { r: 0, g: 0, b: 0 }),
        },
    })
}

/// Whether the CSS pair an edge is drawn as is the edge's OWN, or the nearest one CSS has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fidelity {
    Exact,
    Nearest,
}

/// The ONE table over Excel's thirteen edges: the CSS pair each is drawn as, and whether that pair is
/// its own. The six with none of their own are drawn at the same weight in the nearest family, and
/// [`approximated`] names each one from this same arm — so moving an edge cannot move the line drawn
/// without moving the warning with it.
fn drawn(style: BorderStyle) -> (BorderLine, Fidelity) {
    match style {
        BorderStyle::Thin => (BorderLine::ThinSolid, Fidelity::Exact),
        BorderStyle::Hair => (BorderLine::ThinSolid, Fidelity::Nearest),
        BorderStyle::Medium => (BorderLine::MediumSolid, Fidelity::Exact),
        BorderStyle::Thick => (BorderLine::ThickSolid, Fidelity::Exact),
        BorderStyle::Double => (BorderLine::ThickDouble, Fidelity::Exact),
        BorderStyle::Dotted => (BorderLine::ThinDotted, Fidelity::Exact),
        BorderStyle::Dashed => (BorderLine::ThinDashed, Fidelity::Exact),
        BorderStyle::DashDot | BorderStyle::DashDotDot => {
            (BorderLine::ThinDashed, Fidelity::Nearest)
        }
        BorderStyle::MediumDashed => (BorderLine::MediumDashed, Fidelity::Exact),
        BorderStyle::MediumDashDot | BorderStyle::MediumDashDotDot | BorderStyle::SlantDashDot => {
            (BorderLine::MediumDashed, Fidelity::Nearest)
        }
    }
}

fn line_of(style: BorderStyle) -> BorderLine {
    drawn(style).0
}

/// The colour a cell SHOWS: its own where its font names one, the Normal font's where it does not,
/// and the document's own default text colour where neither does. Whether that is worth a declaration
/// is not this reading's question — it is the one [`restored`] answers.
fn color(ctx: &Ctx<'_>, style: &XlsxStyle) -> Option<Declaration> {
    style
        .font
        .color
        .or(ctx.normal.color)
        .or(ctx.default_text)
        .map(Declaration::Color)
}

fn normal_color(ctx: &Ctx<'_>) -> Option<Declaration> {
    ctx.normal
        .color
        .or(ctx.default_text)
        .map(Declaration::Color)
}

/// ONE enum, never a set: a cell carrying both keeps its underline, and [`name_losses`] names the
/// strikethrough that leaves.
fn decoration(font: &XlsxFont) -> Declaration {
    Declaration::TextDecoration(match (font.underline.is_some(), font.strike) {
        (true, _) => TextDecoration::Underline,
        (false, true) => TextDecoration::LineThrough,
        (false, false) => TextDecoration::None,
    })
}

/// The workbook's Normal font is what a cell stating no style of its own wears, and the format has no
/// place to record it: such a cell crosses wearing it only by DECLARING it. A size or a face the
/// declaration vocabulary cannot spell therefore reaches no cell at all — one loss for the sheet,
/// since every cell of it wears that same one fact.
pub fn name_normal_font_losses(sheet: &SheetSource, warnings: &mut Vec<UnpackWarning>) {
    let normal = sheet.styles.normal_font();
    let mut dropped = Vec::new();
    if let Some(size) = normal.size
        && points(size).is_none()
    {
        dropped.push(format!("size {size}"));
    }
    if let Some(name) = normal.name.as_deref()
        && family(name).is_none()
    {
        dropped.push(format!("family {name:?}"));
    }
    for attribute in dropped {
        warnings.push(UnpackWarning::NormalFontDropped {
            sheet: sheet.name.clone(),
            attribute,
        });
    }
}

/// Every appearance a SHEET's cells state that no rule carries, in reading order. Walked over the
/// sheet rather than over its blocks, so which losses are reported does not depend on where the
/// partition happened to cut: a blank whose only look is a hatch or a `<diagonal>` is by construction
/// not occupancy, so no block ever contains it and a per-block walk would drop it in silence.
pub fn name_losses(sheet: &SheetSource, warnings: &mut Vec<UnpackWarning>) {
    for row in 0..sheet.rows {
        for col in 0..sheet.cols {
            let Some(style) = sheet.style_at(col, row) else {
                continue;
            };
            let at = || (sheet.name.clone(), format_cell(col, row));
            let mut dropped = Vec::new();
            if style.indent > 0 {
                dropped.push(format!("indent level {}", style.indent));
            }
            if style.shrink_to_fit {
                dropped.push("shrink to fit".to_string());
            }
            // `general` is the absence of an alignment rather than one CSS cannot spell.
            if let Some(align) = style.horizontal
                && align != HorizontalAlign::General
                && horizontal_of(align).is_none()
            {
                dropped.push(format!("horizontal alignment {}", align.spell()));
            }
            if let Some(align) = style.vertical
                && vertical_of(align).is_none()
            {
                dropped.push(format!("vertical alignment {}", align.spell()));
            }
            if style.diagonal {
                dropped.push("diagonal border".to_string());
            }
            if style.rotation != 0 {
                dropped.push("text rotation".to_string());
            }
            if let Some(raised) = style.font.vert_align {
                dropped.push(raised.spell().to_string());
            }
            if style.font.outline {
                dropped.push("font outline".to_string());
            }
            if style.font.shadow {
                dropped.push("font shadow".to_string());
            }
            if style.quote_prefix {
                dropped.push("quote prefix".to_string());
            }
            match &style.fill.pattern {
                FillPattern::Other(hatch) if hatch == "gradient" => {
                    dropped.push("gradient fill".to_string());
                }
                FillPattern::Other(hatch) => dropped.push(format!("{hatch} pattern fill")),
                FillPattern::Solid if style.fill.fg.is_none() => {
                    dropped.push("fill colour".to_string());
                }
                FillPattern::None | FillPattern::Solid => {}
            }
            // A size or a face the vocabulary cannot spell can never equal the default that would restore it, so being unspellable IS the loss.
            if let Some(size) = style.font.size
                && points(size).is_none()
            {
                dropped.push(format!("font size {size}"));
            }
            if let Some(name) = style.font.name.as_deref()
                && family(name).is_none()
            {
                dropped.push(format!("font family {name:?}"));
            }
            for attribute in dropped {
                let (sheet, cell) = at();
                warnings.push(UnpackWarning::CellAttributeDropped {
                    sheet,
                    cell,
                    attribute,
                });
            }
            for border in [
                style.border_top,
                style.border_bottom,
                style.border_left,
                style.border_right,
            ]
            .into_iter()
            .flatten()
            {
                if let Some(nearest) = approximated(border.style) {
                    let (sheet, cell) = at();
                    warnings.push(UnpackWarning::BorderStyleApproximated {
                        sheet,
                        cell,
                        style: border.style.spell().to_string(),
                        nearest: nearest.to_string(),
                    });
                }
            }
            if let Some(underline) = style.font.underline
                && underline != Underline::Single
            {
                let (sheet, cell) = at();
                warnings.push(UnpackWarning::UnderlineStyleNarrowed {
                    sheet,
                    cell,
                    style: underline.spell().to_string(),
                });
            }
            if style.font.strike && style.font.underline.is_some() {
                let (sheet, cell) = at();
                warnings.push(UnpackWarning::StrikethroughDropped { sheet, cell });
            }
        }
    }
}

/// The CSS style word an edge with no pair of its own was actually drawn as, or `None` where the pair
/// is the edge's own. Both halves come from [`drawn`] — the arm that chose the line, and
/// `BORDER_LINES` read backward for its word — so the warning can never name a style the edge was not
/// drawn in.
fn approximated(style: BorderStyle) -> Option<&'static str> {
    match drawn(style) {
        (line, Fidelity::Nearest) => Some(line.style_word()),
        (_, Fidelity::Exact) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{SourceCell, SourceValue, StyleTable};
    use crate::xlsx_style::{VertAlign, XlsxFill};

    /// One cell wearing `look`, over a workbook whose Normal style is plain Calibri 11.
    fn one_cell(look: XlsxStyle) -> SheetSource {
        SheetSource {
            name: "S".to_string(),
            rows: 1,
            cols: 1,
            cells: vec![SourceCell {
                value: SourceValue::Number(1.0),
                style: Some(0),
            }],
            styles: StyleTable::of(
                vec![look],
                XlsxFont {
                    name: Some("Calibri".to_string()),
                    size: Some(11.0),
                    ..Default::default()
                },
            ),
            ..Default::default()
        }
    }

    const CELL: Block = Block {
        col: 1,
        row: 1,
        cols: 1,
        rows: 1,
    };

    /// A `rows` x `cols` block of font sizes, one style per distinct size, over a Normal font of
    /// Calibri 11 — so 11 is the size a cell has by declaring nothing.
    fn sizes(rows: u32, cols: u32, points: &[f64]) -> (SheetSource, Block) {
        let mut looks: Vec<f64> = points.to_vec();
        looks.sort_by(f64::total_cmp);
        looks.dedup();
        let mut sheet = one_cell(XlsxStyle::default());
        sheet.rows = rows;
        sheet.cols = cols;
        sheet.cells = points
            .iter()
            .map(|size| SourceCell {
                value: SourceValue::Number(*size),
                style: looks.iter().position(|s| s == size).map(|i| i as u32),
            })
            .collect();
        sheet.styles = StyleTable::of(
            looks
                .iter()
                .map(|size| XlsxStyle {
                    font: XlsxFont {
                        name: Some("Calibri".to_string()),
                        size: Some(*size),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .collect(),
            XlsxFont {
                name: Some("Calibri".to_string()),
                size: Some(11.0),
                ..Default::default()
            },
        );
        (
            sheet,
            Block {
                col: 1,
                row: 1,
                cols,
                rows,
            },
        )
    }

    fn block_of(rows: u32, cols: u32, points: &[f64]) -> String {
        let (sheet, block) = sizes(rows, cols, points);
        let presentation = encode(&sheet, block, &BlockGeometry::default())
            .unwrap_or_else(|| panic!("{points:?} should earn a rule"));
        fsa1_model::spell_block(&presentation, fsa1_ast::Shape { rows, cols })
    }

    /// One property over one block, resolved All -> Col -> Row -> Cell: the modal value carries the
    /// block, a uniform column or row overrides it where it differs, and only what those three leave
    /// wrong costs a cell rule. The rules are WRITTEN all, rows, columns, cells — not that order.
    #[test]
    fn each_property_is_encoded_by_specificity_and_written_canonically() {
        assert_eq!(
            block_of(
                3,
                3,
                &[14.0, 11.0, 11.0, 14.0, 11.0, 11.0, 14.0, 11.0, 11.0]
            ),
            "@scope {\n  td:first-child { font-size: 14pt }\n}",
            "a uniform column against a modal that is the default costs ONE rule",
        );
        assert_eq!(
            block_of(
                3,
                3,
                &[14.0, 14.0, 14.0, 11.0, 11.0, 11.0, 12.0, 12.0, 12.0]
            ),
            "@scope {\n  td { font-size: 14pt }\n  tr:nth-child(2) td { font-size: 11pt }\n  \
             tr:last-child td { font-size: 12pt }\n}",
            "the modal takes the tie by first appearance, and row 1 is already correct",
        );
        assert_eq!(
            block_of(
                3,
                3,
                &[14.0, 14.0, 14.0, 11.0, 11.0, 20.0, 11.0, 11.0, 20.0]
            ),
            "@scope {\n  tr:first-child td { font-size: 14pt }\n  \
             tr:nth-child(2) td:last-child { font-size: 20pt }\n  \
             tr:last-child td:last-child { font-size: 20pt }\n}",
            "a uniform row, then a cell rule for each coordinate the cascade still leaves wrong",
        );
    }

    /// A bare cell WEARS the source workbook's Normal font, but the format has no place to record
    /// WHICH font that was: the leg writing an .xlsx back restores [`fsa1_model::default_style`]'s.
    /// That default — never the source's own Normal — is the only baseline a value may be left
    /// undeclared against, so a workbook unlike it declares its font rather than losing it in silence.
    #[test]
    fn a_normal_font_unlike_the_formats_default_is_declared_not_assumed() {
        let mut sheet = one_cell(XlsxStyle::default());
        sheet.styles = StyleTable::of(
            vec![XlsxStyle::default()],
            XlsxFont {
                name: Some("Arial".to_string()),
                size: Some(9.0),
                ..Default::default()
            },
        );
        let presentation = encode(&sheet, CELL, &BlockGeometry::default())
            .expect("a Normal font unlike the format's default earns a rule");
        assert_eq!(
            fsa1_model::spell_block(&presentation, fsa1_ast::Shape { rows: 1, cols: 1 }),
            "@scope {\n  td { font-family: Arial; font-size: 9pt }\n}",
            "the cell states no style of its own, so the Normal font is all it can be wearing",
        );

        let default = fsa1_model::default_style();
        assert_eq!(default.font_family.as_deref(), Some("Calibri"));
        assert_eq!(default.font_size.map(|pt| pt.0), Some(11.0));
        assert_eq!(
            encode(
                &one_cell(XlsxStyle::default()),
                CELL,
                &BlockGeometry::default()
            ),
            None,
            "a Normal font that IS the format's default is restored unwritten, so it earns nothing",
        );
    }

    /// The OTHER leg of the same rule. A Normal font unlike the format's default must be declared on
    /// the cells, so one the vocabulary cannot spell reaches no cell at all — and a loss it can neither
    /// declare nor name is exactly the silence this whole property exists to break.
    #[test]
    fn a_normal_font_no_declaration_can_spell_is_named_not_dropped_in_silence() {
        let mut sheet = one_cell(XlsxStyle::default());
        sheet.styles = StyleTable::of(
            vec![XlsxStyle::default()],
            XlsxFont {
                name: Some("Arial,Helvetica".to_string()),
                size: Some(0.0),
                ..Default::default()
            },
        );
        let mut warnings = Vec::new();
        name_normal_font_losses(&sheet, &mut warnings);
        assert_eq!(
            warnings,
            vec![
                UnpackWarning::NormalFontDropped {
                    sheet: "S".to_string(),
                    attribute: "size 0".to_string(),
                },
                UnpackWarning::NormalFontDropped {
                    sheet: "S".to_string(),
                    attribute: "family \"Arial,Helvetica\"".to_string(),
                },
            ],
        );
        assert_eq!(
            encode(&sheet, CELL, &BlockGeometry::default()),
            None,
            "and neither half reached a rule, which is what the two lines say",
        );

        let mut clean = Vec::new();
        name_normal_font_losses(&one_cell(XlsxStyle::default()), &mut clean);
        assert!(clean.is_empty(), "a spellable Normal font loses nothing");
    }

    /// The occupancy answer, and the reason it is derived from the encoder rather than re-decided off
    /// the raw style. A typeface needs a glyph; a hatch, a gradient, a `<diagonal>` and a colourless
    /// solid fill all show in Excel but are dropped and named here — so counting any of the five
    /// occupies a coordinate the export comes back WITHOUT, and a whole file's worth of shape is lost.
    #[test]
    fn only_a_look_the_encoder_can_spell_makes_a_valueless_cell_content() {
        let fill = |pattern, fg| XlsxStyle {
            fill: XlsxFill {
                pattern,
                fg,
                bg: None,
            },
            ..Default::default()
        };
        let edge = |style| XlsxStyle {
            border_bottom: Some(XlsxBorder { style, color: None }),
            ..Default::default()
        };
        let blue = Some(Rgb {
            r: 0,
            g: 0xb0,
            b: 0xf0,
        });
        let carried = vec![
            fill(FillPattern::Solid, blue),
            edge(BorderStyle::Thin),
            // Approximated to `dashed` and named as that, but an edge IS still drawn.
            edge(BorderStyle::DashDot),
        ];
        let dropped = vec![
            XlsxStyle {
                font: XlsxFont {
                    name: Some("Arial".to_string()),
                    size: Some(9.0),
                    ..Default::default()
                },
                horizontal: Some(HorizontalAlign::Center),
                wrap_text: true,
                indent: 2,
                rotation: 90,
                shrink_to_fit: true,
                quote_prefix: true,
                ..Default::default()
            },
            fill(FillPattern::Other("gray125".to_string()), None),
            fill(FillPattern::Other("gradient".to_string()), None),
            fill(FillPattern::Solid, None),
            XlsxStyle {
                diagonal: true,
                ..Default::default()
            },
        ];
        let count = carried.len() as u32;
        let table = StyleTable::of(
            carried.into_iter().chain(dropped).collect(),
            XlsxFont::default(),
        );
        for index in 0..count {
            assert!(
                paints_blank(&table, index),
                "{:?} is spelled, so it covers an empty cell",
                table.get(index).unwrap(),
            );
        }
        for index in count..count + 5 {
            assert!(
                !paints_blank(&table, index),
                "{:?} reaches no declaration, so it cannot occupy a coordinate the export drops",
                table.get(index).unwrap(),
            );
        }
        assert!(
            !paints_blank(&table, 99),
            "an index outside the table names no style"
        );
    }

    /// Every file whose own range contains the axis would state the same number, so the choice is
    /// about canonicity: the one carrying most of the axis's occupancy states it, ties by file order.
    #[test]
    fn one_axis_is_sized_by_the_containing_block_that_carries_most_of_it() {
        let (mut sheet, _) = sizes(10, 1, &[11.0; 10]);
        sheet.cells = (0..10)
            .map(|row| SourceCell {
                value: match row {
                    0..=2 | 9 => SourceValue::Number(1.0),
                    _ => SourceValue::Blank,
                },
                style: None,
            })
            .collect();
        sheet.col_widths.insert(0, 12.0);
        let block = |row, rows| Block {
            col: 1,
            row,
            cols: 1,
            rows,
        };

        let mut warnings = Vec::new();
        let owned = assign_geometry(&sheet, &[block(1, 3), block(10, 1)], &mut warnings);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(owned[0].widths, vec![(1, Chars(12.0))], "A1:A3 carries 3");
        assert!(owned[1].widths.is_empty(), "A10 carries 1");

        let tied = assign_geometry(&sheet, &[block(1, 1), block(2, 1)], &mut Vec::new());
        assert_eq!(
            tied[0].widths,
            vec![(1, Chars(12.0))],
            "a tie keeps the first"
        );
        assert!(tied[1].widths.is_empty());
    }

    /// The collapse runs on BOTH axes off one function, so the column leg earns the same reading as
    /// the row leg the corpus freezes.
    #[test]
    fn a_banded_column_run_collapses_to_one_periodic_rule() {
        assert_eq!(
            block_of(1, 6, &[14.0, 11.0, 14.0, 11.0, 14.0, 11.0]),
            "@scope {\n  td { font-size: 14pt }\n  td:nth-child(2n) { font-size: 11pt }\n}",
        );
    }

    /// Two lines are the shape a literal pair already spells at the same cost, so the collapse buys
    /// nothing there and the floor keeps it from claiming a period off a coincidence.
    #[test]
    fn two_alike_lines_stay_literal() {
        assert_eq!(
            block_of(1, 4, &[14.0, 11.0, 14.0, 11.0]),
            "@scope {\n  td { font-size: 14pt }\n  td:nth-child(2) { font-size: 11pt }\n  \
             td:last-child { font-size: 11pt }\n}",
        );
    }

    /// A size on a periodic selector is REFUSED by the reader, so collapsing per-line heights would
    /// make `unpack` write a block its own `check` rejects. Alternating heights are the exact shape
    /// that tempts it: three lines, evenly spaced, one declaration.
    #[test]
    fn alternating_axis_sizes_are_never_collapsed_into_a_periodic_rule() {
        let (sheet, block) = sizes(6, 1, &[11.0; 6]);
        let geometry = BlockGeometry {
            widths: Vec::new(),
            heights: (1..=6)
                .map(|row| (row, Points(if row % 2 == 0 { 20.0 } else { 15.0 })))
                .collect(),
        };
        let spelled = fsa1_model::spell_block(
            &encode(&sheet, block, &geometry).expect("the heights earn rules"),
            fsa1_ast::Shape { rows: 6, cols: 1 },
        );
        assert!(
            !spelled.contains("n)"),
            "a height never rides a periodic selector: {spelled}",
        );
        assert!(
            spelled.contains("tr:nth-child(2) td { height: 20pt }"),
            "{spelled}"
        );
    }

    /// An axis of extent 1 carries no selector of its own, so a stray cell's own file — the shape
    /// every partition reaches in the limit — can only ever spell `td`.
    #[test]
    fn a_block_of_extent_one_spells_only_a_bare_td() {
        assert_eq!(
            block_of(1, 1, &[14.0]),
            "@scope {\n  td { font-size: 14pt }\n}",
        );
        assert_eq!(
            block_of(1, 3, &[14.0, 11.0, 14.0]),
            "@scope {\n  td { font-size: 14pt }\n  td:nth-child(2) { font-size: 11pt }\n}",
            "one row spells columns, never cells",
        );
        assert_eq!(
            block_of(3, 1, &[14.0, 11.0, 14.0]),
            "@scope {\n  td { font-size: 14pt }\n  tr:nth-child(2) td { font-size: 11pt }\n}",
            "one column spells rows, never cells",
        );
    }

    /// A geometry rule collapses like every other property: one `td` where the whole block agrees.
    /// The guard is what keeps it honest — a block holding ONE unsized axis has no modal size, because
    /// a bare `td { width }` would size that axis too and no finer rule could take it back.
    #[test]
    fn a_block_sized_alike_on_every_axis_collapses_to_one_td_rule() {
        let (sheet, block) = sizes(2, 3, &[11.0; 6]);
        let spell = |geometry: &BlockGeometry| {
            let presentation = encode(&sheet, block, geometry).expect("a sized block earns a rule");
            fsa1_model::spell_block(&presentation, fsa1_ast::Shape { rows: 2, cols: 3 })
        };
        assert_eq!(
            spell(&BlockGeometry {
                widths: vec![(1, Chars(12.5)), (2, Chars(12.5)), (3, Chars(12.5))],
                heights: vec![(1, Points(20.0)), (2, Points(20.0))],
            }),
            "@scope {\n  td { height: 20pt; width: 12.5ch }\n}",
        );
        assert_eq!(
            spell(&BlockGeometry {
                widths: vec![(1, Chars(12.5)), (2, Chars(12.5))],
                heights: Vec::new(),
            }),
            "@scope {\n  td:first-child { width: 12.5ch }\n  td:nth-child(2) { width: 12.5ch }\n}",
            "column 3 is unsized, so no td may claim a width",
        );
        assert_eq!(
            spell(&BlockGeometry {
                widths: vec![(1, Chars(12.5)), (2, Chars(12.5)), (3, Chars(9.0))],
                heights: Vec::new(),
            }),
            "@scope {\n  td:first-child { width: 12.5ch }\n  td:nth-child(2) { width: 12.5ch }\n  \
             td:last-child { width: 9ch }\n}",
            "the three do not agree, so none of them is modal",
        );
    }

    /// The numbers an .xlsx may state for a size: the signed zero a collapsed axis writes, both ends
    /// of every accepted range, and the values outside them.
    const ADVERSARIAL: &[f64] = &[
        -0.0,
        0.0,
        1.0,
        8.43,
        11.0,
        255.0,
        409.0,
        409.5,
        -1.0,
        300.0,
        1000.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ];

    /// One cell whose face is `name`, over a workbook whose Normal font is Arial 9 — unlike the
    /// format's own default, so the block stays non-empty whether the face is carried or dropped back
    /// onto the Normal one, and the reparse below is asserted either way.
    fn faced(name: &str) -> SheetSource {
        let look = XlsxStyle {
            font: XlsxFont {
                name: Some(name.to_string()),
                size: Some(11.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut sheet = one_cell(look.clone());
        sheet.styles = StyleTable::of(
            vec![look],
            XlsxFont {
                name: Some("Arial".to_string()),
                size: Some(9.0),
                ..Default::default()
            },
        );
        sheet
    }

    /// The face names swept below: the shapes a name can take, then EVERY ASCII punctuation character
    /// in each of the three positions one can occupy, plus the whitespace and non-ASCII a hand list
    /// would also have missed. Swept rather than listed, so the next character cannot hide behind the
    /// ones someone thought to write down — `:` and `!` did exactly that.
    fn adversarial_families() -> Vec<String> {
        let mut names: Vec<String> = [
            "Calibri",
            "Times New Roman",
            "MS Sans Serif",
            "Two  Spaces",
            " Leading",
            "Trailing ",
            "",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let punctuation = (b' '..=b'~')
            .map(char::from)
            .filter(char::is_ascii_punctuation);
        for c in punctuation.chain([' ', '\t', '\n', 'é', '中']) {
            names.extend([format!("My{c}Font"), format!("{c}Font"), format!("Font{c}")]);
        }
        names
    }

    /// The whole class B2 named, not its one instance: `check` must accept every `@scope` block the
    /// write leg can emit, whatever the source states. Both directions read one vocabulary, so a value
    /// the encoder admits is by construction one the parser accepts and re-reads identically — this
    /// is the assertion that fails the moment a second copy of a range or a spelling appears.
    #[test]
    fn every_scope_block_the_encoder_can_emit_reparses_to_the_one_it_emitted() {
        let reparses = |sheet: &SheetSource, geometry: &BlockGeometry, what: &str| {
            let Some(presentation) = encode(sheet, CELL, geometry) else {
                return;
            };
            let block =
                fsa1_model::spell_block(&presentation, fsa1_ast::Shape { rows: 1, cols: 1 });
            let parsed = fsa1_model::parse_file("A1", &format!("1\n{block}"))
                .unwrap_or_else(|d| panic!("{what}: check refuses `{block}`: {d:?}"));
            assert_eq!(
                parsed.presentation.as_ref(),
                Some(&presentation),
                "{what}: `{block}` re-reads as a different presentation",
            );
        };
        for &n in ADVERSARIAL {
            let (mut sheet, _) = sizes(1, 1, &[n]);
            sheet.col_widths.insert(0, n);
            sheet.row_heights.insert(0, n);
            let geometry = assign_geometry(&sheet, &[CELL], &mut Vec::new());
            reparses(&sheet, &geometry[0], &format!("{n} on every measure"));
        }
        for name in adversarial_families() {
            reparses(
                &faced(&name),
                &BlockGeometry::default(),
                &format!("family {name:?}"),
            );
        }
    }

    /// The OTHER way a size fails to cross. An axis no file covers is already named; a number no width
    /// or height can state was dropped in silence, right beside it.
    #[test]
    fn a_size_outside_what_the_format_can_state_is_named_not_dropped_in_silence() {
        let (mut sheet, _) = sizes(1, 1, &[11.0]);
        sheet.col_widths.insert(0, 300.0);
        sheet.row_heights.insert(0, 900.0);
        let mut warnings = Vec::new();
        let geometry = assign_geometry(&sheet, &[CELL], &mut warnings);
        assert_eq!(
            warnings,
            vec![
                UnpackWarning::ColumnWidthUnspellable {
                    sheet: "S".to_string(),
                    column: "A".to_string(),
                    width: "300".to_string(),
                },
                UnpackWarning::RowHeightUnspellable {
                    sheet: "S".to_string(),
                    row: 1,
                    height: "900".to_string(),
                },
            ],
        );
        assert_eq!(geometry[0], BlockGeometry::default(), "and nothing crossed");
    }

    fn losses(look: XlsxStyle) -> Vec<UnpackWarning> {
        let mut warnings = Vec::new();
        name_losses(&one_cell(look), &mut warnings);
        warnings
    }

    fn dropped(look: XlsxStyle) -> Vec<String> {
        losses(look)
            .into_iter()
            .filter_map(|w| match w {
                UnpackWarning::CellAttributeDropped { attribute, .. } => Some(attribute),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_attribute_css_has_no_word_for_is_named() {
        assert_eq!(
            dropped(XlsxStyle {
                indent: 2,
                diagonal: true,
                rotation: 90,
                font: XlsxFont {
                    vert_align: Some(VertAlign::Superscript),
                    ..Default::default()
                },
                fill: XlsxFill {
                    pattern: FillPattern::Other("gradient".to_string()),
                    fg: None,
                    bg: None,
                },
                ..Default::default()
            }),
            vec![
                "indent level 2",
                "diagonal border",
                "text rotation",
                "superscript",
                "gradient fill",
            ],
        );
        assert_eq!(
            dropped(XlsxStyle {
                fill: XlsxFill {
                    pattern: FillPattern::Other("gray125".to_string()),
                    fg: None,
                    bg: None,
                },
                ..Default::default()
            }),
            vec!["gray125 pattern fill"],
        );
        assert_eq!(
            dropped(XlsxStyle {
                shrink_to_fit: true,
                quote_prefix: true,
                font: XlsxFont {
                    outline: true,
                    shadow: true,
                    ..Default::default()
                },
                ..Default::default()
            }),
            vec![
                "shrink to fit",
                "font outline",
                "font shadow",
                "quote prefix",
            ],
            "the tail that sits beside the attributes already named, and left with them",
        );
    }

    /// These four measure ZERO across the survey corpus, verified against every workbook's closed
    /// `xl/styles.xml`. They are carried for correctness on workbooks unlike it, so they are pinned
    /// from hand-built input rather than from a corpus count.
    #[test]
    fn every_value_level_narrowing_is_named() {
        for (align, want) in [
            (HorizontalAlign::Fill, "horizontal alignment fill"),
            (
                HorizontalAlign::CenterContinuous,
                "horizontal alignment centerContinuous",
            ),
            (
                HorizontalAlign::Distributed,
                "horizontal alignment distributed",
            ),
        ] {
            let look = XlsxStyle {
                horizontal: Some(align),
                ..Default::default()
            };
            assert_eq!(dropped(look.clone()), vec![want]);
            assert_eq!(text_align(&plain(), &look), None, "and it declares nothing");
        }
        for (align, want) in [
            (XlsxVerticalAlign::Justify, "vertical alignment justify"),
            (
                XlsxVerticalAlign::Distributed,
                "vertical alignment distributed",
            ),
        ] {
            let look = XlsxStyle {
                vertical: Some(align),
                ..Default::default()
            };
            assert_eq!(dropped(look.clone()), vec![want]);
            assert_eq!(vertical_align(&plain(), &look), None);
        }
        assert!(
            dropped(XlsxStyle {
                horizontal: Some(HorizontalAlign::General),
                ..Default::default()
            })
            .is_empty(),
            "`general` is the absence of an alignment, not one CSS cannot spell"
        );

        for style in [
            Underline::Double,
            Underline::SingleAccounting,
            Underline::DoubleAccounting,
        ] {
            let look = XlsxStyle {
                font: XlsxFont {
                    underline: Some(style),
                    ..Default::default()
                },
                ..Default::default()
            };
            assert_eq!(
                losses(look.clone()),
                vec![UnpackWarning::UnderlineStyleNarrowed {
                    sheet: "S".to_string(),
                    cell: "A1".to_string(),
                    style: style.spell().to_string(),
                }],
            );
            assert_eq!(
                decoration(&look.font),
                Declaration::TextDecoration(TextDecoration::Underline),
                "every one of the four collapses to the one underline",
            );
        }
        assert!(
            losses(XlsxStyle {
                font: XlsxFont {
                    underline: Some(Underline::Single),
                    ..Default::default()
                },
                ..Default::default()
            })
            .is_empty(),
            "a single underline is spelled exactly, so it loses nothing"
        );

        let both = XlsxStyle {
            font: XlsxFont {
                underline: Some(Underline::Single),
                strike: true,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            losses(both.clone()),
            vec![UnpackWarning::StrikethroughDropped {
                sheet: "S".to_string(),
                cell: "A1".to_string(),
            }],
            "the decoration is one enum, not a set",
        );
        assert_eq!(
            decoration(&both.font),
            Declaration::TextDecoration(TextDecoration::Underline),
        );
    }

    fn plain<'a>() -> Ctx<'a> {
        Ctx {
            normal: &NORMAL,
            default_text: None,
        }
    }

    static NORMAL: XlsxFont = XlsxFont {
        name: None,
        size: None,
        bold: false,
        italic: false,
        strike: false,
        underline: None,
        color: None,
        vert_align: None,
        outline: false,
        shadow: false,
    };

    /// Every one of Excel's thirteen edges is drawn, and the six with no CSS pair of their own are
    /// named as the approximation they became.
    #[test]
    fn every_border_style_is_drawn_and_the_six_without_a_pair_are_named() {
        let cases: &[(BorderStyle, BorderLine, Option<&str>)] = &[
            (BorderStyle::Thin, BorderLine::ThinSolid, None),
            (BorderStyle::Medium, BorderLine::MediumSolid, None),
            (BorderStyle::Thick, BorderLine::ThickSolid, None),
            (BorderStyle::Double, BorderLine::ThickDouble, None),
            (BorderStyle::Dotted, BorderLine::ThinDotted, None),
            (BorderStyle::Dashed, BorderLine::ThinDashed, None),
            (BorderStyle::MediumDashed, BorderLine::MediumDashed, None),
            (BorderStyle::Hair, BorderLine::ThinSolid, Some("solid")),
            (BorderStyle::DashDot, BorderLine::ThinDashed, Some("dashed")),
            (
                BorderStyle::DashDotDot,
                BorderLine::ThinDashed,
                Some("dashed"),
            ),
            (
                BorderStyle::MediumDashDot,
                BorderLine::MediumDashed,
                Some("dashed"),
            ),
            (
                BorderStyle::MediumDashDotDot,
                BorderLine::MediumDashed,
                Some("dashed"),
            ),
            (
                BorderStyle::SlantDashDot,
                BorderLine::MediumDashed,
                Some("dashed"),
            ),
        ];
        for &(style, line, nearest) in cases {
            let look = XlsxStyle {
                border_top: Some(XlsxBorder { style, color: None }),
                ..Default::default()
            };
            assert_eq!(
                edge(Edge::Top, look.border_top),
                Some(Declaration::Border {
                    edge: Edge::Top,
                    border: Border {
                        line,
                        color: Rgb { r: 0, g: 0, b: 0 },
                    },
                }),
                "{style:?}",
            );
            let want: Vec<UnpackWarning> = nearest
                .map(|nearest| UnpackWarning::BorderStyleApproximated {
                    sheet: "S".to_string(),
                    cell: "A1".to_string(),
                    style: style.spell().to_string(),
                    nearest: nearest.to_string(),
                })
                .into_iter()
                .collect();
            assert_eq!(losses(look), want, "{style:?}");
        }
    }
}
