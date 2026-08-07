// Concern: freezes the document's output contract | Non-concern: the CLI dispatch (fsa1-cli/tests owns it), the ASCII table | IO: (range files) -> assertions

use fsa1_model::{RenderMode, ViewScope, Workbook, view};

use crate::document;

fn doc(files: &[(&str, &str)]) -> String {
    let wb = Workbook::from_tabs(&[("Sheet1", files)]).expect("loads clean");
    let v = view(&wb, ViewScope::Workbook, RenderMode::Values).expect("a view");
    document(&wb, &v)
}

/// Cell text is author-controlled, and this document is the boundary it crosses into markup.
#[test]
fn a_cell_spelling_markup_renders_as_visible_text() {
    let html = doc(&[("A1", "<script>alert(1)</script>")]);
    assert!(
        html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
        "the cell must arrive escaped:\n{html}"
    );
    assert!(
        !html.contains("<script"),
        "no <script> may reach the document:\n{html}"
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
fn an_axis_size_reaches_the_documents_css() {
    let html = doc(&[
        ("A1:B2", "1\t2\n3\t4"),
        ("A1:B2.css", "  td { height: 22.5pt; width: 14.5ch }\n"),
    ]);
    assert!(
        html.contains(".c0 { height: 22.5pt; width: 14.5ch }"),
        "the author's own sizes must reach the stylesheet:\n{html}"
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
        html.matches("<td class=\"c0\">").count(),
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
