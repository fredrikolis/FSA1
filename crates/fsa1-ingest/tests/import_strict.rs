// Concern: asserts a --strict refusal names its offending cell, part or axis | Non-concern: the numFmt scan edges, value correctness | IO: (fixtures) -> a temp workbook

use std::path::{Path, PathBuf};

use fsa1_ingest::{ErrorKind, import_file};

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
        "fsa1-ingest-strict-{tag}-{}-{nanos}",
        std::process::id()
    ))
}

#[test]
fn strict_accepts_the_simple_core_and_writes_the_same_workbook_as_lossy() {
    // smoke.xlsx: values, formulas, two sheets, all General format, only ALLOW parts.
    let dest = temp_dest("accept");
    let report =
        import_file(&fixture("smoke.xlsx"), &dest, true).expect("strict import should accept");
    assert!(
        report.files > 0,
        "the accepted workbook materializes cell files"
    );
    assert!(
        dest.join("Sheet1").exists(),
        "the first tab folder is written"
    );
    std::fs::remove_dir_all(&dest).ok();
}

#[test]
fn strict_refuses_a_non_default_number_format_naming_the_cell_and_numfmt() {
    // literals.xlsx carries a datetime-formatted cell at D2/D3.
    let dest = temp_dest("numfmt");
    let err = import_file(&fixture("literals.xlsx"), &dest, true).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Invalid, "message: {}", err.message);
    let located = err.to_string();
    assert!(
        located.contains("D2") && located.contains("164"),
        "the diagnostic names the offending cell + numFmtId: {located}"
    );
    assert!(
        !dest.exists(),
        "a strict refusal runs before any write, so it leaves no output"
    );
}

#[test]
fn strict_refuses_an_out_of_scope_package_part_naming_the_part() {
    // resolution.xlsx carries an xl/tables/table1.xml part.
    let dest = temp_dest("part");
    let err = import_file(&fixture("resolution.xlsx"), &dest, true).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Invalid, "message: {}", err.message);
    let located = err.to_string();
    assert!(
        located.contains("xl/tables/table1.xml"),
        "the diagnostic names the offending part: {located}"
    );
    assert!(
        !dest.exists(),
        "a strict refusal runs before any write, so it leaves no output"
    );
}

/// The refusal the SOURCE alone cannot predict: a width authored on a column the sheet's root does
/// not span, so the tree states it nowhere. `--strict` promises a file it accepts round-trips
/// identically, so a size the tree never states is a refusal — the axis is named, and the
/// destination the run had already started writing is left absent.
#[test]
fn strict_refuses_a_size_the_tree_never_states_naming_the_sheet_and_the_axis() {
    let dest = temp_dest("geometry");
    let err = import_file(&fixture("visuals.xlsx"), &dest, true).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Invalid, "message: {}", err.message);
    let located = err.to_string();
    assert!(
        located.contains("on sheet Visual") && located.contains("column C"),
        "the diagnostic names the offending sheet + axis: {located}"
    );
    assert!(
        located.contains("without --strict"),
        "a refusal names the fix, not only the fault: {located}"
    );
    assert!(
        !dest.exists(),
        "the refusal lands mid-write, so the atomic cleanup must leave no output at all"
    );
}

#[test]
fn the_default_lossy_import_still_accepts_every_strict_refusal_case() {
    // A display format is not grid, so dropping it does not make the lossy grid wrong.
    for name in ["literals.xlsx", "resolution.xlsx"] {
        let dest = temp_dest(&format!("lossy-{}", name.trim_end_matches(".xlsx")));
        import_file(&fixture(name), &dest, false)
            .unwrap_or_else(|e| panic!("lossy import of {name} must still succeed: {e}"));
        assert!(dest.exists(), "lossy import of {name} wrote a workbook");
        std::fs::remove_dir_all(&dest).ok();
    }
}
