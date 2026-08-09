// Concern: freezes the document's output contract | Non-concern: the CLI dispatch (fsa1-cli/tests owns it), the ASCII table | IO: (range files, sidecars, figures) -> assertions

use fsa1_model::{Figure, Overlay, RenderMode, ViewScope, Workbook, view};

use crate::document;

fn doc(files: &[(&str, &str)]) -> String {
    figured(files, &[])
}

/// `figures` is `(name, spec text)`, expanded here exactly as `fsa1-verbs` expands it.
fn figured(files: &[(&str, &str)], figures: &[(&str, &str)]) -> String {
    let wb = Workbook::from_tabs(&[("Sheet1", files)]).expect("loads clean");
    let overlay = Overlay::from_tabs(&[("Sheet1", files)]).expect("its sidecars load clean");
    let v = view(
        &wb,
        Some(&overlay),
        ViewScope::Workbook,
        RenderMode::Values,
        &[],
    )
    .expect("a view");
    let bound: Vec<(String, String)> = figures
        .iter()
        .map(|(name, text)| {
            let figure = Figure::parse(name, text).expect("the figure parses");
            let spec = figure.expand(&wb, 0).expect("its bindings resolve");
            ((*name).to_string(), spec.to_string())
        })
        .collect();
    document(&wb, &overlay, &v, &bound)
}

/// Cell text is author-controlled, and this document is the boundary it crosses into markup.
#[test]
fn a_cell_spelling_markup_renders_as_visible_text() {
    let html = doc(&[("A1", "<script>alert(1)</script>")]);
    assert!(
        html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
        "the cell must arrive escaped:\n{html}"
    );
    // The document ships ONE script, its own; a cell's text may never become a second.
    assert_eq!(
        html.matches("<script").count(),
        1,
        "only the formula bar's own script may appear:\n{html}"
    );
}

/// `font-family` is the one free-text style value. Each of these ends the raw-text `<style>` element
/// or opens a construct consuming to end-of-stylesheet, discarding a LATER cell's rule in silence.
/// The allow-list is what makes this a closed set rather than the three bytes someone thought of.
#[test]
fn no_font_family_byte_can_swallow_a_later_cells_rule() {
    for hostile in [
        "</style>x",   // ends the raw-text element
        "Arial/*evil", // opens a comment, consumes to EOF
        "Arial[evil",  // unmatched bracket, consumes to `]` or EOF
        "a\\65 vil",   // an escape that could re-form one of the above
    ] {
        let html = doc(&[
            ("A1", "x"),
            ("A2", "y"),
            ("A1.css", &format!("  td {{ font-family: {hostile} }}\n")),
            ("A2.css", "  td { background-color: #ff0000 }\n"),
        ]);
        assert_eq!(
            html.matches("</style>").count(),
            1,
            "{hostile:?}: only the document's own </style> may appear:\n{html}"
        );
        assert!(
            html.contains("background-color: #ff0000"),
            "{hostile:?}: the later cell's rule must survive:\n{html}"
        );
    }
}

/// The two carriers must not disagree about a cell: the ASCII table preserves an embedded newline
/// and padding, so the HTML one may not let a browser collapse them.
#[test]
fn an_embedded_newline_and_padding_survive_the_html_carrier() {
    let html = doc(&[("A1", "line one\\nline two")]);
    assert!(
        html.contains("white-space: pre-wrap"),
        "cells preserve their own whitespace:\n{html}"
    );
    assert!(
        html.contains("line one\nline two"),
        "the newline reaches the cell verbatim:\n{html}"
    );
    let padded = doc(&[("A1", "  spaced  ")]);
    assert!(
        padded.contains(">  spaced  <"),
        "leading and trailing spaces reach the cell verbatim:\n{padded}"
    );
}

/// An axis size is CSS the browser already understands, so it crosses into the document as written
/// rather than being dropped for want of a table-level carrier.
#[test]
fn an_axis_size_reaches_the_document_where_the_browser_honours_it() {
    let html = doc(&[
        ("A1:B2", "1\t2\n3\t4"),
        ("A1:B2.css", "  td { height: 22.5pt; width: 14.5ch }\n"),
    ]);
    assert!(
        html.contains(".c0 { height: 22.5pt }"),
        "a height rides the cell's own class:\n{html}"
    );
    assert!(
        html.contains("table-layout: fixed") && html.contains(r#"<col style="width: 14.5ch">"#),
        "a WIDTH rides the <colgroup>, which is the only place it binds:\n{html}"
    );
    assert!(
        !html.contains(".c0 { height: 22.5pt; width: 14.5ch }"),
        "and never the cell, where it does nothing:\n{html}"
    );
}

/// The output contract: the stylesheet is one rule per DISTINCT style, not one per cell.
#[test]
fn two_cells_with_the_same_declarations_share_one_class_and_one_rule() {
    let html = doc(&[
        ("A1:B1", "1\t2"),
        ("A1:B1.css", "  td { font-weight: bold }\n"),
    ]);
    assert_eq!(
        html.matches("<td class=\"c0\"").count(),
        2,
        "both cells wear the same class:\n{html}"
    );
    assert_eq!(
        html.matches(".c0 { font-weight: bold }").count(),
        1,
        "one rule, spelled as the author wrote it:\n{html}"
    );
    assert!(!html.contains(".c1"), "no second class:\n{html}");
}

/// `table-layout: fixed` stops an unwidened column sizing to its content, so a document reaching no
/// authored width must not carry it — the layout would move while every byte of text stayed put.
#[test]
fn a_document_stating_no_width_keeps_the_browsers_own_layout() {
    let styled = doc(&[
        ("A1:B2", "1\t2\n3\t4"),
        ("A1:B2.css", "  td { font-weight: bold }\n"),
    ]);
    assert!(
        !styled.contains("table-layout"),
        "styled but unwidened is still auto:\n{styled}"
    );
    let widened = doc(&[
        ("A1:B2", "1\t2\n3\t4"),
        ("A:A.css", "  td { width: 30ch }\n"),
    ]);
    assert!(
        widened.contains("table-layout: fixed") && widened.contains(r#"<col style="width: 30ch">"#),
        "one authored width turns it on:\n{widened}"
    );
}

/// A workbook stating no figure pays for none: the runtime is a megabyte, and a document that never
/// draws one must not carry a byte of it, nor a `<figure>`, nor a second script.
#[test]
fn a_document_with_no_figure_carries_no_runtime_and_no_figure() {
    let html = doc(&[("A1:B2", "1\t2\n3\t4")]);
    assert!(!html.contains("<figure"), "no figure element:\n{html}");
    assert!(!html.contains("vegaLite"), "no runtime");
    assert_eq!(
        html.matches("<script").count(),
        1,
        "only the formula bar's own script:\n{html}"
    );
}

/// The export is ONE self-contained file. A CDN `src=` would make a saved page stop drawing the day
/// the network is gone, so the runtime is inlined, once, however many figures the document holds.
#[test]
fn a_figured_document_carries_the_runtime_once_and_fetches_nothing() {
    let spec = r#"{"data":{"name":"A1:B2"},"mark":"bar"}"#;
    let html = figured(
        &[("A1:B2", "x\ty\n3\t4")],
        &[("Sheet1/one.json", spec), ("Sheet1/two.json", spec)],
    );
    assert_eq!(html.matches("<figure").count(), 2, "one element per figure");
    assert_eq!(
        html.matches("vegaLite.compile").count(),
        1,
        "one mounting script, however many figures:\n{}",
        &html[..400]
    );
    for fetch in [
        "<script src=",
        "<link ",
        "@import",
        "src=\"http",
        "href=\"http",
    ] {
        assert!(!html.contains(fetch), "{fetch:?} would leave the file");
    }
    assert!(
        html.contains(r#""datasets":{"A1:B2":[{"x":3,"y":4}]}"#),
        "the spec arrives BOUND:\n{html}"
    );
}

/// A `<script>` is a raw-text element, so a cell spelling `</script>` inside a bound spec would end
/// it and turn the rest of the document into markup.
#[test]
fn a_cell_cannot_close_the_spec_script_it_rides_in() {
    let html = figured(
        &[("A1:A2", "h\n</script><img src=x>")],
        &[("Sheet1/f.json", r#"{"data":{"name":"A1:A2"},"mark":"bar"}"#)],
    );
    assert!(
        !html.contains("<img src=x>"),
        "the cell may never become markup:\n{html}"
    );
    assert!(html.contains(r"\u003c/script>"), "escaped instead:\n{html}");
}
