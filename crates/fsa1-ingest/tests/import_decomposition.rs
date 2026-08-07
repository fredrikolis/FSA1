// Concern: which policy a source resolves to, and what an explicit one writes or refuses | Non-concern: how either policy cuts (partition.rs), the fidelity report | IO: (fixtures) -> a temp workbook

use std::path::{Path, PathBuf};

use fsa1_ingest::{Decomposition, ErrorKind, import_file, import_file_as};

/// The committed fixtures are pure data, so these tests need no python toolchain.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Unique, and never pre-created, so the never-clobber path is exercised.
fn temp_dest(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "fsa1-ingest-decompose-{tag}-{}-{nanos}",
        std::process::id()
    ))
}

/// Every written file as `(path relative to the root, its bytes)`, sorted — the whole tree, so a
/// comparison catches a file only one side wrote as readily as a byte only one side spelled.
fn tree(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read the written tree") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .expect("every file is under the root")
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            out.push((rel, std::fs::read(&path).expect("read a written file")));
        }
    }
    out.sort();
    out
}

/// Paths with each range file's name in the canonical `:` spelling, whatever separator this host
/// wrote: an assertion below names a REGION, not the platform the test ran on.
fn names_in(root: &Path) -> Vec<String> {
    tree(root)
        .into_iter()
        .map(|(path, _)| match path.rsplit_once('/') {
            Some((dir, name)) => format!("{dir}/{}", fsa1_model::canonical_range_name(name)),
            None => path,
        })
        .collect()
}

/// The whole resolution rule on the side that has no appearance channel: `.ods` states no style, so
/// the unflagged import must write what `occupancy` writes, down to the byte, and say so.
#[test]
fn an_ods_resolves_to_occupancy_and_writes_exactly_what_occupancy_writes() {
    for source in ["smoke.ods", "styled.ods", "fidelity.ods"] {
        let stem = source.trim_end_matches(".ods");
        let resolved_dest = temp_dest(&format!("{stem}-resolved"));
        let named_dest = temp_dest(&format!("{stem}-named"));
        let resolved =
            import_file(&fixture(source), &resolved_dest, false).expect("the import completes");
        import_file_as(
            &fixture(source),
            &named_dest,
            false,
            Decomposition::Occupancy,
        )
        .expect("the import completes");

        assert_eq!(
            resolved.decomposition,
            Decomposition::Occupancy,
            "{source} carries no appearance channel"
        );
        assert_eq!(
            tree(&resolved_dest),
            tree(&named_dest),
            "{source} resolved to a tree `--decompose occupancy` did not write"
        );
        std::fs::remove_dir_all(&resolved_dest).ok();
        std::fs::remove_dir_all(&named_dest).ok();
    }
}

/// An xlsx CAN feed `appearance`, and still is not cut by it unless asked: the codec is opt-in, so
/// no tree moves under anyone who named no policy.
#[test]
fn an_xlsx_resolves_to_occupancy_but_accepts_appearance_by_name() {
    let dest = temp_dest("xlsx-resolved");
    let report = import_file(&fixture("smoke.xlsx"), &dest, false).expect("the import completes");
    assert_eq!(report.decomposition, Decomposition::Occupancy);
    std::fs::remove_dir_all(&dest).ok();

    let named = temp_dest("xlsx-named");
    let report = import_file_as(
        &fixture("smoke.xlsx"),
        &named,
        false,
        Decomposition::Appearance,
    )
    .expect("an xlsx carries an appearance channel, so appearance is accepted");
    assert_eq!(report.decomposition, Decomposition::Appearance);
    std::fs::remove_dir_all(&named).ok();
}

/// Under `appearance` every signature on a source with no appearance channel would be `None`, so the
/// sheet would coarsen instead of splitting. The refusal is a located one, and it fires early enough
/// that the destination is never created.
#[test]
fn appearance_on_a_source_with_no_appearance_channel_is_refused_and_writes_nothing() {
    let dest = temp_dest("ods-appearance");
    let src = fixture("styled.ods");
    let err = import_file_as(&src, &dest, false, Decomposition::Appearance)
        .expect_err("an .ods cannot feed the appearance decomposition");

    assert_eq!(err.kind, ErrorKind::Invalid, "{}", err.message);
    assert!(
        err.message.contains(&src.display().to_string()),
        "the refusal names the source: {}",
        err.message
    );
    assert!(
        err.message.contains("appearance channel")
            && err
                .message
                .contains(&format!("by {}", Decomposition::Appearance.name())),
        "the refusal names the missing channel and the policy asking for it: {}",
        err.message
    );
    assert!(
        !dest.exists(),
        "a refused decomposition writes nothing at all"
    );
}

/// The resolution is what moved; the policy did not. These are the RANGE FILES `unpack` wrote for
/// these two before a policy could be named, so `--decompose occupancy` still reaches them. The
/// `.gitattributes` beside them is a reserved entry, not one of them: it is excluded from the count
/// the report carries, and no cell's value derives from it.
#[test]
fn occupancy_on_an_xlsx_writes_the_range_files_unpack_always_wrote() {
    for (source, expected) in [
        ("smoke.xlsx", vec!["Sheet1/A1:B3", "Sheet2/A1"]),
        ("literals.xlsx", vec!["Data/A1:D3"]),
    ] {
        let dest = temp_dest(source);
        let report = import_file_as(&fixture(source), &dest, false, Decomposition::Occupancy)
            .expect("the import completes");
        assert_eq!(report.decomposition, Decomposition::Occupancy);
        let all = names_in(&dest);
        // These are root-relative paths; `is_reserved_entry` matches a bare ENTRY name.
        let written: Vec<String> = all
            .iter()
            .filter(|path| {
                let entry = path.rsplit('/').next().unwrap_or(path);
                !fsa1_model::Workbook::is_reserved_entry(entry)
            })
            .cloned()
            .collect();
        assert_eq!(written, expected, "{source} under occupancy");
        assert_eq!(report.files, expected.len());
        assert!(
            all.contains(&".gitattributes".to_string()),
            "unpack pins the tree to LF so a Windows checkout cannot CRLF-mangle a grid: {all:?}",
        );
        std::fs::remove_dir_all(&dest).ok();
    }
}
