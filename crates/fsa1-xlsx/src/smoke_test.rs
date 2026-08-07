// Concern: smoke-tests the writer — a calamine reopen, and an occupied-dest refusal | Non-concern: the graded conformance corpus (conformance/serde/) | IO: (a Workbook + an Overlay) -> a temp .xlsx

use std::path::PathBuf;

use calamine::{Data, Reader, open_workbook_auto};
use fsa1_model::{Overlay, Workbook};

use crate::write_xlsx;

fn temp_xlsx(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "fsa1-xlsx-smoke-{tag}-{}-{nanos}.xlsx",
        std::process::id()
    ))
}

/// The tree carries no sidecar, so its overlay states nothing — built from the SAME tree rather
/// than defaulted, so the two loads can never be given different input.
fn tiny_workbook() -> (Workbook, Overlay) {
    let tabs: &[(&str, &[(&str, &str)])] = &[
        (
            "Sheet1",
            &[
                ("A1", "Item"),
                ("A2", "42"),
                ("A3", "=A2*2"),
                ("A4", "TRUE"),
                ("A5", "#REF!"),
            ],
        ),
        ("Summary", &[("A1", "=Sheet1!A2")]),
    ];
    (
        Workbook::from_tabs(tabs).expect("the tiny in-memory workbook loads cleanly"),
        Overlay::from_tabs(tabs).expect("its sidecars, of which there are none, load cleanly"),
    )
}

#[test]
fn writer_emits_a_package_calamine_reopens() {
    let (wb, overlay) = tiny_workbook();
    let dest = temp_xlsx("reopen");
    let _ = std::fs::remove_file(&dest);

    write_xlsx(&wb, &overlay, &dest).expect("write_xlsx succeeds");

    let mut book = open_workbook_auto(&dest).expect("calamine re-opens the emitted .xlsx");

    let mut names = book.sheet_names().to_vec();
    names.sort();
    assert_eq!(names, vec!["Sheet1".to_string(), "Summary".to_string()]);

    let range = book
        .worksheet_range("Sheet1")
        .expect("calamine reads Sheet1's values");
    assert_eq!(range.get_value((0, 0)), Some(&Data::String("Item".into())));
    assert_eq!(range.get_value((1, 0)), Some(&Data::Float(42.0)));
    assert_eq!(range.get_value((3, 0)), Some(&Data::Bool(true)));

    let formulas = book
        .worksheet_formula("Sheet1")
        .expect("calamine reads Sheet1's formulas");
    assert_eq!(
        formulas.get_value((2, 0)).map(String::as_str),
        Some("A2*2"),
        "the in-sheet formula survives without a cached value"
    );

    let cross = book
        .worksheet_formula("Summary")
        .expect("calamine reads Summary's formulas");
    assert_eq!(
        cross.get_value((0, 0)).map(String::as_str),
        Some("Sheet1!A2"),
        "the cross-sheet formula survives"
    );

    let _ = std::fs::remove_file(&dest);
}

#[test]
fn excels_future_function_prefix_survives_the_pack_leg_verbatim() {
    let tabs: &[(&str, &[(&str, &str)])] = &[(
        "Sheet1",
        &[
            ("A1", "3"),
            ("A2", "1"),
            ("B1", "=_xlfn.MINIFS(A1:A2,A1:A2,\">0\")"),
            ("B2", "=_xlfn._xlws.FILTER(A1:A2,A1:A2>0)"),
        ],
    )];
    let wb = Workbook::from_tabs(tabs).expect("the prefixed workbook loads cleanly");
    let overlay = Overlay::from_tabs(tabs).expect("its sidecars load cleanly");
    let dest = temp_xlsx("xlfn");
    write_xlsx(&wb, &overlay, &dest).expect("export writes the package");

    let mut book = open_workbook_auto(&dest).expect("calamine reopens the export");
    let f = book
        .worksheet_formula("Sheet1")
        .expect("calamine reads Sheet1's formulas");
    assert_eq!(
        f.get_value((0, 1)).map(String::as_str),
        Some("_xlfn.MINIFS(A1:A2,A1:A2,\">0\")"),
        "the `_xlfn.` prefix must be re-emitted verbatim — Excel cannot resolve the bare name"
    );
    assert_eq!(
        f.get_value((1, 1)).map(String::as_str),
        Some("_xlfn._xlws.FILTER(A1:A2,A1:A2>0)"),
        "the nested worksheet-only prefix must survive too"
    );

    let _ = std::fs::remove_file(&dest);
}

/// A stylesheet Excel rejects is one calamine still reads, so the reopen is a floor, not the proof:
/// what it does catch is a part order or a table count the reader trusts and we got wrong.
#[test]
fn a_styled_workbook_reopens_with_its_values_intact() {
    let tabs: &[(&str, &[(&str, &str)])] = &[(
        "Sheet1",
        &[
            ("A1:A2", "Item\n42"),
            (
                "A1:A2.css",
                "  td { background-color: #ffe0b2; border-top: 1px solid #3f0421; \
                 color: #3f0421; font-weight: bold; height: 22.5pt; text-align: center; width: 14.5ch }\n",
            ),
        ],
    )];
    let wb = Workbook::from_tabs(tabs).expect("the styled workbook loads cleanly");
    let overlay = Overlay::from_tabs(tabs).expect("its sidecars load cleanly");
    let dest = temp_xlsx("styled");
    write_xlsx(&wb, &overlay, &dest).expect("export writes the package");

    let mut book = open_workbook_auto(&dest).expect("calamine re-opens the styled export");
    let range = book
        .worksheet_range("Sheet1")
        .expect("calamine reads Sheet1's values");
    assert_eq!(range.get_value((0, 0)), Some(&Data::String("Item".into())));
    assert_eq!(range.get_value((1, 0)), Some(&Data::Float(42.0)));

    let _ = std::fs::remove_file(&dest);
}

#[test]
fn export_refuses_an_occupied_dest() {
    let (wb, overlay) = tiny_workbook();
    let dest = temp_xlsx("occupied");
    std::fs::write(&dest, b"pre-existing").expect("seed the dest");

    let err = write_xlsx(&wb, &overlay, &dest).expect_err("an occupied dest is refused (CORE3)");
    assert!(
        matches!(err, crate::ExportError::DestExists(_)),
        "the refusal names the occupied destination"
    );

    let _ = std::fs::remove_file(&dest);
}

#[test]
fn defined_names_survive_the_pack_leg() {
    // A name exists only on a workbook loaded from disk, so this builds a real tree, not `from_tabs`.
    let root = std::env::temp_dir().join(format!(
        "fsa1-xlsx-names-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(root.join("Data")).expect("tab dir");
    std::fs::write(root.join("Data/C1"), "10").expect("cell");
    std::fs::write(root.join("Data/C2"), "=C1*2").expect("cell");
    fsa1_model::write_name_alias("Data/C1", &root.join("TaxRate")).expect("workbook-scoped name");
    fsa1_model::write_name_alias("C1", &root.join("Data/Local")).expect("sheet-scoped name");

    let wb = Workbook::load_dir(&root)
        .expect("fs read ok")
        .expect("the workbook loads");
    let overlay = Overlay::load_dir(&root)
        .expect("fs read ok")
        .expect("its sidecars load");
    let dest = temp_xlsx("names");
    write_xlsx(&wb, &overlay, &dest).expect("export writes the package");

    let xml = {
        let f = std::fs::File::open(&dest).expect("open the package");
        let mut zip = zip::ZipArchive::new(f).expect("read the zip");
        let mut e = zip
            .by_name("xl/workbook.xml")
            .expect("workbook.xml present");
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut e, &mut buf).expect("read workbook.xml");
        buf
    };

    assert!(
        xml.contains(r#"<definedName name="TaxRate">Data!$C$1</definedName>"#),
        "the workbook-scoped name must emit bare, and ANCHORED — OOXML re-bases a relative \
         defined name onto the referencing cell, so `Data!C1` would bind to a different cell \
         for every reader (a silent wrong value in Excel): {xml}"
    );
    assert!(
        xml.contains(r#"<definedName name="Local" localSheetId="0">Data!$C$1</definedName>"#),
        "the sheet-scoped name must carry its 0-based sheet index: {xml}"
    );
    let (s_end, dn, calc) = (
        xml.find("</sheets>").expect("sheets"),
        xml.find("<definedNames>").expect("definedNames"),
        xml.find("<calcPr").expect("calcPr"),
    );
    assert!(
        s_end < dn && dn < calc,
        "ECMA-376 fixes <workbook>'s child order: sheets < definedNames < calcPr"
    );

    std::fs::remove_dir_all(&root).ok();
    let _ = std::fs::remove_file(&dest);
}
