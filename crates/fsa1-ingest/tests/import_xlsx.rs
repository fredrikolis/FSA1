// Concern: imports each .xlsx fixture and asserts the Excel values it evaluates to | Non-concern: translation edges, the .ods path | IO: (fixtures/*.xlsx) -> a temp workbook

use std::path::{Path, PathBuf};

use fsa1_ast::Value;
use fsa1_ingest::{ErrorKind, import_file};
use fsa1_model::{
    FormulaOutcome, RenderMode, Severity, Workbook, display_value, parse_viewport, render,
};

/// The committed fixtures are pure data, so these tests need no python toolchain.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Unique, and never pre-created, so the never-clobber path is exercised.
fn temp_dest(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "fsa1-ingest-xlsx-{tag}-{}-{nanos}",
        std::process::id()
    ))
}

fn import_and_load(fixture_name: &str, tag: &str) -> (Workbook, PathBuf) {
    let dest = temp_dest(tag);
    import_file(&fixture(fixture_name), &dest, false).expect("import should succeed");
    let wb = Workbook::load_dir(&dest)
        .expect("filesystem read ok")
        .expect("the imported workbook must load clean");
    let diags = wb.lint();
    assert!(
        diags.iter().all(|d| d.code.severity() != Severity::Error),
        "imported {fixture_name} must lint clean: {diags:?}"
    );
    (wb, dest)
}

#[test]
fn sum_chain_and_cross_sheet_compute_the_excel_values() {
    let (wb, dest) = import_and_load("smoke.xlsx", "smoke");
    assert_eq!(wb.sheet_names(), vec!["Sheet1", "Sheet2"]);

    // A1=10, A2=20, A3=SUM(A1:A2)=30, B1=A3*2=60; Sheet2!A1 = Sheet1!A3 = 30.
    assert_eq!(wb.value_at(0, 0, 0), Value::Number(10.0));
    assert_eq!(wb.value_at(0, 0, 2), Value::Number(30.0));
    assert_eq!(wb.value_at(0, 1, 0), Value::Number(60.0));
    assert_eq!(wb.value_at(1, 0, 0), Value::Number(30.0));

    let s1 = render(&wb, 0, parse_viewport("A3").unwrap(), RenderMode::Functions);
    assert_eq!(
        s1.rows[0].cells[0], "=SUM(A1:A2)",
        "an xlsx formula is already Excel-A1, so it round-trips verbatim"
    );
    let s2 = render(&wb, 1, parse_viewport("A1").unwrap(), RenderMode::Functions);
    assert_eq!(s2.rows[0].cells[0], "=Sheet1!A3");

    std::fs::remove_dir_all(&dest).ok();
}

#[test]
fn every_literal_type_and_a_date_map_correctly() {
    let (wb, dest) = import_and_load("literals.xlsx", "literals");
    assert_eq!(wb.sheet_names(), vec!["Data"]);

    assert_eq!(wb.value_at(0, 0, 0), Value::Text("Name".to_string()));
    // Widget row: text, number, bool, date-as-serial.
    assert_eq!(wb.value_at(0, 0, 1), Value::Text("Widget".to_string()));
    assert_eq!(wb.value_at(0, 1, 1), Value::Number(42.0));
    assert_eq!(wb.value_at(0, 2, 1), Value::Bool(true));
    assert_eq!(wb.value_at(0, 3, 1), Value::Number(45306.0), "2024-01-15");
    // Gadget row: a negative float, an interior blank, another date.
    assert_eq!(wb.value_at(0, 1, 2), Value::Number(-3.5));
    assert_eq!(wb.value_at(0, 2, 2), Value::Blank);
    assert_eq!(wb.value_at(0, 3, 2), Value::Number(45473.0), "2024-06-30");

    std::fs::remove_dir_all(&dest).ok();
}

#[test]
fn vlookup_and_if_evaluate_to_excel_values() {
    let (wb, dest) = import_and_load("functions.xlsx", "functions");
    assert_eq!(
        wb.value_at(0, 3, 0),
        Value::Number(2.0),
        "D1 = VLOOKUP(\"banana\", A1:B3, 2, FALSE)"
    );
    assert_eq!(
        wb.value_at(0, 3, 1),
        Value::Text("pos".to_string()),
        "D2 = IF(B1>0, \"pos\", \"neg\")"
    );

    let d1 = render(&wb, 0, parse_viewport("D1").unwrap(), RenderMode::Functions);
    assert_eq!(d1.rows[0].cells[0], "=VLOOKUP(\"banana\",A1:B3,2,FALSE)");

    std::fs::remove_dir_all(&dest).ok();
}

#[test]
fn excels_stored_future_function_prefix_imports_and_evaluates() {
    // Excel STORES `_xlfn.NAME` while DISPLAYING the bare name; a failure here is a load error.
    let (wb, dest) = import_and_load("future_functions.xlsx", "future");
    // Column A = 3, 1, 2.
    assert_eq!(
        wb.value_at(0, 2, 0),
        Value::Number(2.0),
        "C1 = _xlfn.MINIFS(A1:A3, A1:A3, \">1\")"
    );
    assert_eq!(
        wb.value_at(0, 2, 1),
        Value::Number(2.0),
        "C2 = _xlfn.XLOOKUP(2, A1:A3, A1:A3)"
    );
    assert_eq!(
        wb.value_at(0, 2, 2),
        Value::Number(6.0),
        "C3 = SUM(A1:A3), the unprefixed control"
    );

    let c1 = render(&wb, 0, parse_viewport("C1").unwrap(), RenderMode::Functions);
    assert_eq!(
        c1.rows[0].cells[0], "=_xlfn.MINIFS(A1:A3,A1:A3,\">1\")",
        "the prefix is preserved in the source, so Excel reads back the spelling it wrote"
    );

    std::fs::remove_dir_all(&dest).ok();
}

#[test]
fn blanks_and_repeated_cells_fill_the_rectangle() {
    let (wb, dest) = import_and_load("blanks_repeats.xlsx", "blanks");
    // Used rectangle A1:D3, sparse values at A1=1, D1=4, A3=7.
    assert_eq!(wb.value_at(0, 0, 0), Value::Number(1.0));
    assert_eq!(wb.value_at(0, 3, 0), Value::Number(4.0));
    assert_eq!(wb.value_at(0, 0, 2), Value::Number(7.0));
    assert_eq!(wb.value_at(0, 1, 0), Value::Blank);
    assert_eq!(wb.value_at(0, 2, 0), Value::Blank);
    assert_eq!(wb.value_at(0, 0, 1), Value::Blank);
    match wb.eval_formula(0, "=SUM(A1:D1)").unwrap() {
        FormulaOutcome::Value(v) => {
            assert_eq!(v, "5", "a SUM over the sparse row skips blanks")
        }
        other => panic!("expected a value, got {other:?}"),
    }

    std::fs::remove_dir_all(&dest).ok();
}

#[test]
fn the_imported_sum_cell_renders_its_excel_value() {
    let (wb, dest) = import_and_load("smoke.xlsx", "render");
    let g = render(&wb, 0, parse_viewport("A1:B3").unwrap(), RenderMode::Values);
    assert_eq!(g.rows[2].cells[0], "30", "A3");
    assert_eq!(g.rows[0].cells[1], "60", "B1");
    assert_eq!(display_value(&wb.value_at(0, 0, 2)), "30");
    std::fs::remove_dir_all(&dest).ok();
}

#[test]
fn a_structurally_corrupt_xlsx_is_a_located_source_io_refusal_never_a_panic() {
    // A valid `.xlsx` extension over garbage passes the gate, exercising the OPENER's error path.
    let bad = temp_dest("corrupt").with_extension("xlsx");
    std::fs::write(&bad, b"PK\x03\x04 not really a zip -- just garbage bytes")
        .expect("write fixture");
    let dest = temp_dest("corrupt-dest");
    let err = import_file(&bad, &dest, false).unwrap_err();
    assert_eq!(err.kind, ErrorKind::SourceIo, "message: {}", err.message);
    assert!(
        !dest.exists(),
        "a failed import must not create the destination"
    );
    std::fs::remove_file(&bad).ok();
}
