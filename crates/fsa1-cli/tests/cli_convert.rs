// Concern: locks the unpack/pack argv dispatch, stdout and exit codes | Non-concern: the spreadsheet logic beneath it | IO: spawns the binary -> stdout + exit status

mod common;

use common::{Fixture, at, run, run_err, run_err_in, run_in, snapshot};
use std::path::{Path, PathBuf};

fn ingest_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fsa1-ingest/tests/fixtures")
        .join(name)
}

#[test]
fn unpack_ods_with_explicit_dst_then_render_and_eval_the_converted_workbook() {
    let fx = Fixture::new("unpack");
    let dest = fx.path().join("wb");
    let src = ingest_fixture("smoke.ods");
    let (code, out) = run(&["unpack", src.to_str().unwrap(), dest.to_str().unwrap()]);
    assert_eq!(code, 0, "unpack should succeed:\n{out}");
    assert!(
        out.contains("2 range file(s)"),
        "one file per block (Sheet1: A1:B3; Sheet2: A1), never one per cell:\n{out}"
    );

    let a3 = run(&[
        "eval",
        dest.join("Sheet1").to_str().unwrap(),
        "--formula",
        "=A3",
    ]);
    assert_eq!((a3.0, a3.1.trim()), (0, "30"));
    let b1 = run(&[
        "eval",
        dest.join("Sheet1").to_str().unwrap(),
        "--formula",
        "=B1",
    ]);
    assert_eq!((b1.0, b1.1.trim()), (0, "60"));
    let cross = run(&[
        "eval",
        dest.join("Sheet2").to_str().unwrap(),
        "--formula",
        "=A1",
    ]);
    assert_eq!((cross.0, cross.1.trim()), (0, "30"));
}

#[test]
fn unpack_derives_dst_in_the_cwd_from_the_src_stem() {
    let fx = Fixture::new("unpack-derive");
    let src = ingest_fixture("smoke.xlsx");
    let (code, out) = run_in(fx.path(), &["unpack", src.to_str().unwrap()]);
    assert_eq!(code, 0, "a dst-less unpack derives and exits 0:\n{out}");
    let derived = fx.path().join("smoke");
    assert!(
        derived.join("Sheet1").is_dir(),
        "the workbook is written at <cwd>/smoke/ (derived from the stem):\n{out}"
    );
    let (rc, rout) = run(&["render", derived.join("Sheet1").to_str().unwrap()]);
    assert_eq!(rc, 0, "the derived workbook renders:\n{rout}");
}

#[test]
fn unpack_explicit_dst_does_not_create_the_derived_name() {
    let fx = Fixture::new("unpack-explicit");
    let dest = fx.path().join("out/wb");
    let src = ingest_fixture("smoke.xlsx");
    let (code, out) = run_in(fx.path(), &["unpack", src.to_str().unwrap(), "out/wb"]);
    assert_eq!(code, 0, "explicit dst exits 0:\n{out}");
    assert!(
        dest.join("Sheet1").is_dir(),
        "the explicit dst is used:\n{out}"
    );
    assert!(
        !fx.path().join("smoke").exists(),
        "no derived ./smoke/ is created when <dst> is explicit:\n{out}"
    );
}

#[test]
fn unpack_into_a_non_empty_derived_dst_is_a_conflict() {
    let fx = Fixture::new("unpack-derive-conflict");
    let derived = fx.path().join("smoke");
    std::fs::create_dir_all(&derived).unwrap();
    std::fs::write(derived.join("sentinel"), "keep").unwrap();
    let src = ingest_fixture("smoke.xlsx");
    let (code, _) = run_in(fx.path(), &["unpack", src.to_str().unwrap()]);
    assert_eq!(code, 4, "a non-empty derived dst is a conflict (exit 4)");
    let kept = std::fs::read_to_string(derived.join("sentinel")).expect("sentinel survives");
    assert_eq!(kept, "keep", "the pre-existing derived dir is untouched");
}

#[test]
fn unpack_into_a_non_empty_explicit_dst_is_a_conflict() {
    let fx = Fixture::new("unpack-conflict");
    fx.file("Existing", "A1", "1");
    let src = ingest_fixture("smoke.ods");
    let (code, _) = run(&["unpack", src.to_str().unwrap(), fx.path().to_str().unwrap()]);
    assert_eq!(code, 4, "a non-empty destination is a conflict (exit 4)");
}

#[test]
fn unpack_a_missing_source_is_not_found() {
    let fx = Fixture::new("unpack-missing");
    let dest = fx.path().join("wb");
    let (code, _) = run(&["unpack", "/no/such/file.ods", dest.to_str().unwrap()]);
    assert_eq!(code, 24, "a missing source is not found (exit 24)");
}

/// `appearance` would cut Sheet1 as A1:B1 then A2:A3: no `<c>` in smoke.xlsx states `s=`, so its
/// four cells share one signature, cover row-major, and no merge pays for either. Sheet2 states A1
/// alone. This test asserts the UNNAMED cut, which is always `occupancy` — the same four cells are
/// one A1:B3, so the count is 2.
#[test]
fn unpack_xlsx_then_eval_the_converted_workbook() {
    let fx = Fixture::new("unpack-xlsx");
    let dest = fx.path().join("wb");
    let src = ingest_fixture("smoke.xlsx");
    let (code, out) = run(&["unpack", src.to_str().unwrap(), dest.to_str().unwrap()]);
    assert_eq!(code, 0, "xlsx unpack should succeed:\n{out}");
    assert!(
        out.contains("2 range file(s)"),
        "unnamed is occupancy: Sheet1's four cells are one A1:B3, Sheet2's is A1:\n{out}"
    );

    let a3 = run(&[
        "eval",
        dest.join("Sheet1").to_str().unwrap(),
        "--formula",
        "=A3",
    ]);
    assert_eq!((a3.0, a3.1.trim()), (0, "30"));
    let cross = run(&[
        "eval",
        dest.join("Sheet2").to_str().unwrap(),
        "--formula",
        "=A1",
    ]);
    assert_eq!((cross.0, cross.1.trim()), (0, "30"));
}

/// An agent that cannot read which shape it just got has to go and look at the tree to find out.
/// Unnamed resolves to `occupancy` whatever the source carries, so both rows say the same thing --
/// what is asserted is that the run SAYS which policy cut it, not which one that turns out to be.
#[test]
fn unpack_names_the_policy_it_resolved_to() {
    for (source, policy) in [("smoke.ods", "occupancy"), ("smoke.xlsx", "occupancy")] {
        let fx = Fixture::new("unpack-resolved");
        let src = ingest_fixture(source);
        let (code, out) = run(&["unpack", src.to_str().unwrap(), &at(&fx, "wb")]);
        assert_eq!(code, 0, "{source} unpacks:\n{out}");
        assert!(
            out.contains(&format!("decomposed by {policy}")),
            "{source} resolves to {policy} and must say so:\n{out}"
        );
    }
}

#[test]
fn unpack_decompose_takes_its_value_inline_or_as_the_next_argument() {
    let src = ingest_fixture("smoke.xlsx");
    for spelling in [
        vec!["--decompose=appearance".to_string()],
        vec!["--decompose".to_string(), "appearance".to_string()],
    ] {
        let fx = Fixture::new("unpack-decompose-spelling");
        let dest = at(&fx, "wb");
        let mut args = vec!["unpack".to_string()];
        args.extend(spelling.clone());
        args.push(src.to_str().unwrap().to_string());
        args.push(dest);
        let argv: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, out) = run(&argv);
        assert_eq!(code, 0, "{spelling:?} should parse:\n{out}");
        assert!(out.contains("decomposed by appearance"), "{out}");
    }
}

#[test]
fn unpack_decompose_of_an_unknown_policy_is_bad_args_and_writes_nothing() {
    let fx = Fixture::new("unpack-decompose-bad");
    let src = ingest_fixture("smoke.xlsx");
    let dest = at(&fx, "wb");
    let (code, _out, err) = run_err(&[
        "unpack",
        "--decompose",
        "semantic",
        src.to_str().unwrap(),
        &dest,
    ]);
    assert_eq!(code, 2, "an unknown policy is bad args (exit 2):\n{err}");
    assert!(
        err.contains("unknown --decompose \"semantic\"") && err.contains("occupancy, appearance"),
        "the refusal names the value and every choice:\n{err}"
    );
    assert!(!Path::new(&dest).exists(), "a refused flag writes nothing");
}

/// The one combination the resolution rule forbids, at the surface an agent reaches it from.
#[test]
fn unpack_decompose_appearance_of_an_ods_is_refused_before_anything_is_written() {
    let fx = Fixture::new("unpack-decompose-ods");
    let src = ingest_fixture("styled.ods");
    let dest = at(&fx, "wb");
    let (code, _out, err) = run_err(&[
        "unpack",
        "--decompose",
        "appearance",
        src.to_str().unwrap(),
        &dest,
    ]);
    assert_eq!(
        code, 3,
        "a source that cannot feed the policy is exit 3:\n{err}"
    );
    assert!(
        err.contains("styled.ods") && err.contains("appearance channel"),
        "the refusal names the source and the missing channel:\n{err}"
    );
    assert!(
        !Path::new(&dest).exists(),
        "the refusal fires before any write"
    );
}

#[test]
fn unpack_an_unsupported_extension_is_a_validation_refusal() {
    let fx = Fixture::new("unpack-badext");
    let src = fx.path().join("data.csv");
    std::fs::write(&src, "a,b\n1,2\n").unwrap();
    let (code, _) = run_in(fx.path(), &["unpack", src.to_str().unwrap()]);
    assert_eq!(
        code, 3,
        "an unsupported extension is a validation error (exit 3)"
    );
}

#[test]
fn unpack_an_un_derivable_src_without_dst_is_bad_args() {
    for src in [".", "..", "/"] {
        let (code, _, err) = run_err(&["unpack", src]);
        assert_eq!(code, 2, "unpack {src:?} is un-derivable -> exit 2");
        assert!(
            err.contains("cannot derive a workbook directory name"),
            "the located refusal for {src:?}:\n{err}"
        );
    }
}

#[test]
fn unpack_dotxlsx_derives_then_refuses_downstream_not_at_arg_parse() {
    let fx = Fixture::new("unpack-dotxlsx");
    let (code, _) = run_in(fx.path(), &["unpack", ".xlsx"]);
    assert_eq!(
        code, 3,
        "`.xlsx` derives then fails downstream at the source-format gate (exit 3), not exit 2"
    );
}

#[test]
fn unpack_reports_coerced_number_formats_on_stderr_exit_0() {
    let fx = Fixture::new("fidelity-coerce");
    let src = ingest_fixture("literals.xlsx");
    let (code, _out, err) = run_err(&["unpack", src.to_str().unwrap(), &at(&fx, "wb")]);
    assert_eq!(code, 0, "a lossy unpack stays exit 0:\n{err}");
    assert!(
        err.contains("number formats coerced to plain"),
        "the coercion section is on stderr:\n{err}"
    );
    assert!(
        err.contains("Data!D2:") && err.contains("value kept as plain"),
        "the coerced cell is located:\n{err}"
    );
}

#[test]
fn unpack_reports_skipped_name_and_verbatim_formula_on_stderr_exit_0() {
    let fx = Fixture::new("fidelity-cd");
    let src = ingest_fixture("fidelity.xlsx");
    let (code, _out, err) = run_err(&["unpack", src.to_str().unwrap(), &at(&fx, "wb")]);
    assert_eq!(code, 0, "a lossy unpack stays exit 0:\n{err}");
    assert!(
        err.contains("defined names skipped")
            && err.contains("\"A1\" (workbook): identifier parses as an A1 address"),
        "the name-skip section is located on stderr:\n{err}"
    );
    assert!(
        err.contains("formulas kept verbatim") && err.contains("Data!B1:"),
        "the verbatim-formula section is located on stderr:\n{err}"
    );
}

#[test]
fn unpack_of_a_clean_file_reports_nothing_lost_exit_0() {
    let fx = Fixture::new("fidelity-clean");
    let src = ingest_fixture("smoke.xlsx");
    let (code, _out, err) = run_err(&["unpack", src.to_str().unwrap(), &at(&fx, "wb")]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("unpack fidelity: nothing lost"), "{err}");
    assert!(
        !err.contains("fidelity report"),
        "a clean unpack shows no per-category report:\n{err}"
    );
}

#[test]
fn unpack_strict_that_passes_still_reports_c_and_d_losses_exit_0() {
    let fx = Fixture::new("fidelity-strict-pass");
    let src = ingest_fixture("fidelity.xlsx");
    let (code, _out, err) = run_err(&["unpack", "--strict", src.to_str().unwrap(), &at(&fx, "wb")]);
    assert_eq!(
        code, 0,
        "a strict unpack that passes pre-flight exits 0:\n{err}"
    );
    assert!(
        err.contains("formulas kept verbatim") && err.contains("Data!B1:"),
        "strict does not police formula translation, so (d) still reports:\n{err}"
    );
    assert!(
        err.contains("defined names skipped") && err.contains("\"A1\""),
        "strict does not police name idents, so (c) still reports:\n{err}"
    );
}

#[test]
fn unpack_strict_of_a_clean_file_reports_nothing_lost_exit_0() {
    let fx = Fixture::new("fidelity-strict-clean");
    let src = ingest_fixture("smoke.xlsx");
    let (code, _out, err) = run_err(&["unpack", "--strict", src.to_str().unwrap(), &at(&fx, "wb")]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("unpack fidelity: nothing lost"), "{err}");
}

#[test]
fn unpack_strict_that_fails_preflight_produces_no_report_exit_3() {
    let fx = Fixture::new("fidelity-strict-refuse");
    let src = ingest_fixture("literals.xlsx");
    let (code, _out, err) = run_err(&["unpack", "--strict", src.to_str().unwrap(), &at(&fx, "wb")]);
    assert_eq!(code, 3, "a strict pre-flight refusal is exit 3:\n{err}");
    assert!(
        !err.contains("fidelity report") && !err.contains("nothing lost"),
        "a refusal imports nothing, so there is no fidelity report:\n{err}"
    );
}

#[test]
fn unpack_report_is_full_and_uncapped() {
    let fx = Fixture::new("fidelity-uncapped");
    let src = ingest_fixture("many_formats.xlsx");
    let (code, _out, err) = run_err(&["unpack", src.to_str().unwrap(), &at(&fx, "wb")]);
    assert_eq!(code, 0, "{err}");
    let lines = err.matches("dropped; value kept as plain").count();
    assert!(
        lines >= 50,
        "expected >= 50 coerced lines, got {lines}:\n{err}"
    );
    assert_eq!(lines, 60, "every one of the 60 formatted cells is reported");
    assert!(
        !err.contains("and ") || !err.contains("more"),
        "no truncation/\"and N more\" marker:\n{err}"
    );
}

#[test]
fn unpack_ods_reports_only_the_verbatim_formula_category_exit_0() {
    let fx = Fixture::new("fidelity-ods");
    let src = ingest_fixture("fidelity.ods");
    let (code, _out, err) = run_err(&["unpack", src.to_str().unwrap(), &at(&fx, "wb")]);
    assert_eq!(code, 0, "{err}");
    assert!(
        err.contains("formulas kept verbatim") && err.contains("Calc!A1:"),
        "the (d) section appears for .ods:\n{err}"
    );
    assert!(
        !err.contains("number formats coerced")
            && !err.contains("tables dropped")
            && !err.contains("defined names skipped"),
        "categories (a)/(b)/(c) are xlsx-only:\n{err}"
    );

    let fx2 = Fixture::new("fidelity-ods-clean");
    let clean = ingest_fixture("smoke.ods");
    let (code, _out, err) = run_err(&["unpack", clean.to_str().unwrap(), &at(&fx2, "wb")]);
    assert_eq!(code, 0, "{err}");
    assert!(
        err.contains("no loss in any category this source was inspected for"),
        "{err}"
    );
}

/// The reproduction: this fixture's A1 is bold, red, on a yellow fill, in a 2.5cm column, and every bit
/// of that leaves in silence — the .ods reader is values and formulas by design. Exit 0 and a written
/// tree are correct; a line crediting appearance and geometry to a run that opened neither is not.
#[test]
fn unpack_of_a_styled_ods_never_claims_nothing_lost_and_names_what_it_did_not_inspect() {
    let fx = Fixture::new("fidelity-ods-styled");
    let src = ingest_fixture("styled.ods");
    let (code, _out, err) = run_err(&["unpack", src.to_str().unwrap(), &at(&fx, "wb")]);
    assert_eq!(code, 0, "{err}");
    assert!(
        !err.contains("nothing lost"),
        "the appearance is gone; nothing may claim otherwise:\n{err}"
    );
    for withheld in [
        "number formats",
        "tables",
        "defined names",
        "appearance",
        "column widths, row heights",
        "charts",
        "workbook parts",
    ] {
        assert!(
            err.contains(withheld),
            "{withheld:?} is neither reported nor withheld by name:\n{err}"
        );
    }
    assert!(
        err.contains("not inspected on this source, so not vouched for"),
        "{err}"
    );
    // No styling reader ran, so the tab states no presentation at all — not an empty sidecar.
    let styled: Vec<String> = std::fs::read_dir(Path::new(&at(&fx, "wb")).join("Styled"))
        .expect("the tab is readable")
        .map(|e| {
            e.expect("a readable entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|n| fsa1_model::is_presentation_entry(n))
        .collect();
    assert!(styled.is_empty(), "{styled:?}");
}

#[test]
fn unpack_ods_strict_is_a_no_op_and_reports_the_same_verbatim_formula_exit_0() {
    let fx = Fixture::new("fidelity-ods-strict");
    let src = ingest_fixture("fidelity.ods");
    let (code, _out, err) = run_err(&["unpack", "--strict", src.to_str().unwrap(), &at(&fx, "wb")]);
    assert_eq!(code, 0, "strict is a no-op for .ods, so exit 0:\n{err}");
    assert!(
        err.contains("formulas kept verbatim") && err.contains("Calc!A1:"),
        "the (d) section still appears under --strict for .ods:\n{err}"
    );
}

#[test]
fn the_old_import_and_export_verbs_no_longer_resolve() {
    let src = ingest_fixture("smoke.xlsx");
    for verb in ["import", "export"] {
        let (code, _, err) = run_err(&[verb, src.to_str().unwrap(), "whatever"]);
        assert_eq!(
            code, 2,
            "the old `{verb}` verb is an unknown command (exit 2)"
        );
        assert!(
            err.contains("unknown command") && err.contains(verb),
            "the refusal names the unknown `{verb}`:\n{err}"
        );
    }
}

fn pack_workbook(cwd: &Path, basename: &str) -> PathBuf {
    let book = cwd.join(basename);
    for (tab, cell, body) in [
        ("Sheet1", "A1", "42"),
        ("Sheet1", "B1", "=A1*2"),
        ("Summary", "A1", "=Sheet1!A1"),
    ] {
        let d = book.join(tab);
        std::fs::create_dir_all(&d).expect("create tab dir");
        std::fs::write(d.join(cell), body).expect("write cell");
    }
    book
}

#[test]
fn pack_derives_the_output_xlsx_calamine_reopens_and_refuses_to_clobber() {
    use calamine::{Data, Reader, open_workbook_auto};

    let cwd = Fixture::new("pack");
    let book = pack_workbook(cwd.path(), "book");
    let dest = cwd.path().join("book.xlsx");

    let before = snapshot(&book);

    let (code, out) = run_in(cwd.path(), &["pack", book.to_str().unwrap()]);
    assert_eq!(code, 0, "clean pack exits 0; got:\n{out}");
    assert!(
        dest.exists(),
        "pack must write the derived ./book.xlsx in the CWD"
    );

    // Formula cells carry no cached value by design, so this checks structure and literals, never the recompute — the oracle harness grades that.
    let mut wb = open_workbook_auto(&dest).expect("calamine re-opens the packed .xlsx");
    let mut names = wb.sheet_names().to_vec();
    names.sort();
    assert_eq!(
        names,
        vec!["Sheet1".to_string(), "Summary".to_string()],
        "both authored sheets are emitted"
    );
    let range = wb
        .worksheet_range("Sheet1")
        .expect("calamine reads Sheet1's values");
    assert_eq!(
        range.get_value((0, 0)),
        Some(&Data::Float(42.0)),
        "Sheet1!A1 round-trips as the literal 42"
    );

    assert_eq!(
        before,
        snapshot(&book),
        "pack must leave the source workbook's authoritative cells/tabs/names byte-identical (CORE3)"
    );

    let (code2, _) = run_in(cwd.path(), &["pack", book.to_str().unwrap()]);
    assert_eq!(
        code2, 4,
        "pack refuses an already-occupied derived dest (never clobbers, exit 4)"
    );
}

#[test]
fn pack_derives_the_basename_only_discarding_the_folders_path_prefix() {
    let cwd = Fixture::new("pack-basename");
    let book = pack_workbook(&cwd.path().join("nested"), "book");
    let derived = cwd.path().join("book.xlsx");

    let (code, out) = run_in(cwd.path(), &["pack", book.to_str().unwrap()]);
    assert_eq!(code, 0, "pack of a nested folder exits 0:\n{out}");
    assert!(
        derived.exists(),
        "the output is ./book.xlsx in the CWD (basename only), not beside the folder:\n{out}"
    );
    assert!(
        !book.join("book.xlsx").exists() && !cwd.path().join("nested/book.xlsx").exists(),
        "nothing is written beside the source folder:\n{out}"
    );

    // The derived dest is now occupied, so the explicit-default case needs its own CWD.
    let cwd2 = Fixture::new("pack-target-default");
    let book2 = pack_workbook(cwd2.path(), "book");
    let (tc, tout) = run_in(
        cwd2.path(),
        &["pack", book2.to_str().unwrap(), "--target", "xlsx"],
    );
    assert_eq!(tc, 0, "--target xlsx is the explicit default:\n{tout}");
    assert!(
        cwd2.path().join("book.xlsx").exists(),
        "--target xlsx yields the identical derived output:\n{tout}"
    );
}

#[test]
fn pack_refuses_a_non_xlsx_target() {
    let cwd = Fixture::new("pack-target-bad");
    let book = pack_workbook(cwd.path(), "book");
    for bad in ["ods", "csv"] {
        let (code, _, err) = run_err_in(
            cwd.path(),
            &["pack", book.to_str().unwrap(), "--target", bad],
        );
        assert_eq!(code, 2, "`--target {bad}` is bad args (exit 2)");
        assert!(
            err.contains("only --target xlsx is supported"),
            "the located --target refusal for {bad}:\n{err}"
        );
    }
    assert!(
        !cwd.path().join("book.xlsx").exists(),
        "a refused --target writes no file"
    );
}

#[test]
fn pack_an_un_derivable_folder_is_bad_args() {
    for folder in [".", "..", "/"] {
        let (code, _, err) = run_err(&["pack", folder]);
        assert_eq!(code, 2, "pack {folder:?} is un-derivable -> exit 2");
        assert!(
            err.contains("cannot derive an output name"),
            "the located refusal for {folder:?}:\n{err}"
        );
    }
}
