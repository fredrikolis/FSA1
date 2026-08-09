// Concern: reads the marks, series and A1 references a chart states, and where its drawing anchors them | Non-concern: spelling a figure body (fsa1-ingest) | IO: (a package) -> charts + anchors
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

/// The ONE sentence naming why a [`SourceSeries::literal`] series binds no rectangle. It names the
/// element the inline values sit in, which is this crate's fact.
pub fn inline_values_reason() -> String {
    "a series carries its values inline (<c:numLit>) rather than referencing cells, so it binds no \
     rectangle"
        .to_string()
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

/// One `<xdr:*Anchor>` as the part states it — where it starts, where it ends, how big it is, and
/// what it holds. A cell offset is EMU into that cell, exactly as the element spells it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceAnchor {
    /// The element's LOCAL name: `oneCellAnchor`, `twoCellAnchor` or `absoluteAnchor`.
    pub element: String,
    /// `<xdr:from>` as `(col, colOff, row, rowOff)`, all zero where the element states none.
    pub from: (u32, i64, u32, i64),
    /// `<xdr:to>`, which only a `twoCellAnchor` states.
    pub to: Option<(u32, i64, u32, i64)>,
    /// `<xdr:ext>` as `(cx, cy)`.
    pub ext: Option<(i64, i64)>,
    /// `editAs`, which states what the object does when its cells resize.
    pub edit_as: Option<String>,
    /// The chart part `<c:chart r:id>` reaches through the drawing's own relationships.
    pub chart: Option<String>,
    /// Every corner element — `<xdr:from><xdr:col>` — whose text is no number, and whose coordinate
    /// therefore stayed 0. A producer other than Excel is what writes one, and a position read as 0
    /// is a WRONG position, so the element is named here rather than left to look stated.
    pub unreadable: Vec<String>,
}

/// One drawing part, and what it anchors. A drawing holds shapes, text boxes and pictures as well as
/// charts, so an anchor this import does not reach is content that leaves with the part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceDrawing {
    pub part: String,
    /// Every `<xdr:*Anchor>` the part states, whatever it holds.
    pub anchors: Vec<SourceAnchor>,
    /// The chart parts its relationships reach, one per anchored chart.
    pub charts: Vec<String>,
}

impl SourceDrawing {
    /// The sentence naming what the part anchors that this import does not read back, or `None` where
    /// every anchor it holds is a chart. A chart that yielded no FIGURE is one named loss of its own,
    /// so it is deliberately not a second one here.
    pub fn non_chart_content(&self) -> Option<String> {
        if self.anchors.is_empty() {
            return Some("it anchors nothing this import reads back".to_string());
        }
        if self.anchors.len() > self.charts.len() {
            return Some(format!(
                "it anchors {} object(s) and only {} of them is a chart; a shape, a text box and a \
                 picture each leave with the part",
                self.anchors.len(),
                self.charts.len()
            ));
        }
        None
    }
}

/// Every chart the package draws on a WORKSHEET, every drawing part it holds, and every chart part
/// NO drawing reaches. That third list is the one a census would once have named: a chart part is
/// carried now, so an unreached one belongs to no warning family unless this pass hands it over.
pub fn read_package(path: &Path) -> Result<Package, IngestError> {
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
    let charts = read_charts(&mut zip)?;
    let drawings = read_drawings(&mut zip)?;
    let mut unreached: Vec<String> = zip
        .file_names()
        .filter(|n| crate::xlsx_meta::is_chart_part(n))
        .filter(|n| !charts.iter().any(|c| c.part == **n))
        .map(str::to_string)
        .collect();
    unreached.sort();
    Ok(Package {
        charts,
        drawings,
        unreached,
    })
}

/// What one package states about its charts.
#[derive(Default)]
pub struct Package {
    pub charts: Vec<SourceChart>,
    pub drawings: Vec<SourceDrawing>,
    /// Chart parts no worksheet drawing reaches — a chartsheet's, or an orphan.
    pub unreached: Vec<String>,
}

/// Every drawing part in the package, in package-path order — including one no worksheet points at,
/// which is content the package holds and this import still does not carry.
fn read_drawings(zip: &mut ZipArchive<BufReader<File>>) -> Result<Vec<SourceDrawing>, IngestError> {
    let mut parts: Vec<String> = zip
        .file_names()
        .filter(|n| n.starts_with("xl/drawings/") && n.ends_with(".xml") && !n.contains("/_rels/"))
        .map(str::to_string)
        .collect();
    parts.sort();
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        let rels = read_rels(zip, &part)?;
        let mut charts: Vec<String> = rels
            .values()
            .filter(|t| t.starts_with("xl/charts/") && t.ends_with(".xml"))
            .cloned()
            .collect();
        charts.sort();
        let anchors = match read_entry(zip, &part)? {
            Some(xml) => read_anchors(&part, &xml, &rels)?,
            None => Vec::new(),
        };
        out.push(SourceDrawing {
            part,
            anchors,
            charts,
        });
    }
    Ok(out)
}

/// Every `<xdr:oneCellAnchor>`, `<xdr:twoCellAnchor>` and `<xdr:absoluteAnchor>` the part states, by
/// LOCAL name, so a package leaving the drawing namespace default reads the same. An anchor holding
/// no chart is kept: it is the part's content just as much, and what it holds is the caller's call.
/// A graphic frame carries an `<a:ext>` of its own, so only the anchor's own child is its size.
fn read_anchors(
    part: &str,
    xml: &str,
    rels: &HashMap<String, String>,
) -> Result<Vec<SourceAnchor>, IngestError> {
    let mut reader = Reader::from_str(xml);
    let mut out: Vec<SourceAnchor> = Vec::new();
    let mut path: Vec<String> = Vec::new();
    loop {
        let event = reader.read_event().map_err(|e| {
            IngestError::io(ErrorKind::Invalid, format!("cannot read {part:?}: {e}"))
        })?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                let empty = matches!(event, Event::Empty(_));
                let in_anchor = path.last().is_some_and(|p| p.ends_with("Anchor"));
                if name.ends_with("Anchor") {
                    out.push(SourceAnchor {
                        element: name.clone(),
                        edit_as: attr(e, b"editAs"),
                        ..SourceAnchor::default()
                    });
                }
                if let Some(anchor) = out.last_mut() {
                    match name.as_str() {
                        "to" if in_anchor => anchor.to = Some((0, 0, 0, 0)),
                        "ext" if in_anchor => {
                            anchor.ext = Some((emu(attr(e, b"cx")), emu(attr(e, b"cy"))));
                        }
                        "chart" => {
                            anchor.chart = attr(e, b"r:id").and_then(|id| rels.get(&id).cloned());
                        }
                        _ => {}
                    }
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
                let Some(corner) = path.len().checked_sub(2).and_then(|i| path.get(i)) else {
                    continue;
                };
                let Some(field) = path.last() else {
                    continue;
                };
                let Some(anchor) = out.last_mut() else {
                    continue;
                };
                let slot = match corner.as_str() {
                    "from" => Some(&mut anchor.from),
                    "to" => anchor.to.as_mut(),
                    _ => None,
                };
                if let Some(slot) = slot {
                    let text = decode_text(t)?;
                    let number = text.trim();
                    let read = match field.as_str() {
                        "col" => Some(number.parse().map(|n| slot.0 = n).is_ok()),
                        "colOff" => Some(number.parse().map(|n| slot.1 = n).is_ok()),
                        "row" => Some(number.parse().map(|n| slot.2 = n).is_ok()),
                        "rowOff" => Some(number.parse().map(|n| slot.3 = n).is_ok()),
                        _ => None,
                    };
                    if read == Some(false) {
                        anchor
                            .unreadable
                            .push(format!("<xdr:{corner}><xdr:{field}>"));
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

fn emu(value: Option<String>) -> i64 {
    value.and_then(|v| v.trim().parse().ok()).unwrap_or(0)
}

fn read_charts(zip: &mut ZipArchive<BufReader<File>>) -> Result<Vec<SourceChart>, IngestError> {
    let mut on_sheet: Vec<(String, String)> = chart_sheets(zip)?.into_iter().collect();
    on_sheet.sort();
    let mut out = Vec::with_capacity(on_sheet.len());
    for (part, sheet) in on_sheet {
        let Some(xml) = read_entry(zip, &part)? else {
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
pub fn parse_chart(part: String, sheet: String, xml: &str) -> Result<SourceChart, IngestError> {
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

    fn anchors(xml: &str) -> Vec<SourceAnchor> {
        let rels = HashMap::from([("rId1".to_string(), "xl/charts/chart1.xml".to_string())]);
        read_anchors("xl/drawings/drawing1.xml", xml, &rels).expect("the part parses")
    }

    /// The corpus's own drawing: a fixed box at D2, sized by the anchor's own `<ext>`.
    #[test]
    fn a_one_cell_anchor_states_its_corner_its_size_and_its_chart() {
        let read = anchors(
            r#"<wsDr><oneCellAnchor><from><col>3</col><colOff>0</colOff><row>1</row>
               <rowOff>0</rowOff></from><ext cx="5400000" cy="2700000"/><graphicFrame><xfrm/>
               <a:graphic><a:graphicData><c:chart r:id="rId1"/></a:graphicData></a:graphic>
               </graphicFrame><clientData/></oneCellAnchor></wsDr>"#,
        );
        assert_eq!(
            read,
            vec![SourceAnchor {
                element: "oneCellAnchor".to_string(),
                from: (3, 0, 1, 0),
                to: None,
                ext: Some((5_400_000, 2_700_000)),
                edit_as: None,
                chart: Some("xl/charts/chart1.xml".to_string()),
                unreadable: Vec::new(),
            }]
        );
    }

    /// A producer other than Excel can write a coordinate that is no number. Read as 0 it would be a
    /// WRONG position stated as a right one, so the element it could not read is named and the
    /// caller decides what to say about it.
    #[test]
    fn a_coordinate_that_is_no_number_names_the_element_it_could_not_read() {
        let read = anchors(
            r#"<wsDr><xdr:twoCellAnchor><xdr:from><xdr:col>A</xdr:col><xdr:colOff>0</xdr:colOff>
               <xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>
               <xdr:to><xdr:col>10</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1e3</xdr:row>
               <xdr:rowOff>0</xdr:rowOff></xdr:to><xdr:graphicFrame><a:graphic><a:graphicData>
               <c:chart r:id="rId1"/></a:graphicData></a:graphic></xdr:graphicFrame>
               </xdr:twoCellAnchor></wsDr>"#,
        );
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].from, (0, 0, 1, 0));
        assert_eq!(read[0].to, Some((10, 0, 0, 0)));
        assert_eq!(
            read[0].unreadable,
            vec![
                "<xdr:from><xdr:col>".to_string(),
                "<xdr:to><xdr:row>".to_string()
            ],
        );
    }

    /// Both corners, their sub-cell offsets, and the `editAs` that says the object resizes. The
    /// frame's own `<a:ext>` is not the anchor's size.
    #[test]
    fn a_two_cell_anchor_states_both_corners_and_its_edit_as() {
        let read = anchors(
            r#"<wsDr><xdr:twoCellAnchor editAs="oneCell">
               <xdr:from><xdr:col>3</xdr:col><xdr:colOff>12700</xdr:colOff><xdr:row>1</xdr:row>
               <xdr:rowOff>6350</xdr:rowOff></xdr:from>
               <xdr:to><xdr:col>10</xdr:col><xdr:colOff>9525</xdr:colOff><xdr:row>16</xdr:row>
               <xdr:rowOff>3175</xdr:rowOff></xdr:to>
               <xdr:graphicFrame><xdr:xfrm><a:off x="0" y="0"/><a:ext cx="1" cy="2"/></xdr:xfrm>
               <a:graphic><a:graphicData><c:chart r:id="rId1"/></a:graphicData></a:graphic>
               </xdr:graphicFrame></xdr:twoCellAnchor></wsDr>"#,
        );
        assert_eq!(read[0].element, "twoCellAnchor");
        assert_eq!(read[0].from, (3, 12700, 1, 6350));
        assert_eq!(read[0].to, Some((10, 9525, 16, 3175)));
        assert_eq!(read[0].ext, None);
        assert_eq!(read[0].edit_as.as_deref(), Some("oneCell"));
        assert_eq!(read[0].chart.as_deref(), Some("xl/charts/chart1.xml"));
    }

    /// An `absoluteAnchor` states a sheet position in EMU and no cell at all.
    #[test]
    fn an_absolute_anchor_states_no_cell() {
        let read = anchors(
            r#"<wsDr><absoluteAnchor><pos x="914400" y="457200"/><ext cx="5400000" cy="2700000"/>
               <graphicFrame><a:graphic><a:graphicData><c:chart r:id="rId1"/></a:graphicData>
               </a:graphic></graphicFrame></absoluteAnchor></wsDr>"#,
        );
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].element, "absoluteAnchor");
        assert_eq!(read[0].from, (0, 0, 0, 0));
        assert_eq!(read[0].ext, Some((5_400_000, 2_700_000)));
        assert_eq!(read[0].chart.as_deref(), Some("xl/charts/chart1.xml"));
    }

    /// An anchor holding a picture is kept, so the count a loss reports is every object the part
    /// anchors and not only the charts among them.
    #[test]
    fn an_anchor_holding_no_chart_is_still_one_anchor() {
        let read = anchors(
            r#"<wsDr><oneCellAnchor><from><col>0</col><colOff>0</colOff><row>0</row>
               <rowOff>0</rowOff></from><ext cx="100" cy="200"/><pic><blipFill/></pic>
               </oneCellAnchor><twoCellAnchor><from><col>3</col><colOff>0</colOff><row>1</row>
               <rowOff>0</rowOff></from><to><col>10</col><colOff>0</colOff><row>16</row>
               <rowOff>0</rowOff></to><graphicFrame><a:graphic><a:graphicData>
               <c:chart r:id="rId1"/></a:graphicData></a:graphic></graphicFrame>
               </twoCellAnchor></wsDr>"#,
        );
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].chart, None);
        assert_eq!(read[0].ext, Some((100, 200)));
        assert_eq!(read[1].chart.as_deref(), Some("xl/charts/chart1.xml"));
        assert_eq!(read[1].to, Some((10, 0, 16, 0)));
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
