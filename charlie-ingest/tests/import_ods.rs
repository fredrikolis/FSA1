// Concern: the END-TO-END ingest contract — import each committed `.ods` fixture into a temp workbook, then load it back through charlie-model's format-blind engine and ASSERT Excel-correct COMPUTED values (a SUM/formula chain, a cross-sheet reference, VLOOKUP/IF) plus that literals of every type (number/text/bool/date-serial/blank) mapped correctly and formulas ROUND-TRIPPED (the imported SUM cell renders `=SUM(A1:A2)` and evaluates to its Excel value); also that the located-refusal contract holds (a missing source, a non-empty destination) | Non-concern: unit-level translation/serialization edge cases (the crate's `#[cfg(test)]` modules own those) and the CLI surface (charlie-cli tests) | IO: reads tests/fixtures/*.ods, writes a temp workbook, reads it back
//! Integration: `import_ods` a fixture, then evaluate it via `charlie_model::Workbook`.

use std::path::{Path, PathBuf};

use charlie_ast::Value;
use charlie_ingest::{ErrorKind, import_ods};
use charlie_model::{RenderMode, Severity, Workbook, display_value, parse_viewport, render};

/// The committed fixture directory (pure-Rust: no python/odfpy needed to run these).
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A fresh, unique temp workbook directory (never pre-created, so the never-clobber path is exercised).
fn temp_dest(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "charlie-ingest-{tag}-{}-{nanos}",
        std::process::id()
    ))
}

/// Import a fixture into a fresh temp dir and load it back as a charlie workbook, asserting a clean lint.
fn import_and_load(fixture_name: &str, tag: &str) -> (Workbook, PathBuf) {
    let dest = temp_dest(tag);
    import_ods(&fixture(fixture_name), &dest).expect("import should succeed");
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

    // The formula chain: A1=10, A2=20, A3=SUM(A1:A2)=30, B1=A3*2=60.
    assert_eq!(wb.value_at(0, 0, 0), Value::Number(10.0)); // Sheet1!A1
    assert_eq!(wb.value_at(0, 0, 2), Value::Number(30.0)); // Sheet1!A3
    assert_eq!(wb.value_at(0, 1, 0), Value::Number(60.0)); // Sheet1!B1
    // The cross-sheet reference: Sheet2!A1 = Sheet1!A3 = 30.
    assert_eq!(wb.value_at(1, 0, 0), Value::Number(30.0));

    // The formula ROUND-TRIPPED: A3's source text is charlie's translated `=SUM(A1:A2)`, and Sheet2!A1
    // reads the cross-sheet ref in charlie's `Sheet1!A3` spelling.
    let s1 = render(&wb, 0, parse_viewport("A3").unwrap(), RenderMode::Functions);
    assert_eq!(s1.rows[0].cells[0], "=SUM(A1:A2)");
    let s2 = render(&wb, 1, parse_viewport("A1").unwrap(), RenderMode::Functions);
    assert_eq!(s2.rows[0].cells[0], "=Sheet1!A3");

    std::fs::remove_dir_all(&dest).ok();
}

#[test]
fn every_literal_type_and_a_date_map_correctly() {
    let (wb, dest) = import_and_load("literals.ods", "literals");
    assert_eq!(wb.sheet_names(), vec!["Data"]);

    // Header row is text.
    assert_eq!(wb.value_at(0, 0, 0), Value::Text("Name".to_string()));
    // Widget row: text, number, bool, date-as-serial.
    assert_eq!(wb.value_at(0, 0, 1), Value::Text("Widget".to_string()));
    assert_eq!(wb.value_at(0, 1, 1), Value::Number(42.0));
    assert_eq!(wb.value_at(0, 2, 1), Value::Bool(true));
    assert_eq!(wb.value_at(0, 3, 1), Value::Number(45306.0)); // 2024-01-15 Excel serial
    // Gadget row: a negative float, an interior blank (no Active), another date.
    assert_eq!(wb.value_at(0, 1, 2), Value::Number(-3.5));
    assert_eq!(wb.value_at(0, 2, 2), Value::Blank);
    assert_eq!(wb.value_at(0, 3, 2), Value::Number(45473.0)); // 2024-06-30

    std::fs::remove_dir_all(&dest).ok();
}

#[test]
fn vlookup_and_if_evaluate_to_excel_values() {
    let (wb, dest) = import_and_load("functions.ods", "functions");
    // D1 = VLOOKUP("banana", A1:B3, 2, FALSE) -> 2.
    assert_eq!(wb.value_at(0, 3, 0), Value::Number(2.0));
    // D2 = IF(B1>0, "pos", "neg") -> "pos".
    assert_eq!(wb.value_at(0, 3, 1), Value::Text("pos".to_string()));

    // The `;`→`,` translation round-tripped into charlie's grammar (functions render with commas).
    let d1 = render(&wb, 0, parse_viewport("D1").unwrap(), RenderMode::Functions);
    // The `;`→`,` translation and the niladic `FALSE()`→`FALSE` literal normalization both round-trip.
    assert_eq!(d1.rows[0].cells[0], "=VLOOKUP(\"banana\",A1:B3,2,FALSE)");

    std::fs::remove_dir_all(&dest).ok();
}

#[test]
fn blanks_and_repeated_cells_fill_the_rectangle() {
    let (wb, dest) = import_and_load("blanks_repeats.ods", "blanks");
    // Used rectangle is A1:D3; the sparse values sit at A1=1, D1=4, A3=7; all else blank.
    assert_eq!(wb.value_at(0, 0, 0), Value::Number(1.0));
    assert_eq!(wb.value_at(0, 3, 0), Value::Number(4.0));
    assert_eq!(wb.value_at(0, 0, 2), Value::Number(7.0));
    // Interior repeated blanks and a whole blank row read as Blank.
    assert_eq!(wb.value_at(0, 1, 0), Value::Blank);
    assert_eq!(wb.value_at(0, 2, 0), Value::Blank);
    assert_eq!(wb.value_at(0, 0, 1), Value::Blank);
    // A SUM over the sparse row skips blanks (Excel): SUM(A1:D1) = 1 + 4 = 5.
    match wb.eval_formula(0, "=SUM(A1:D1)").unwrap() {
        charlie_model::FormulaOutcome::Value(v) => assert_eq!(v, "5"),
        other => panic!("expected a value, got {other:?}"),
    }

    std::fs::remove_dir_all(&dest).ok();
}

#[test]
fn the_imported_sum_cell_renders_its_excel_value() {
    // The batch smoke, asserted through the render VALUES surface (the imported SUM cell shows 30).
    let (wb, dest) = import_and_load("smoke.ods", "render");
    let g = render(&wb, 0, parse_viewport("A1:B3").unwrap(), RenderMode::Values);
    // A3 (row index 2, col 0) shows 30; B1 (row 0, col 1) shows 60.
    assert_eq!(g.rows[2].cells[0], "30");
    assert_eq!(g.rows[0].cells[1], "60");
    assert_eq!(display_value(&wb.value_at(0, 0, 2)), "30");
    std::fs::remove_dir_all(&dest).ok();
}

#[test]
fn a_missing_source_is_a_located_not_found_refusal() {
    let dest = temp_dest("missing");
    let err = import_ods(Path::new("does/not/exist.ods"), &dest).unwrap_err();
    assert_eq!(err.kind, ErrorKind::SourceNotFound);
    // Nothing written.
    assert!(!dest.exists());
}

#[test]
fn a_non_empty_destination_is_refused_never_clobbered() {
    let (_, dest) = import_and_load("smoke.ods", "conflict");
    // Re-importing into the now-populated dir must refuse with a conflict, not overwrite.
    let err = import_ods(&fixture("smoke.ods"), &dest).unwrap_err();
    assert_eq!(err.kind, ErrorKind::DestConflict);
    std::fs::remove_dir_all(&dest).ok();
}
