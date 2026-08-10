// Concern: locks what a check or render GRADES against what its demand pulled in | Non-concern: the argv dispatch, the read set | IO: spawns the binary -> stdout + exit status

mod common;

use common::{Fixture, at, run};

/// A file the closure pulled in through a reference is read for its VALUES and is not graded: only a
/// demand that names it asks about its faults.
#[test]
fn a_pulled_file_is_read_and_not_graded() {
    let fx = Fixture::new("pulled-not-graded");
    fx.file("Sheet1", "A1", "=Sheet2!A1")
        .file("Sheet2", "A1", "1")
        .file("Sheet2", "D1", "=SUM(");

    let (code, out) = run(&["check", &at(&fx, "Sheet1/A1:B2")]);
    assert_eq!(code, 0, "Sheet2 is pulled in, not demanded:\n{out}");
    assert!(
        !out.contains("formula-syntax"),
        "a pulled file's fault is not graded:\n{out}"
    );

    for demand in ["", "Sheet2"] {
        let (code, out) = run(&["check", &at(&fx, demand)]);
        assert_eq!(code, 3, "{demand:?} demands Sheet2 itself:\n{out}");
        assert!(
            out.contains("formula-syntax"),
            "{demand:?} grades it:\n{out}"
        );
    }
}

/// The one exception: a pulled file that will not PARSE is what the demanded value depends on, so it
/// loads as a load error rather than a blank and its fault is reported against the demanded work.
#[test]
fn a_pulled_files_parse_fault_is_reported_and_never_reads_blank() {
    let fx = Fixture::new("pulled-parse-fault");
    fx.file("Sheet1", "A1", "=Sheet2!A1")
        .file("Sheet2", "A1:B2", "x\ty");

    let (code, out) = run(&["check", &at(&fx, "Sheet1/A1")]);
    assert_eq!(
        code, 3,
        "the pulled file's parse fault is fatal work:\n{out}"
    );
    assert!(
        out.contains("dimension-mismatch") && out.contains("Sheet2/A1:B2"),
        "located at the pulled file:\n{out}"
    );

    let (code, out) = run(&["render", &at(&fx, "Sheet1/A1"), "--mode", "values"]);
    assert_eq!(code, 0, "the demanded cell still draws:\n{out}");
    assert!(
        out.contains("#SPILL!"),
        "the reference reads the load error's class, never a blank:\n{out}"
    );
}

/// The load-error grid is one cell per DECLARED coordinate, so a 4-byte file naming the whole sheet
/// would size an allocation off its NAME and abort the process. Over the bound it stays what it was
/// before a pulled fault was ever survivable: a cheap fatal refusal off the filename, no grid built.
#[test]
fn a_pulled_faults_declared_region_cannot_size_an_allocation() {
    for name in ["A1:XFD1048576", "A1:A1048576"] {
        let fx = Fixture::new("pulled-huge-region");
        fx.file("Sheet1", "A1", "=Sheet2!A1")
            .file("Sheet2", name, "x\ty");

        let (code, out) = run(&["check", &at(&fx, "Sheet1/A1")]);
        assert_eq!(code, 3, "{name} is a fatal refusal, never an abort:\n{out}");
        assert!(
            out.contains("dimension-mismatch") && out.contains(name),
            "{name} is refused off its filename:\n{out}"
        );
    }
}
