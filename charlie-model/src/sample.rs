// Concern: the canonical TUTORIAL workbook exposed as DATA — a `Vec<(relative path, file content)>` that IS a valid charlie workbook (two tabs; a header row; a column of EXPLICIT per-row `=B2*C2` formulas, VAL1; a `=SUM` aggregate; a cross-sheet `=Orders!D5`), so the format is taught by a real, renderable, editable artifact rather than prose; the `charlie-cli sample <dir>` CLI writes it to disk, and the liveness test loads it, asserts a clean lint, and pins rendered values so the tutorial can never silently go stale | Non-concern: WRITING the files to disk (charlie-cli owns that IO), evaluating the grid (workbook.rs), and the filename/grid grammar it exercises (filename.rs/grid.rs) | IO: () -> `Vec<(PathBuf, String)>` (in-memory data; no filesystem access)
//! The live sample workbook: [`sample_workbook`] returns the canonical tutorial as `(path, content)`
//! data. It is a real charlie workbook — the `charlie-cli sample` command writes it out, and a reader
//! learns the on-disk model by rendering, checking, and editing it. A colocated liveness test loads
//! the same data and pins its rendered values, so the tutorial and the code can never drift.

use std::path::PathBuf;

/// One tutorial file: its line-1 `# ` annotation glued to its TSV body (the two joined by a newline,
/// exactly as an on-disk file is laid out).
fn file(annotation: &str, body: &str) -> String {
    format!("{annotation}\n{body}")
}

/// The canonical tutorial workbook as `(relative path, file content)` data — a valid, annotated,
/// closed-range charlie workbook that teaches the format by being real. Two tabs:
///
/// - **`Orders/`** — a header row (`A1:D1`), three product rows (name / unit price / quantity), a
///   `D2:D4` column of EXPLICIT per-row formulas (`=B2*C2`, `=B3*C3`, `=B4*C4` — one per cell, no
///   drag-fill; VAL1), and a `=SUM(D2:D4)` grand total in `D5`.
/// - **`Summary/`** — a header row and a single cell (`B2`) that reads the Orders total cross-sheet
///   with `=Orders!D5`.
///
/// The [`crate::Workbook`] loader turns each sub-folder into a tab and each file's name into its
/// closed A1 range; the returned paths use `TabName/RangeName`. The liveness test in this module
/// writes this data to a temp dir, asserts a clean lint, and pins the rendered totals.
pub fn sample_workbook() -> Vec<(PathBuf, String)> {
    vec![
        // --- Orders tab -----------------------------------------------------------------------
        (
            PathBuf::from("Orders/A1:D1"),
            file(
                "# Concern: the order-book column headers | Non-concern: the data rows below | IO: input",
                "Product\tUnit Price\tQty\tLine Total",
            ),
        ),
        (
            PathBuf::from("Orders/A2:A4"),
            file(
                "# Concern: the product names | Non-concern: their prices and quantities | IO: input",
                "Widget\nGadget\nGizmo",
            ),
        ),
        (
            PathBuf::from("Orders/B2:B4"),
            file(
                "# Concern: the per-product unit price | Non-concern: quantity and line totals | IO: input",
                "10\n15\n4",
            ),
        ),
        (
            PathBuf::from("Orders/C2:C4"),
            file(
                "# Concern: the per-product quantity ordered | Non-concern: price and line totals | IO: input",
                "4\n2\n10",
            ),
        ),
        (
            PathBuf::from("Orders/D2:D4"),
            file(
                "# Concern: per-line revenue = unit price x quantity, one explicit formula per row (VAL1) | Non-concern: the grand total (D5) | IO: none",
                "=B2*C2\n=B3*C3\n=B4*C4",
            ),
        ),
        (
            PathBuf::from("Orders/A5"),
            file(
                "# Concern: the total-row label | Non-concern: the total value (D5) | IO: input",
                "Total",
            ),
        ),
        (
            PathBuf::from("Orders/D5"),
            file(
                "# Concern: the grand total revenue = SUM of every line | Non-concern: the per-line math (D2:D4) | IO: output",
                "=SUM(D2:D4)",
            ),
        ),
        // --- Summary tab ----------------------------------------------------------------------
        (
            PathBuf::from("Summary/A1:B1"),
            file(
                "# Concern: the summary metric labels | Non-concern: the metric values | IO: input",
                "Metric\tValue",
            ),
        ),
        (
            PathBuf::from("Summary/A2"),
            file(
                "# Concern: the revenue metric label | Non-concern: its value (B2) | IO: input",
                "Total Revenue",
            ),
        ),
        (
            PathBuf::from("Summary/B2"),
            file(
                "# Concern: total revenue pulled cross-sheet from the Orders tab | Non-concern: how Orders computes it | IO: none",
                "=Orders!D5",
            ),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Severity;
    use crate::render::display_value;
    use crate::workbook::Workbook;
    use charlie_ast::Value;

    /// The liveness guarantee: the sample is a REAL workbook. Write it to a temp dir, load it,
    /// assert the lint is clean (no error-severity diagnostic), and pin the rendered totals — so the
    /// tutorial can never silently go stale (a broken sample fails this test, not a reader).
    #[test]
    fn the_sample_workbook_loads_clean_and_renders_the_pinned_totals() {
        let base = std::env::temp_dir().join(format!(
            "charlie-sample-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        for (rel, content) in sample_workbook() {
            let full = base.join(&rel);
            std::fs::create_dir_all(full.parent().expect("a tutorial path has a tab folder"))
                .expect("create tab folder");
            std::fs::write(&full, content).expect("write sample file");
        }

        let wb = Workbook::load_dir(&base)
            .expect("filesystem read ok")
            .expect("the sample workbook must load clean");

        // Tabs are the sub-folders, in sorted order.
        assert_eq!(wb.sheet_names(), vec!["Orders", "Summary"]);

        // A clean workbook lints with no error-severity diagnostic.
        let diags = wb.lint();
        assert!(
            diags.iter().all(|d| d.code.severity() != Severity::Error),
            "the sample workbook must lint clean: {diags:?}"
        );

        // Pinned values (Orders is tab 0, Summary tab 1; cols/rows are zero-based):
        // D2 = B2*C2 = 10*4 = 40; D5 = SUM(D2:D4) = 40+30+40 = 110; Summary!B2 = Orders!D5 = 110.
        assert_eq!(wb.value_at(0, 3, 1), Value::Number(40.0)); // Orders!D2
        assert_eq!(wb.value_at(0, 3, 4), Value::Number(110.0)); // Orders!D5
        assert_eq!(wb.value_at(1, 1, 1), Value::Number(110.0)); // Summary!B2 (cross-sheet)
        assert_eq!(wb.value_at(0, 0, 0), Value::Text("Product".to_string())); // Orders!A1 header

        // Rendered display strings match the render surface's spelling.
        assert_eq!(display_value(&wb.value_at(0, 3, 4)), "110");

        std::fs::remove_dir_all(&base).ok();
    }
}
