// Concern: what survives a pack and re-unpack, and the refuse corpus's refusals | Non-concern: value grading (the Python oracle owns it), blocks a policy cuts | IO: (the serde corpus) -> exit status

use std::path::{Path, PathBuf};
use std::process::Command;

use fsa1_model::{Cell, Overlay, Rect, Workbook};

fn serde_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/serde")
}

fn corpus(family: &str) -> Vec<PathBuf> {
    let dir = serde_dir().join(family);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "xlsx"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no .xlsx under {}", dir.display());
    files
}

fn run(args: &[&str]) -> (i32, String, String) {
    run_in(None, args)
}

/// `pack` derives its output into the CWD, so its cases run the child in a temp dir to keep the
/// artifact out of the source tree.
fn run_in(cwd: Option<&Path>, args: &[&str]) -> (i32, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fsa1-cli"));
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let out = cmd.output().expect("spawn fsa1-cli");
    let code = out.status.code().expect("exit code");
    (
        code,
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Temp {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "FSA1-roundtrip-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("create temp root");
        Temp(root)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn file_bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("read file bytes")
}

/// Both halves of what a tree states: its values, and the presentation beside them. SER2 promises
/// the look survives a pack, so a comparison reading only the workbook would not see it move.
struct Loaded {
    workbook: Workbook,
    overlay: Overlay,
}

impl Loaded {
    fn stated(&self, sheet: u32) -> Option<Rect> {
        self.overlay.stated_region(&self.workbook, sheet)
    }
}

fn load(root: &Path, at: &str) -> Loaded {
    let workbook = Workbook::load_dir(root)
        .unwrap_or_else(|e| panic!("reading the unpacked {at}: {e}"))
        .unwrap_or_else(|d| panic!("the unpacked {at} must load: {d:?}"));
    let overlay = Overlay::load_dir(root)
        .unwrap_or_else(|e| panic!("reading the unpacked {at}'s sidecars: {e}"))
        .unwrap_or_else(|d| panic!("the unpacked {at}'s sidecars must load: {d:?}"));
    Loaded { workbook, overlay }
}

/// Every coordinate either side states, so a cell one workbook holds and the other does not is
/// compared rather than skipped.
fn coordinates(a: &Loaded, b: &Loaded, sheet: u32) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for r in [a.stated(sheet), b.stated(sheet)].into_iter().flatten() {
        for row in r.min_row..=r.max_row {
            out.extend((r.min_col..=r.max_col).map(|col| (col, row)));
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// The four properties SER2 promises a reopened workbook still presents, at one coordinate. Which
/// range file covers it is deliberately absent: `pack` keeps one `<cellXfs>` entry per LOOK, so two
/// distinct xf indices that draw alike collapse into one and a re-unpack may legitimately cut the
/// sheet into different blocks. The tree is the decomposition's; only these four are the contract's.
fn content_at(loaded: &Loaded, sheet: u32, col: u32, row: u32) -> [(&'static str, String); 4] {
    let wb = &loaded.workbook;
    let (formula, format) = match wb.source_at(sheet, col, row).map(|s| s.cell) {
        Some(Cell::Formula { src, format, .. }) => (src.clone(), *format),
        Some(Cell::Value { format, .. }) => (String::new(), *format),
        Some(Cell::LoadError { .. }) | None => (String::new(), None),
    };
    [
        ("value", format!("{:?}", wb.value_at(sheet, col, row))),
        ("formula", formula),
        (
            "display format",
            format.map(|f| f.code()).unwrap_or_default(),
        ),
        (
            "style",
            format!("{:?}", loaded.overlay.cell_style(wb, sheet, col, row)),
        ),
    ]
}

/// `col` and `row` are zero-based, as every `Workbook` coordinate is.
fn a1(col: u32, row: u32) -> String {
    let mut letters = String::new();
    let mut c = col + 1;
    while c > 0 {
        letters.insert(0, char::from(b'A' + ((c - 1) % 26) as u8));
        c = (c - 1) / 26;
    }
    format!("{letters}{}", row + 1)
}

#[test]
fn ser2_every_accept_fixture_reopens_with_the_same_content_and_pack_leaves_the_source_untouched() {
    // Re-unpacking the pack output is what exercises the calamine leg of the SER2 triangulation here.
    let tmp = Temp::new("ser2");
    for fixture in corpus("accept") {
        let stem = fixture.file_stem().unwrap().to_string_lossy().into_owned();
        let wb_a = tmp.join(&format!("{stem}_a"));
        let wb_b = tmp.join(&format!("{stem}_b"));
        let out_xlsx = tmp.join(&format!("{stem}_a.xlsx"));
        let f = fixture.to_str().unwrap();

        let before = file_bytes(&fixture);

        let (ic, _, ie) = run(&["unpack", "--strict", f, wb_a.to_str().unwrap()]);
        assert_eq!(
            ic, 0,
            "unpack --strict of accept fixture {stem} must succeed:\n{ie}"
        );

        let (ec, _, ee) = run_in(Some(tmp.0.as_path()), &["pack", wb_a.to_str().unwrap()]);
        assert_eq!(ec, 0, "pack of {stem} must succeed:\n{ee}");
        assert!(
            out_xlsx.exists(),
            "pack must write the derived {}",
            out_xlsx.display()
        );

        assert_eq!(
            before,
            file_bytes(&fixture),
            "pack must leave the source {stem}.xlsx byte-identical (CORE3)"
        );

        let (rc, _, re) = run(&[
            "unpack",
            "--strict",
            out_xlsx.to_str().unwrap(),
            wb_b.to_str().unwrap(),
        ]);
        assert_eq!(
            rc, 0,
            "re-unpack --strict of the packed {stem} must succeed (calamine reopens it):\n{re}"
        );

        let (a, b) = (load(&wb_a, &stem), load(&wb_b, &stem));
        assert_eq!(
            a.workbook.sheet_names(),
            b.workbook.sheet_names(),
            "SER2: re-unpacking the pack of {stem} must reopen the same tabs"
        );
        for (sheet, tab) in a.workbook.sheet_names().iter().enumerate() {
            let sheet = sheet as u32;
            for (col, row) in coordinates(&a, &b, sheet) {
                let (was, now) = (
                    content_at(&a, sheet, col, row),
                    content_at(&b, sheet, col, row),
                );
                for ((what, before), (_, after)) in was.iter().zip(&now) {
                    assert_eq!(
                        before,
                        after,
                        "SER2: {stem} {tab}!{} reopens with a different {what} after a pack and \
                         re-unpack",
                        a1(col, row)
                    );
                }
            }
        }

        let (cc, _, _) = run_in(Some(tmp.0.as_path()), &["pack", wb_a.to_str().unwrap()]);
        assert_eq!(
            cc, 4,
            "pack of {stem} must refuse an already-occupied derived dest (exit 4)"
        );
    }
}

#[test]
fn ser3_every_refuse_probe_is_refused_with_a_located_diagnostic_naming_the_part_or_cell() {
    let expected: &[(&str, &str)] = &[
        ("cond_literal.xlsx", "number format"),
        ("mask_literal.xlsx", "number format"),
        ("exotic_formula.xlsx", "number format"),
        ("chart.xlsx", "xl/charts/chart1.xml"),
        ("drawing.xlsx", "xl/drawings/drawing1.xml"),
        ("pivot.xlsx", "xl/pivotTables/pivotTable1.xml"),
        ("resolution.xlsx", "xl/tables/table1.xml"),
    ];
    let tmp = Temp::new("ser3");
    let mut seen = std::collections::BTreeSet::new();
    for probe in corpus("refuse") {
        let name = probe.file_name().unwrap().to_string_lossy().into_owned();
        let (want_name, needle) = expected
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("refuse probe {name} has no expected-diagnostic mapping"));
        seen.insert(*want_name);
        let dest = tmp.join(&format!(
            "{}_dest",
            probe.file_stem().unwrap().to_string_lossy()
        ));
        let (code, stdout, stderr) = run(&[
            "unpack",
            "--strict",
            probe.to_str().unwrap(),
            dest.to_str().unwrap(),
        ]);
        let diag = format!("{stdout}{stderr}");
        assert_ne!(
            code, 0,
            "refuse probe {name} must be refused (non-zero exit); diag:\n{diag}"
        );
        assert!(
            diag.contains(needle),
            "refuse probe {name} must name its offending part/cell ({needle:?}); diag:\n{diag}"
        );
    }
    let want: std::collections::BTreeSet<&str> = expected.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        seen, want,
        "every expected refuse probe must be present in conformance/serde/refuse/"
    );
}
