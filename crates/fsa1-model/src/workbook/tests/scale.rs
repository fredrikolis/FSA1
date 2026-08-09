// Concern: pins that a whole-workbook drive is ONE pass computing each cell once | Non-concern: traversal order, the value catalogue | IO: bulk workbooks -> asserted pass and eval counts
use super::*;

fn load(tabs: &[(String, Vec<(String, String)>)]) -> Workbook {
    let borrowed: Vec<(&str, Vec<(&str, &str)>)> = tabs
        .iter()
        .map(|(t, fs)| {
            (
                t.as_str(),
                fs.iter()
                    .map(|(n, c)| (n.as_str(), c.as_str()))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let spec: Vec<(&str, &[(&str, &str)])> =
        borrowed.iter().map(|(t, fs)| (*t, fs.as_slice())).collect();
    Workbook::from_tabs(&spec).expect("workbook loads clean")
}

#[test]
fn whole_workbook_lint_drives_every_cell_in_one_batched_pass() {
    let mut files: Vec<(String, String)> = Vec::new();
    for col in ["A", "B", "C", "D"] {
        files.push((format!("{col}1"), "1".to_string()));
        for row in 2..=60 {
            files.push((format!("{col}{row}"), format!("=SUM({col}{})+1", row - 1)));
        }
    }
    let tabs = vec![("Sheet1".to_string(), files)];
    let wb = load(&tabs);

    assert!(wb.lint().is_empty(), "the workbook is clean");
    assert_eq!(
        wb.pass_count(),
        1,
        "lint must drive the whole workbook in ONE plan+evaluate pass, not one per coordinate"
    );
    assert_eq!(
        wb.eval_count(),
        4 * 59,
        "each formula cell computes exactly once across that single pass (ENG2)"
    );
}

#[test]
fn eval_time_refusals_are_reported_in_file_order_not_evaluation_order() {
    // The FILE order here, A10 before A2, is the REVERSE of the coordinate order the pass uses.
    let files = vec![
        ("A10".to_string(), "=SUM(A1:AZ100000)".to_string()),
        ("A2".to_string(), "=SUM(B1:BZ100000)".to_string()),
    ];
    let tabs = vec![("Sheet1".to_string(), files)];
    let wb = load(&tabs);

    let diags = wb.lint();
    assert_eq!(diags.len(), 2, "one refusal per over-large range");
    let locs: Vec<String> = diags.iter().map(|d| format!("{}", d.loc)).collect();
    assert_eq!(
        locs,
        vec!["Sheet1/A10".to_string(), "Sheet1/A2".to_string()],
        "reported in file order (the tab's file sequence), not in the pass's evaluation order"
    );
    assert_eq!(wb.pass_count(), 1, "still one batched pass");
}

/// The field that distinguishes two otherwise-identical over-large-range refusals.
fn refusal_col_spans(diags: &[Diagnostic]) -> Vec<u64> {
    diags
        .iter()
        .filter_map(|d| {
            let m = &d.message;
            let start = m.find("rows x ")? + "rows x ".len();
            let rest = &m[start..];
            let end = rest.find(" cols")?;
            rest[..end].parse().ok()
        })
        .collect()
}

#[test]
fn within_a_multi_cell_file_refusals_follow_deterministic_topo_order() {
    // Two ANTI-DIAGONAL cells of one 2x2 file, each raising a DISTINCT refusal over far, independent columns. Sharing the file's one `Loc::TabFile`, the (tab, file) sort cannot order them, so what is pinned is the within-file topo order and the set-equality.
    let body = "1\t=SUM(M1:BZ100000)\n=SUM(N1:CD100000)\t2".to_string();
    let tabs = vec![("T".to_string(), vec![("A1:B2".to_string(), body)])];
    let wb = load(&tabs);

    let diags = wb.lint();
    let mut spans = refusal_col_spans(&diags);
    spans.sort_unstable();
    assert_eq!(
        spans,
        vec![66, 69],
        "both distinct over-large-range refusals reported"
    );
    assert_eq!(
        refusal_col_spans(&diags),
        vec![69, 66],
        "within one multi-cell file, refusals follow engine topo order (sorted (col,row): A2 then B1)"
    );
    // A multi-cell file exists here, so the paired single-cell test's zero is meaningful.
    assert!(
        wb.covering_scan_steps() > 0,
        "a multi-cell file's lookups exercise (and count) the spans fallback"
    );
}

#[test]
fn single_cell_lookups_never_scan_the_multi_cell_fallback() {
    // 3,000 single-cell files: an O(files) scan per lookup would be millions of steps here.
    let cols: Vec<String> = (0..20).map(col_name).collect();
    let mut files: Vec<(String, String)> = Vec::new();
    for c in &cols {
        files.push((format!("{c}1"), "1".to_string()));
        for row in 2..=150 {
            files.push((format!("{c}{row}"), format!("=SUM({c}{})+1", row - 1)));
        }
    }
    let tabs = vec![("Sheet1".to_string(), files)];
    let wb = load(&tabs);

    assert!(wb.lint().is_empty(), "the workbook is clean");
    assert_eq!(
        wb.covering_scan_steps(),
        0,
        "a single-cell-file lookup resolves through the `single` index; the `spans` fallback never scans"
    );
}

#[test]
fn a_whole_workbook_view_is_one_pass_whatever_it_is_drawn_as() {
    // The counters do NOT grade the name path: a name is spelled through `eval_formula`, whose core plans and evaluates without going through `demand`. What pins that is the value agreement at the bottom.
    let mut tabs: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for tab in ["Alpha", "Beta", "Gamma"] {
        let mut files: Vec<(String, String)> = vec![("A1".to_string(), "1".to_string())];
        for row in 2..=40 {
            files.push((format!("A{row}"), format!("=A{}+1", row - 1)));
        }
        if tab == "Alpha" {
            files.push(("Total".to_string(), "=SUM(A1:A40)".to_string())); // a sheet-scoped name
        }
        tabs.push((tab.to_string(), files));
    }
    let wb = load(&tabs);

    for (scope, expect_names) in [
        (crate::view::ViewScope::Workbook, true),
        (crate::view::ViewScope::Tab(0), true),
        (
            crate::view::ViewScope::Region(
                0,
                Rect {
                    min_col: 0,
                    min_row: 0,
                    max_col: 0,
                    max_row: 39,
                },
            ),
            false,
        ),
    ] {
        let fresh = load(&tabs);
        let v = crate::view::view(
            &fresh,
            None,
            scope,
            crate::render::RenderMode::Combined,
            &[],
        )
        .expect("the view fits the render bound");
        assert_eq!(
            fresh.pass_count(),
            1,
            "a {scope:?} view must demand its whole content in ONE pass"
        );
        assert_eq!(
            v.sheets[0].names.is_empty(),
            !expect_names,
            "a {scope:?} view carries its in-scope FS4 names"
        );
    }

    // The name's value must be the one the grid shows: the same pass computed both.
    let v = crate::view::view(
        &wb,
        None,
        crate::view::ViewScope::Tab(0),
        crate::render::RenderMode::Values,
        &[],
    )
    .expect("the view fits the render bound");
    assert_eq!(v.sheets[0].names[0].text, "820"); // 1 + 2 + ... + 40
    assert_eq!(v.sheets[0].cell(0, 39).map(|(_, t)| t), Some("40")); // A40
}

/// Zero-based: 0 is "A", 26 is "AA".
fn col_name(mut c: u32) -> String {
    let mut s = String::new();
    c += 1;
    while c > 0 {
        let r = (c - 1) % 26;
        s.insert(0, (b'A' + r as u8) as char);
        c = (c - 1) / 26;
    }
    s
}
