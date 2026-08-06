// Concern: imports each .ods fixture and asserts the Excel values it evaluates to | Non-concern: translation edges, the CLI surface | IO: (fixtures/*.ods) -> a temp workbook

use std::path::{Path, PathBuf};

use fsa1_ast::Value;
use fsa1_ingest::{ErrorKind, import_file};
use fsa1_model::{RenderMode, Severity, Workbook, display_value, parse_viewport, render};

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
    std::env::temp_dir().join(format!("fsa1-ingest-{tag}-{}-{nanos}", std::process::id()))
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
    let (wb, dest) = import_and_load("smoke.ods", "smoke");
    assert_eq!(wb.sheet_names(), vec!["Sheet1", "Sheet2"]);

    // A1=10, A2=20, A3=SUM(A1:A2)=30, B1=A3*2=60; Sheet2!A1 = Sheet1!A3 = 30.
    assert_eq!(wb.value_at(0, 0, 0), Value::Number(10.0));
    assert_eq!(wb.value_at(0, 0, 2), Value::Number(30.0));
    assert_eq!(wb.value_at(0, 1, 0), Value::Number(60.0));
    assert_eq!(wb.value_at(1, 0, 0), Value::Number(30.0));

    let s1 = render(&wb, 0, parse_viewport("A3").unwrap(), RenderMode::Functions);
    assert_eq!(
        s1.rows[0].cells[0], "=SUM(A1:A2)",
        "the formula round-tripped into FSA1's spelling"
    );
    let s2 = render(&wb, 1, parse_viewport("A1").unwrap(), RenderMode::Functions);
    assert_eq!(s2.rows[0].cells[0], "=Sheet1!A3");

    std::fs::remove_dir_all(&dest).ok();
}

#[test]
fn every_literal_type_and_a_date_map_correctly() {
    let (wb, dest) = import_and_load("literals.ods", "literals");
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
    let (wb, dest) = import_and_load("functions.ods", "functions");
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
    assert_eq!(
        d1.rows[0].cells[0], "=VLOOKUP(\"banana\",A1:B3,2,FALSE)",
        "the `;`->`,` and niladic FALSE() rewrites both round-trip"
    );

    std::fs::remove_dir_all(&dest).ok();
}

#[test]
fn blanks_and_repeated_cells_fill_the_rectangle() {
    let (wb, dest) = import_and_load("blanks_repeats.ods", "blanks");
    // Used rectangle A1:D3, sparse values at A1=1, D1=4, A3=7.
    assert_eq!(wb.value_at(0, 0, 0), Value::Number(1.0));
    assert_eq!(wb.value_at(0, 3, 0), Value::Number(4.0));
    assert_eq!(wb.value_at(0, 0, 2), Value::Number(7.0));
    assert_eq!(wb.value_at(0, 1, 0), Value::Blank);
    assert_eq!(wb.value_at(0, 2, 0), Value::Blank);
    assert_eq!(wb.value_at(0, 0, 1), Value::Blank);
    match wb.eval_formula(0, "=SUM(A1:D1)").unwrap() {
        fsa1_model::FormulaOutcome::Value(v) => {
            assert_eq!(v, "5", "a SUM over the sparse row skips blanks")
        }
        other => panic!("expected a value, got {other:?}"),
    }

    std::fs::remove_dir_all(&dest).ok();
}

#[test]
fn the_imported_sum_cell_renders_its_excel_value() {
    let (wb, dest) = import_and_load("smoke.ods", "render");
    let g = render(&wb, 0, parse_viewport("A1:B3").unwrap(), RenderMode::Values);
    assert_eq!(g.rows[2].cells[0], "30", "A3");
    assert_eq!(g.rows[0].cells[1], "60", "B1");
    assert_eq!(display_value(&wb.value_at(0, 0, 2)), "30");
    std::fs::remove_dir_all(&dest).ok();
}

#[test]
fn a_missing_source_is_a_located_not_found_refusal() {
    let dest = temp_dest("missing");
    let err = import_file(Path::new("does/not/exist.ods"), &dest, false).unwrap_err();
    assert_eq!(err.kind, ErrorKind::SourceNotFound);
    assert!(!dest.exists(), "nothing written");
}

#[test]
fn a_non_empty_destination_is_refused_never_clobbered() {
    let (_, dest) = import_and_load("smoke.ods", "conflict");
    let err = import_file(&fixture("smoke.ods"), &dest, false).unwrap_err();
    assert_eq!(err.kind, ErrorKind::DestConflict);
    std::fs::remove_dir_all(&dest).ok();
}
