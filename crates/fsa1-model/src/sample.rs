// Concern: the canonical tutorial workbook as (path, content) data | Non-concern: writing it to disk (`fsa1-cli sample` does that) | IO: () -> Vec<(PathBuf, String)>

use std::path::PathBuf;

fn file(body: &str) -> String {
    body.to_string()
}

pub fn sample_workbook() -> Vec<(PathBuf, String)> {
    vec![
        (
            PathBuf::from("Orders/A1:D1"),
            file("Product\tUnit Price\tQty\tLine Total"),
        ),
        (PathBuf::from("Orders/A2:A4"), file("Widget\nGadget\nGizmo")),
        (PathBuf::from("Orders/B2:B4"), file("10\n15\n4")),
        (PathBuf::from("Orders/C2:C4"), file("4\n2\n10")),
        (
            PathBuf::from("Orders/D2:D4"),
            file("=B2*C2\n=B3*C3\n=B4*C4"),
        ),
        (PathBuf::from("Orders/A5"), file("Total")),
        (PathBuf::from("Orders/D5"), file("=SUM(D2:D4)")),
        (PathBuf::from("Summary/A1:B1"), file("Metric\tValue")),
        (PathBuf::from("Summary/A2"), file("Total Revenue")),
        (PathBuf::from("Summary/B2"), file("=Orders!D5")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Severity;
    use crate::render::display_value;
    use crate::workbook::Workbook;
    use fsa1_ast::Value;

    /// The tutorial is the first thing a new user runs, so it has to be writable wherever they are.
    /// The table is canonical `:` on every host, so each range needs a `-` spelling naming the same
    /// region, which is what a Windows run writes instead.
    #[test]
    fn every_tutorial_range_has_a_windows_legal_spelling() {
        for (rel, _) in sample_workbook() {
            let name = rel
                .file_name()
                .and_then(|n| n.to_str())
                .expect("a file name");
            let here = crate::parse_filename(name).expect("the table is well-formed");
            if here.declared_shape.rows == 1 && here.declared_shape.cols == 1 {
                continue; // a single cell carries no separator to re-spell
            }
            let win = crate::reseparate_range_name(name, crate::RANGE_SEP_WINDOWS)
                .expect("every range in the table has a Windows spelling");
            assert!(!win.contains(':'), "{win} is not writable on Windows");
            let there = crate::parse_filename(&win).expect("the re-spelling parses");
            assert_eq!(there.region, here.region, "{name} and {win} differ");
        }
    }

    #[test]
    fn the_sample_workbook_loads_clean_and_renders_the_pinned_totals() {
        let base = std::env::temp_dir().join(format!(
            "FSA1-sample-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        for (rel, content) in sample_workbook() {
            let full = base.join(crate::range_file_path(&rel));
            std::fs::create_dir_all(full.parent().expect("a tutorial path has a tab folder"))
                .expect("create tab folder");
            std::fs::write(&full, content).expect("write sample file");
        }

        let wb = Workbook::load_dir(&base)
            .expect("filesystem read ok")
            .expect("the sample workbook must load clean");

        assert_eq!(wb.sheet_names(), vec!["Orders", "Summary"]);

        let diags = wb.lint();
        assert!(
            diags.iter().all(|d| d.code.severity() != Severity::Error),
            "the sample workbook must lint clean: {diags:?}"
        );

        // value_at(sheet, col, row), all zero-based.
        assert_eq!(wb.value_at(0, 3, 1), Value::Number(40.0)); // Orders!D2
        assert_eq!(wb.value_at(0, 3, 4), Value::Number(110.0)); // Orders!D5
        assert_eq!(wb.value_at(1, 1, 1), Value::Number(110.0)); // Summary!B2 (cross-sheet)
        assert_eq!(wb.value_at(0, 0, 0), Value::Text("Product".to_string())); // Orders!A1 header

        assert_eq!(display_value(&wb.value_at(0, 3, 4)), "110");

        std::fs::remove_dir_all(&base).ok();
    }
}
