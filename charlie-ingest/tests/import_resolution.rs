// Concern: the END-TO-END import-time REFERENCE-RESOLUTION contract — import the committed `resolution.xlsx` fixture (a worksheet TABLE `Sales` + workbook DEFINED NAMES `TaxRate`/`AllQOne`) into a temp workbook, load it back through charlie-model's A1-only engine, and ASSERT both that (a) each reference is resolved to PLAIN A1 — a `Table[…]` structured ref INLINE at import, a defined NAME at LOAD via its emitted FS4 entry (render `--functions` shows `=SUM(B2:B4)`, `=B2`, `=B1`, `=Data!H1*100`, `=SUM(Data!B2:B4)`, never a `Sales[…]`/name token) — and (b) the resolved formulas COMPUTE the hand-verified Excel values (60/75/10/20/"Q1"/20/60) with a clean lint — proving tables are materialized to A1 in ingest AND names are emitted as on-disk entries the loader resolves, so the engine never learns of either (HARD RULE 4) | Non-concern: the resolution LOGIC edge cases (resolve.rs `#[cfg(test)]` owns those), the xlsx metadata parsing (xlsx_meta.rs owns that), the mirrored value-type corpus (import_xlsx.rs owns that), and the CLI surface | IO: reads tests/fixtures/resolution.xlsx, writes+reads a temp workbook
//! Integration: `import_file` the reference-resolution fixture, then evaluate + render it via
//! `charlie_model` — the whole point is that a defined name / `Table[…]` structured ref becomes plain
//! A1 at import, so the format-blind engine computes it with no knowledge of names or tables.

use std::path::{Path, PathBuf};

use charlie_ast::Value;
use charlie_ingest::import_file;
use charlie_model::{RenderMode, Severity, Workbook, parse_viewport, render};

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
        "charlie-ingest-res-{tag}-{}-{nanos}",
        std::process::id()
    ))
}

/// The rendered `--functions` text of a single cell (the resolved A1 formula the importer wrote).
fn func_at(wb: &Workbook, a1: &str) -> String {
    render(wb, 0, parse_viewport(a1).unwrap(), RenderMode::Functions).rows[0].cells[0].clone()
}

#[test]
fn defined_names_and_table_refs_resolve_to_a1_and_compute() {
    let dest = temp_dest("resolve");
    import_file(&fixture("resolution.xlsx"), &dest).expect("import should succeed");
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

    // (a) Every reference resolves to PLAIN A1 — a `Table[…]` structured ref INLINE at import, a defined
    // NAME at LOAD (via its emitted FS4 entry) — so no `Sales[…]` or name token survives in the grid.
    assert_eq!(func_at(&wb, "E2"), "=SUM(B2:B4)"); // Sales[Q1] -> data body
    assert_eq!(func_at(&wb, "E3"), "=SUM(C2:C4)"); // Sales[Q2] -> data body
    assert_eq!(func_at(&wb, "F2"), "=B2"); // Sales[@Q1] on row 2
    assert_eq!(func_at(&wb, "F3"), "=B3"); // Sales[@Q1] on row 3
    assert_eq!(func_at(&wb, "G2"), "=B1"); // Sales[[#Headers],[Q1]] -> header cell
    // FS4: defined NAMES are emitted as on-disk entries and resolved at LOAD (not inlined at import),
    // via the SYMLINK representation for a static ref — so the load-resolved A1 is unanchored (`$`-free),
    // the same reference Excel evaluates. `TaxRate` -> a bare symlink to `Data/H1`; `AllQ1` -> a
    // `.begin`/`.end` pair spanning `Data/B2`..`Data/B4`.
    assert_eq!(func_at(&wb, "H2"), "=Data!H1*100"); // TaxRate (workbook name) -> Data!H1
    assert_eq!(func_at(&wb, "H3"), "=SUM(Data!B2:B4)"); // AllQOne -> range Data!B2:B4

    // (b) The resolved formulas compute the hand-verified Excel values. value_at is (sheet, col, row).
    assert_eq!(wb.value_at(0, 4, 1), Value::Number(60.0)); // E2 = SUM(10,20,30)
    assert_eq!(wb.value_at(0, 4, 2), Value::Number(75.0)); // E3 = SUM(15,25,35)
    assert_eq!(wb.value_at(0, 5, 1), Value::Number(10.0)); // F2 = B2
    assert_eq!(wb.value_at(0, 5, 2), Value::Number(20.0)); // F3 = B3
    assert_eq!(wb.value_at(0, 6, 1), Value::Text("Q1".to_string())); // G2 = header cell text
    assert_eq!(wb.value_at(0, 7, 1), Value::Number(20.0)); // H2 = 0.2 * 100
    assert_eq!(wb.value_at(0, 7, 2), Value::Number(60.0)); // H3 = SUM(B2:B4)

    std::fs::remove_dir_all(&dest).ok();
}
