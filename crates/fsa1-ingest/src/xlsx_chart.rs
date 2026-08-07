// Concern: reads a chart part into the marks, series and A1 references it states | Non-concern: spelling a figure body (figure_body.rs), a drawing's geometry | IO: (a package) -> Vec<SourceChart>
//! What a chart part STATES, and nothing about what a figure may do with it. The element names are
//! kept verbatim — `barChart`, `radarChart` — so a chart no figure can hold names itself in the loss.

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;
use zip::ZipArchive;

use crate::error::{ErrorKind, IngestError};
use crate::xlsx_meta::{attr, decode_text, read_entry, read_rels, sheet_name_by_part};

/// One `<c:ser>`: the two references it plots against each other, and the reference naming it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceSeries {
    /// `<c:tx><c:strRef><c:f>`, which NAMES the header row rather than leaving it assumed.
    pub name_ref: Option<String>,
    /// `<c:cat>`, or a scatter's `<c:xVal>`.
    pub cat: Option<String>,
    /// `<c:val>`, or a scatter's `<c:yVal>`.
    pub val: Option<String>,
    /// A `<c:numLit>` / `<c:strLit>` carries the VALUES themselves rather than a reference to them,
    /// so the series names no rectangle at all.
    pub literal: bool,
}

/// One chart part, and the tab whose drawing reaches it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceChart {
    /// The package path — `xl/charts/chart1.xml` — which is both what a loss names and the stem a
    /// figure falls back to.
    pub part: String,
    pub sheet: String,
    pub title: Option<String>,
    /// Every plot element `<c:plotArea>` states, by its own local name, in document order. ONE is a
    /// chart with a mark; several is a combination chart, which has none.
    pub plots: Vec<String>,
    /// `<c:barDir val="bar">`, the horizontal bar that swaps the axes.
    pub horizontal_bars: bool,
    /// `<c:grouping>` where the plot states one — `stacked`, `percentStacked`, `clustered`. Vega-Lite
    /// overlays layers by default, so a grouping this import cannot spell is a look the figure would
    /// silently get wrong, and it is NAMED rather than assumed away.
    pub grouping: Option<String>,
    pub series: Vec<SourceSeries>,
}

/// Every chart the package draws on a WORKSHEET, in package-path order. A chart no drawing reaches,
/// and a chartsheet's, are absent: neither is drawn on a tab this import writes.
pub fn read_charts(path: &Path) -> Result<Vec<SourceChart>, IngestError> {
    let file = File::open(path).map_err(|e| {
        IngestError::io(
            ErrorKind::SourceIo,
            format!("cannot open {:?} for its charts: {e}", path.display()),
        )
    })?;
    let mut zip = ZipArchive::new(BufReader::new(file)).map_err(|e| {
        IngestError::io(
            ErrorKind::SourceIo,
            format!("cannot read {:?} as a zip archive: {e}", path.display()),
        )
    })?;

    let mut on_sheet: Vec<(String, String)> = chart_sheets(&mut zip)?.into_iter().collect();
    on_sheet.sort();
    let mut out = Vec::with_capacity(on_sheet.len());
    for (part, sheet) in on_sheet {
        let Some(xml) = read_entry(&mut zip, &part)? else {
            continue;
        };
        out.push(parse_chart(part, sheet, &xml)?);
    }
    Ok(out)
}

/// `xl/worksheets/sheetN.xml` -> its `<drawing>` relationship -> `xl/drawings/drawingN.xml` -> its
/// chart relationships. The chart part maps to the SHEET NAME, which is the tab folder it lands in.
fn chart_sheets(
    zip: &mut ZipArchive<BufReader<File>>,
) -> Result<HashMap<String, String>, IngestError> {
    let sheet_names = sheet_name_by_part(zip)?;
    let mut worksheets: Vec<String> = zip
        .file_names()
        .filter(|n| {
            n.starts_with("xl/worksheets/") && n.ends_with(".xml") && !n.contains("/_rels/")
        })
        .map(str::to_string)
        .collect();
    worksheets.sort();
    let mut out = HashMap::new();
    for worksheet in worksheets {
        let Some(sheet) = sheet_names.get(&worksheet) else {
            continue; // a part `xl/workbook.xml` names no sheet for is on no tab
        };
        let drawings: Vec<String> = read_rels(zip, &worksheet)?
            .into_values()
            .filter(|t| t.starts_with("xl/drawings/") && t.ends_with(".xml"))
            .collect();
        for drawing in drawings {
            for target in read_rels(zip, &drawing)?.into_values() {
                if target.starts_with("xl/charts/") && target.ends_with(".xml") {
                    out.insert(target, sheet.clone());
                }
            }
        }
    }
    Ok(out)
}

/// The element path, by LOCAL name, so a package spelling the chart namespace `c:` and one leaving it
/// the default read identically. `<c:tx>` occurs under a series and under a title, and `<c:title>`
/// under the chart and under each axis, so every read below is anchored on a path rather than a name.
fn parse_chart(part: String, sheet: String, xml: &str) -> Result<SourceChart, IngestError> {
    let mut reader = Reader::from_str(xml);
    let mut path: Vec<String> = Vec::new();
    let mut chart = SourceChart {
        part,
        sheet,
        title: None,
        plots: Vec::new(),
        horizontal_bars: false,
        grouping: None,
        series: Vec::new(),
    };
    let mut title = String::new();
    loop {
        let event = reader.read_event().map_err(|e| {
            IngestError::io(
                ErrorKind::Invalid,
                format!("cannot read {:?}: {e}", chart.part),
            )
        })?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                let empty = matches!(event, Event::Empty(_));
                if path.last().is_some_and(|p| p == "plotArea") && name.ends_with("Chart") {
                    chart.plots.push(name.clone());
                }
                if name == "ser" && in_plot(&path) {
                    chart.series.push(SourceSeries::default());
                }
                if name == "barDir" && attr(e, b"val").as_deref() == Some("bar") {
                    chart.horizontal_bars = true;
                }
                if name == "grouping" && in_plot(&path) {
                    chart.grouping = attr(e, b"val").map(|v| v.to_string());
                }
                if let Some(series) = chart.series.last_mut()
                    && matches!(name.as_str(), "numLit" | "strLit" | "multiLvlStrRef")
                    && matches!(slot(&path), Some("cat" | "val" | "xVal" | "yVal"))
                {
                    series.literal = true;
                }
                path.push(name);
                if empty {
                    path.pop();
                }
            }
            Event::End(_) => {
                path.pop();
            }
            Event::Text(ref t) => {
                let text = decode_text(t)?;
                if is_chart_title(&path) {
                    title.push_str(&text);
                } else if path.last().is_some_and(|p| p == "f")
                    && let Some(series) = chart.series.last_mut()
                {
                    match slot(&path) {
                        Some("tx") => series.name_ref = Some(text),
                        Some("cat" | "xVal") => series.cat = Some(text),
                        Some("val" | "yVal") => series.val = Some(text),
                        _ => {}
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    chart.title = (!title.is_empty()).then_some(title);
    Ok(chart)
}

/// Which half of a series this path sits under — `tx`, `cat`, `val`, `xVal` or `yVal` — or `None`
/// outside a series altogether.
fn slot(path: &[String]) -> Option<&str> {
    let at = path.iter().rposition(|p| p == "ser")?;
    path.get(at + 1).map(String::as_str)
}

/// A `<c:ser>` of a PLOT, never one of the `<c:ser>` a data table or an axis could hold.
fn in_plot(path: &[String]) -> bool {
    path.last()
        .is_some_and(|p| p.ends_with("Chart") || p == "plotArea")
}

/// The chart's OWN title text: `chartSpace/chart/title/tx/rich/…/a:t`. An axis title sits under
/// `plotArea`, so anchoring on the path is what keeps an axis label out of the figure's name.
fn is_chart_title(path: &[String]) -> bool {
    path.last().is_some_and(|p| p == "t")
        && path.len() > 3
        && path[1] == "chart"
        && path[2] == "title"
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape openpyxl writes: no namespace prefix, the series name naming the header cell, and
    /// `<c:cat>` spelled as a `numRef` whatever the category column actually holds.
    const BAR: &str = r#"<chartSpace><chart><title><tx><rich><a:p><a:r><a:t>Units by region</a:t>
      </a:r></a:p></rich></tx></title><plotArea><barChart><barDir val="col"/><ser><idx val="0"/>
      <tx><strRef><f>'Sheet1'!B1</f></strRef></tx>
      <cat><numRef><f>'Sheet1'!$A$2:$A$4</f></numRef></cat>
      <val><numRef><f>'Sheet1'!$B$2:$B$4</f></numRef></val></ser></barChart>
      <catAx><title><tx><rich><a:p><a:r><a:t>an axis label</a:t></a:r></a:p></rich></tx></title>
      </catAx></plotArea></chart></chartSpace>"#;

    fn parse(xml: &str) -> SourceChart {
        parse_chart(
            "xl/charts/chart1.xml".to_string(),
            "Sheet1".to_string(),
            xml,
        )
        .expect("the part parses")
    }

    #[test]
    fn a_series_states_its_name_ref_and_its_two_references() {
        let chart = parse(BAR);
        assert_eq!(chart.plots, vec!["barChart".to_string()]);
        assert!(!chart.horizontal_bars);
        assert_eq!(chart.series.len(), 1);
        assert_eq!(chart.series[0].name_ref.as_deref(), Some("'Sheet1'!B1"));
        assert_eq!(chart.series[0].cat.as_deref(), Some("'Sheet1'!$A$2:$A$4"));
        assert_eq!(chart.series[0].val.as_deref(), Some("'Sheet1'!$B$2:$B$4"));
        assert!(!chart.series[0].literal);
    }

    /// An axis carries a `<c:title>` of its own, and reading it as the chart's would name the figure
    /// after a label rather than after the chart.
    #[test]
    fn the_title_is_the_charts_own_and_never_an_axis_label() {
        assert_eq!(parse(BAR).title.as_deref(), Some("Units by region"));
    }

    /// `<c:barDir val="bar">` is the horizontal bar, and the element's own local name is what a
    /// mark is read from — a chart with no mark names itself in the loss.
    #[test]
    fn the_plot_element_and_the_bar_direction_are_read_verbatim() {
        let chart = parse(
            r#"<chartSpace><chart><plotArea><barChart><barDir val="bar"/></barChart></plotArea>
               </chart></chartSpace>"#,
        );
        assert_eq!(chart.plots, vec!["barChart".to_string()]);
        assert!(chart.horizontal_bars);
        let radar = parse(
            r#"<chartSpace><chart><plotArea><radarChart><radarStyle val="standard"/></radarChart>
               </plotArea></chart></chartSpace>"#,
        );
        assert_eq!(radar.plots, vec!["radarChart".to_string()]);
    }

    /// A `<c:numLit>` carries the values themselves, so the series references no rectangle and a
    /// figure has nothing to bind.
    #[test]
    fn a_literal_series_is_marked_as_one() {
        let chart = parse(
            r#"<chartSpace><chart><plotArea><lineChart><ser>
               <val><numLit><pt idx="0"><v>1</v></pt></numLit></val>
               </ser></lineChart></plotArea></chart></chartSpace>"#,
        );
        assert!(chart.series[0].literal);
        assert_eq!(chart.series[0].val, None);
    }

    /// A scatter states `<c:xVal>`/`<c:yVal>` where every other chart states `<c:cat>`/`<c:val>`.
    #[test]
    fn a_scatter_series_reads_its_x_and_y_references() {
        let chart = parse(
            r#"<chartSpace><chart><plotArea><scatterChart><ser>
               <xVal><numRef><f>Sheet1!$A$2:$A$4</f></numRef></xVal>
               <yVal><numRef><f>Sheet1!$B$2:$B$4</f></numRef></yVal>
               </ser></scatterChart></plotArea></chart></chartSpace>"#,
        );
        assert_eq!(chart.series[0].cat.as_deref(), Some("Sheet1!$A$2:$A$4"));
        assert_eq!(chart.series[0].val.as_deref(), Some("Sheet1!$B$2:$B$4"));
    }
}
