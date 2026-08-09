// Concern: xl/charts/chartN.xml and the drawing anchoring it to a sheet | Non-concern: which figure becomes one, the rels and content types | IO: (Chart) -> chart bytes; (placements, axes) -> drawing

use fsa1_ast::a1::format_cell;
use fsa1_model::{Axis, Placement};
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};

const NS_CHART: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const NS_DRAWING: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_SHEET_DRAWING: &str =
    "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";
const NS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const INFALLIBLE: &str = "writing XML to an in-memory buffer is infallible";

/// The package path of the FIRST chart part, which is what a caller writing one chart names it by.
pub const CHART_PART: &str = "xl/charts/chart1.xml";

/// One hand-written bar-chart part, as this crate spells one: a specimen a caller grades its own
/// chart handling against without first packing a workbook to obtain one.
pub const BAR_CHART_PART: &str = r#"<c:chartSpace xmlns:c="c"><c:chart><c:plotArea><c:barChart>
        <c:barDir val="col"/><c:ser>
        <c:tx><c:strRef><c:f>Sheet1!$B$1</c:f></c:strRef></c:tx>
        <c:cat><c:numRef><c:f>Sheet1!$A$2:$A$4</c:f></c:numRef></c:cat>
        <c:val><c:numRef><c:f>Sheet1!$B$2:$B$4</c:f></c:numRef></c:val>
        </c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;

/// One `<c:ser>`: the two references it plots against each other, and the one cell naming it. Each is
/// plain A1 — `Sheet1!A2:A4` — because the `$` is Excel's spelling of a reference, not a fact about
/// the range, and this file is where it is put back on.
pub(crate) struct ChartSeries {
    pub name_ref: String,
    pub cat: String,
    pub val: String,
}

/// One chart part, ready to write: everything a `<c:chartSpace>` states and nothing about the figure
/// it was derived from.
pub struct Chart {
    pub(crate) sheet: u32,
    pub(crate) title: Option<String>,
    /// The plot element's own local name — `barChart`, `pieChart` — which is what the reader reads a
    /// mark back from.
    pub(crate) element: &'static str,
    /// `<c:barDir val="bar">`, the horizontal bar that swaps the axes. Only a bar chart has one.
    pub(crate) horizontal: bool,
    pub(crate) series: Vec<ChartSeries>,
    /// Where the figure said it sits, or `None` for one that said nothing and is derived a spot.
    pub(crate) placement: Option<Placement>,
}

impl Chart {
    /// Which sheet the chart is drawn on, so a caller can grade it against that tab.
    pub fn sheet(&self) -> u32 {
        self.sheet
    }
}

/// The part bytes as text, so a caller can read the chart back before deciding to ship it.
pub fn chart_xml(chart: &Chart) -> String {
    String::from_utf8(emit_chart(chart)).expect("the writer emits UTF-8")
}

pub(crate) fn emit_chart(chart: &Chart) -> Vec<u8> {
    let mut w = Writer::new(Vec::new());
    decl(&mut w);
    let mut root = BytesStart::new("c:chartSpace");
    root.push_attribute(("xmlns:c", NS_CHART));
    root.push_attribute(("xmlns:a", NS_DRAWING));
    root.push_attribute(("xmlns:r", NS_REL));
    start(&mut w, root);
    start(&mut w, BytesStart::new("c:chart"));
    if let Some(title) = &chart.title {
        write_title(&mut w, title);
    }
    start(&mut w, BytesStart::new("c:plotArea"));
    empty(&mut w, "c:layout", &[]);
    write_plot(&mut w, chart);
    write_axes(&mut w, chart);
    end(&mut w, "c:plotArea");
    empty(&mut w, "c:plotVisOnly", &[("val", "1")]);
    end(&mut w, "c:chart");
    end(&mut w, "c:chartSpace");
    w.into_inner()
}

fn write_title(w: &mut Writer<Vec<u8>>, title: &str) {
    start(w, BytesStart::new("c:title"));
    start(w, BytesStart::new("c:tx"));
    start(w, BytesStart::new("c:rich"));
    empty(w, "a:bodyPr", &[]);
    start(w, BytesStart::new("a:p"));
    start(w, BytesStart::new("a:r"));
    start(w, BytesStart::new("a:t"));
    w.write_event(Event::Text(BytesText::new(title)))
        .expect(INFALLIBLE);
    end(w, "a:t");
    end(w, "a:r");
    end(w, "a:p");
    end(w, "c:rich");
    end(w, "c:tx");
    empty(w, "c:overlay", &[("val", "0")]);
    end(w, "c:title");
    empty(w, "c:autoTitleDeleted", &[("val", "0")]);
}

/// The plot element, its series, and the axis ids the axes below cross on. A pie states neither a
/// direction, a grouping nor an axis; a scatter plots two MEASURES, so its series states `<c:xVal>`
/// and `<c:yVal>` where every other chart states `<c:cat>` and `<c:val>`.
fn write_plot(w: &mut Writer<Vec<u8>>, chart: &Chart) {
    let element = format!("c:{}", chart.element);
    start(w, BytesStart::new(element.as_str()));
    if chart.element == "barChart" {
        let dir = if chart.horizontal { "bar" } else { "col" };
        empty(w, "c:barDir", &[("val", dir)]);
    }
    match chart.element {
        "barChart" => empty(w, "c:grouping", &[("val", "clustered")]),
        "lineChart" | "areaChart" => empty(w, "c:grouping", &[("val", "standard")]),
        // `marker`, never `lineMarker`: a `point` states no line, and Excel would draw one the figure never asked for.
        "scatterChart" => empty(w, "c:scatterStyle", &[("val", "marker")]),
        _ => {}
    }
    // A pie's `color` channel IS its per-slice colour; every other mark keeps one colour per series.
    let vary = u8::from(chart.element == "pieChart").to_string();
    empty(w, "c:varyColors", &[("val", &vary)]);
    let (cat_tag, val_tag) = match chart.element {
        "scatterChart" => ("c:xVal", "c:yVal"),
        _ => ("c:cat", "c:val"),
    };
    for (at, series) in chart.series.iter().enumerate() {
        let at = at.to_string();
        start(w, BytesStart::new("c:ser"));
        empty(w, "c:idx", &[("val", at.as_str())]);
        empty(w, "c:order", &[("val", at.as_str())]);
        write_ref(w, "c:tx", "c:strRef", &series.name_ref);
        write_ref(w, cat_tag, "c:numRef", &series.cat);
        write_ref(w, val_tag, "c:numRef", &series.val);
        end(w, "c:ser");
    }
    for id in axis_ids(chart) {
        empty(w, "c:axId", &[("val", id)]);
    }
    end(w, element.as_str());
}

/// One `<c:f>` under the slot and reference kind that hold it, spelled the way Excel spells a
/// reference: `Sheet1!A2:A4` becomes `Sheet1!$A$2:$A$4`.
fn write_ref(w: &mut Writer<Vec<u8>>, slot: &str, kind: &str, reference: &str) {
    start(w, BytesStart::new(slot));
    start(w, BytesStart::new(kind));
    start(w, BytesStart::new("c:f"));
    w.write_event(Event::Text(BytesText::new(&absolute(reference))))
        .expect(INFALLIBLE);
    end(w, "c:f");
    end(w, kind);
    end(w, slot);
}

/// A category axis and a value axis crossing on the ids the plot states. A scatter has two VALUE
/// axes, and a pie has none at all.
fn write_axes(w: &mut Writer<Vec<u8>>, chart: &Chart) {
    let [cat, val] = match axis_ids(chart) {
        [] => return,
        ids => [ids[0], ids[1]],
    };
    let category = match chart.element {
        "scatterChart" => "c:valAx",
        _ => "c:catAx",
    };
    let (cat_pos, val_pos) = if chart.horizontal {
        ("l", "b")
    } else {
        ("b", "l")
    };
    for (tag, id, pos, cross) in [
        (category, cat, cat_pos, val),
        ("c:valAx", val, val_pos, cat),
    ] {
        start(w, BytesStart::new(tag));
        empty(w, "c:axId", &[("val", id)]);
        start(w, BytesStart::new("c:scaling"));
        empty(w, "c:orientation", &[("val", "minMax")]);
        end(w, "c:scaling");
        empty(w, "c:delete", &[("val", "0")]);
        empty(w, "c:axPos", &[("val", pos)]);
        empty(w, "c:crossAx", &[("val", cross)]);
        end(w, tag);
    }
}

/// Fixed ids, because one chart part holds one plot and its two axes: nothing here can collide.
fn axis_ids(chart: &Chart) -> &'static [&'static str] {
    match chart.element {
        "pieChart" => &[],
        _ => &["111111111", "222222222"],
    }
}

/// `Sheet1!A2:A4` -> `Sheet1!$A$2:$A$4`. The sheet qualifier is left exactly as the caller quoted it.
fn absolute(reference: &str) -> String {
    let Some((tab, addr)) = reference.rsplit_once('!') else {
        return reference.to_string();
    };
    let corners: Vec<String> = addr.split(':').map(pin).collect();
    format!("{tab}!{}", corners.join(":"))
}

fn pin(corner: &str) -> String {
    match corner.find(|c: char| c.is_ascii_digit()) {
        Some(at) => format!("${}${}", &corner[..at], &corner[at..]),
        None => corner.to_string(),
    }
}

/// One sheet's whole drawing: one anchor per chart, in the order `placements` gives them, each
/// holding the graphic frame that names the chart part through the drawing's own `rIdN`. Nothing
/// else is anchored — which is what lets the reader call the part carried. Both stated forms emit a
/// `<xdr:twoCellAnchor>`; `oneCellAnchor` and `absoluteAnchor` are never written.
pub(crate) fn emit_drawing(
    placements: &[Option<Placement>],
    base_col: u32,
    cols: &Axis,
    rows: &Axis,
) -> Vec<u8> {
    let mut w = Writer::new(Vec::new());
    decl(&mut w);
    let mut root = BytesStart::new("xdr:wsDr");
    root.push_attribute(("xmlns:xdr", NS_SHEET_DRAWING));
    root.push_attribute(("xmlns:a", NS_DRAWING));
    start(&mut w, root);
    for (at, placement) in placements.iter().enumerate() {
        let mut anchor = BytesStart::new("xdr:twoCellAnchor");
        // A fixed box keeps its size when a column resizes, which is exactly what `oneCell` states.
        if matches!(placement, Some(Placement::Box { .. })) {
            anchor.push_attribute(("editAs", "oneCell"));
        }
        start(&mut w, anchor);
        let (from, to) = corners(placement.as_ref(), at, base_col, cols, rows);
        write_anchor(&mut w, "xdr:from", from);
        write_anchor(&mut w, "xdr:to", to);
        write_frame(&mut w, at);
        empty(&mut w, "xdr:clientData", &[]);
        end(&mut w, "xdr:twoCellAnchor");
    }
    end(&mut w, "xdr:wsDr");
    w.into_inner()
}

/// One anchor's two ends as (col, colOff, row, rowOff). `to` is EXCLUSIVE, exactly as the derived
/// form writes it: `base_col + 8` spans eight columns, so a range anchor's far corner is one past
/// the last cell it fills.
fn corners(
    placement: Option<&Placement>,
    at: usize,
    base_col: u32,
    cols: &Axis,
    rows: &Axis,
) -> (Anchor, Anchor) {
    match placement {
        // Stacked down the first free column, so two charts on one sheet never sit on top of each other.
        None => {
            let top = (at * 16) as u32;
            (
                Anchor(base_col, 0, top, 0),
                Anchor(base_col + 8, 0, top + 15, 0),
            )
        }
        Some(Placement::Cells(rect)) => (
            Anchor(rect.min_col, 0, rect.min_row, 0),
            Anchor(rect.max_col + 1, 0, rect.max_row + 1, 0),
        ),
        Some(Placement::Box {
            at: (col, row),
            left,
            top,
            w,
            h,
        }) => {
            let (x, y) = (cols.edge(*col) + left, rows.edge(*row) + top);
            let (from_col, from_col_off) = cols.locate(x);
            let (from_row, from_row_off) = rows.locate(y);
            let (to_col, to_col_off) = cols.locate(x + w);
            let (to_row, to_row_off) = rows.locate(y + h);
            (
                Anchor(from_col, from_col_off, from_row, from_row_off),
                Anchor(to_col, to_col_off, to_row, to_row_off),
            )
        }
    }
}

/// One end of an anchor: its column and sub-column offset, then its row and sub-row offset.
struct Anchor(u32, i64, u32, i64);

fn write_anchor(w: &mut Writer<Vec<u8>>, tag: &str, Anchor(col, col_off, row, row_off): Anchor) {
    start(w, BytesStart::new(tag));
    for (part, value) in [
        ("xdr:col", i64::from(col)),
        ("xdr:colOff", col_off),
        ("xdr:row", i64::from(row)),
        ("xdr:rowOff", row_off),
    ] {
        text(w, part, &value.to_string());
    }
    end(w, tag);
}

fn write_frame(w: &mut Writer<Vec<u8>>, at: usize) {
    start(w, BytesStart::new("xdr:graphicFrame"));
    start(w, BytesStart::new("xdr:nvGraphicFramePr"));
    let mut name = BytesStart::new("xdr:cNvPr");
    name.push_attribute(("id", (at + 2).to_string().as_str()));
    name.push_attribute(("name", format!("Chart {}", at + 1).as_str()));
    w.write_event(Event::Empty(name)).expect(INFALLIBLE);
    empty(w, "xdr:cNvGraphicFramePr", &[]);
    end(w, "xdr:nvGraphicFramePr");
    start(w, BytesStart::new("xdr:xfrm"));
    empty(w, "a:off", &[("x", "0"), ("y", "0")]);
    empty(w, "a:ext", &[("cx", "0"), ("cy", "0")]);
    end(w, "xdr:xfrm");
    start(w, BytesStart::new("a:graphic"));
    let mut data = BytesStart::new("a:graphicData");
    data.push_attribute(("uri", NS_CHART));
    start(w, data);
    let mut reference = BytesStart::new("c:chart");
    reference.push_attribute(("xmlns:c", NS_CHART));
    reference.push_attribute(("xmlns:r", NS_REL));
    reference.push_attribute(("r:id", format!("rId{}", at + 1).as_str()));
    w.write_event(Event::Empty(reference)).expect(INFALLIBLE);
    end(w, "a:graphicData");
    end(w, "a:graphic");
    end(w, "xdr:graphicFrame");
}

/// The first column past the tab's content, so a chart never covers the cells it plots.
pub(crate) fn anchor_column(content: Option<fsa1_model::Rect>) -> u32 {
    content.map_or(0, |r| r.max_col + 2)
}

/// The A1 spelling of one coordinate, which the derivation builds every reference from.
pub(crate) fn cell(col: u32, row: u32) -> String {
    format_cell(col, row)
}

fn decl(w: &mut Writer<Vec<u8>>) {
    w.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))
    .expect(INFALLIBLE);
}

fn start(w: &mut Writer<Vec<u8>>, tag: BytesStart<'_>) {
    w.write_event(Event::Start(tag)).expect(INFALLIBLE);
}

fn end(w: &mut Writer<Vec<u8>>, name: &str) {
    w.write_event(Event::End(BytesEnd::new(name)))
        .expect(INFALLIBLE);
}

fn empty(w: &mut Writer<Vec<u8>>, name: &str, attrs: &[(&str, &str)]) {
    let mut tag = BytesStart::new(name);
    for attr in attrs {
        tag.push_attribute(*attr);
    }
    w.write_event(Event::Empty(tag)).expect(INFALLIBLE);
}

fn text(w: &mut Writer<Vec<u8>>, name: &str, value: &str) {
    start(w, BytesStart::new(name));
    w.write_event(Event::Text(BytesText::new(value)))
        .expect(INFALLIBLE);
    end(w, name);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `$` is Excel's spelling of a reference, put back on at the one boundary that writes one.
    #[test]
    fn a_reference_is_pinned_on_the_way_out_and_its_sheet_left_alone() {
        assert_eq!(absolute("Sheet1!A2:A4"), "Sheet1!$A$2:$A$4");
        assert_eq!(absolute("'My Tab'!B1"), "'My Tab'!$B$1");
        assert_eq!(absolute("A1"), "A1");
    }

    /// A pie has no axes, so it states no `<c:axId>` for one to cross on.
    #[test]
    fn a_pie_states_no_axis_and_a_bar_states_two() {
        let series = || ChartSeries {
            name_ref: "Sheet1!B1".to_string(),
            cat: "Sheet1!A2:A3".to_string(),
            val: "Sheet1!B2:B3".to_string(),
        };
        let chart = |element| Chart {
            sheet: 0,
            title: None,
            element,
            horizontal: false,
            series: vec![series()],
            placement: None,
        };
        let pie = chart_xml(&chart("pieChart"));
        assert!(!pie.contains("axId"), "{pie}");
        assert!(!pie.contains("c:catAx"), "{pie}");
        let bar = chart_xml(&chart("barChart"));
        assert_eq!(bar.matches("<c:axId").count(), 4, "{bar}");
        assert!(bar.contains("<c:f>Sheet1!$A$2:$A$3</c:f>"), "{bar}");
    }
}
