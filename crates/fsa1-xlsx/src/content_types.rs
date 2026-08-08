// Concern: [Content_Types].xml — a Default per extension, an Override per emitted part | Non-concern: the parts' own bytes, zip assembly | IO: (sheet, chart and drawing counts) -> the part bytes

use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, Event};

const NS_CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const INFALLIBLE: &str = "writing XML to an in-memory buffer is infallible";

const CT_WORKBOOK: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
const CT_WORKSHEET: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const CT_STYLES: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml";
const CT_SHARED_STRINGS: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml";
const CT_THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";
const CT_CORE: &str = "application/vnd.openxmlformats-package.core-properties+xml";
const CT_APP: &str = "application/vnd.openxmlformats-officedocument.extended-properties+xml";
const CT_CHART: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
const CT_DRAWING: &str = "application/vnd.openxmlformats-officedocument.drawing+xml";

/// A chart part written without its content-type override opens as a repair prompt, which is worse
/// than no chart, so the two are emitted from one list.
pub(crate) fn emit(sheet_count: usize, charts: usize, drawings: usize) -> Vec<u8> {
    let mut w = Writer::new(Vec::new());
    w.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))
    .expect(INFALLIBLE);
    let mut root = BytesStart::new("Types");
    root.push_attribute(("xmlns", NS_CT));
    w.write_event(Event::Start(root)).expect(INFALLIBLE);

    for (ext, ct) in [
        (
            "rels",
            "application/vnd.openxmlformats-package.relationships+xml",
        ),
        ("xml", "application/xml"),
    ] {
        let mut d = BytesStart::new("Default");
        d.push_attribute(("Extension", ext));
        d.push_attribute(("ContentType", ct));
        w.write_event(Event::Empty(d)).expect(INFALLIBLE);
    }

    let mut overrides: Vec<(String, &str)> = vec![("/xl/workbook.xml".to_string(), CT_WORKBOOK)];
    for i in 0..sheet_count {
        overrides.push((format!("/xl/worksheets/sheet{}.xml", i + 1), CT_WORKSHEET));
    }
    for i in 0..drawings {
        overrides.push((format!("/xl/drawings/drawing{}.xml", i + 1), CT_DRAWING));
    }
    for i in 0..charts {
        overrides.push((format!("/xl/charts/chart{}.xml", i + 1), CT_CHART));
    }
    overrides.push(("/xl/styles.xml".to_string(), CT_STYLES));
    overrides.push(("/xl/sharedStrings.xml".to_string(), CT_SHARED_STRINGS));
    overrides.push(("/xl/theme/theme1.xml".to_string(), CT_THEME));
    overrides.push(("/docProps/core.xml".to_string(), CT_CORE));
    overrides.push(("/docProps/app.xml".to_string(), CT_APP));

    for (part, ct) in &overrides {
        let mut o = BytesStart::new("Override");
        o.push_attribute(("PartName", part.as_str()));
        o.push_attribute(("ContentType", *ct));
        w.write_event(Event::Empty(o)).expect(INFALLIBLE);
    }

    w.write_event(Event::End(BytesEnd::new("Types")))
        .expect(INFALLIBLE);
    w.into_inner()
}
