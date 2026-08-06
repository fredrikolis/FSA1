// Concern: asserts every imported table and name ref reads as plain A1 and computes | Non-concern: the resolution edges | IO: (resolution.xlsx) -> a temp workbook

use std::path::{Path, PathBuf};

use fsa1_ast::Value;
use fsa1_ingest::import_file;
use fsa1_model::{RenderMode, Severity, Workbook, parse_viewport, render};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn temp_dest(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "fsa1-ingest-res-{tag}-{}-{nanos}",
        std::process::id()
    ))
}

fn func_at(wb: &Workbook, a1: &str) -> String {
    render(wb, 0, parse_viewport(a1).unwrap(), RenderMode::Functions).rows[0].cells[0].clone()
}

#[test]
fn defined_names_and_table_refs_resolve_to_a1_and_compute() {
    let dest = temp_dest("resolve");
    import_file(&fixture("resolution.xlsx"), &dest, false).expect("import should succeed");
    let wb = Workbook::load_dir(&dest)
        .expect("filesystem read ok")
        .expect("the imported workbook must load clean");
    assert!(
        wb.lint()
            .iter()
            .all(|d| d.code.severity() != Severity::Error),
        "the resolved workbook must lint clean: {:?}",
        wb.lint()
    );

    // No `Sales[…]` or name token survives: a structured ref resolves at import, a name at load.
    assert_eq!(func_at(&wb, "E2"), "=SUM(B2:B4)", "Sales[Q1]");
    assert_eq!(func_at(&wb, "E3"), "=SUM(C2:C4)", "Sales[Q2]");
    assert_eq!(func_at(&wb, "F2"), "=B2", "Sales[@Q1] on row 2");
    assert_eq!(func_at(&wb, "F3"), "=B3", "Sales[@Q1] on row 3");
    assert_eq!(func_at(&wb, "G2"), "=B1", "Sales[[#Headers],[Q1]]");
    assert_eq!(func_at(&wb, "H2"), "=Data!H1*100", "TaxRate, a cell name");
    assert_eq!(
        func_at(&wb, "H3"),
        "=SUM(Data!B2:B4)",
        "AllQOne, a range name"
    );

    // The hand-verified Excel values. `value_at` is (sheet, col, row).
    assert_eq!(wb.value_at(0, 4, 1), Value::Number(60.0));
    assert_eq!(wb.value_at(0, 4, 2), Value::Number(75.0));
    assert_eq!(wb.value_at(0, 5, 1), Value::Number(10.0));
    assert_eq!(wb.value_at(0, 5, 2), Value::Number(20.0));
    assert_eq!(wb.value_at(0, 6, 1), Value::Text("Q1".to_string()));
    assert_eq!(wb.value_at(0, 7, 1), Value::Number(20.0));
    assert_eq!(wb.value_at(0, 7, 2), Value::Number(60.0));

    std::fs::remove_dir_all(&dest).ok();
}
