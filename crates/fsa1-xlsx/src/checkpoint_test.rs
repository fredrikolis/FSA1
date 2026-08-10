// Concern: the calamine leg of the numFmt export checkpoint | Non-concern: the openpyxl and `formulas` legs, SER2 grading | IO: (a formatted Workbook) -> a temp .xlsx

use std::io::Read;
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
        "fsa1-xlsx-checkpoint-{tag}-{}-{nanos}.xlsx",
        std::process::id()
    ))
}

fn formatted_workbook() -> (Workbook, Overlay) {
    let tabs: &[(&str, &[(&str, &str)])] = &[(
        "Sheet1",
        &[
            ("A1", "2021-05-15~m/d/yyyy"),
            ("A2", "$1,234.00"),
            ("A3", "12.50%"),
            ("A4", "12.50"),
            ("A5", "=1+1~$#,##0.00"),
        ],
    )];
    (
        Workbook::from_tabs(tabs).expect("the formatted checkpoint workbook loads cleanly"),
        Overlay::from_tabs(tabs).expect("its sidecars, of which there are none, load cleanly"),
    )
}

fn read_zip_entry(path: &std::path::Path, name: &str) -> String {
    let file = std::fs::File::open(path).expect("open the exported .xlsx");
    let mut zip = zip::ZipArchive::new(file).expect("the export is a valid zip");
    let mut entry = zip
        .by_name(name)
        .unwrap_or_else(|_| panic!("the export contains {name}"));
    let mut s = String::new();
    entry.read_to_string(&mut s).expect("the entry is UTF-8");
    s
}

#[test]
fn formatted_export_is_accepted_by_calamine_and_carries_numfmts_and_s() {
    let (wb, overlay) = formatted_workbook();
    // An env-named dest is what `conformance/serde/checkpoint_numfmts.py` grades.
    let dest = match std::env::var_os("FSA1_CHECKPOINT_XLSX") {
        Some(p) => PathBuf::from(p),
        None => temp_xlsx("calamine"),
    };
    let _ = std::fs::remove_file(&dest);

    write_xlsx(&wb, &overlay, &[], &dest, false)
        .expect("write_xlsx succeeds for a formatted workbook");

    let mut book = open_workbook_auto(&dest).expect("calamine re-opens the formatted export");
    let range = book
        .worksheet_range("Sheet1")
        .expect("calamine reads Sheet1");
    assert_eq!(
        range.get_value((1, 0)),
        Some(&Data::Float(1234.0)),
        "the quote-free currency literal exports as a bare number under a custom numFmt"
    );
    assert_eq!(
        range.get_value((2, 0)),
        Some(&Data::Float(0.125)),
        "the percent literal exports as the stored ratio under built-in numFmt 10"
    );
    let formulas = book
        .worksheet_formula("Sheet1")
        .expect("calamine reads Sheet1 formulas");
    assert_eq!(
        formulas.get_value((4, 0)).map(String::as_str),
        Some("1+1"),
        "the formatted formula's body carries no `~code` marker"
    );

    let styles = read_zip_entry(&dest, "xl/styles.xml");
    assert!(
        styles.contains("<numFmts"),
        "the style table declares a per-workbook <numFmts> block"
    );
    assert!(
        styles.contains(r#"formatCode="$#,##0.00""#),
        "the quote-free currency code is emitted verbatim"
    );
    assert!(
        styles.contains(r#"formatCode="m/d/yyyy""#),
        "the date code is emitted"
    );
    let sheet = read_zip_entry(&dest, "xl/worksheets/sheet1.xml");
    assert!(
        sheet.contains(r#"<c r="A2" s="#),
        "the formatted currency value cell carries s="
    );
    assert!(
        sheet.contains(r#"<c r="A5" s="#),
        "the formatted formula cell carries s= on its <c>"
    );

    if std::env::var_os("FSA1_CHECKPOINT_XLSX").is_none() {
        let _ = std::fs::remove_file(&dest);
    }
}
