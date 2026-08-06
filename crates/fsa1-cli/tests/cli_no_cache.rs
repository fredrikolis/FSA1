// Concern: locks every verb's cache-free argv dispatch, stdout and exit codes | Non-concern: the spreadsheet logic beneath it | IO: spawns the binary -> stdout + exit status

mod common;

use common::{Fixture, at, run, run_err_in, run_in, snapshot};
use std::path::Path;

/// Over fsa1-model's materialization bound, so `check` reports `range-too-large` and exits 3.
const OVER_BOUND: &str = "=SUM(A1:AZ100000)";

fn cache_dir_exists(root: &Path) -> bool {
    root.join(".cache").exists()
}

/// Real formula depth, so "nothing was written" is measured rather than an artifact of an empty tree.
fn evaluating_workbook(tag: &str) -> Fixture {
    let fx = Fixture::new(tag);
    fx.file("Sheet1", "A1:A8", "1\n2\n3\n4\n5\n6\n7\n8")
        .file(
            "Sheet1",
            "B1:B8",
            "=A1*2\n=A2*2\n=A3*2\n=A4*2\n=A5*2\n=A6*2\n=A7*2\n=A8*2",
        )
        .file("Sheet1", "C1", "=SUM(B1:B8)")
        .file("Summary", "A1", "=Sheet1!C1+1");
    fx
}

#[test]
fn no_command_creates_a_cache_directory_under_a_workbook() {
    let cases: [(&str, &[&str], bool); 6] = [
        ("render", &[], false),
        ("check", &[], false),
        ("eval", &["--formula", "=Sheet1!C1"], false),
        ("trace", &[], true),
        ("tree", &[], false),
        ("pack", &[], false),
    ];
    for (verb, extra, single_cell) in cases {
        let cwd = Fixture::new(&format!("nocachedir-{verb}-cwd"));
        let fx = evaluating_workbook(&format!("nocachedir-{verb}"));
        let target = if single_cell {
            at(&fx, "Sheet1/C1")
        } else {
            fx.path().to_str().unwrap().to_string()
        };
        let before = snapshot(fx.path());
        let mut args: Vec<&str> = vec![verb, &target];
        args.extend_from_slice(extra);
        let (code, out) = run_in(cwd.path(), &args);
        assert_eq!(code, 0, "{verb} exits 0 on a clean workbook:\n{out}");
        assert!(
            !cache_dir_exists(fx.path()),
            "{verb} must not create a .cache/ directory"
        );
        assert_eq!(
            before,
            snapshot(fx.path()),
            "{verb} must leave the source workbook byte-identical (CORE3)"
        );
    }

    let cwd = Fixture::new("nocachedir-sample-cwd");
    let dst = at(&cwd, "made");
    let (code, out) = run_in(cwd.path(), &["sample", &dst]);
    assert_eq!(code, 0, "sample exits 0:\n{out}");
    let (code, out) = run_in(cwd.path(), &["render", &dst]);
    assert_eq!(code, 0, "rendering the sample exits 0:\n{out}");
    assert!(
        !cache_dir_exists(Path::new(&dst)),
        "reading a freshly sampled workbook must not create a .cache/ either"
    );
}

#[test]
fn a_pre_existing_cache_directory_is_ignored_by_every_command() {
    let plain = evaluating_workbook("stalecache-plain");
    let stale = evaluating_workbook("stalecache-stale");
    // `A1` is also a valid cell filename, so a `.cache/` read as a tab would contribute a wrong value.
    std::fs::create_dir_all(stale.path().join(".cache")).unwrap();
    std::fs::write(stale.path().join(".cache/v2-0123456789abcdef"), [0u8; 17]).unwrap();
    std::fs::write(stale.path().join(".cache/A1"), "999").unwrap();
    std::fs::write(stale.path().join(".cache/.tmp-v2-dead-1"), b"torn").unwrap();

    for (verb, extra, single_cell) in [
        ("render", &[][..], false),
        ("check", &[], false),
        ("tree", &[], false),
        ("eval", &["--formula", "=Sheet1!C1"], false),
        ("trace", &[], true),
    ] {
        let mut a: Vec<&str> = vec![verb];
        let pa = if single_cell {
            at(&plain, "Sheet1/C1")
        } else {
            plain.path().to_str().unwrap().to_string()
        };
        a.push(&pa);
        a.extend_from_slice(extra);
        let (pc, pout) = run(&a);

        let mut b: Vec<&str> = vec![verb];
        let sb = if single_cell {
            at(&stale, "Sheet1/C1")
        } else {
            stale.path().to_str().unwrap().to_string()
        };
        b.push(&sb);
        b.extend_from_slice(extra);
        let (sc, sout) = run(&b);

        // No verb prints the workbook root, so two fixtures at different paths compare directly.
        assert_eq!(
            (pc, &pout),
            (sc, &sout),
            "{verb} must be unaffected by a pre-existing .cache/"
        );
    }

    let (code, out) = run(&["tree", stale.path().to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("Sheet1") && out.contains("Summary"),
        "both authored tabs are present:\n{out}"
    );
    assert!(
        !out.contains(".cache") && !out.contains("999"),
        "the reserved .cache/ is neither a tab nor a cell:\n{out}"
    );
}

#[test]
fn check_over_an_over_bound_range_reports_the_same_refusal_on_every_run() {
    let fx = Fixture::new("check-repeat");
    fx.file("S", "A1", "1")
        .file("S", "B1", OVER_BOUND)
        .file("S", "C1", "=B1+1") // an ancestor of the refusing cell
        .file("S", "D1", OVER_BOUND); // a duplicate-text sibling of B1
    let wb = fx.path().to_str().unwrap();

    let (first_code, first_out) = run(&["check", wb]);
    assert_eq!(
        first_code, 3,
        "check reports the refusal and exits 3:\n{first_out}"
    );
    assert!(
        first_out.contains("range-too-large"),
        "the located refusal:\n{first_out}"
    );

    let (second_code, second_out) = run(&["check", wb]);
    assert_eq!(
        (second_code, &second_out),
        (first_code, &first_out),
        "a repeated check must produce IDENTICAL stdout and exit code"
    );

    let (rc, _) = run(&["render", &at(&fx, "S/C1")]);
    assert_eq!(rc, 0, "rendering the ancestor succeeds (it prints #NUM!)");
    let (after_code, after_out) = run(&["check", wb]);
    assert_eq!(
        (after_code, &after_out),
        (first_code, &first_out),
        "a check after the ancestor was read must still report the refusal"
    );

    assert!(
        !cache_dir_exists(fx.path()),
        "and none of those runs wrote a .cache/"
    );
}

#[test]
fn no_cache_is_an_unknown_flag_on_every_verb() {
    let cwd = Fixture::new("nc-unknown");
    let sample_dst = at(&cwd, "out");
    let fx = evaluating_workbook("nc-unknown-wb");
    let wb = fx.path().to_str().unwrap();
    let cell = at(&fx, "Sheet1/C1");
    for args in [
        vec!["render", wb, "--no-cache"],
        vec!["check", wb, "--no-cache"],
        vec!["eval", wb, "--formula", "=1+1", "--no-cache"],
        vec!["trace", cell.as_str(), "--no-cache"],
        vec!["tree", wb, "--no-cache"],
        vec!["pack", wb, "--no-cache"],
        vec!["sample", sample_dst.as_str(), "--no-cache"],
        vec!["unpack", "book.xlsx", "--no-cache"],
        vec!["--no-cache", "check", wb],
        vec!["--no-cache", "render", wb],
        vec!["--no-cache"],
        vec!["check", wb, "--no-cache=zzz"],
    ] {
        let (code, _, err) = run_err_in(cwd.path(), &args);
        assert_eq!(code, 2, "{args:?} must be refused as a bad argument");
        assert!(
            !err.is_empty(),
            "{args:?} must explain the refusal on stderr"
        );
    }
    assert!(
        !target_exists(&sample_dst),
        "a refused sample must have written nothing"
    );
    assert!(
        !cache_dir_exists(fx.path()),
        "and no refused run wrote a .cache/"
    );
}

fn target_exists(p: &str) -> bool {
    Path::new(p).exists()
}

#[test]
fn trace_prints_error_rather_than_a_digest_for_an_over_bound_cone() {
    let fx = Fixture::new("trace-overbound");
    fx.file("S", "A1", "1")
        .file("S", "B1", OVER_BOUND)
        .file("S", "C1", "=B1+1") // an ANCESTOR: hashless by upward propagation
        .file("S", "X1", "=A1+1"); // an ordinary, fully-hashable sibling

    let (code, out) = run(&["trace", &at(&fx, "S/C1")]);
    assert_eq!(code, 0, "trace itself succeeds (the walk is total):\n{out}");
    assert_eq!(
        out.matches("[error]").count(),
        2,
        "C1 and B1 are both hashless -> both show [error]:\n{out}"
    );

    let (xc, xout) = run(&["trace", &at(&fx, "S/X1")]);
    assert_eq!(xc, 0, "{xout}");
    assert!(
        !xout.contains("[error]"),
        "an ordinary cone still shows digests:\n{xout}"
    );
}
