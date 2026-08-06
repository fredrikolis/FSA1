// Concern: pins defined names over a REAL temp-dir tree, symlinks included | Non-concern: the pure name-table and rewrite logic | IO: temp-dir trees -> asserted values
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use fsa1_ast::{ErrKind, Value};

use crate::diagnostic::Code;
use crate::workbook::Workbook;

/// Unique per test, so parallel runs each own their root.
fn temp_base(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "FSA1-fs4-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ))
}

fn write(base: &Path, tab: &str, name: &str, body: &str) {
    let dir = base.join(tab);
    fs::create_dir_all(&dir).expect("create tab dir");
    fs::write(dir.join(name), body).expect("write file");
}

fn load(base: &Path) -> Result<Workbook, Vec<crate::diagnostic::Diagnostic>> {
    Workbook::load_dir(base).expect("fs read ok")
}

fn value(wb: &Workbook, sheet: u32, col: u32, row: u32) -> Value {
    wb.value_at(sheet, col, row)
}

#[test]
fn a_symlink_named_range_is_summed_by_a_formula() {
    let base = temp_base("range-sum");
    write(&base, "Sheet1", "A2", "10");
    write(&base, "Sheet1", "A3", "20");
    write(&base, "Sheet1", "A4", "30");
    write(&base, "Sheet1", "H1", "=SUM(Days)");
    symlink("A2", base.join("Sheet1").join("Days.begin")).unwrap();
    symlink("A4", base.join("Sheet1").join("Days.end")).unwrap();

    let wb = load(&base).expect("loads clean");
    assert_eq!(value(&wb, 0, 7, 0), Value::Number(60.0)); // H1
    fs::remove_dir_all(&base).ok();
}

#[test]
fn a_range_name_over_a_blank_corner_materializes_to_a_sum() {
    // The shape a range over SPARSE data has: the corner symlinks read lexically, so no cell file need exist, and a blank corner counts as 0 rather than failing the load.
    let base = temp_base("blank-corner");
    write(&base, "Sheet1", "A3", "20"); // only the interior cell is populated; A2/A4 are blank
    write(&base, "Sheet1", "H1", "=SUM(Days)");
    symlink("A2", base.join("Sheet1").join("Days.begin")).unwrap(); // blank corner (no A2 file)
    symlink("A4", base.join("Sheet1").join("Days.end")).unwrap(); // blank corner (no A4 file)

    let wb = load(&base).expect("loads clean despite the blank corners");
    assert_eq!(value(&wb, 0, 7, 0), Value::Number(20.0)); // H1 = SUM(A2:A4) over blank/20/blank
    fs::remove_dir_all(&base).ok();
}

#[test]
fn a_bare_single_cell_symlink_writes_through_to_its_cell() {
    let base = temp_base("write-through");
    write(&base, "Sheet1", "B5", "7");
    write(&base, "Sheet1", "C1", "=total*2");
    symlink("B5", base.join("Sheet1").join("total")).unwrap();

    let wb = load(&base).expect("loads clean");
    assert_eq!(value(&wb, 0, 1, 4), Value::Number(7.0)); // B5
    assert_eq!(value(&wb, 0, 2, 0), Value::Number(14.0)); // C1 = total*2

    // The symlink resolves to B5, so writing "through" the name writes the cell file itself.
    fs::write(base.join("Sheet1").join("total"), "9").unwrap();
    assert_eq!(
        fs::read_to_string(base.join("Sheet1").join("B5")).unwrap(),
        "9",
        "writing through the symlink changed the target cell file"
    );
    let wb2 = load(&base).expect("reloads clean");
    assert_eq!(value(&wb2, 0, 1, 4), Value::Number(9.0)); // B5 now 9
    assert_eq!(value(&wb2, 0, 2, 0), Value::Number(18.0)); // C1 reflects it
    fs::remove_dir_all(&base).ok();
}

#[test]
fn a_named_formula_ref_file_and_a_named_constant() {
    let base = temp_base("named-formula");
    write(&base, "Sheet1", "A1", "=Rate");
    write(&base, "Sheet1", "A2", "=Half*2");
    write(&base, "Sheet1", "Base", "100");
    write(&base, "Sheet1", "Rate", "=Base*1.05");
    write(&base, "Sheet1", "Half", "3.5");

    let wb = load(&base).expect("loads clean");
    assert_eq!(value(&wb, 0, 0, 0), Value::Number(105.0)); // A1 = Rate = 100*1.05
    assert_eq!(value(&wb, 0, 0, 1), Value::Number(7.0)); // A2 = Half*2 (named constant)
    fs::remove_dir_all(&base).ok();
}

#[test]
fn overlapping_names_over_the_same_cells_both_resolve() {
    // A cell may belong to any number of names.
    let base = temp_base("overlap");
    write(&base, "Sheet1", "A1", "1");
    write(&base, "Sheet1", "A2", "2");
    write(&base, "Sheet1", "A3", "3");
    write(&base, "Sheet1", "Whole", "=A1:A3");
    write(&base, "Sheet1", "Same", "=A1:A3");
    write(&base, "Sheet1", "C1", "=SUM(Whole)+SUM(Same)");

    let wb = load(&base).expect("loads clean");
    assert_eq!(value(&wb, 0, 2, 0), Value::Number(12.0)); // (1+2+3)*2
    fs::remove_dir_all(&base).ok();
}

#[test]
fn a_sheet_scoped_name_shadows_a_workbook_scoped_one() {
    let base = temp_base("shadow");
    write(&base, "Sheet1", "A1", "=Rate");
    write(&base, "Sheet2", "A1", "=Rate");
    write(&base, "Sheet1", "Rate", "2"); // sheet-scoped
    fs::write(base.join("Rate"), "1").unwrap(); // workbook-scoped (root)

    let wb = load(&base).expect("loads clean");
    let s1 = wb.tab_index("Sheet1").unwrap();
    let s2 = wb.tab_index("Sheet2").unwrap();
    assert_eq!(value(&wb, s1, 0, 0), Value::Number(2.0)); // shadowed on Sheet1
    assert_eq!(value(&wb, s2, 0, 0), Value::Number(1.0)); // workbook name on Sheet2
    fs::remove_dir_all(&base).ok();
}

#[test]
fn a_name_that_parses_as_a1_is_refused() {
    let base = temp_base("ident-a1");
    write(&base, "Sheet1", "B5", "1");
    symlink("B5", base.join("Sheet1").join("A5")).unwrap(); // `A5` parses as A1 -> refused
    let diags = load(&base).expect_err("must refuse");
    assert!(
        diags.iter().any(|d| d.code == Code::NameRefusal),
        "{diags:?}"
    );
    fs::remove_dir_all(&base).ok();
}

#[test]
fn a_lone_corner_is_refused() {
    let base = temp_base("lone-corner");
    write(&base, "Sheet1", "A2", "1");
    symlink("A2", base.join("Sheet1").join("r.begin")).unwrap();
    let diags = load(&base).expect_err("must refuse");
    assert!(
        diags.iter().any(|d| d.code == Code::NameRefusal),
        "{diags:?}"
    );
    fs::remove_dir_all(&base).ok();
}

#[test]
fn an_inverted_corner_range_is_refused() {
    let base = temp_base("inverted");
    write(&base, "Sheet1", "A2", "1");
    write(&base, "Sheet1", "A4", "3");
    symlink("A4", base.join("Sheet1").join("r.begin")).unwrap();
    symlink("A2", base.join("Sheet1").join("r.end")).unwrap();
    let diags = load(&base).expect_err("must refuse");
    assert!(
        diags.iter().any(|d| d.code == Code::NameRefusal),
        "{diags:?}"
    );
    fs::remove_dir_all(&base).ok();
}

#[test]
fn a_degraded_symlink_ref_file_reads_equivalently() {
    let base = temp_base("degraded");
    write(&base, "Sheet1", "B5", "7");
    write(&base, "Sheet1", "total", "B5"); // regular file holding the bare A1 target (degraded link)
    write(&base, "Sheet1", "C1", "=total*2");
    let wb = load(&base).expect("loads clean");
    assert_eq!(value(&wb, 0, 2, 0), Value::Number(14.0)); // C1 = total*2 = B5*2
    fs::remove_dir_all(&base).ok();
}

#[test]
fn a_degraded_cross_scope_symlink_ref_file_reads_equivalently() {
    // Both must re-qualify to `Data!H1`, so a flattened tree opens with a live tree's values.
    let base = temp_base("degraded-cross");
    write(&base, "Data", "H1", "5");
    write(&base, "Sheet1", "A1", "=TaxRate*3"); // workbook name, degraded to `Data/H1`
    write(&base, "Sheet1", "A2", "=Cross+1"); // sheet name -> other sheet, degraded to `../Data/H1`
    fs::write(base.join("TaxRate"), "Data/H1").unwrap(); // workbook-scoped, at the root
    write(&base, "Sheet1", "Cross", "../Data/H1"); // sheet-scoped, cross-sheet
    let wb = load(&base).expect("loads clean");
    let s1 = wb.tab_index("Sheet1").unwrap(); // tabs sort alphabetically, so Data precedes Sheet1
    assert_eq!(value(&wb, s1, 0, 0), Value::Number(15.0)); // A1 = 5*3
    assert_eq!(value(&wb, s1, 0, 1), Value::Number(6.0)); // A2 = 5+1
    fs::remove_dir_all(&base).ok();
}

#[test]
fn a_name_targeting_a_sheet_with_a_space_resolves_to_the_value() {
    // An unquoted `My Data!B5` would split at the space in the lexer and resolve to `#NAME?`.
    let base = temp_base("spaced-sheet");
    write(&base, "My Data", "B5", "7");
    write(&base, "Sheet1", "C1", "=total*2");
    symlink("My Data/B5", base.join("total")).unwrap(); // workbook-scoped, at the root
    let wb = load(&base).expect("loads clean");
    let s1 = wb.tab_index("Sheet1").unwrap();
    assert_eq!(value(&wb, s1, 2, 0), Value::Number(14.0)); // C1 = total*2, total -> 'My Data'!B5 = 7
    fs::remove_dir_all(&base).ok();
}

#[test]
fn a_range_name_over_a_spaced_sheet_is_summed() {
    // The corner-pair path quotes the sheet too, in `CornerAcc::finish`.
    let base = temp_base("spaced-range");
    write(&base, "My Data", "A2", "10");
    write(&base, "My Data", "A3", "20");
    write(&base, "My Data", "A4", "30");
    write(&base, "My Data", "H1", "=SUM(Days)");
    symlink("A2", base.join("My Data").join("Days.begin")).unwrap();
    symlink("A4", base.join("My Data").join("Days.end")).unwrap();
    let wb = load(&base).expect("loads clean");
    let s = wb.tab_index("My Data").unwrap();
    assert_eq!(value(&wb, s, 7, 0), Value::Number(60.0)); // H1 = SUM('My Data'!A2:A4)
    fs::remove_dir_all(&base).ok();
}

#[test]
fn an_unresolvable_name_reference_is_a_located_name_error() {
    let base = temp_base("unresolvable");
    write(&base, "Sheet1", "A1", "=SUM(Ghost)");
    let wb =
        load(&base).expect("loads clean (the bad cell is a per-cell GRID6 error, not a refusal)");
    assert_eq!(value(&wb, 0, 0, 0), Value::Error(ErrKind::Name)); // #NAME?
    assert!(
        wb.lint().iter().any(|d| d.code == Code::FormulaSyntax),
        "check reports the unresolvable name"
    );
    fs::remove_dir_all(&base).ok();
}

#[test]
fn an_ad_hoc_eval_resolves_names_like_a_stored_formula() {
    // The ad-hoc entry parses a FRESH formula, so it must resolve names on the same semantics.
    use crate::workbook::FormulaOutcome;
    let base = temp_base("adhoc");
    write(&base, "Sheet1", "A2", "10");
    write(&base, "Sheet1", "A3", "20");
    write(&base, "Sheet1", "A4", "30");
    symlink("A2", base.join("Sheet1").join("Days.begin")).unwrap();
    symlink("A4", base.join("Sheet1").join("Days.end")).unwrap();
    let wb = load(&base).expect("loads clean");
    let s = wb.tab_index("Sheet1").unwrap();
    assert_eq!(
        wb.eval_formula(s, "=SUM(Days)").unwrap(),
        FormulaOutcome::Value("60".to_string())
    );
    fs::remove_dir_all(&base).ok();
}
