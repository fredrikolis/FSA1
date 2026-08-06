// Concern: xl/workbook.xml — the <sheets> list, <definedNames>, and <calcPr> | Non-concern: each sheet's cell data, the rel targets | IO: (sheet names + the name table) -> the part bytes

use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};

use fsa1_model::{Name, NameScope, NameTarget};

const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const NS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const INFALLIBLE: &str = "writing XML to an in-memory buffer is infallible";

pub(crate) fn emit(sheet_names: &[&str], names: &[Name]) -> Vec<u8> {
    let mut w = Writer::new(Vec::new());
    w.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))
    .expect(INFALLIBLE);

    let mut wb = BytesStart::new("workbook");
    wb.push_attribute(("xmlns", NS_MAIN));
    wb.push_attribute(("xmlns:r", NS_REL));
    w.write_event(Event::Start(wb)).expect(INFALLIBLE);

    w.write_event(Event::Start(BytesStart::new("sheets")))
        .expect(INFALLIBLE);
    for (i, name) in sheet_names.iter().enumerate() {
        let sheet_id = (i + 1).to_string();
        let rid = format!("rId{}", i + 1);
        let mut sheet = BytesStart::new("sheet");
        sheet.push_attribute(("name", *name));
        sheet.push_attribute(("sheetId", sheet_id.as_str()));
        sheet.push_attribute(("r:id", rid.as_str()));
        w.write_event(Event::Empty(sheet)).expect(INFALLIBLE);
    }
    w.write_event(Event::End(BytesEnd::new("sheets")))
        .expect(INFALLIBLE);

    emit_defined_names(&mut w, sheet_names, names);

    let mut calc = BytesStart::new("calcPr");
    calc.push_attribute(("calcId", "0"));
    calc.push_attribute(("fullCalcOnLoad", "1"));
    w.write_event(Event::Empty(calc)).expect(INFALLIBLE);

    w.write_event(Event::End(BytesEnd::new("workbook")))
        .expect(INFALLIBLE);
    w.into_inner()
}

fn emit_defined_names(w: &mut Writer<Vec<u8>>, sheet_names: &[&str], names: &[Name]) {
    if names.is_empty() {
        return;
    }
    let index_of = |sheet: &str| sheet_names.iter().position(|s| *s == sheet);

    let mut emitted = false;
    for n in names {
        let local = match &n.scope {
            NameScope::Workbook => None,
            NameScope::Sheet(sheet) => match index_of(sheet) {
                Some(i) => Some(i),
                None => continue, // a wrong localSheetId would rebind the name to another sheet
            },
        };
        if !emitted {
            w.write_event(Event::Start(BytesStart::new("definedNames")))
                .expect(INFALLIBLE);
            emitted = true;
        }
        let mut dn = BytesStart::new("definedName");
        dn.push_attribute(("name", n.ident.as_str()));
        let idx;
        if let Some(i) = local {
            idx = i.to_string();
            dn.push_attribute(("localSheetId", idx.as_str()));
        }
        w.write_event(Event::Start(dn)).expect(INFALLIBLE);
        let anchored;
        let body = match &n.target {
            NameTarget::Ref(a1) => {
                anchored = anchor_a1(a1);
                anchored.as_str()
            }
            NameTarget::Expr(e) => e.as_str(),
        };
        w.write_event(Event::Text(BytesText::new(body)))
            .expect(INFALLIBLE);
        w.write_event(Event::End(BytesEnd::new("definedName")))
            .expect(INFALLIBLE);
    }
    if emitted {
        w.write_event(Event::End(BytesEnd::new("definedNames")))
            .expect(INFALLIBLE);
    }
}

fn anchor_a1(a1: &str) -> String {
    let (sheet, refs) = match a1.rfind('!') {
        Some(i) => (&a1[..=i], &a1[i + 1..]),
        None => ("", a1),
    };
    let anchored: Vec<String> = refs
        .split(':')
        .map(|part| {
            if part.starts_with('$') || part.is_empty() {
                return part.to_string();
            }
            let split = part
                .find(|c: char| c.is_ascii_digit())
                .unwrap_or(part.len());
            let (col, row) = part.split_at(split);
            if col.is_empty() || row.is_empty() {
                part.to_string()
            } else {
                format!("${col}${row}")
            }
        })
        .collect();
    format!("{sheet}{}", anchored.join(":"))
}
