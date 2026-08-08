// Concern: spells a source chart as a Vega-Lite figure, and names what cannot cross | Non-concern: reading a chart part (xlsx_chart.rs), a sheet's blocks | IO: (a chart, its sheet) -> (name, body)
//! Excel-to-Vega-Lite is TOTAL over the charts it admits and LOSSY within them: every reason a chart
//! yields no figure leaves here as a sentence a [`UnpackWarning::ChartNotCarried`] carries, and never
//! as a silence. A chart that would produce a figure `check` then refuses is one of those reasons —
//! the binding a figure states is graded against the tab it lands beside, so it is graded here first.

use fsa1_ast::a1::{format_cell, parse_a1};
use fsa1_model::{Cell, FIGURE_SUFFIX, Workbook, display_value, lex_literal, quote_sheet};
use serde_json::{Map, Value as Json};

use crate::resolve::Resolution;
use crate::serialize::cell_field;
use crate::source::{SheetSource, SourceValue};
use crate::warnings::UnpackWarning;
use crate::xlsx_chart::{SourceChart, SourceSeries};

/// The dialect the pinned runtime compiles — `vega-manifest.txt` names the version.
const SCHEMA: &str = "https://vega.github.io/schema/vega-lite/v5.json";

/// The tab a chart is drawn on, as the SPELLING needs it — nothing about how the tab was obtained.
/// One speller then grades both legs: the source book an unpack reads, and the loaded workbook
/// `pack` writes, which is what makes "representable" one definition rather than two.
pub(crate) trait ChartTable {
    fn tab(&self) -> &str;
    /// The bounding box of the tab's occupancy, which every binding must sit inside.
    fn content(&self) -> Option<Region>;
    /// The header cell's text. `None` for a FORMULA, whose content and value are two spellings of one
    /// cell: the field would key on the source text and the binding on its result.
    fn header(&self, col: u32, row: u32) -> Option<String>;
}

/// The unpack leg's tab: cells still in the reader's intermediate, lexed as the loaded workbook will
/// read them.
pub(crate) struct SourceTable<'a> {
    pub sheet: &'a SheetSource,
    pub res: &'a Resolution,
}

impl ChartTable for SourceTable<'_> {
    fn tab(&self) -> &str {
        &self.sheet.name
    }

    fn content(&self) -> Option<Region> {
        let mut found: Option<Region> = None;
        for row in 0..self.sheet.rows {
            for col in 0..self.sheet.cols {
                if !self.sheet.is_occupied(col, row) {
                    continue;
                }
                let one = Region {
                    min_col: col,
                    min_row: row,
                    max_col: col,
                    max_row: row,
                };
                found = Some(found.map_or(one, |r| r.union(one)));
            }
        }
        found
    }

    fn header(&self, col: u32, row: u32) -> Option<String> {
        let cell = self.sheet.cell(col, row)?;
        if matches!(cell.value, SourceValue::Formula { .. }) {
            return None;
        }
        let (field, _) = cell_field(&cell.value, self.res, &self.sheet.name, row);
        Some(display_value(&lex_literal(&field).0))
    }
}

/// The pack leg's tab: the loaded workbook a chart is about to be written from.
pub(crate) struct BookTable<'a> {
    pub wb: &'a Workbook,
    pub sheet: u32,
}

impl ChartTable for BookTable<'_> {
    fn tab(&self) -> &str {
        self.wb.sheet_name(self.sheet).unwrap_or_default()
    }

    fn content(&self) -> Option<Region> {
        self.wb.content_region(self.sheet).map(|r| Region {
            min_col: r.min_col,
            min_row: r.min_row,
            max_col: r.max_col,
            max_row: r.max_row,
        })
    }

    fn header(&self, col: u32, row: u32) -> Option<String> {
        match self.wb.source_at(self.sheet, col, row).map(|s| s.cell) {
            Some(Cell::Formula { .. }) => None,
            _ => Some(display_value(&self.wb.value_at(self.sheet, col, row))),
        }
    }
}

/// The figures one tab's charts became, and the chart parts that DID cross — which is what stops the
/// census calling a carried chart "not carried".
#[derive(Clone, Debug, Default)]
pub struct SheetFigures {
    /// `(<name>.vl.json, the spec)`, written beside the tab's range files.
    pub files: Vec<(String, String)>,
    pub carried: Vec<String>,
}

/// Every chart drawn on ONE sheet, in package-path order. A chart that cannot cross costs one warning
/// and no file; nothing here can fail the import, because an unpack completes.
pub fn figures(
    sheet: &SheetSource,
    charts: &[&SourceChart],
    res: &Resolution,
    warnings: &mut Vec<UnpackWarning>,
) -> SheetFigures {
    let mut out = SheetFigures::default();
    if charts.is_empty() {
        return out;
    }
    let table = SourceTable { sheet, res };
    let content = table.content();
    let mut taken: Vec<String> = Vec::new();
    for chart in charts {
        match spell(chart, &table, content, &taken) {
            Ok((stem, body)) => {
                out.files.push((format!("{stem}{FIGURE_SUFFIX}"), body));
                out.carried.push(chart.part.clone());
                taken.push(stem);
            }
            Err(why) => warnings.push(UnpackWarning::ChartNotCarried {
                sheet: sheet.name.clone(),
                chart: chart.part.clone(),
                why,
            }),
        }
    }
    out
}

/// One chart's whole spelling, or the ONE sentence saying why it has none.
pub(crate) fn spell(
    chart: &SourceChart,
    table: &dyn ChartTable,
    content: Option<Region>,
    taken: &[String],
) -> Result<(String, String), String> {
    let mark = mark_of(chart)?;
    if chart.series.is_empty() {
        return Err("it plots no series at all".to_string());
    }
    // Vega-Lite OVERLAYS layers, so a grouping this pass cannot spell draws a different chart.
    if let Some(grouping) = chart.grouping.as_deref()
        && matches!(grouping, "stacked" | "percentStacked")
        && chart.series.len() > 1
    {
        return Err(format!(
            "it groups its series {grouping:?}, which this pass does not spell, and layers would \
             overlay rather than stack"
        ));
    }
    // Every series, before any name is claimed: a chart is one figure or none, never a partial one.
    let layers = chart
        .series
        .iter()
        .map(|series| layer(series, table, content, mark, chart.horizontal_bars))
        .collect::<Result<Vec<Json>, String>>()?;
    let stem = stem_of(chart, taken)?;

    let mut spec = Map::new();
    spec.insert("$schema".to_string(), Json::String(SCHEMA.to_string()));
    if let Some(title) = &chart.title {
        spec.insert("title".to_string(), Json::String(title.clone()));
    }
    // ONE `<c:ser>` is one layer, and a lone layer states its mark and data at the top rather than in a one-member `layer` array.
    match <[Json; 1]>::try_from(layers) {
        Ok([one]) => {
            let one = one
                .as_object()
                .expect("a layer is built as an object")
                .clone();
            spec.extend(one);
        }
        Err(layers) => {
            spec.insert("layer".to_string(), Json::Array(layers));
        }
    }
    let body = serde_json::to_string_pretty(&Json::Object(spec))
        .map_err(|e| format!("its spec does not serialize: {e}"))?;
    Ok((stem, format!("{body}\n")))
}

/// The mark the chart ELEMENT states, read backwards out of the one table `fsa1-xlsx` writes the
/// element from. Anything else — a radar, a doughnut, a surface, or two plots combined in one plot
/// area — is a named loss and no figure.
fn mark_of(chart: &SourceChart) -> Result<&'static str, String> {
    match chart.plots.as_slice() {
        [] => Err("it states no plot area content at all".to_string()),
        [one] => {
            fsa1_xlsx::mark_for(one).ok_or_else(|| format!("a <c:{one}> has no Vega-Lite mark"))
        }
        many => Err(format!(
            "it combines {} plots ({}) in one plot area, and a combination chart has no one mark",
            many.len(),
            many.join(", ")
        )),
    }
}

/// One `<c:ser>` as one layer: its own `mark`, its own `data` binding ONE rectangle, and the two
/// header names that rectangle's first row states, `field`-encoded.
fn layer(
    series: &SourceSeries,
    table: &dyn ChartTable,
    content: Option<Region>,
    mark: &'static str,
    horizontal_bars: bool,
) -> Result<Json, String> {
    if series.literal {
        return Err(
            "a series carries its values inline (<c:numLit>) rather than referencing cells, so it \
             binds no rectangle"
                .to_string(),
        );
    }
    let cat = reference(series.cat.as_deref(), "its categories", table.tab())?;
    let val = reference(series.val.as_deref(), "its values", table.tab())?;
    // A bound table keys on a header ROW, so a series plotted across a row would have to be transposed to become one — which is a chart FSA1 does not admit.
    if cat.min_col != cat.max_col || val.min_col != val.max_col {
        return Err(
            "its references run across rows rather than down columns, and a bound table keys on its \
             first ROW"
                .to_string(),
        );
    }
    if cat.min_row != val.min_row || cat.max_row != val.max_row {
        return Err(format!(
            "its category and value references cover different rows ({} against {}), so no one \
             rectangle holds both",
            cat.label(),
            val.label()
        ));
    }
    let body = cat.union(val);
    let header = header_row(series, table.tab(), body)?;
    let bound = Region {
        min_row: header,
        ..body
    };
    if !content.is_some_and(|c| c.contains(bound)) {
        return Err(format!(
            "it binds {}, which reaches past the content sheet {:?} states",
            bound.label(),
            table.tab()
        ));
    }
    // EVERY column of the bound rectangle keys a field, so a blank or repeated header anywhere in it is a refusal `check` would raise on the figure this would otherwise write.
    let mut fields: Vec<String> = Vec::new();
    for col in bound.min_col..=bound.max_col {
        let Some(field) = table.header(col, header) else {
            return Err(format!(
                "the header at {} is a formula, and a field NAME is spelled from content while its \
                 binding's key is spelled from the value",
                format_cell(col, header)
            ));
        };
        if field.is_empty() {
            return Err(format!(
                "the header at {} is blank, and a bound row keys on its field NAME",
                format_cell(col, header)
            ));
        }
        if fields.contains(&field) {
            return Err(format!(
                "the header {field:?} at {} repeats, and a duplicate would silently drop a column",
                format_cell(col, header)
            ));
        }
        fields.push(field);
    }
    let field_at = |col: u32| fields[(col - bound.min_col) as usize].clone();
    let (category, quantity) = (field_at(cat.min_col), field_at(val.min_col));

    let mut encoding = Map::new();
    // A scatter states two MEASURES, so its category axis is quantitative like its value axis.
    let category_type = if mark == "point" {
        "quantitative"
    } else {
        "nominal"
    };
    match mark {
        // A pie has no axes: the value is the angle and the category is what colours the slice.
        "arc" => {
            encoding.insert("theta".to_string(), channel(&quantity, "quantitative"));
            encoding.insert("color".to_string(), channel(&category, "nominal"));
        }
        // `<c:barDir val="bar">` is the horizontal bar, which is the same encoding with its axes swapped.
        _ if horizontal_bars => {
            encoding.insert("y".to_string(), channel(&category, category_type));
            encoding.insert("x".to_string(), channel(&quantity, "quantitative"));
        }
        _ => {
            encoding.insert("x".to_string(), channel(&category, category_type));
            encoding.insert("y".to_string(), channel(&quantity, "quantitative"));
        }
    }

    let mut out = Map::new();
    out.insert("mark".to_string(), Json::String(mark.to_string()));
    out.insert(
        "data".to_string(),
        Json::Object(Map::from_iter([(
            "name".to_string(),
            Json::String(binding(table.tab(), bound)),
        )])),
    );
    out.insert("encoding".to_string(), Json::Object(encoding));
    Ok(Json::Object(out))
}

fn channel(field: &str, kind: &str) -> Json {
    Json::Object(Map::from_iter([
        ("field".to_string(), Json::String(field.to_string())),
        ("type".to_string(), Json::String(kind.to_string())),
    ]))
}

/// The row the field names live in, NAMED by `<c:tx>` rather than assumed: the series name is
/// normally `<c:strRef><c:f>Sheet1!$B$1`, which is the header cell of the column it plots. It must
/// sit immediately above the plotted rows and inside the plotted columns, or it names some other row
/// and the table below it would be silently wrong.
fn header_row(series: &SourceSeries, tab: &str, body: Region) -> Result<u32, String> {
    let name = reference(series.name_ref.as_deref(), "its own name", tab)?;
    if name.min_row != name.max_row || name.min_col != name.max_col {
        return Err(format!(
            "its name reference {} is not one cell, so it names no header row",
            name.label()
        ));
    }
    if name.min_row + 1 != body.min_row {
        return Err(format!(
            "its name reference {} does not sit in the row above the plotted rows, so no header row \
             is found",
            name.label()
        ));
    }
    if name.min_col < body.min_col || name.min_col > body.max_col {
        return Err(format!(
            "its name reference {} sits outside the columns it plots, so no header row is found",
            name.label()
        ));
    }
    Ok(name.min_row)
}

/// The A1 reference a `data.name` states. The `$` is dropped and the sheet qualifier KEPT, so a
/// figure reads against the tab its chart was drawn on however it is later moved.
fn binding(sheet: &str, region: Region) -> String {
    format!("{}!{}", quote_sheet(sheet), region.label())
}

/// One `<c:f>` as a rectangle of the chart's OWN sheet. A reference into another workbook, or onto
/// another tab, is a named loss: a figure binds one tab's table.
fn reference(text: Option<&str>, what: &str, sheet: &str) -> Result<Region, String> {
    let Some(text) = text else {
        return Err(format!("it states no reference for {what}"));
    };
    let plain = text.replace('$', "");
    let (tab, addr) = plain
        .rsplit_once('!')
        .ok_or_else(|| format!("its reference for {what} ({text:?}) names no sheet"))?;
    if tab.contains('[') {
        return Err(format!(
            "its reference for {what} ({text:?}) names another workbook"
        ));
    }
    let tab = tab.trim_matches('\'').replace("''", "'");
    if tab != sheet {
        return Err(format!(
            "its reference for {what} ({text:?}) names the tab {tab:?}, and the chart is drawn on \
             {sheet:?}"
        ));
    }
    let corner = |part: &str| {
        parse_a1(part).map_err(|_| {
            format!("its reference for {what} ({text:?}) is not a closed A1 rectangle")
        })
    };
    let (a, b) = match addr.split_once(':') {
        Some((l, r)) => (corner(l)?, corner(r)?),
        None => {
            let one = corner(addr)?;
            (one, one)
        }
    };
    Ok(Region {
        min_col: a.col.min(b.col),
        min_row: a.row.min(b.row),
        max_col: a.col.max(b.col),
        max_row: a.row.max(b.row),
    })
}

/// The figure's name: the chart's `<c:title>` where that yields a legal entry stem, else the chart
/// PART's own stem. A second chart resolving to a taken name falls back to the part name; only if
/// that collides too is it a named loss — never a silent overwrite.
fn stem_of(chart: &SourceChart, taken: &[String]) -> Result<String, String> {
    let part = chart
        .part
        .rsplit('/')
        .next()
        .and_then(|f| f.strip_suffix(".xml"))
        .unwrap_or(&chart.part)
        .to_string();
    let titled = chart.title.as_deref().and_then(legal_stem);
    for candidate in [titled, Some(part.clone())].into_iter().flatten() {
        if !taken.contains(&candidate) {
            return Ok(candidate);
        }
    }
    Err(format!(
        "the entry {part}{FIGURE_SUFFIX} is already taken by another chart on this sheet"
    ))
}

/// A stem is one path segment an author types, so it holds no separator, no control character and no
/// leading or trailing space or dot. `is_cell_filename` names the other exclusion: a stem that reads
/// as a coordinate would sit in a tab folder looking like a cell.
fn legal_stem(title: &str) -> Option<String> {
    let stem = title.trim();
    let legal = !stem.is_empty()
        && stem.chars().count() <= 64
        && !stem.starts_with('.')
        && !stem.ends_with('.')
        && stem
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-' | '(' | ')'))
        && !fsa1_model::is_cell_filename(stem);
    legal.then(|| stem.to_string())
}

/// A closed rectangle in 0-based coordinates, which is what `parse_a1` reports and `format_cell`
/// spells back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Region {
    min_col: u32,
    min_row: u32,
    max_col: u32,
    max_row: u32,
}

impl Region {
    fn union(self, other: Region) -> Region {
        Region {
            min_col: self.min_col.min(other.min_col),
            min_row: self.min_row.min(other.min_row),
            max_col: self.max_col.max(other.max_col),
            max_row: self.max_row.max(other.max_row),
        }
    }

    fn contains(self, other: Region) -> bool {
        other.min_col >= self.min_col
            && other.min_row >= self.min_row
            && other.max_col <= self.max_col
            && other.max_row <= self.max_row
    }

    /// `A1:B4`, or the single corner where the rectangle is one cell.
    fn label(self) -> String {
        let first = format_cell(self.min_col, self.min_row);
        if self.min_col == self.max_col && self.min_row == self.max_row {
            return first;
        }
        format!("{first}:{}", format_cell(self.max_col, self.max_row))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{SourceCell, SourceValue};

    /// A sheet from row-major text, `\t`-separated, so a fixture reads like the grid it becomes.
    fn sheet(name: &str, rows: &[&str]) -> SheetSource {
        let cells: Vec<Vec<&str>> = rows.iter().map(|r| r.split('\t').collect()).collect();
        let cols = cells.iter().map(Vec::len).max().unwrap_or(0) as u32;
        let mut flat = Vec::new();
        for row in &cells {
            for col in 0..cols as usize {
                let text = row.get(col).copied().unwrap_or("");
                let value = match (text.is_empty(), text.parse::<f64>()) {
                    (true, _) => SourceValue::Blank,
                    (_, Ok(n)) => SourceValue::Number(n),
                    (_, Err(_)) => SourceValue::Text(text.to_string()),
                };
                flat.push(SourceCell { value, style: None });
            }
        }
        SheetSource {
            name: name.to_string(),
            rows: cells.len() as u32,
            cols,
            cells: flat,
            ..Default::default()
        }
    }

    fn series(name: &str, cat: &str, val: &str) -> SourceSeries {
        SourceSeries {
            name_ref: Some(name.to_string()),
            cat: Some(cat.to_string()),
            val: Some(val.to_string()),
            literal: false,
        }
    }

    fn chart(plot: &str, series: Vec<SourceSeries>) -> SourceChart {
        SourceChart {
            grouping: None,
            part: "xl/charts/chart1.xml".to_string(),
            sheet: "Sheet1".to_string(),
            title: None,
            plots: vec![plot.to_string()],
            horizontal_bars: false,
            series,
        }
    }

    fn run(sheet: &SheetSource, charts: &[SourceChart]) -> (SheetFigures, Vec<UnpackWarning>) {
        let mut warnings = Vec::new();
        let refs: Vec<&SourceChart> = charts.iter().collect();
        let out = figures(sheet, &refs, &Resolution::empty(), &mut warnings);
        (out, warnings)
    }

    fn spec_of(figures: &SheetFigures, at: usize) -> Json {
        serde_json::from_str(&figures.files[at].1).expect("a figure body is JSON")
    }

    fn why(warnings: &[UnpackWarning]) -> String {
        match warnings {
            [UnpackWarning::ChartNotCarried { why, .. }] => why.clone(),
            other => panic!("expected exactly one chart loss, got {other:?}"),
        }
    }

    /// The whole read leg over the plan's own example: the two references union to a rectangle, the
    /// series name finds the header row above it, and the two headers become `x` and `y`.
    #[test]
    fn a_one_series_bar_chart_binds_the_rectangle_its_two_references_union_to() {
        let s = sheet(
            "Sheet1",
            &["Region\tUnits", "North\t12", "South\t9", "East\t15"],
        );
        let c = chart(
            "barChart",
            vec![series(
                "'Sheet1'!B1",
                "'Sheet1'!$A$2:$A$4",
                "'Sheet1'!$B$2:$B$4",
            )],
        );
        let (out, warnings) = run(&s, &[c]);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(out.carried, vec!["xl/charts/chart1.xml".to_string()]);
        assert_eq!(out.files[0].0, "chart1.vl.json");
        let spec = spec_of(&out, 0);
        assert_eq!(spec["mark"], "bar");
        assert_eq!(spec["data"]["name"], "Sheet1!A1:B4");
        assert_eq!(spec["encoding"]["x"]["field"], "Region");
        assert_eq!(spec["encoding"]["y"]["field"], "Units");
    }

    /// One `<c:ser>` is one layer, each binding its own rectangle — which is what plan 13's expander
    /// already walks.
    #[test]
    fn a_two_series_chart_is_two_layers_each_with_its_own_data() {
        let s = sheet(
            "Sheet1",
            &["Month\tAlpha\tBeta", "Jan\t1\t2", "Feb\t3\t4", "Mar\t5\t6"],
        );
        let c = chart(
            "lineChart",
            vec![
                series("'Sheet1'!B1", "'Sheet1'!$A$2:$A$4", "'Sheet1'!$B$2:$B$4"),
                series("'Sheet1'!C1", "'Sheet1'!$A$2:$A$4", "'Sheet1'!$C$2:$C$4"),
            ],
        );
        let (out, warnings) = run(&s, &[c]);
        assert!(warnings.is_empty(), "{warnings:?}");
        let spec = spec_of(&out, 0);
        let layers = spec["layer"].as_array().expect("two layers");
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0]["data"]["name"], "Sheet1!A1:B4");
        assert_eq!(layers[0]["encoding"]["y"]["field"], "Alpha");
        assert_eq!(layers[1]["data"]["name"], "Sheet1!A1:C4");
        assert_eq!(layers[1]["encoding"]["y"]["field"], "Beta");
    }

    /// The mark comes from the chart element, and an element with no mark is one named loss and no
    /// figure — never a refusal, and never a silently different chart.
    #[test]
    fn a_chart_element_with_no_mark_is_one_named_loss() {
        let s = sheet("Sheet1", &["Axis\tScore", "A\t1", "B\t2", "C\t3"]);
        let c = chart(
            "radarChart",
            vec![series(
                "'Sheet1'!B1",
                "'Sheet1'!$A$2:$A$4",
                "'Sheet1'!$B$2:$B$4",
            )],
        );
        let (out, warnings) = run(&s, &[c]);
        assert!(out.files.is_empty() && out.carried.is_empty());
        assert!(why(&warnings).contains("radarChart"), "{warnings:?}");
    }

    /// `<c:barDir val="bar">` swaps the axes, and a pie has none at all.
    #[test]
    fn the_bar_direction_swaps_the_axes_and_a_pie_encodes_theta() {
        let s = sheet("Sheet1", &["Region\tUnits", "North\t12", "South\t9"]);
        let one = series("'Sheet1'!B1", "'Sheet1'!$A$2:$A$3", "'Sheet1'!$B$2:$B$3");
        let mut bars = chart("barChart", vec![one.clone()]);
        bars.horizontal_bars = true;
        let (out, _) = run(&s, &[bars]);
        let spec = spec_of(&out, 0);
        assert_eq!(spec["encoding"]["y"]["field"], "Region");
        assert_eq!(spec["encoding"]["x"]["field"], "Units");

        let (out, _) = run(&s, &[chart("pieChart", vec![one])]);
        let spec = spec_of(&out, 0);
        assert_eq!(spec["mark"], "arc");
        assert_eq!(spec["encoding"]["theta"]["field"], "Units");
        assert_eq!(spec["encoding"]["color"]["field"], "Region");
    }

    /// A silently wrong chart is worse than an absent one, so each shape that would produce one is
    /// named instead: mismatched rows, a series plotted across a row, and a name reference that names
    /// no header row.
    #[test]
    fn a_series_no_one_rectangle_holds_is_named_rather_than_guessed() {
        let s = sheet(
            "Sheet1",
            &[
                "Region\tUnits\tX",
                "North\t12\t1",
                "South\t9\t2",
                "East\t15\t3",
            ],
        );
        let cases = [
            (
                series("'Sheet1'!B1", "'Sheet1'!$A$2:$A$4", "'Sheet1'!$B$3:$B$4"),
                "different rows",
            ),
            (
                series("'Sheet1'!B1", "'Sheet1'!$A$2:$C$2", "'Sheet1'!$A$3:$C$3"),
                "across rows",
            ),
            (
                series("'Sheet1'!B4", "'Sheet1'!$A$2:$A$4", "'Sheet1'!$B$2:$B$4"),
                "row above",
            ),
        ];
        for (one, needle) in cases {
            let (out, warnings) = run(&s, &[chart("barChart", vec![one])]);
            assert!(out.files.is_empty());
            assert!(why(&warnings).contains(needle), "{warnings:?}");
        }
    }

    /// A blank or repeated header ANYWHERE in the bound rectangle is a refusal `check` would raise on
    /// the figure, so it is named here instead of written and then refused.
    #[test]
    fn a_blank_or_repeated_header_in_the_bound_rectangle_is_named_here() {
        for (grid, needle) in [
            (&["Region\t\tUnits", "N\t1\t2", "S\t3\t4"], "blank"),
            (&["Region\tRegion\tUnits", "N\t1\t2", "S\t3\t4"], "repeats"),
        ] {
            let s = sheet("Sheet1", grid.as_slice());
            let c = chart(
                "barChart",
                vec![series(
                    "'Sheet1'!C1",
                    "'Sheet1'!$A$2:$A$3",
                    "'Sheet1'!$C$2:$C$3",
                )],
            );
            let (out, warnings) = run(&s, &[c]);
            assert!(out.files.is_empty(), "{grid:?}");
            assert!(why(&warnings).contains(needle), "{warnings:?}");
        }
    }

    /// A reference into another workbook, and a series carrying its values inline, each bind nothing.
    #[test]
    fn an_external_reference_and_a_literal_series_each_bind_nothing() {
        let s = sheet("Sheet1", &["Region\tUnits", "North\t12", "South\t9"]);
        let external = series("'Sheet1'!B1", "[1]Sheet1!$A$2:$A$3", "'Sheet1'!$B$2:$B$3");
        let (_, warnings) = run(&s, &[chart("barChart", vec![external])]);
        assert!(why(&warnings).contains("another workbook"), "{warnings:?}");

        let literal = SourceSeries {
            name_ref: Some("'Sheet1'!B1".to_string()),
            cat: Some("'Sheet1'!$A$2:$A$3".to_string()),
            val: None,
            literal: true,
        };
        let (_, warnings) = run(&s, &[chart("barChart", vec![literal])]);
        assert!(why(&warnings).contains("<c:numLit>"), "{warnings:?}");
    }

    /// The title names the figure where it is a legal stem; a second chart resolving to a taken name
    /// falls back to its part, and only a second collision is a loss.
    #[test]
    fn a_title_names_the_figure_and_a_collision_falls_back_to_the_part() {
        let s = sheet("Sheet1", &["Region\tUnits", "North\t12", "South\t9"]);
        let one = series("'Sheet1'!B1", "'Sheet1'!$A$2:$A$3", "'Sheet1'!$B$2:$B$3");
        let titled = |title: &str, part: &str| SourceChart {
            part: part.to_string(),
            title: Some(title.to_string()),
            ..chart("barChart", vec![one.clone()])
        };
        let (out, warnings) = run(
            &s,
            &[
                titled("Units by region", "xl/charts/chart1.xml"),
                titled("Units by region", "xl/charts/chart2.xml"),
                titled("a/slash is no stem", "xl/charts/chart3.xml"),
            ],
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let names: Vec<&str> = out.files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Units by region.vl.json",
                "chart2.vl.json",
                "chart3.vl.json"
            ]
        );
    }
}
