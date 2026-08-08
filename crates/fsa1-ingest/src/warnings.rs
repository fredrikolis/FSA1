// Concern: UnpackWarning — what ONE loss a completed unpack incurred is, and where | Non-concern: refusals, the stderr report (fsa1-cli) | IO: (a loss) -> UnpackWarning, UnpackCategory, one line

use std::fmt;

use fsa1_ast::a1::format_column;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnpackWarning {
    NumberFormatCoerced {
        sheet: String,
        cell: String,
        num_fmt_id: u32,
        format_code: Option<String>,
    },
    TableDropped {
        table: String,
        reason: String,
    },
    NameSkipped {
        name: String,
        scope: Option<String>,
        reason: String,
    },
    FormulaKeptVerbatim {
        sheet: String,
        cell: String,
        source: String,
        reason: String,
    },
    MergedRegionFlattened {
        sheet: String,
        region: String,
    },
    /// `attribute` is the dropped thing's whole noun phrase — `indent level 2`,
    /// `horizontal alignment fill`, `vertical alignment justify`, `diagonal border`.
    CellAttributeDropped {
        sheet: String,
        cell: String,
        attribute: String,
    },
    /// The workbook's Normal font — the appearance every cell stating no style wears — in a spelling
    /// no declaration can carry, so no cell can cross wearing it. `attribute` is the dropped half's
    /// whole noun phrase, as [`UnpackWarning::CellAttributeDropped`]'s is.
    NormalFontDropped {
        sheet: String,
        attribute: String,
    },
    /// The look a whole COLUMN or ROW states, crossing on every cell inside the sheet's extent and on
    /// none of the unbounded many past it that no range file covers — where a look DRAWING on a blank,
    /// a fill or an edge, is what still shows. `run` names the axis that states it, never the loss's
    /// own extent, which has no end.
    AxisDefaultStyleClipped {
        sheet: String,
        axis: Axis,
        run: AxisRef,
    },
    BorderStyleApproximated {
        sheet: String,
        cell: String,
        style: String,
        nearest: String,
    },
    UnderlineStyleNarrowed {
        sheet: String,
        cell: String,
        style: String,
    },
    StrikethroughDropped {
        sheet: String,
        cell: String,
    },
    /// `column` is one column (`C`) or the contiguous run one authored statement covered (`C:XFD`);
    /// `row` is the same fact on the other axis (`7`, `5:100003`). Only [`unowned`] builds an
    /// [`AxisRef`], so neither variant can be reached with an axis that skipped the fold.
    ColumnWidthUnowned {
        sheet: String,
        column: AxisRef,
    },
    RowHeightUnowned {
        sheet: String,
        row: AxisRef,
    },
    /// The OTHER reason a size does not cross: the number itself is outside what a width or a height
    /// may state, so no range file could spell it however the sheet is partitioned. `width` and
    /// `height` are that number as the sheet stated it.
    ColumnWidthUnspellable {
        sheet: String,
        column: String,
        width: String,
    },
    RowHeightUnspellable {
        sheet: String,
        row: u32,
        height: String,
    },
    /// A chart that yielded no figure. `chart` is its package part and `why` the one sentence naming
    /// what stopped it: Excel-to-Vega-Lite is TOTAL over the charts it admits, so a chart outside
    /// them is a named loss and an unpack still completes.
    ChartNotCarried {
        sheet: String,
        chart: String,
        why: String,
    },
    /// A drawing part anchoring something other than a chart. `xl/drawings/` is carried for the charts
    /// it anchors, and a drawing holding a text box, a shape or a picture would otherwise pass as
    /// nothing lost.
    DrawingNotCarried {
        drawing: String,
        why: String,
    },
    WorkbookPartNotCarried {
        part: String,
    },
}

/// The reference naming one contiguous axis run, already spelled: `C`, `C:XFD`, `5:2000`. The text is
/// private to [`axis_run`], the ONE spelling, so a producer holding a bare index cannot build one; and
/// [`unowned`] is the only way to reach it with a SET of axes, so a set can only be named after the
/// fold that decides how many lines it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AxisRef(String);

impl fmt::Display for AxisRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Which axis a size sits on, which is the only thing the two unowned legs differ by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Column,
    Row,
}

impl Axis {
    fn spell(self, index: u32) -> String {
        match self {
            Axis::Column => format_column(index),
            Axis::Row => (u64::from(index) + 1).to_string(),
        }
    }

    fn unowned(self, sheet: &str, run: AxisRef) -> UnpackWarning {
        let sheet = sheet.to_string();
        match self {
            Axis::Column => UnpackWarning::ColumnWidthUnowned { sheet, column: run },
            Axis::Row => UnpackWarning::RowHeightUnowned { sheet, row: run },
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Axis::Column => "column",
            Axis::Row => "row",
        }
    }
}

/// One contiguous run named as one reference, `(first, last)` inclusive and 0-based: `C`, `C:XFD`,
/// `5:2000`. Every [`AxisRef`] is spelled here, so the two producers cannot name the same run two ways.
pub fn axis_run(axis: Axis, first: u32, last: u32) -> AxisRef {
    AxisRef(match first == last {
        true => axis.spell(first),
        false => format!("{}:{}", axis.spell(first), axis.spell(last)),
    })
}

/// The axes no range file covers, as the fewest references that name them all: `(first, last)`
/// inclusive and 0-based, in any order and free to overlap. ONE authored statement sizes axes no file
/// reaches, so the loss is one line per contiguous run and never one per axis — and both producers,
/// the extent clip and the block-ownership pass, reach these two variants only through here.
pub fn unowned(axis: Axis, sheet: &str, axes: &[(u32, u32)]) -> Vec<UnpackWarning> {
    let mut sorted: Vec<(u32, u32)> = axes.iter().copied().filter(|(a, b)| a <= b).collect();
    sorted.sort_unstable();
    let mut folded: Vec<(u32, u32)> = Vec::new();
    for (first, last) in sorted {
        match folded.last_mut() {
            Some((_, end)) if first <= end.saturating_add(1) => *end = (*end).max(last),
            _ => folded.push((first, last)),
        }
    }
    folded
        .into_iter()
        .map(|(first, last)| axis.unowned(sheet, axis_run(axis, first, last)))
        .collect()
}

/// Variant order is the report's section order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnpackCategory {
    NumberFormat,
    Table,
    Name,
    Formula,
    Styling,
    Geometry,
    Chart,
    WorkbookPart,
}

impl UnpackCategory {
    /// Every category there is, in report order. What a run INSPECTED is a subset of this, and a
    /// report may vouch for a category only by naming it here first.
    pub const ALL: [UnpackCategory; 8] = [
        UnpackCategory::NumberFormat,
        UnpackCategory::Table,
        UnpackCategory::Name,
        UnpackCategory::Formula,
        UnpackCategory::Styling,
        UnpackCategory::Geometry,
        UnpackCategory::Chart,
        UnpackCategory::WorkbookPart,
    ];
}

impl UnpackWarning {
    pub fn category(&self) -> UnpackCategory {
        match self {
            UnpackWarning::NumberFormatCoerced { .. } => UnpackCategory::NumberFormat,
            UnpackWarning::TableDropped { .. } => UnpackCategory::Table,
            UnpackWarning::NameSkipped { .. } => UnpackCategory::Name,
            UnpackWarning::FormulaKeptVerbatim { .. } => UnpackCategory::Formula,
            UnpackWarning::MergedRegionFlattened { .. }
            | UnpackWarning::CellAttributeDropped { .. }
            | UnpackWarning::NormalFontDropped { .. }
            | UnpackWarning::AxisDefaultStyleClipped { .. }
            | UnpackWarning::BorderStyleApproximated { .. }
            | UnpackWarning::UnderlineStyleNarrowed { .. }
            | UnpackWarning::StrikethroughDropped { .. } => UnpackCategory::Styling,
            UnpackWarning::ColumnWidthUnowned { .. }
            | UnpackWarning::RowHeightUnowned { .. }
            | UnpackWarning::ColumnWidthUnspellable { .. }
            | UnpackWarning::RowHeightUnspellable { .. } => UnpackCategory::Geometry,
            UnpackWarning::ChartNotCarried { .. } | UnpackWarning::DrawingNotCarried { .. } => {
                UnpackCategory::Chart
            }
            UnpackWarning::WorkbookPartNotCarried { .. } => UnpackCategory::WorkbookPart,
        }
    }
}

impl fmt::Display for UnpackWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnpackWarning::NumberFormatCoerced {
                sheet,
                cell,
                num_fmt_id,
                format_code,
            } => {
                let code = match format_code {
                    Some(c) => format!("\"{c}\""),
                    None => "built-in".to_string(),
                };
                write!(
                    f,
                    "{sheet}!{cell}: numFmtId {num_fmt_id} ({code}) dropped; value kept as plain"
                )
            }
            UnpackWarning::TableDropped { table, reason } => write!(f, "{table:?}: {reason}"),
            UnpackWarning::NameSkipped {
                name,
                scope,
                reason,
            } => {
                let scope = scope.as_deref().unwrap_or("workbook");
                write!(f, "{name:?} ({scope}): {reason}; refs load as #NAME?")
            }
            UnpackWarning::FormulaKeptVerbatim {
                sheet,
                cell,
                source,
                reason,
            } => write!(
                f,
                "{sheet}!{cell}: {reason}; kept verbatim as ={source}; loads as an error at load"
            ),
            UnpackWarning::MergedRegionFlattened { sheet, region } => write!(
                f,
                "merged region {sheet}!{region} flattened; its value stays in the top-left cell"
            ),
            UnpackWarning::CellAttributeDropped {
                sheet,
                cell,
                attribute,
            } => write!(f, "{attribute} at {sheet}!{cell} dropped"),
            UnpackWarning::NormalFontDropped { sheet, attribute } => write!(
                f,
                "the Normal font's {attribute} on sheet {sheet} dropped: no declaration can spell it"
            ),
            UnpackWarning::AxisDefaultStyleClipped { sheet, axis, run } => write!(
                f,
                "the fill or border {} {run} states on sheet {sheet} is dropped past the sheet's extent: no range file covers a cell there",
                axis.noun()
            ),
            UnpackWarning::BorderStyleApproximated {
                sheet,
                cell,
                style,
                nearest,
            } => write!(
                f,
                "border style {style} at {sheet}!{cell} approximated as {nearest}"
            ),
            UnpackWarning::UnderlineStyleNarrowed { sheet, cell, style } => write!(
                f,
                "underline style {style} at {sheet}!{cell} narrowed to underline"
            ),
            UnpackWarning::StrikethroughDropped { sheet, cell } => write!(
                f,
                "strikethrough at {sheet}!{cell} dropped; the cell also carries underline"
            ),
            UnpackWarning::ColumnWidthUnowned { sheet, column } => write!(
                f,
                "column width for {column} on sheet {sheet} dropped: no range file covers column {column}"
            ),
            UnpackWarning::RowHeightUnowned { sheet, row } => write!(
                f,
                "row height for {row} on sheet {sheet} dropped: no range file covers row {row}"
            ),
            UnpackWarning::ColumnWidthUnspellable {
                sheet,
                column,
                width,
            } => write!(
                f,
                "column width for {column} on sheet {sheet} dropped: {width} is outside the widths a column can state"
            ),
            UnpackWarning::RowHeightUnspellable { sheet, row, height } => write!(
                f,
                "row height for {row} on sheet {sheet} dropped: {height} is outside the heights a row can state"
            ),
            UnpackWarning::ChartNotCarried { sheet, chart, why } => {
                write!(f, "{chart} on sheet {sheet} carries no figure: {why}")
            }
            UnpackWarning::DrawingNotCarried { drawing, why } => {
                write!(f, "{drawing} is not carried: {why}")
            }
            UnpackWarning::WorkbookPartNotCarried { part } => write!(f, "{part} not carried"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_variant_maps_to_its_category() {
        assert_eq!(
            UnpackWarning::NumberFormatCoerced {
                sheet: "S".into(),
                cell: "B2".into(),
                num_fmt_id: 164,
                format_code: Some("0.00%".into()),
            }
            .category(),
            UnpackCategory::NumberFormat
        );
        assert_eq!(
            UnpackWarning::TableDropped {
                table: "Sales".into(),
                reason: "r".into(),
            }
            .category(),
            UnpackCategory::Table
        );
        assert_eq!(
            UnpackWarning::NameSkipped {
                name: "A1".into(),
                scope: None,
                reason: "r".into(),
            }
            .category(),
            UnpackCategory::Name
        );
        assert_eq!(
            UnpackWarning::FormulaKeptVerbatim {
                sheet: "S".into(),
                cell: "A2".into(),
                source: "SUM({1,2,3})".into(),
                reason: "r".into(),
            }
            .category(),
            UnpackCategory::Formula
        );
        for w in [
            UnpackWarning::MergedRegionFlattened {
                sheet: "S".into(),
                region: "A1:B2".into(),
            },
            UnpackWarning::CellAttributeDropped {
                sheet: "S".into(),
                cell: "B3".into(),
                attribute: "diagonal border".into(),
            },
            UnpackWarning::BorderStyleApproximated {
                sheet: "S".into(),
                cell: "B3".into(),
                style: "hair".into(),
                nearest: "solid".into(),
            },
            UnpackWarning::UnderlineStyleNarrowed {
                sheet: "S".into(),
                cell: "B3".into(),
                style: "double".into(),
            },
            UnpackWarning::StrikethroughDropped {
                sheet: "S".into(),
                cell: "B3".into(),
            },
            UnpackWarning::AxisDefaultStyleClipped {
                sheet: "S".into(),
                axis: Axis::Column,
                run: axis_run(Axis::Column, 1, 1),
            },
        ] {
            assert_eq!(w.category(), UnpackCategory::Styling);
        }
        for w in [
            unowned(Axis::Column, "S", &[(2, 2)]).remove(0),
            unowned(Axis::Row, "S", &[(6, 6)]).remove(0),
            UnpackWarning::ColumnWidthUnspellable {
                sheet: "S".into(),
                column: "C".into(),
                width: "300".into(),
            },
            UnpackWarning::RowHeightUnspellable {
                sheet: "S".into(),
                row: 7,
                height: "900".into(),
            },
        ] {
            assert_eq!(w.category(), UnpackCategory::Geometry);
        }
        assert_eq!(
            UnpackWarning::ChartNotCarried {
                sheet: "S".into(),
                chart: "xl/charts/chart1.xml".into(),
                why: "r".into(),
            }
            .category(),
            UnpackCategory::Chart
        );
        assert_eq!(
            UnpackWarning::WorkbookPartNotCarried {
                part: "autofilter".into(),
            }
            .category(),
            UnpackCategory::WorkbookPart
        );
    }

    /// `ALL` is hand-written over a closed enum, so this is what holds it exhaustive: a ninth
    /// category breaks the match below, and one left out of `ALL` fails the count.
    #[test]
    fn all_lists_every_category_once() {
        let mut seen: Vec<UnpackCategory> = Vec::new();
        for category in UnpackCategory::ALL {
            match category {
                UnpackCategory::NumberFormat
                | UnpackCategory::Table
                | UnpackCategory::Name
                | UnpackCategory::Formula
                | UnpackCategory::Styling
                | UnpackCategory::Geometry
                | UnpackCategory::Chart
                | UnpackCategory::WorkbookPart => {}
            }
            assert!(!seen.contains(&category), "{category:?} listed twice");
            seen.push(category);
        }
        assert_eq!(seen.len(), 8);
    }

    /// The fold is what both producers hand their axes to, and neither sorts, dedupes or merges
    /// first: a `<cols>` restating one run states it as many overlapping runs, and two runs that
    /// touch are one authored region. Adjacency counts, or `1:5` and `6:10` cost two lines.
    #[test]
    fn the_fold_names_overlapping_touching_and_unsorted_axes_as_the_fewest_runs() {
        let spelled = |axis, axes: &[(u32, u32)]| {
            unowned(axis, "S", axes)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            spelled(Axis::Row, &[(9, 9), (4, 6), (5, 12), (7, 8)]),
            vec![
                "row height for 5:13 on sheet S dropped: no range file covers row 5:13".to_string()
            ],
            "unsorted, overlapping and touching runs are one region",
        );
        assert_eq!(spelled(Axis::Column, &[(2, 2), (2, 2), (2, 2)]).len(), 1);
        assert_eq!(
            spelled(Axis::Column, &[(0, 1), (3, 3)]),
            vec![
                "column width for A:B on sheet S dropped: no range file covers column A:B"
                    .to_string(),
                "column width for D on sheet S dropped: no range file covers column D".to_string(),
            ],
            "a gap keeps them apart, and a run of one is spelled as one",
        );
        assert!(spelled(Axis::Column, &[]).is_empty());
    }

    #[test]
    fn display_spells_each_located_line() {
        assert_eq!(
            UnpackWarning::NumberFormatCoerced {
                sheet: "Sheet1".into(),
                cell: "B2".into(),
                num_fmt_id: 164,
                format_code: Some("0.00%".into()),
            }
            .to_string(),
            "Sheet1!B2: numFmtId 164 (\"0.00%\") dropped; value kept as plain"
        );
        assert_eq!(
            UnpackWarning::NumberFormatCoerced {
                sheet: "Data".into(),
                cell: "D2".into(),
                num_fmt_id: 14,
                format_code: None,
            }
            .to_string(),
            "Data!D2: numFmtId 14 (built-in) dropped; value kept as plain"
        );
        assert_eq!(
            UnpackWarning::TableDropped {
                table: "Sales".into(),
                reason: "could not map to a sheet (displayName/sheet divergence); structured refs load as #NAME?".into(),
            }
            .to_string(),
            "\"Sales\": could not map to a sheet (displayName/sheet divergence); structured refs load as #NAME?"
        );
        assert_eq!(
            UnpackWarning::NameSkipped {
                name: "A1".into(),
                scope: None,
                reason: "identifier parses as an A1 address".into(),
            }
            .to_string(),
            "\"A1\" (workbook): identifier parses as an A1 address; refs load as #NAME?"
        );
        assert_eq!(
            UnpackWarning::NameSkipped {
                name: "B2".into(),
                scope: Some("Data".into()),
                reason: "identifier parses as an A1 address".into(),
            }
            .to_string(),
            "\"B2\" (Data): identifier parses as an A1 address; refs load as #NAME?"
        );
        assert_eq!(
            UnpackWarning::FormulaKeptVerbatim {
                sheet: "Data".into(),
                cell: "B1".into(),
                source: "SUM({1,2,3})".into(),
                reason: "an inline array `{…}` is not translatable from ODS".into(),
            }
            .to_string(),
            "Data!B1: an inline array `{…}` is not translatable from ODS; kept verbatim as =SUM({1,2,3}); loads as an error at load"
        );
    }

    #[test]
    fn display_spells_each_presentation_loss() {
        assert_eq!(
            UnpackWarning::MergedRegionFlattened {
                sheet: "Sheet1".into(),
                region: "A1:B2".into(),
            }
            .to_string(),
            "merged region Sheet1!A1:B2 flattened; its value stays in the top-left cell"
        );
        assert_eq!(
            UnpackWarning::CellAttributeDropped {
                sheet: "Sheet1".into(),
                cell: "B3".into(),
                attribute: "indent level 2".into(),
            }
            .to_string(),
            "indent level 2 at Sheet1!B3 dropped"
        );
        assert_eq!(
            UnpackWarning::CellAttributeDropped {
                sheet: "Sheet1".into(),
                cell: "B3".into(),
                attribute: "text rotation".into(),
            }
            .to_string(),
            "text rotation at Sheet1!B3 dropped"
        );
        assert_eq!(
            UnpackWarning::CellAttributeDropped {
                sheet: "Sheet1".into(),
                cell: "B3".into(),
                attribute: "horizontal alignment centerContinuous".into(),
            }
            .to_string(),
            "horizontal alignment centerContinuous at Sheet1!B3 dropped"
        );
        assert_eq!(
            UnpackWarning::CellAttributeDropped {
                sheet: "Sheet1".into(),
                cell: "B3".into(),
                attribute: "vertical alignment justify".into(),
            }
            .to_string(),
            "vertical alignment justify at Sheet1!B3 dropped"
        );
        assert_eq!(
            UnpackWarning::BorderStyleApproximated {
                sheet: "Sheet1".into(),
                cell: "B3".into(),
                style: "mediumDashDot".into(),
                nearest: "dashed".into(),
            }
            .to_string(),
            "border style mediumDashDot at Sheet1!B3 approximated as dashed"
        );
        assert_eq!(
            UnpackWarning::UnderlineStyleNarrowed {
                sheet: "Sheet1".into(),
                cell: "B3".into(),
                style: "doubleAccounting".into(),
            }
            .to_string(),
            "underline style doubleAccounting at Sheet1!B3 narrowed to underline"
        );
        assert_eq!(
            UnpackWarning::StrikethroughDropped {
                sheet: "Sheet1".into(),
                cell: "B3".into(),
            }
            .to_string(),
            "strikethrough at Sheet1!B3 dropped; the cell also carries underline"
        );
        assert_eq!(
            UnpackWarning::AxisDefaultStyleClipped {
                sheet: "Data".into(),
                axis: Axis::Column,
                run: axis_run(Axis::Column, 1, 16_383),
            }
            .to_string(),
            "the fill or border column B:XFD states on sheet Data is dropped past the sheet's \
             extent: no range file covers a cell there"
        );
        assert_eq!(
            UnpackWarning::AxisDefaultStyleClipped {
                sheet: "Data".into(),
                axis: Axis::Row,
                run: axis_run(Axis::Row, 6, 6),
            }
            .to_string(),
            "the fill or border row 7 states on sheet Data is dropped past the sheet's extent: \
             no range file covers a cell there"
        );
        assert_eq!(
            unowned(Axis::Column, "Data", &[(2, 2)])[0].to_string(),
            "column width for C on sheet Data dropped: no range file covers column C"
        );
        assert_eq!(
            unowned(Axis::Row, "Data", &[(6, 6)])[0].to_string(),
            "row height for 7 on sheet Data dropped: no range file covers row 7"
        );
        assert_eq!(
            UnpackWarning::ColumnWidthUnspellable {
                sheet: "Data".into(),
                column: "C".into(),
                width: "300".into(),
            }
            .to_string(),
            "column width for C on sheet Data dropped: 300 is outside the widths a column can state"
        );
        assert_eq!(
            UnpackWarning::RowHeightUnspellable {
                sheet: "Data".into(),
                row: 7,
                height: "900".into(),
            }
            .to_string(),
            "row height for 7 on sheet Data dropped: 900 is outside the heights a row can state"
        );
        assert_eq!(
            UnpackWarning::WorkbookPartNotCarried {
                part: "conditional formatting".into(),
            }
            .to_string(),
            "conditional formatting not carried"
        );
        assert_eq!(
            UnpackWarning::ChartNotCarried {
                sheet: "Sheet1".into(),
                chart: "xl/charts/chart1.xml".into(),
                why: "a radarChart has no Vega-Lite mark".into(),
            }
            .to_string(),
            "xl/charts/chart1.xml on sheet Sheet1 carries no figure: a radarChart has no Vega-Lite \
             mark"
        );
    }
}
