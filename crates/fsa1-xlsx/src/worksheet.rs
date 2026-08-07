// Concern: frames one sheet's stated region as <dimension>, <cols> and sized <row> | Non-concern: per-cell spelling, the style table | IO: (a Workbook, an Overlay, a sheet) -> sheetN.xml bytes

use std::collections::HashMap;
use std::io::Write;

use fsa1_ast::Value;
use fsa1_ast::a1::format_cell;
use fsa1_model::{AxisRun, Cell, Chars, Overlay, Points, Rect, Workbook};
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, Event};

use crate::cell;
use crate::shared_strings::SharedStrings;
use crate::styles::StyleTable;

const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const NS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const INFALLIBLE: &str = "writing XML to an in-memory buffer is infallible";

pub(crate) fn emit(
    wb: &Workbook,
    overlay: &Overlay,
    sheet: u32,
    ss: &mut SharedStrings,
    styles: &StyleTable,
) -> Vec<u8> {
    emit_inner(wb, overlay, sheet, ss, styles).expect(INFALLIBLE)
}

fn emit_inner(
    wb: &Workbook,
    overlay: &Overlay,
    sheet: u32,
    ss: &mut SharedStrings,
    styles: &StyleTable,
) -> std::io::Result<Vec<u8>> {
    // A `<cols>` run and a style-only block both sit outside the content, so the sheet is framed over the union of what the tab values and what it presents.
    let region = overlay.stated_region(wb, sheet);
    let anchors = array_anchors(wb, sheet);

    let mut w = Writer::new(Vec::new());
    w.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))?;

    let mut ws = BytesStart::new("worksheet");
    ws.push_attribute(("xmlns", NS_MAIN));
    ws.push_attribute(("xmlns:r", NS_REL));
    w.write_event(Event::Start(ws))?;

    let dim = dimension_ref(region);
    let mut dimension = BytesStart::new("dimension");
    dimension.push_attribute(("ref", dim.as_str()));
    w.write_event(Event::Empty(dimension))?;

    write_cols(&mut w, &overlay.column_widths(wb, sheet))?;

    let heights = overlay.row_heights(wb, sheet);
    // What a coordinate a scope root covers and no file does holds: nothing but its look, which is exactly what a covered blank holds, so the two take one arm below.
    let style_only = Cell::Value {
        value: Value::Blank,
        format: None,
    };
    w.write_event(Event::Start(BytesStart::new("sheetData")))?;
    if let Some(region) = region {
        for row in region.min_row..=region.max_row {
            let mut cells: Vec<cell::Sited<'_>> = Vec::new();
            for col in region.min_col..=region.max_col {
                // The stated region spans block roots too, so a coordinate inside it may be stated by nothing at all; that is the gap, and it writes no `<c>`.
                let Some(style) = overlay.cell_style(wb, sheet, col, row) else {
                    continue;
                };
                let source = wb.source_at(sheet, col, row);
                if source.is_some_and(|cs| cs.array_continuation) {
                    continue;
                }
                let cell = source.map_or(&style_only, |cs| cs.cell);
                if matches!(
                    cell,
                    Cell::Value {
                        value: Value::Blank,
                        ..
                    }
                ) {
                    // A blank's whole content is its look, so every look is carried — deliberately wider than the read leg's occupancy, which is only what SHOWS on a blank.
                    let carried = styles.index_of(&style, cell);
                    debug_assert!(
                        !style.blank_paint().shows() || carried.is_some(),
                        "a fill or an edge mints an <xf>, so a blank the read leg counts as occupancy is always carried",
                    );
                    if carried.is_none() {
                        continue;
                    }
                }
                cells.push(cell::Sited {
                    col,
                    cell,
                    style,
                    array_ref: anchors.get(&(col, row)).copied(),
                });
            }
            let height = size_at(&heights, row);
            // A sized row survives its own emptiness: dropping it would drop the height with it.
            if cells.is_empty() && height.is_none() {
                continue;
            }
            let rnum = (row + 1).to_string();
            let ht = height.map(|pt| pt.to_string());
            let mut row_el = BytesStart::new("row");
            row_el.push_attribute(("r", rnum.as_str()));
            if let Some(ht) = ht.as_deref() {
                row_el.push_attribute(("ht", ht));
                row_el.push_attribute(("customHeight", "1"));
            }
            if cells.is_empty() {
                w.write_event(Event::Empty(row_el))?;
                continue;
            }
            w.write_event(Event::Start(row_el))?;
            for sited in &cells {
                cell::write_cell(&mut w, row, sited, ss, styles)?;
            }
            w.write_event(Event::End(BytesEnd::new("row")))?;
        }
    }
    w.write_event(Event::End(BytesEnd::new("sheetData")))?;
    w.write_event(Event::End(BytesEnd::new("worksheet")))?;
    Ok(w.into_inner())
}

/// `<cols>` precedes `<sheetData>`, and each run is one `<col>` over the 1-based sheet columns it
/// covers. The width is the authored number itself: a `<col width>` and a `ch` are both the `0`
/// digit's width in the workbook's font, so no conversion stands between them.
fn write_cols<W: Write>(w: &mut Writer<W>, runs: &[AxisRun<Chars>]) -> std::io::Result<()> {
    if runs.is_empty() {
        return Ok(());
    }
    w.write_event(Event::Start(BytesStart::new("cols")))?;
    for run in runs {
        let (min, max, width) = (
            (run.start + 1).to_string(),
            (run.end + 1).to_string(),
            run.size.0.to_string(),
        );
        let mut col = BytesStart::new("col");
        col.push_attribute(("min", min.as_str()));
        col.push_attribute(("max", max.as_str()));
        col.push_attribute(("width", width.as_str()));
        col.push_attribute(("customWidth", "1"));
        w.write_event(Event::Empty(col))?;
    }
    w.write_event(Event::End(BytesEnd::new("cols")))?;
    Ok(())
}

fn size_at(runs: &[AxisRun<Points>], axis: u32) -> Option<f64> {
    runs.iter()
        .find(|run| run.start <= axis && axis <= run.end)
        .map(|run| run.size.0)
}

fn array_anchors(wb: &Workbook, sheet: u32) -> HashMap<(u32, u32), Rect> {
    wb.tab_files(sheet)
        .into_iter()
        .flatten()
        .filter(|f| f.array_formula)
        .map(|f| ((f.region.min_col, f.region.min_row), f.region))
        .collect()
}

fn dimension_ref(region: Option<Rect>) -> String {
    match region {
        None => "A1".to_string(),
        Some(r) => {
            let tl = format_cell(r.min_col, r.min_row);
            if r.min_col == r.max_col && r.min_row == r.max_row {
                tl
            } else {
                format!("{tl}:{}", format_cell(r.max_col, r.max_row))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::styles;

    /// One tree, read by both loads, exactly as a verb reads a directory twice.
    fn loaded(files: &[(&str, &str)]) -> (Workbook, Overlay) {
        let wb = Workbook::from_tabs(&[("Sheet1", files)])
            .unwrap_or_else(|d| panic!("{files:?} should load: {:?}", d[0]));
        let overlay = Overlay::from_tabs(&[("Sheet1", files)])
            .unwrap_or_else(|d| panic!("{files:?}'s sidecars should load: {:?}", d[0]));
        (wb, overlay)
    }

    fn sheet_xml(files: &[(&str, &str)]) -> String {
        let (wb, overlay) = loaded(files);
        let styles = styles::build(&wb, &overlay);
        let mut ss = SharedStrings::new();
        String::from_utf8(emit(&wb, &overlay, 0, &mut ss, &styles)).expect("the part is UTF-8")
    }

    #[test]
    fn a_sized_axis_reaches_its_col_and_its_row_verbatim() {
        let xml = sheet_xml(&[
            ("B2:C3", "1\t2\n3\t4"),
            ("B2:C3.css", "  td { height: 22.5pt; width: 14.5ch }\n"),
        ]);
        assert!(
            xml.contains(
                r#"<cols><col min="2" max="3" width="14.5" customWidth="1"/></cols><sheetData>"#
            ),
            "a run spells one <col> over the sheet columns it covers, before <sheetData>: {xml}"
        );
        assert!(
            xml.contains(r#"<row r="2" ht="22.5" customHeight="1">"#),
            "{xml}"
        );
        assert!(
            xml.contains(r#"<row r="3" ht="22.5" customHeight="1">"#),
            "{xml}"
        );
    }

    #[test]
    fn an_unsized_sheet_carries_no_cols_and_no_ht() {
        let xml = sheet_xml(&[("A1:B1", "1\t2")]);
        assert!(!xml.contains("<cols"), "no blanket default: {xml}");
        assert!(!xml.contains("ht="), "no blanket default: {xml}");
        assert!(xml.contains(r#"<row r="1">"#), "{xml}");
    }

    /// The declaration is a whole tab's, not a file's: two files sizing adjacent columns alike are one
    /// run, and a column selector carves the run its own file spans.
    #[test]
    fn runs_merge_across_the_tabs_files_and_a_column_rule_carves_them() {
        let xml = sheet_xml(&[
            ("A1:B1", "1\t2"),
            ("C1:E1", "3\t4\t5"),
            ("A1:B1.css", "  td { width: 10ch }\n"),
            (
                "C1:E1.css",
                "  td { width: 10ch }\n  td:nth-child(2) { width: 4ch }\n",
            ),
        ]);
        assert!(
            xml.contains(
                r#"<cols><col min="1" max="3" width="10" customWidth="1"/><col min="4" max="4" width="4" customWidth="1"/><col min="5" max="5" width="10" customWidth="1"/></cols>"#
            ),
            "{xml}"
        );
    }

    /// The writer takes a style for every cell it sites, without a fallback: `source_at` and
    /// `cell_style` both answer for the covering file, so a value can never be written styleless and
    /// can never be skipped for want of a style. A file with no sidecar beside it is the case that
    /// looks like an absent style and is not one — it is the EMPTY style.
    #[test]
    fn a_style_exists_wherever_a_cell_source_does() {
        let files: &[(&str, &str)] = &[
            ("A1:A2", "1\n2"),
            ("C1:C2", "3\n4"),
            ("C1:C2.css", "  td { font-weight: bold }\n"),
        ];
        let (wb, overlay) = loaded(files);
        let region = wb.content_region(0).expect("two files span a region");
        for row in region.min_row..=region.max_row {
            for col in region.min_col..=region.max_col {
                assert_eq!(
                    wb.source_at(0, col, row).is_some(),
                    overlay.cell_style(&wb, 0, col, row).is_some(),
                    "column {col}, row {row}: a covered coordinate has both, a gap has neither",
                );
            }
        }
        let xml = sheet_xml(files);
        for value in ["1", "2", "3", "4"] {
            assert!(xml.contains(&format!("<v>{value}</v>")), "{value}: {xml}");
        }
    }

    /// A blank cell's WHOLE content is the look it wears, and that look is exactly what the read leg
    /// counts as occupancy — so a blank with an `s=` has to reach the sheet as a `<c>` of its own, or
    /// the cell leaves on the pack and never comes back.
    #[test]
    fn a_blank_wearing_a_look_is_written_as_a_c_carrying_only_its_style() {
        let xml = sheet_xml(&[
            ("A1:B2", "1\t\n\t2"),
            ("A1:B2.css", "  td { background-color: #00ff00 }\n"),
        ]);
        assert!(xml.contains(r#"<c r="B1" s="1"/>"#), "{xml}");
        assert!(xml.contains(r#"<c r="A2" s="1"/>"#), "{xml}");

        let plain = sheet_xml(&[("A1:B1", "1\t")]);
        assert!(
            !plain.contains(r#"r="B1""#),
            "a blank wearing the default look states nothing at all: {plain}"
        );
    }

    /// A height belongs to the ROW, not to the cells on it, so an all-blank row keeps it.
    #[test]
    fn a_sized_row_holding_no_cell_survives_as_an_empty_row() {
        let xml = sheet_xml(&[("A1:A2", "1\n\n"), ("A1:A2.css", "  td { height: 20pt }\n")]);
        assert!(
            xml.contains(r#"<row r="2" ht="20" customHeight="1"/>"#),
            "{xml}"
        );
    }
}
