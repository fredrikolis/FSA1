// Concern: locks what a path naming a FILE the tab holds resolves to | Non-concern: the resolver's own unit coverage (fsa1-verbs/src/address.rs) | IO: spawns the binary -> stdout + stderr + exit status

mod common;

use common::{Fixture, at, run, run_err};

/// One file of every name-answerable kind, in one tab.
fn held(tag: &str) -> Fixture {
    let fx = Fixture::new(tag);
    fx.file("Orders", "A1:A5", "1\n2\n3\n4\n5\n")
        .file("Orders", "A1:A5.css", "td { color: crimson }\n")
        .file("Orders", "H1:K10.json", r#"{"mark":"bar"}"#)
        .file("Orders", "Chart1.json", r#"{"mark":"bar"}"#)
        .file(
            "Orders",
            "Chart1.css",
            "  figure { anchor: B2; height: 100px; left: 0px; top: 0px; width: 100px }\n",
        );
    fx
}

/// The host may spell a range with `-`, and the path must name the file as written.
fn held_at(fx: &Fixture, name: &str) -> String {
    at(fx, &format!("Orders/{}", fsa1_model::range_file_name(name)))
}

/// A path's final segment may name a FILE the tab holds, and it scopes what that file governs.
/// A name stating a rectangle scopes that rectangle; one stating no extent scopes the whole tab,
/// which is the honest superset — never a narrower region that is wrong.
#[test]
fn a_path_may_name_any_file_the_tab_holds() {
    let fx = held("path-names-a-file");

    let (code, out) = run(&["check", &held_at(&fx, "A1:A5.css")]);
    assert_eq!(code, 0, "a rooted sidecar's path is a scope:\n{out}");
    assert!(out.contains("no diagnostics"), "clean in scope:\n{out}");

    let (_, bare) = run(&["render", &held_at(&fx, "A1:A5")]);
    let (code, sidecar) = run(&["render", &held_at(&fx, "A1:A5.css")]);
    assert_eq!(code, 0, "{sidecar}");
    assert_eq!(sidecar, bare, "the sidecar scopes what its stem does");

    let (code, out) = run(&["check", &held_at(&fx, "H1:K10.json")]);
    assert_eq!(code, 0, "a range-form figure scopes its rectangle:\n{out}");

    for name in ["Chart1.json", "Chart1.css"] {
        let (code, out) = run(&["check", &held_at(&fx, name)]);
        assert_eq!(
            code, 0,
            "{name} states no extent, so it scopes the tab:\n{out}"
        );
    }

    let (code, _, err) = run_err(&["check", &held_at(&fx, "Nope.json")]);
    assert_eq!(
        code, 2,
        "an extension does not make a segment a file:\n{err}"
    );
    assert!(
        err.contains("not a canonical A1 cell or range") && err.contains("no defined name"),
        "the two-part refusal is unchanged:\n{err}"
    );

    let (code, _, err) = run_err(&["trace", &held_at(&fx, "A1:A5.css")]);
    assert_eq!(code, 2, "trace hits its own guard:\n{err}");
    assert!(
        err.contains("trace targets one cell") && err.contains("A1:A5.css"),
        "the refusal names the file the caller typed:\n{err}"
    );
}

/// `eval` takes `<wb>` or `<wb>/<tab>`, and a segment naming a FILE is neither — whichever kind of
/// file it was. A caller cannot see from the filename whether it states a region, so a name-form
/// figure must not slip through the region guard that a rooted sidecar is caught by.
#[test]
fn eval_refuses_a_file_segment_whatever_that_file_governs() {
    let fx = held("path-names-a-file-eval");
    for name in ["A1:A5.css", "H1:K10.json", "Chart1.json", "Chart1.css"] {
        let (code, out, err) = run_err(&["eval", &held_at(&fx, name), "--formula", "=1+1"]);
        assert_eq!(code, 2, "{name} is not a workbook-or-tab path:\n{out}{err}");
        assert!(
            err.contains("eval takes <wb> or <wb>/<tab>")
                && err.contains(&fsa1_model::range_file_name(name)),
            "the refusal names the file the caller typed:\n{err}"
        );
    }

    let (code, out) = run(&["eval", &at(&fx, "Orders"), "--formula", "=1+1"]);
    assert_eq!(code, 0, "a bare tab path still evaluates:\n{out}");
}

/// Naming a file is what gets its CONTENT graded: the refusal fired on the FILENAME, so an agent
/// authoring presentation or a figure could never have it checked at all.
#[test]
fn a_named_files_own_content_is_graded_where_the_name_refusal_hid_it() {
    let broken = held("path-names-a-file-figure");
    broken.file("Orders", "Chart1.json", "{");
    let (code, out) = run(&["check", &held_at(&broken, "Chart1.json")]);
    assert_eq!(code, 3, "the file the path NAMED is graded:\n{out}");
    assert!(
        out.contains("figure-syntax") && out.contains("Orders/Chart1.json"),
        "located at the named file:\n{out}"
    );

    let styled = held("path-names-a-file-css");
    styled.file("Orders", "A1:A5.css", "td { color:: }\n");
    let (code, out) = run(&["check", &held_at(&styled, "A1:A5.css")]);
    assert_eq!(code, 3, "the content is judged now:\n{out}");
    assert!(
        out.contains("presentation-syntax") && out.contains("Orders/A1:A5.css"),
        "located at the named file:\n{out}"
    );

    let unclaimed = held("path-names-a-file-unclaimed");
    unclaimed.file("Orders", "Units.css", "x\n");
    let (code, out) = run(&["check", &held_at(&unclaimed, "Units.css")]);
    assert_eq!(code, 3, "a file participating in nothing legal:\n{out}");
    assert!(
        out.contains("unclaimed-sidecar"),
        "the fault is reported, not hidden behind a path refusal:\n{out}"
    );
}
