// Concern: locks the sample/--guide/--version/unknown-verb argv dispatch, stdout and exit codes | Non-concern: the spreadsheet logic beneath it | IO: spawns the binary -> stdout + exit status

mod common;

use common::{Fixture, run};

#[test]
fn version_prints_a_json_envelope() {
    let (code, out) = run(&["--version"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("\"status\":\"success\""),
        "version envelope:\n{out}"
    );
    assert!(
        out.contains("\"name\":\"fsa1-cli\""),
        "version envelope:\n{out}"
    );
}

#[test]
fn unknown_command_is_bad_args() {
    let (code, _) = run(&["frobnicate"]);
    assert_eq!(code, 2, "an unknown command is exit 2 (bad args)");
}

#[test]
fn sample_writes_a_renderable_workbook_and_prints_next_steps() {
    let fx = Fixture::new("sample");
    let target = fx.path().join("wb");
    let target_s = target.to_str().unwrap().to_string();

    let (code, out) = run(&["sample", &target_s]);
    assert_eq!(code, 0, "a fresh sample exits 0; got:\n{out}");
    assert!(target.join("Orders").is_dir(), "Orders tab written:\n{out}");
    assert!(
        target.join("Summary").is_dir(),
        "Summary tab written:\n{out}"
    );
    assert!(
        target
            .join("Orders")
            .join(fsa1_model::range_file_name("A1:D1"))
            .is_file(),
        "the header range file exists:\n{out}"
    );
    assert!(
        target.join("Orders/D5").is_file(),
        "the SUM total cell exists:\n{out}"
    );
    assert!(out.contains("next:"), "next-steps hint printed:\n{out}");
    assert!(
        out.contains("fsa1-cli render"),
        "next-steps names fsa1-cli:\n{out}"
    );
    // Nothing else pins the hint to the sample, so a changed total would go silently stale without this.
    assert!(
        out.contains("110"),
        "next-steps hint quotes the sample total (110):\n{out}"
    );

    let (rcode, rout) = run(&["render", &target_s]);
    assert_eq!(rcode, 0, "the written sample renders cleanly:\n{rout}");
    assert!(
        rout.contains("110"),
        "D5 grand total renders as 110:\n{rout}"
    );
}

#[test]
fn sample_writes_into_an_existing_empty_dir() {
    let fx = Fixture::new("sample-empty");
    let target = fx.path().join("wb");
    std::fs::create_dir_all(&target).expect("pre-create an empty target dir");
    let target_s = target.to_str().unwrap().to_string();

    let (code, out) = run(&["sample", &target_s]);
    assert_eq!(
        code, 0,
        "an existing EMPTY dir is not a clobber -> sample proceeds and exits 0; got:\n{out}"
    );
    assert!(
        target.join("Orders/D5").is_file(),
        "the workbook was written into the pre-existing empty dir:\n{out}"
    );
}

#[test]
fn sample_refuses_to_clobber_a_nonempty_dir_and_writes_nothing() {
    let fx = Fixture::new("sample-clobber");
    fx.file("keep", "A1", "42");
    let target_s = fx.path().to_str().unwrap().to_string();

    let (code, _out) = run(&["sample", &target_s]);
    assert_eq!(
        code, 4,
        "clobber refusal is a CONFLICT (exit 4), not bad-args (exit 2)"
    );
    assert!(
        !fx.path().join("Orders").exists(),
        "refusal must write no Orders tab"
    );
    assert!(
        !fx.path().join("Summary").exists(),
        "refusal must write no Summary tab"
    );
    let kept = std::fs::read_to_string(fx.path().join("keep/A1")).expect("sentinel survives");
    assert!(kept.contains("42"), "the pre-existing file is untouched");
}

#[test]
fn guide_prints_and_exits_zero() {
    let (code, out) = run(&["--guide"]);
    assert_eq!(code, 0, "--guide exits 0:\n{out}");
    assert!(
        out.contains("fsa1-cli"),
        "the guide names the binary:\n{out}"
    );
    assert!(
        out.contains("STRUCTURE"),
        "the guide has its structure section:\n{out}"
    );
}
