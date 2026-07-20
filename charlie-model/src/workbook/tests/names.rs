// Concern: the FS4 NAME fitness pins over a REAL temp-dir workbook loaded through `Workbook::load_dir` (the only path that reads symlinks) — a symlink named RANGE (`.begin`/`.end`) a formula sums; a bare single-cell symlink with WRITE-THROUGH (editing the target cell through the name changes the value on re-load); a named FORMULA ref-file (`=Base*1.05`) and a named CONSTANT; OVERLAPPING names over the same cells; a sheet-scoped name SHADOWING a workbook-scoped one; a name identifier that parses as A1 REFUSED; a lone/inverted corner REFUSED; a degraded-symlink ref-file (a bare same-sheet target AND a cross-scope relative path `Data/H1`/`../Data/H1`) read equivalently; a name/range whose TARGET SHEET carries a SPACE (`My Data`) resolving to the value (the injected ref is `'…'!`-quoted, not a split-at-the-space `#NAME?`); and an unresolvable name reference as a located `#NAME?` | Non-concern: the pure name-table/rewrite logic (the `names` module owns it, unit-tested there) and the in-memory ref-file path (the parent `tests` module and `names` unit tests cover it) | IO: temp-dir workbook trees with symlinks on disk -> asserted `Value`s / `Diagnostic` codes
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use charlie_ast::{ErrKind, Value};

use crate::diagnostic::Code;
use crate::workbook::Workbook;

/// A fresh, unique temp directory for one test's workbook (parallel tests each own their own root).
fn temp_base(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "charlie-fs4-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ))
}

/// Write a cell/name file under `base/tab/name` (creating the tab folder).
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
    // Sheet1: A2=10, A3=20, A4=30; a range name `Days` = A2:A4 via two corner symlinks; H1=SUM(Days).
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
    // Blank-corner materialization (model/loader side): a range name whose CORNER cells are blank (no
    // cell file — the shape a range over sparse data has) must still resolve. The corner symlinks are
    // read LEXICALLY (the target cell need not exist), the range A2:A4 materializes at eval, and a blank
    // cell counts as 0 — so SUM(Days) is the one populated interior cell, never a load error or a panic.
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
    // WRITE-THROUGH (CORE3): `total` is a bare symlink to B5. Editing THROUGH the name (writing the
    // cell file) changes the cell, and a re-render reflects it; a formula referencing the name follows.
    let base = temp_base("write-through");
    write(&base, "Sheet1", "B5", "7");
    write(&base, "Sheet1", "C1", "=total*2");
    symlink("B5", base.join("Sheet1").join("total")).unwrap();

    let wb = load(&base).expect("loads clean");
    assert_eq!(value(&wb, 0, 1, 4), Value::Number(7.0)); // B5
    assert_eq!(value(&wb, 0, 2, 0), Value::Number(14.0)); // C1 = total*2

    // Edit through the name: the symlink resolves to B5, so writing "through" it writes the cell file.
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
    // A named CONSTANT `Base`=100, a named FORMULA `Rate`=`=Base*1.05` (ref-files), and `Pi`=3.14.
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
    // Two names over the SAME cells (a cell may belong to any number of names, FS4).
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
    // Workbook-scoped `Rate`=1 (root ref-file); sheet-scoped `Rate`=2 on Sheet1 shadows it there,
    // while Sheet2 (no local name) sees the workbook one.
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
    // A symlink whose identifier is an A1 address is a located refusal (FS4/CORE2).
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
    // A `.begin` without its `.end` is a located refusal.
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
    // begin below/right of end is a located refusal.
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
    // A symlink that degraded to a regular file whose content is a bare A1 target reads as the ref-file
    // form — the reader-union catches it, so the same workbook opens with the same value.
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
    // The GENERAL degraded case (not just a same-sheet bare target): a WORKBOOK-scoped name and a
    // CROSS-SHEET name each degrade to a regular file holding the writer's RELATIVE symlink path
    // (`Data/H1`, `../Data/H1`) — the shape a symlink-flattening zip produces. The reader-union must
    // re-qualify both to `Data!H1`, so the workbook opens with the same values a live-symlink tree would.
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
    // ENG6/CORE1 end-to-end: a workbook-scoped name pointing at a MULTI-WORD sheet (`My Data`) — a very
    // common imported shape — must resolve to the cell's value, not a located `#NAME?`. The reader has
    // to inject the `'My Data'!B5` quoted form; an unquoted `My Data!B5` splits at the space in the lexer.
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
    // A corner-pair range whose corners live on a spaced sheet must sum (CornerAcc::finish quotes it).
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
    // A formula referencing a name that has no entry is a located `#NAME?` (VAL3/GRID6), never a crash
    // and never silently wrong; `lint` reports it.
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
    // `charlie-cli eval` (the ad-hoc entry) parses a FRESH formula, so it must resolve names too — the
    // same semantics as a stored cell formula (CLI1).
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
