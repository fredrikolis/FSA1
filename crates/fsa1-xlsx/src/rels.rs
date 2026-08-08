// Concern: every _rels part — the package's, the workbook's, a sheet's and a drawing's rId wiring | Non-concern: the target parts' bytes, the content types | IO: (what each points at) -> the bytes

use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, Event};

const NS_PKG_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const INFALLIBLE: &str = "writing XML to an in-memory buffer is infallible";

const REL_OFFICE_DOC: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const REL_CORE_PROPS: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const REL_EXTENDED_PROPS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
const REL_WORKSHEET: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";
const REL_STYLES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
const REL_SHARED_STRINGS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings";
const REL_THEME: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";
const REL_DRAWING: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing";
const REL_CHART: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart";

/// A sheet's only relationship: the drawing its `<drawing r:id>` names.
pub(crate) fn emit_sheet_rels(drawing: usize) -> Vec<u8> {
    let target = format!("../drawings/drawing{drawing}.xml");
    emit_rels(&[("rId1", REL_DRAWING, target.as_str())])
}

/// One relationship per chart the drawing anchors, in the order its anchors state them.
pub(crate) fn emit_drawing_rels(charts: &[usize]) -> Vec<u8> {
    let rows: Vec<(String, String)> = charts
        .iter()
        .enumerate()
        .map(|(at, chart)| {
            (
                format!("rId{}", at + 1),
                format!("../charts/chart{chart}.xml"),
            )
        })
        .collect();
    let borrowed: Vec<(&str, &str, &str)> = rows
        .iter()
        .map(|(id, target)| (id.as_str(), REL_CHART, target.as_str()))
        .collect();
    emit_rels(&borrowed)
}

pub(crate) fn emit_package_rels() -> Vec<u8> {
    let rels = [
        ("rId1", REL_OFFICE_DOC, "xl/workbook.xml"),
        ("rId2", REL_CORE_PROPS, "docProps/core.xml"),
        ("rId3", REL_EXTENDED_PROPS, "docProps/app.xml"),
    ];
    emit_rels(&rels)
}

pub(crate) fn emit_workbook_rels(sheet_count: usize) -> Vec<u8> {
    let mut rows: Vec<(String, &str, String)> = Vec::new();
    for i in 0..sheet_count {
        rows.push((
            format!("rId{}", i + 1),
            REL_WORKSHEET,
            format!("worksheets/sheet{}.xml", i + 1),
        ));
    }
    let tail = [
        (REL_STYLES, "styles.xml"),
        (REL_SHARED_STRINGS, "sharedStrings.xml"),
        (REL_THEME, "theme/theme1.xml"),
    ];
    for (offset, (ty, target)) in tail.into_iter().enumerate() {
        let next = sheet_count + 1 + offset;
        rows.push((format!("rId{next}"), ty, target.to_string()));
    }
    let borrowed: Vec<(&str, &str, &str)> = rows
        .iter()
        .map(|(id, ty, target)| (id.as_str(), *ty, target.as_str()))
        .collect();
    emit_rels(&borrowed)
}

fn emit_rels(rels: &[(&str, &str, &str)]) -> Vec<u8> {
    let mut w = Writer::new(Vec::new());
    w.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))
    .expect(INFALLIBLE);
    let mut root = BytesStart::new("Relationships");
    root.push_attribute(("xmlns", NS_PKG_REL));
    w.write_event(Event::Start(root)).expect(INFALLIBLE);
    for (id, ty, target) in rels {
        let mut rel = BytesStart::new("Relationship");
        rel.push_attribute(("Id", *id));
        rel.push_attribute(("Type", *ty));
        rel.push_attribute(("Target", *target));
        w.write_event(Event::Empty(rel)).expect(INFALLIBLE);
    }
    w.write_event(Event::End(BytesEnd::new("Relationships")))
        .expect(INFALLIBLE);
    w.into_inner()
}
