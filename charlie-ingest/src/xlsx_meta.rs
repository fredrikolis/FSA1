// Concern: read the xlsx METADATA PARTS that calamine's high-level `Reader` seam does not surface — the workbook's `definedName` entries WITH their `localSheetId` scope (from `xl/workbook.xml`) and every table's full extent (`xl/tables/*.xml`: `displayName`, `ref`, `headerRowCount`, `totalsRowCount`, and the ordered column header names) — returning them as neutral raw structs the reader folds into a `Resolution`. This is the SECOND format-firewall module (HARD RULE 4): `zip`/`quick-xml` are confined HERE (calamine stays in reader.rs), so charlie-model/ast never see a format library. A missing part (no defined names, no tables) is simply empty; only an unreadable/corrupt zip or malformed metadata part is a located CORE2 refusal | Non-concern: the table→sheet MAPPING (reader.rs derives it from calamine, which already navigates the sheet rels) and interpreting/validating a name target or table ref (resolve.rs owns classification + A1 geometry); reading cell VALUES/formulas (reader.rs owns calamine) | IO: (a `.xlsx` path) -> `Result<XlsxMeta, IngestError>` (parsed name + table metadata), reading only the workbook + table xml parts of the zip
//! xlsx metadata parts: [`read_meta`] pulls the scoped `definedName`s and the table `ref`/header/totals/
//! columns that the calamine `Reader` seam hides, as neutral [`RawName`]/[`RawTable`] structs. `zip` +
//! `quick-xml` live only here and in nowhere else but this crate's firewall (with calamine in reader.rs).

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use zip::ZipArchive;

use crate::error::{ErrorKind, IngestError};

/// One raw `definedName`: its spelling, its `localSheetId` (0-based sheet index) when sheet-local, and
/// its raw target text (`Sheet1!$A$1`, `MATCH(…)`, `{1,2,3}`, …). Classification happens in resolve.rs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawName {
    pub name: String,
    pub local_sheet_id: Option<u32>,
    pub target: String,
}

/// One raw table part: its `displayName`, its full `ref` rectangle (`A1:B89`), the header/totals row
/// counts (Excel defaults: 1 header row, 0 totals rows), and the ordered column header names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawTable {
    pub name: String,
    pub ref_str: String,
    pub header_rows: u32,
    pub totals_rows: u32,
    pub columns: Vec<String>,
}

/// The parsed xlsx metadata: scoped names + table extents. The reader folds these into a `Resolution`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct XlsxMeta {
    pub names: Vec<RawName>,
    pub tables: Vec<RawTable>,
}

/// Read the `definedName` + table metadata parts of an xlsx. A missing part yields no entries; an
/// unreadable zip or a malformed present part is a located [`IngestError`] (CORE2), never a panic.
pub fn read_meta(path: &Path) -> Result<XlsxMeta, IngestError> {
    let file = File::open(path).map_err(|e| {
        IngestError::io(
            ErrorKind::SourceIo,
            format!("cannot open {:?} for metadata: {e}", path.display()),
        )
    })?;
    let mut zip = ZipArchive::new(BufReader::new(file)).map_err(|e| {
        IngestError::io(
            ErrorKind::SourceIo,
            format!("cannot read {:?} as a zip archive: {e}", path.display()),
        )
    })?;

    // The workbook part holds the defined names (with their scope). It is always present in a real xlsx;
    // if it is somehow absent we simply have no names (the values still imported via calamine).
    let names = match read_entry(&mut zip, "xl/workbook.xml")? {
        Some(xml) => parse_defined_names(&xml)?,
        None => Vec::new(),
    };

    // Table parts live at `xl/tables/tableN.xml`. Collect their names first (an immutable borrow), then
    // read each (a mutable borrow) — the two cannot overlap.
    let table_parts: Vec<String> = zip
        .file_names()
        .filter(|n| n.starts_with("xl/tables/") && n.ends_with(".xml"))
        .map(str::to_string)
        .collect();
    let mut tables = Vec::with_capacity(table_parts.len());
    for part in table_parts {
        if let Some(xml) = read_entry(&mut zip, &part)?
            && let Some(t) = parse_table(&xml)?
        {
            tables.push(t);
        }
    }

    Ok(XlsxMeta { names, tables })
}

/// Read one zip entry to a UTF-8 string, or `None` if it is absent. A present-but-unreadable entry is a
/// located refusal.
fn read_entry(
    zip: &mut ZipArchive<BufReader<File>>,
    name: &str,
) -> Result<Option<String>, IngestError> {
    let mut entry = match zip.by_name(name) {
        Ok(e) => e,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => {
            return Err(IngestError::io(
                ErrorKind::SourceIo,
                format!("cannot read zip entry {name:?}: {e}"),
            ));
        }
    };
    let mut buf = String::new();
    entry.read_to_string(&mut buf).map_err(|e| {
        IngestError::io(
            ErrorKind::SourceIo,
            format!("cannot decode zip entry {name:?}: {e}"),
        )
    })?;
    Ok(Some(buf))
}

/// Parse `<definedName name= localSheetId=>target</definedName>` entries from `xl/workbook.xml`.
fn parse_defined_names(xml: &str) -> Result<Vec<RawName>, IngestError> {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    loop {
        match reader.read_event().map_err(xml_err)? {
            Event::Start(e) if e.local_name().as_ref() == b"definedName" => {
                let name = attr(&e, b"name").unwrap_or_default();
                let local_sheet_id = attr(&e, b"localSheetId").and_then(|s| s.parse::<u32>().ok());
                // The target is the element's text content (there is exactly one text run in practice).
                let mut target = String::new();
                loop {
                    match reader.read_event().map_err(xml_err)? {
                        Event::Text(t) => target.push_str(&decode_text(&t)?),
                        Event::End(end) if end.local_name().as_ref() == b"definedName" => break,
                        Event::Eof => break,
                        _ => {}
                    }
                }
                if !name.is_empty() {
                    out.push(RawName {
                        name,
                        local_sheet_id,
                        target,
                    });
                }
            }
            // Defined names are all inside <definedNames>; once the sheets/etc. follow we could stop, but
            // scanning to EOF is simple and cheap for a workbook part.
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

/// Parse one `xl/tables/tableN.xml` into a [`RawTable`] (`None` if it carries no usable `ref`).
fn parse_table(xml: &str) -> Result<Option<RawTable>, IngestError> {
    let mut reader = Reader::from_str(xml);
    let mut name = String::new();
    let mut ref_str = String::new();
    let mut header_rows = 1u32; // Excel default when the attribute is absent
    let mut totals_rows = 0u32;
    let mut columns = Vec::new();
    loop {
        let ev = reader.read_event().map_err(xml_err)?;
        match ev {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"table" => {
                // Prefer displayName (the reference spelling); fall back to name.
                name = attr(&e, b"displayName")
                    .or_else(|| attr(&e, b"name"))
                    .unwrap_or_default();
                ref_str = attr(&e, b"ref").unwrap_or_default();
                if let Some(h) = attr(&e, b"headerRowCount").and_then(|s| s.parse().ok()) {
                    header_rows = h;
                }
                if let Some(t) = attr(&e, b"totalsRowCount").and_then(|s| s.parse().ok()) {
                    totals_rows = t;
                }
            }
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"tableColumn" => {
                if let Some(c) = attr(&e, b"name") {
                    columns.push(c);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if name.is_empty() || ref_str.is_empty() {
        return Ok(None);
    }
    Ok(Some(RawTable {
        name,
        ref_str,
        header_rows,
        totals_rows,
        columns,
    }))
}

/// Fetch an element attribute by key, entity-unescaped. `None` if absent/undecodable. The XML is read
/// from a `&str` (UTF-8), so attribute value bytes are valid UTF-8; only entity unescaping remains.
fn attr(e: &BytesStart, key: &[u8]) -> Option<String> {
    for a in e.attributes().flatten() {
        if a.key.as_ref() == key {
            let raw = std::str::from_utf8(a.value.as_ref()).ok()?;
            return quick_xml::escape::unescape(raw)
                .ok()
                .map(|c| c.into_owned());
        }
    }
    None
}

/// Decode + entity-unescape a text run into an owned string.
fn decode_text(t: &quick_xml::events::BytesText<'_>) -> Result<String, IngestError> {
    let raw = t.decode().map_err(|e| {
        IngestError::io(ErrorKind::SourceIo, format!("cannot decode xml text: {e}"))
    })?;
    quick_xml::escape::unescape(&raw)
        .map(|c| c.into_owned())
        .map_err(|e| IngestError::io(ErrorKind::SourceIo, format!("bad xml entity: {e}")))
}

/// Map a quick-xml error into a located CORE2 refusal.
fn xml_err(e: quick_xml::Error) -> IngestError {
    IngestError::io(ErrorKind::SourceIo, format!("malformed xml metadata: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scoped_and_global_defined_names() {
        let xml = r#"<workbook><definedNames>
            <definedName name="TaxRate">Data!$H$1</definedName>
            <definedName name="Local" localSheetId="2">Sheet3!$A$1</definedName>
            <definedName name="Arr">{1,2,3}</definedName>
            </definedNames></workbook>"#;
        let names = parse_defined_names(xml).unwrap();
        assert_eq!(names.len(), 3);
        assert_eq!(
            names[0],
            RawName {
                name: "TaxRate".into(),
                local_sheet_id: None,
                target: "Data!$H$1".into()
            }
        );
        assert_eq!(names[1].local_sheet_id, Some(2));
        assert_eq!(names[2].target, "{1,2,3}");
    }

    #[test]
    fn parses_a_table_part_with_defaults_and_explicit_counts() {
        // 58296's shape: no headerRowCount/totalsRowCount attrs -> defaults (1 header, 0 totals).
        let t1 = parse_table(
            r#"<table displayName="Table13" ref="A1:B89" totalsRowShown="0">
                <tableColumns count="2"><tableColumn id="1" name="Date"/>
                <tableColumn id="3" name="Amount"/></tableColumns></table>"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(t1.name, "Table13");
        assert_eq!(t1.ref_str, "A1:B89");
        assert_eq!(t1.header_rows, 1);
        assert_eq!(t1.totals_rows, 0);
        assert_eq!(t1.columns, vec!["Date", "Amount"]);

        // Explicit totals row.
        let t2 = parse_table(
            r#"<table displayName="T" ref="A1:C5" headerRowCount="1" totalsRowCount="1">
                <tableColumns><tableColumn name="R"/><tableColumn name="Q1"/>
                <tableColumn name="Q2"/></tableColumns></table>"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(t2.totals_rows, 1);
        assert_eq!(t2.columns, vec!["R", "Q1", "Q2"]);
    }

    #[test]
    fn a_table_without_a_ref_is_skipped() {
        assert_eq!(parse_table("<table displayName=\"X\"/>").unwrap(), None);
    }
}
