// Concern: asserts what a completed import carries into the tree and what it names as lost | Non-concern: the refusal contract, the warning Display | IO: (fixtures) -> ImportReport + the tree

use std::path::{Path, PathBuf};

use fsa1_ast::Value;
use fsa1_ingest::{
    Decomposition, ImportReport, UnpackCategory, UnpackWarning, import_file, import_file_as,
};
use fsa1_model::{RenderMode, parse_viewport, render};

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
        "fsa1-ingest-fidelity-{tag}-{}-{nanos}",
        std::process::id()
    ))
}

/// The on-disk output is asserted elsewhere, so the dest is cleaned up and only the report returned.
fn report_of(name: &str, strict: bool, tag: &str) -> ImportReport {
    let dest = temp_dest(tag);
    let report = import_file(&fixture(name), &dest, strict).expect("import should complete");
    std::fs::remove_dir_all(&dest).ok();
    report
}

#[test]
fn a_lossy_xlsx_coerces_every_non_general_cell() {
    // literals.xlsx: D2/D3 carry a datetime numFmt; every other cell is General.
    let report = report_of("literals.xlsx", false, "coerce");
    let coerced: Vec<&UnpackWarning> = report
        .warnings
        .iter()
        .filter(|w| matches!(w, UnpackWarning::NumberFormatCoerced { .. }))
        .collect();
    assert_eq!(coerced.len(), 2, "D2 + D3 coerced: {:?}", report.warnings);
    assert!(coerced.iter().any(|w| matches!(
        w,
        UnpackWarning::NumberFormatCoerced { sheet, cell, num_fmt_id, .. }
            if sheet == "Data" && cell == "D2" && *num_fmt_id == 164
    )));
}

#[test]
fn a_lossy_xlsx_reports_a_skipped_a1_name_and_a_verbatim_formula() {
    // fidelity.xlsx carries a defined name "A1" and an inline-array formula at B1, and nothing else.
    let report = report_of("fidelity.xlsx", false, "cd");
    assert!(report.warnings.contains(&UnpackWarning::NameSkipped {
        name: "A1".to_string(),
        scope: None,
        reason: "identifier parses as an A1 address".to_string(),
    }));
    assert!(report.warnings.iter().any(|w| matches!(
        w,
        UnpackWarning::FormulaKeptVerbatim { sheet, cell, reason, .. }
            if sheet == "Data" && cell == "B1" && reason.contains("inline array")
    )));
    assert!(
        !report.warnings.iter().any(|w| matches!(
            w,
            UnpackWarning::NumberFormatCoerced { .. } | UnpackWarning::TableDropped { .. }
        )),
        "an all-General, table-less file loses neither"
    );
}

#[test]
fn a_clean_strict_pass_reports_nothing() {
    let report = report_of("smoke.xlsx", true, "strict-clean");
    assert!(
        report.warnings.is_empty(),
        "a clean strict pass has no warnings: {:?}",
        report.warnings
    );
}

#[test]
fn a_strict_pass_still_reports_the_losses_it_never_policed() {
    // fidelity.xlsx PASSES the pre-flight, which polices neither of the two losses it still incurs.
    let report = report_of("fidelity.xlsx", true, "strict-cd");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| matches!(w, UnpackWarning::NameSkipped { .. })),
        "strict still reports a skipped name: {:?}",
        report.warnings
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| matches!(w, UnpackWarning::FormulaKeptVerbatim { .. })),
        "strict still reports a verbatim formula: {:?}",
        report.warnings
    );
    assert!(
        !report.warnings.iter().any(|w| matches!(
            w,
            UnpackWarning::NumberFormatCoerced { .. } | UnpackWarning::TableDropped { .. }
        )),
        "both are empty by construction on a strict xlsx pass"
    );
}

/// The .ods path reads values and formulas and NOTHING else — deliberately, and unchanged here. What
/// this pins is the other half of that fact: the run declares the categories it never opened, so an
/// empty warning list in one of them is not evidence of anything. Before, this fixture's whole
/// appearance and its column width left in silence under a line crediting both.
#[test]
fn a_styled_ods_loses_its_look_and_the_report_vouches_for_no_category_it_never_read() {
    let dest = temp_dest("ods-styled");
    let report = import_file(&fixture("styled.ods"), &dest, false).expect("import completes");
    let content = std::fs::read_to_string(
        dest.join("Styled")
            .join(fsa1_model::range_file_name("A1:A2")),
    )
    .expect("the block");
    assert_eq!(content, "Total\n42", "the values cross, the look does not");
    assert!(
        !content.contains("@scope"),
        "the .ods path reads no styling at all:\n{content}"
    );
    assert_eq!(
        report.inspected,
        vec![UnpackCategory::Formula],
        "an .ods run may vouch for the formula translation and nothing else"
    );
    assert!(
        report.warnings.is_empty(),
        "no reader on this path can raise one: {:?}",
        report.warnings
    );
    std::fs::remove_dir_all(&dest).ok();
}

/// Every category, because every one of them is read out of the package parts.
#[test]
fn an_xlsx_run_inspects_every_category_the_report_tracks() {
    let report = report_of("smoke.xlsx", false, "inspected-xlsx");
    assert_eq!(report.inspected, UnpackCategory::ALL.to_vec());
    assert_eq!(
        report_of("smoke.xlsx", true, "inspected-xlsx-strict").inspected,
        UnpackCategory::ALL.to_vec(),
        "--strict adds a pre-flight; it narrows nothing"
    );
}

/// The reproduction the census used to miss whole: `--strict` refuses this package and names the chart
/// part (that leg is `import_strict.rs`'s), while a plain `unpack` wrote the tree and vouched for
/// "workbook parts". Both now read `classify_part`, so a part one refuses is a part the other reports.
#[test]
fn a_lossy_import_names_every_package_part_strict_would_refuse() {
    let placeholder = |_: String| "<placeholder/>".to_string();
    let src = patched_parts(
        "smoke.xlsx",
        "tail-parts",
        &[
            ("xl/charts/chart1.xml", &placeholder),
            ("xl/drawings/drawing1.xml", &placeholder),
            ("xl/comments/comment1.xml", &placeholder),
        ],
    );
    let dest = temp_dest("tail-parts-out");
    let report = import_file(&src, &dest, false).expect("a lossy import still completes");
    let named: Vec<String> = report
        .warnings
        .iter()
        .filter(|w| matches!(w, UnpackWarning::WorkbookPartNotCarried { .. }))
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        named,
        vec![
            "xl/charts/chart1.xml not carried",
            "xl/comments/comment1.xml not carried",
            "xl/drawings/drawing1.xml not carried",
        ],
    );
    std::fs::remove_dir_all(&dest).ok();
    std::fs::remove_file(&src).ok();
}

#[test]
fn an_ods_import_only_reports_verbatim_formulas() {
    // The ODS reader reads no number-format, table or name metadata.
    let report = report_of("fidelity.ods", false, "ods");
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert!(matches!(
        &report.warnings[0],
        UnpackWarning::FormulaKeptVerbatim { sheet, cell, .. } if sheet == "Calc" && cell == "A1"
    ));

    let strict = report_of("fidelity.ods", true, "ods-strict");
    assert_eq!(
        strict.warnings, report.warnings,
        "--strict is a no-op for .ods"
    );
}

#[test]
fn the_ods_verbatim_report_source_equals_the_on_disk_cell_content() {
    let dest = temp_dest("ods-disk");
    let report = import_file(&fixture("fidelity.ods"), &dest, false).expect("import completes");
    let UnpackWarning::FormulaKeptVerbatim { source, .. } = &report.warnings[0] else {
        panic!("expected a FormulaKeptVerbatim: {:?}", report.warnings);
    };
    // Calc's occupancy is one block, so A1 is the first FIELD of the range file that covers it.
    let on_disk =
        std::fs::read_to_string(dest.join("Calc").join(fsa1_model::range_file_name("A1:B1")))
            .expect("the block holding A1");
    let a1 = on_disk
        .split(['\t', '\n'])
        .next()
        .expect("A1 is the block's first field");
    assert_eq!(
        format!("={source}"),
        a1,
        "the report's `=<source>` must equal the on-disk cell content"
    );
    assert_eq!(a1, "=SUM({1;2;3})", "the `of:=` lead is stripped");
    assert_eq!(source, "SUM({1;2;3})");
    std::fs::remove_dir_all(&dest).ok();
}

/// The write leg over a real styled package: the appearance that CONVERTS rides in the tab's own
/// stylesheet, the workbook still loads clean, and every attribute that did not convert is named.
#[test]
fn a_styled_xlsx_carries_its_appearance_and_names_every_loss() {
    let dest = temp_dest("visuals");
    let report = import_file(&fixture("visuals.xlsx"), &dest, false).expect("import completes");
    assert_eq!(report.files, 1, "one block over the sheet's occupancy");
    assert_eq!(written(&dest, "Visual"), vec!["A1:B4".to_string()]);

    let content = stylesheet(&dest, "Visual").expect("the tab states presentation");
    assert!(
        content.starts_with("@scope (A1:B4) {"),
        "one root, the sheet's used region: {content}"
    );
    for declaration in [
        "background-color: #ffc000",
        "border-top: 1px solid #ff0000",
        "color: #95b3d7",
        "font-family: Times New Roman",
        "font-size: 14pt",
        "font-style: italic",
        "font-weight: bold",
        "text-align: center",
        "text-decoration: underline",
        "vertical-align: top",
        "white-space: normal",
        "height: 22.5pt",
        "background-color: #00b0f0",
    ] {
        assert!(content.contains(declaration), "{declaration}:\n{content}");
    }
    let wb = fsa1_model::Workbook::load_dir(&dest)
        .expect("filesystem read ok")
        .expect("the styled workbook must load clean");
    assert!(wb.lint().is_empty(), "{:?}", wb.lint());

    // The order warnings are RAISED in; the report groups them by category before printing either way.
    let named: Vec<String> = report.warnings.iter().map(ToString::to_string).collect();
    assert_eq!(
        named,
        vec![
            "column width for C on sheet Visual dropped: no range file covers column C",
            "merged region Visual!D1:E1 flattened; its value stays in the top-left cell",
            "indent level 2 at Visual!A1 dropped",
            "strikethrough at Visual!A1 dropped; the cell also carries underline",
        ],
        "column C's width is unowned: the sheet's content stops at column B",
    );
    std::fs::remove_dir_all(&dest).ok();
}

/// A copy of `name` with each named part `edit`ed, every other part copied byte for byte; a part the
/// package does not have is ADDED, its edit reading the empty string. openpyxl writes none of the
/// constructs below, and hand-writing a whole package would make the fixture's own well-formedness
/// the thing under test instead of the reading of it.
fn patched_parts(name: &str, tag: &str, edits: &[(&str, &dyn Fn(String) -> String)]) -> PathBuf {
    let dest = temp_dest(tag).with_extension("xlsx");
    let source = std::fs::File::open(fixture(name)).expect("the fixture opens");
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(source)).expect("a zip archive");
    let mut out = zip::ZipWriter::new(std::fs::File::create(&dest).expect("a temp package"));
    let options = zip::write::SimpleFileOptions::default();
    let mut carried: Vec<&str> = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("a readable entry");
        let entry_name = entry.name().to_string();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).expect("a readable entry");
        if let Some((part, edit)) = edits.iter().find(|(part, _)| *part == entry_name) {
            bytes = edit(String::from_utf8(bytes).expect("the part is UTF-8")).into_bytes();
            carried.push(part);
        }
        out.start_file(entry_name, options)
            .expect("a writable entry");
        std::io::Write::write_all(&mut out, &bytes).expect("a writable entry");
    }
    for (part, edit) in edits.iter().filter(|(part, _)| !carried.contains(part)) {
        out.start_file(*part, options).expect("a writable entry");
        std::io::Write::write_all(&mut out, edit(String::new()).as_bytes())
            .expect("a writable entry");
    }
    out.finish().expect("the package closes");
    dest
}

fn patched_sheet1(name: &str, tag: &str, edit: impl Fn(String) -> String) -> PathBuf {
    patched_parts(name, tag, &[("xl/worksheets/sheet1.xml", &edit)])
}

/// Both axes, since both are the same fact: a size for an axis no range file covers.
fn unowned_axis_warnings(report: &ImportReport) -> Vec<String> {
    report
        .warnings
        .iter()
        .filter(|w| {
            matches!(
                w,
                UnpackWarning::ColumnWidthUnowned { .. } | UnpackWarning::RowHeightUnowned { .. }
            )
        })
        .map(ToString::to_string)
        .collect()
}

/// `<col min="1" max="16384">` is schema-legal on a sheet holding two cells, and expanding it per
/// column made one authored fact cost 16,382 stderr lines. The width is carried only where the
/// sheet's content reaches, and the rest is the one statement it was written as.
#[test]
fn a_col_run_past_the_sheets_content_costs_one_line_not_one_per_column() {
    let src = patched_sheet1("smoke.xlsx", "blanket-col", |xml| {
        xml.replace(
            "<sheetData>",
            r#"<cols><col min="1" max="16384" width="12.5" customWidth="1"/></cols><sheetData>"#,
        )
    });
    let dest = temp_dest("blanket-col-out");
    let report = import_file(&src, &dest, false).expect("import completes");
    assert_eq!(
        unowned_axis_warnings(&report),
        vec![
            "column width for C:XFD on sheet Sheet1 dropped: no range file covers column C:XFD"
                .to_string()
        ],
        "Sheet1 reaches column B; A and B carry the width, and the tail is ONE loss",
    );
    std::fs::remove_dir_all(&dest).ok();
    std::fs::remove_file(&src).ok();
}

/// The SAME rule on the other axis, which the row leg once had no counterpart to. Excel writes a
/// height applied to a row RANGE as one `<row ht>` per row of it, and a sheet addresses 1,048,576 of
/// them — so taking them verbatim made one authored fact cost one stderr line per row. Both axes read
/// one clipping-and-folding rule, so neither can lose it while the other keeps it.
#[test]
fn a_row_run_past_the_sheets_content_costs_one_line_not_one_per_row() {
    let src = patched_sheet1("smoke.xlsx", "blanket-row", |xml| {
        let rows: String = (5..=1004)
            .map(|r| format!(r#"<row r="{r}" ht="18" customHeight="1"/>"#))
            .collect();
        xml.replace("</sheetData>", &format!("{rows}</sheetData>"))
    });
    let dest = temp_dest("blanket-row-out");
    let report = import_file(&src, &dest, false).expect("import completes");
    assert_eq!(
        unowned_axis_warnings(&report),
        vec![
            "row height for 5:1004 on sheet Sheet1 dropped: no range file covers row 5:1004"
                .to_string()
        ],
        "Sheet1's content stops short of row 5, so the whole run is ONE loss",
    );
    std::fs::remove_dir_all(&dest).ok();
    std::fs::remove_file(&src).ok();
}

/// The `<cols>` bound. `max` may legally be the last addressable column, and nothing bounds how many
/// runs a `<cols>` states: 20,000 of them expanded to 327 million per-column entries, which asked for
/// 4 GB and died on SIGABRT — exit 134, a code `unpack --help` does not list, and not a refusal. An
/// imported .xlsx is a defended boundary, so this ends as a completed unpack or it does not end.
#[test]
fn a_cols_block_restating_the_whole_axis_completes_rather_than_aborting() {
    let blanket = |xml: String| {
        let runs = r#"<col min="1" max="16384" width="12.5" customWidth="1"/>"#.repeat(20_000);
        xml.replace("<sheetData>", &format!("<cols>{runs}</cols><sheetData>"))
    };
    let src = patched_sheet1("smoke.xlsx", "col-bomb", blanket);
    let dest = temp_dest("col-bomb-out");
    let report = import_file(&src, &dest, false).expect("the unpack completes");
    assert_eq!(
        unowned_axis_warnings(&report),
        vec![
            "column width for C:XFD on sheet Sheet1 dropped: no range file covers column C:XFD"
                .to_string()
        ],
        "20,000 restatements of one run are one authored fact, and one loss",
    );
    std::fs::remove_dir_all(&dest).ok();
    std::fs::remove_file(&src).ok();

    // The sheet's OWN extent is now the whole axis, where clipping alone still costs it once per run.
    let wide = patched_sheet1("smoke.xlsx", "col-bomb-wide", |xml| {
        blanket(xml).replace(
            "</sheetData>",
            r#"<row r="9"><c r="XFD9"><v>1</v></c></row></sheetData>"#,
        )
    });
    let wide_dest = temp_dest("col-bomb-wide-out");
    let wide_report = import_file(&wide, &wide_dest, false).expect("the unpack completes");
    assert!(
        unowned_axis_warnings(&wide_report).is_empty(),
        "the sheet's own content reaches XFD, so the root spans every restated column: {:?}",
        unowned_axis_warnings(&wide_report),
    );
    std::fs::remove_dir_all(&wide_dest).ok();
    std::fs::remove_file(&wide).ok();
}

/// The producer of that warning that no longer fires. An axis INSIDE the sheet's extent is inside the
/// root by construction, so a height over rows 5..=2000 with content at row 3000 is CARRIED rather
/// than named as a loss — the retired half of the pass that once printed 1,996 lines for one fact.
#[test]
fn rows_inside_the_extent_that_no_block_covers_are_carried_by_the_root() {
    let src = patched_sheet1("smoke.xlsx", "orphan-heights", |xml| {
        let heights: String = (5..=2000)
            .map(|r| format!(r#"<row r="{r}" ht="30" customHeight="1"/>"#))
            .collect();
        xml.replace(
            "</sheetData>",
            &format!(r#"{heights}<row r="3000"><c r="A3000"><v>1</v></c></row></sheetData>"#),
        )
    });
    let dest = temp_dest("orphan-heights-out");
    let report = import_file(&src, &dest, false).expect("the unpack completes");
    assert!(
        unowned_axis_warnings(&report).is_empty(),
        "the rows lie inside the root, so none of them is unowned: {:?}",
        report.warnings
    );
    let css = stylesheet(&dest, "Sheet1").expect("the tab states its heights");
    assert!(css.starts_with("@scope (A1:B3000) {"), "{}", &css[..40]);
    assert_eq!(
        css.matches("height: 30pt").count(),
        1996,
        "one rule per authored row, and no row it was never authored on",
    );
    std::fs::remove_dir_all(&dest).ok();
    std::fs::remove_file(&src).ok();
}

/// smoke.xlsx with two entries appended to its style table: `cellXfs` 1 is bold, `cellXfs` 2 is a
/// solid blue fill. The `count` attributes are Excel's own cached claim and are left alone — nothing
/// here reads one — so the patch is the two elements and nothing else.
fn with_two_styles(tag: &str, sheet: &dyn Fn(String) -> String) -> PathBuf {
    let styles = |xml: String| {
        xml.replace(
            "</fonts>",
            r#"<font><b val="1"/><sz val="11"/><name val="Calibri"/></font></fonts>"#,
        )
        .replace(
            "</fills>",
            r#"<fill><patternFill patternType="solid"><fgColor rgb="FF00B0F0"/></patternFill></fill></fills>"#,
        )
        .replace(
            "</cellXfs>",
            concat!(
                r#"<xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/>"#,
                r#"<xf numFmtId="0" fontId="0" fillId="2" borderId="0" xfId="0" applyFill="1"/>"#,
                "</cellXfs>",
            ),
        )
    };
    patched_parts(
        "smoke.xlsx",
        tag,
        &[
            ("xl/styles.xml", &styles),
            ("xl/worksheets/sheet1.xml", sheet),
        ],
    )
}

/// smoke.xlsx's column B given a value in every row. What an axis statement does to the BLANKS it
/// also dresses is the other half of the story, and has its own test below.
fn column_b_valued(xml: String) -> String {
    xml.replace(
        r#"<c r="A2" t="n"><v>20</v></c>"#,
        r#"<c r="A2" t="n"><v>20</v></c><c r="B2" t="n"><v>5</v></c>"#,
    )
    .replace(
        r#"<c r="A3"><f>SUM(A1:A2)</f><v /></c>"#,
        r#"<c r="A3"><f>SUM(A1:A2)</f><v /></c><c r="B3" t="n"><v>6</v></c>"#,
    )
}

/// A name as the canonical `:` spelling, whatever separator this host wrote. The reader takes both,
/// so a test that compared raw names would be asserting against the host rather than the format.
fn canonical(name: &str) -> String {
    fsa1_model::canonical_range_name(name)
}

/// The RANGE files one tab was written, by name and in order — the blocks the cut produced. The
/// tab's stylesheet is presentation rather than a block, and [`stylesheet`] is what reads it.
fn written(dest: &Path, tab: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dest.join(tab))
        .expect("the tab is written")
        .map(|e| {
            e.expect("a readable entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name != fsa1_model::PRESENTATION_ENTRY)
        .map(|name| canonical(&name))
        .collect();
    names.sort();
    names
}

/// The tab's whole stylesheet. A tab that states no presentation writes none, so its absence is a
/// fact a caller may assert rather than an empty string it has to tell apart from one.
fn stylesheet(dest: &Path, tab: &str) -> Option<String> {
    std::fs::read_to_string(dest.join(tab).join(fsa1_model::PRESENTATION_ENTRY)).ok()
}

/// A whole-column and a whole-row format, which neither states on any `<c>`: Excel and openpyxl write
/// them as `<col style>` and `<row s customFormat>`, and reading `s=` off `<c>` alone lost both while
/// the run still printed `nothing lost` over the Styling category it had just lost. They are resolved
/// onto the cells inside the sheet's extent, which is what makes them ordinary column and row rules.
#[test]
fn a_column_and_a_row_default_style_reach_the_cells_that_state_none() {
    let column = with_two_styles("col-default", &|xml: String| {
        column_b_valued(xml).replace(
            "<sheetData>",
            r#"<cols><col min="2" max="2" style="1"/></cols><sheetData>"#,
        )
    });
    let dest = temp_dest("col-default-out");
    let report = import_file(&column, &dest, false).expect("the unpack completes");
    assert_eq!(
        stylesheet(&dest, "Sheet1").as_deref(),
        Some("@scope (A1:B3) {\n  td:last-child { font-weight: bold }\n}\n"),
        "no `<c>` of column B states a style, and the whole column is bold anyway",
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    std::fs::remove_dir_all(&dest).ok();
    std::fs::remove_file(&column).ok();

    let row = with_two_styles("row-default", &|xml: String| {
        column_b_valued(xml).replace(r#"<row r="2">"#, r#"<row r="2" customFormat="1" s="1">"#)
    });
    let row_dest = temp_dest("row-default-out");
    let row_report = import_file(&row, &row_dest, false).expect("the unpack completes");
    assert_eq!(
        stylesheet(&row_dest, "Sheet1").as_deref(),
        Some("@scope (A1:B3) {\n  tr:nth-child(2) td { font-weight: bold }\n}\n"),
        "and neither does any `<c>` of row 2",
    );
    assert!(row_report.warnings.is_empty(), "{:?}", row_report.warnings);
    std::fs::remove_dir_all(&row_dest).ok();
    std::fs::remove_file(&row).ok();
}

/// The occupancy half, and why it asks `paints_blank` rather than a rule of its own. A `<col style>`
/// dresses its column all the way down, so its blanks are the hazard: a bold one shows nothing and
/// must stay bare, or `pack` drops a look the tree declared. A FILLED one shows, so it is content —
/// inside the extent, the only place an unbounded statement resolves; what it draws past it is named.
#[test]
fn an_axis_default_occupies_a_blank_only_where_it_draws_on_one() {
    let bold = with_two_styles("bold-column", &|xml: String| {
        xml.replace(
            "<sheetData>",
            r#"<cols><col min="1" max="16384" style="1"/></cols><sheetData>"#,
        )
    });
    let dest = temp_dest("bold-column-out");
    let report = import_file_as(&bold, &dest, false, Decomposition::Appearance)
        .expect("the unpack completes");
    // Two files rather than one is `appearance`'s row-major cover of a single signature — the cut's shape, not this test's claim.
    assert_eq!(
        written(&dest, "Sheet1"),
        vec!["A1:B1".to_string(), "A2:A3".to_string()],
        "a bold blank is not content, so 16,384 dressed columns are still the sheet's own two",
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    std::fs::remove_dir_all(&dest).ok();
    std::fs::remove_file(&bold).ok();

    let filled = with_two_styles("filled-column", &|xml: String| {
        xml.replace(
            "<sheetData>",
            r#"<cols><col min="2" max="16384" style="2"/></cols><sheetData>"#,
        )
    });
    let filled_dest = temp_dest("filled-column-out");
    let filled_report = import_file(&filled, &filled_dest, false).expect("the unpack completes");
    assert!(
        stylesheet(&filled_dest, "Sheet1")
            .expect("the tab states presentation")
            .contains("background-color: #00b0f0"),
        "the fill Excel paints over column B's blanks crosses with them",
    );
    assert_eq!(
        filled_report
            .warnings
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec![
            "the fill or border column B:XFD states on sheet Sheet1 is dropped past the sheet's \
             extent: no range file covers a cell there"
                .to_string()
        ],
        "the columns and rows the statement reaches past the extent have no file to sit on",
    );
    std::fs::remove_dir_all(&filled_dest).ok();
    std::fs::remove_file(&filled).ok();
}

/// Every one of these lives inside a part `classify_part` calls `Allow`, so nothing upstream refuses
/// the workbook and nothing downstream carries them: without a producer they leave in silence. The
/// rich-text run is the one living outside a worksheet, so the package gains that whole part.
#[test]
fn the_workbook_features_an_unpacked_tree_has_no_place_for_are_named() {
    let sheet1 = |xml: String| {
        xml.replace("<sheetPr>", r#"<sheetPr><tabColor rgb="FFFF0000"/>"#)
            .replace(
                r#"<sheetView workbookViewId="0">"#,
                r#"<sheetView workbookViewId="0"><pane ySplit="1" topLeftCell="A2" state="frozen"/>"#,
            )
            .replace(
                "<sheetData>",
                r#"<cols><col min="3" max="3" hidden="1"/></cols><sheetData>"#,
            )
            .replace(r#"<row r="2""#, r#"<row hidden="1" r="2""#)
            .replace(
                "</sheetData>",
                concat!(
                    r#"</sheetData><autoFilter ref="A1:B3"/>"#,
                    r#"<conditionalFormatting sqref="A1:A3"><cfRule type="cellIs" dxfId="0" priority="1" operator="greaterThan"><formula>5</formula></cfRule></conditionalFormatting>"#,
                    r#"<dataValidations count="1"><dataValidation type="whole" sqref="B1"><formula1>1</formula1></dataValidation></dataValidations>"#,
                    r#"<hyperlinks><hyperlink ref="A1" location="Sheet1!A3" display="go"/></hyperlinks>"#,
                ),
            )
    };
    // One string in two looks. smoke.xlsx is all numbers, so it has no sharedStrings part until now.
    let shared_strings = |_: String| {
        concat!(
            r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="1" uniqueCount="1">"#,
            r#"<si><r><rPr><b/></rPr><t>Ada</t></r><r><t> Lovelace</t></r></si></sst>"#,
        )
        .to_string()
    };
    let content_types = |xml: String| {
        xml.replace(
            "</Types>",
            concat!(
                r#"<Override PartName="/xl/sharedStrings.xml" "#,
                r#"ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>"#,
                "</Types>",
            ),
        )
    };
    let workbook_rels = |xml: String| {
        xml.replace(
            "</Relationships>",
            concat!(
                r#"<Relationship Id="rIdShared" Target="sharedStrings.xml" "#,
                r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings"/>"#,
                "</Relationships>",
            ),
        )
    };
    let src = patched_parts(
        "smoke.xlsx",
        "uncarried",
        &[
            ("xl/worksheets/sheet1.xml", &sheet1),
            ("xl/sharedStrings.xml", &shared_strings),
            ("[Content_Types].xml", &content_types),
            ("xl/_rels/workbook.xml.rels", &workbook_rels),
        ],
    );
    let dest = temp_dest("uncarried-out");
    let report = import_file(&src, &dest, false).expect("import completes");
    let named: Vec<String> = report
        .warnings
        .iter()
        .filter(|w| matches!(w, UnpackWarning::WorkbookPartNotCarried { .. }))
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        named,
        vec![
            "conditional formatting not carried",
            "rich text runs not carried",
            "freeze panes not carried",
            "autofilter not carried",
            "tab colour not carried",
            "data validation not carried",
            "hyperlinks not carried",
            "hidden columns not carried",
            "hidden rows not carried",
        ],
    );
    std::fs::remove_dir_all(&dest).ok();
    std::fs::remove_file(&src).ok();
}

/// Geometry authored on a column `appearance` leaves in no BLOCK. It is carried anyway, because the
/// root presentation is stated over is the sheet's USED REGION and not any block: a column can carry
/// content and no style and still have an authored width, and column C here has neither. Both cuts
/// therefore keep the width, and which one ran stops being visible in the appearance at all.
#[test]
fn a_width_on_a_column_no_block_covers_is_carried_by_the_sheets_root() {
    let mut carried = Vec::new();
    for (tag, policy) in [
        ("gapped-appearance", Decomposition::Appearance),
        ("gapped-occupancy", Decomposition::Occupancy),
    ] {
        let dest = temp_dest(tag);
        let report = import_file_as(&fixture("gapped_columns.xlsx"), &dest, false, policy)
            .expect("import completes");
        assert!(
            unowned_axis_warnings(&report).is_empty(),
            "{tag}: the root spans column C, so nothing is unowned: {:?}",
            report.warnings
        );
        carried.push((
            written(&dest, "Gapped"),
            stylesheet(&dest, "Gapped").expect("the tab states a width"),
        ));
        std::fs::remove_dir_all(&dest).ok();
    }
    assert_eq!(
        carried[0].0,
        vec!["A1:B20".to_string(), "D1:E20".to_string()],
        "columns A,B and D,E are two zero-rule rectangles, and the empty C between them pays nothing",
    );
    assert_eq!(carried[1].0, vec!["A1:E20".to_string()]);
    assert_eq!(
        carried[0].1, carried[1].1,
        "one root over one used region, so the two cuts state the same presentation",
    );
    assert_eq!(
        carried[0].1, "@scope (A1:E20) {\n  td:nth-child(3) { width: 14.5ch }\n}\n",
        "column C is index 3 of the root A1:E20",
    );
}

/// A block spanning coordinates the sheet never occupied — the shape `conformance/presentation/`'s
/// `sparse_blocks_normal_font_arial_9` was added for and, under the default cut, no longer holds. The
/// two ways it has gone wrong are a block fusing with its neighbour and blank fields multiplying, so
/// the file is asserted whole and then read back, one side catching each.
#[test]
fn a_block_spanning_unoccupied_coordinates_spells_them_blank_and_reads_back_unchanged() {
    let dest = temp_dest("interior-blanks");
    import_file(&fixture("blanks_repeats.xlsx"), &dest, false).expect("import completes");
    assert_eq!(written(&dest, "Sparse"), vec!["A1:D3".to_string()]);
    assert_eq!(
        std::fs::read_to_string(
            dest.join("Sparse")
                .join(fsa1_model::range_file_name("A1:D3"))
        )
        .expect("the block file"),
        "1\t\t\t4\n\t\t\t\n7\t\t\t",
        "nine of the twelve coordinates are blank fields, and no row gains or loses one",
    );

    let wb = fsa1_model::Workbook::load_dir(&dest)
        .expect("filesystem read ok")
        .expect("a block full of blanks must load clean");
    assert!(wb.lint().is_empty(), "{:?}", wb.lint());
    assert_eq!(wb.value_at(0, 0, 0), Value::Number(1.0));
    assert_eq!(wb.value_at(0, 3, 0), Value::Number(4.0));
    assert_eq!(wb.value_at(0, 0, 2), Value::Number(7.0));
    assert_eq!(wb.value_at(0, 1, 0), Value::Blank, "B1 is inside the block");
    assert_eq!(wb.value_at(0, 2, 1), Value::Blank, "so is all of row 2");
    let grid = render(
        &wb,
        0,
        parse_viewport("A1:D3").expect("a viewport"),
        RenderMode::Values,
    );
    let cells: Vec<Vec<String>> = grid.rows.iter().map(|r| r.cells.clone()).collect();
    assert_eq!(
        cells,
        vec![
            vec!["1", "", "", "4"],
            vec!["", "", "", ""],
            vec!["7", "", "", ""],
        ],
        "the reader gives back the grid the write leg spelled, blanks included",
    );
    std::fs::remove_dir_all(&dest).ok();
}

/// Every REFERENCE to the table resolves to plain A1 and computes, which is what makes the values
/// faithful — but the table OBJECT is `xl/tables/table1.xml`, a part `--strict` refuses this very file
/// for. The census names it now: the cells cross, the table does not, and neither fact hides the other.
#[test]
fn a_file_with_resolvable_names_and_a_mapped_table_loses_only_the_table_part() {
    let report = report_of("resolution.xlsx", false, "resolution");
    assert_eq!(
        report
            .warnings
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<String>>(),
        vec!["xl/tables/table1.xml not carried"],
    );
    assert!(
        !report
            .warnings
            .iter()
            .any(|w| matches!(w, UnpackWarning::TableDropped { .. })),
        "the table MAPPED, so its refs resolve: {:?}",
        report.warnings
    );
}
