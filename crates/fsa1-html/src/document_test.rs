// Concern: freezes the document's output contract | Non-concern: the CLI dispatch (fsa1-cli/tests owns it), the ASCII table | IO: (range files, sidecars, figures) -> assertions

use fsa1_model::{
    Figure, Overlay, Placement, Rect, RenderMode, ViewScope, Workbook, figure_occupancy, view,
};

use crate::{BoundFigure, document};

/// A tab layer, two DISJOINT blocks, a row only one of them covers, and a single-cell root NESTED in
/// the wider block — every shape one tab may hold, in the cascade order `Overlay::scopes` yields:
/// the layer, then `A1:B2`, then `D1:D3`, then `B2`.
const SHAPES: &[(&str, &str)] = &[
    ("A1:D3", "a\tb\tc\td\ne\tf\tg\th\ni\tj\tk\tl"),
    (".css", "  fsa1-cell { font-family: Georgia }\n"),
    ("A1:B2.css", "  fsa1-cell { background-color: #eef6ff }\n"),
    ("D1:D3.css", "  fsa1-cell { text-align: right }\n"),
    ("B2.css", "  fsa1-cell { color: #d33333 }\n"),
];

fn doc(files: &[(&str, &str)]) -> String {
    scoped(files, ViewScope::Workbook)
}

fn scoped(files: &[(&str, &str)], scope: ViewScope) -> String {
    figured(files, &[], scope)
}

/// `figures` is `(name, spec text)`, expanded and placed here exactly as `fsa1-verbs` does it: a
/// RANGE-form name is its own placement, and a name-form one with no sidecar beside it has none.
fn figured(files: &[(&str, &str)], figures: &[(&str, &str)], scope: ViewScope) -> String {
    let wb = Workbook::from_tabs(&[("Sheet1", files)]).expect("loads clean");
    let overlay = Overlay::from_tabs(&[("Sheet1", files)]).expect("its sidecars load clean");
    let bound: Vec<BoundFigure> = figures
        .iter()
        .map(|(name, text)| {
            let figure = Figure::parse(name, text).expect("the figure parses");
            let spec = figure.expand(&wb, 0).expect("its bindings resolve");
            BoundFigure {
                name: (*name).to_string(),
                spec: spec.to_string(),
                sheet: 0,
                placement: placement_of(name),
            }
        })
        .collect();
    let covers: Vec<(u32, fsa1_model::FigureView)> = bound
        .iter()
        .map(|f| {
            (
                f.sheet,
                fsa1_model::FigureView {
                    name: f.name.clone(),
                    kind: String::new(),
                    binds: Vec::new(),
                    cover: crate::fills(f.placement),
                    range_form: f.placement.is_some(),
                },
            )
        })
        .collect();
    let v = view(&wb, Some(&overlay), scope, RenderMode::Values, &covers).expect("a view");
    document(&wb, &overlay, &v, &bound)
}

fn placement_of(name: &str) -> Option<Placement> {
    let entry = name.rsplit('/').next().unwrap_or(name);
    figure_occupancy(entry).map(Placement::Cells)
}

/// PRES2's first half, and the whole point of the carrier: the exporter WRAPS a sidecar's bytes and
/// does nothing else to them, so each authored text — indent, spacing and trailing newline — is a
/// literal substring of the page. Re-deriving one from the typed rules would fail every line here.
#[test]
fn every_sidecars_bytes_reach_the_document_unchanged() {
    let html = doc(SHAPES);
    for (name, text) in SHAPES.iter().filter(|(name, _)| name.ends_with(".css")) {
        assert!(
            html.contains(text),
            "{name}'s bytes must be verbatim:\n{html}"
        );
    }
}

/// Layer order IS the model's cascade order, and it outranks specificity — so a `<style>`'s position
/// in the document decides nothing and the narrowest root stands. The names count the SHEET INDEX,
/// never the tab name, which is a directory name free to hold a byte that invalidates the statement.
#[test]
fn the_head_states_the_layer_order_the_model_folds_its_scopes_in() {
    let html = doc(SHAPES);
    assert!(
        html.contains("@layer fsa1-harness, fsa1-s0-0, fsa1-s0-1, fsa1-s0-2, fsa1-s0-3;"),
        "the harness sorts first and each scope follows in cascade order:\n{html}"
    );
    for (at, text) in [
        (0, "font-family: Georgia"),
        (1, "background-color: #eef6ff"),
        (2, "text-align: right"),
        (3, "color: #d33333"),
    ] {
        let layer = format!("@layer fsa1-s0-{at} {{ @scope ");
        let carried = html.split(&layer).nth(1).unwrap_or_default();
        assert!(
            carried
                .split("</style>")
                .next()
                .unwrap_or_default()
                .contains(text),
            "layer fsa1-s0-{at} must carry {text:?}:\n{html}"
        );
    }
}

/// A grid item is blockified and `vertical-align` does nothing on a block, so the strut is what keeps
/// one of the sixteen properties `pack` carries paintable. Frozen because losing it is INVISIBLE:
/// every cell still draws, just never where the author aligned it.
#[test]
fn the_harness_keeps_an_authored_vertical_align_paintable() {
    let html = doc(SHAPES);
    assert!(
        html.contains("vertical-align: bottom"),
        "a cell declaring nothing wears the model's own default:\n{html}"
    );
    assert!(
        html.contains(
            "fsa1-cell::before, fsa1-head::before { content: \"\"; display: inline-block; \
             width: 0; height: 100%; vertical-align: inherit }"
        ),
        "the strut fills the cell and takes the cell's computed alignment:\n{html}"
    );
}

/// PRES2's second half at the base: a cell declaring no font paints the face and the size the model
/// resolves for it, read off `fsa1-model`'s own constants rather than a second copy of the numbers.
/// A harness declaring `sans-serif` at `10pt` made the guarantee false for every undeclared cell.
#[test]
fn a_cell_declaring_no_font_paints_the_models_default() {
    let html = doc(SHAPES);
    let (_, rest) = html
        .split_once("fsa1-cell, fsa1-head {")
        .expect("the harness states one base rule for cells");
    let (base, _) = rest.split_once('}').expect("that rule closes");
    for css in [
        format!("font-family: {}", fsa1_model::DEFAULT_FONT_FAMILY),
        format!("font-size: {}", fsa1_model::DEFAULT_FONT_SIZE.spell()),
    ] {
        assert!(
            base.contains(&css),
            "the harness base must state `{css}`:\n{base}"
        );
    }
    assert!(
        !base.contains("sans-serif"),
        "and no font of its own beside it:\n{base}"
    );
}

/// A scope root is an ELEMENT and its scope reaches exactly its MODEL root: a block gets a region
/// whose own `<style>` roots there prelude-less, while a single-cell root keeps its cell where it is
/// and names it in the prelude instead — a 1x1 root holds only `fsa1-cell`, which must reach one cell.
#[test]
fn a_block_root_is_a_region_and_a_single_cell_root_names_its_cell_in_the_prelude() {
    let html = doc(SHAPES);
    for root in ["A1:B2", "D1:D3"] {
        assert!(
            html.contains(&format!("<fsa1-region data-root=\"{root}\">")),
            "{root} is a region:\n{html}"
        );
    }
    assert!(
        !html.contains("data-root=\"B2\""),
        "a single-cell root moves no cell:\n{html}"
    );
    assert!(
        html.contains(
            "@scope (#fsa1-s0 fsa1-row:has(> fsa1-cell[data-ref=\"B2\"])) to \
             (fsa1-cell:not([data-ref=\"B2\"]))"
        ),
        "it roots the ROW and limits the others out:\n{html}"
    );
    assert!(
        html.contains("</fsa1-row></fsa1-rows><style>"),
        "a region's rows close before its own <style>, so nth-child counts rows alone:\n{html}"
    );
}

/// The tab layer's `<style>` is a child of `<fsa1-sheet>`, whose rows span the STATED region — wider
/// than the layer's own root, the content rect, which is the only place `cell_style` applies it.
#[test]
fn the_tab_layer_is_limited_to_the_rect_the_model_applies_it_over() {
    let html = doc(&[
        ("A1", "1"),
        ("C1:C2.css", "  fsa1-cell { font-weight: bold }\n"),
        (".css", "  fsa1-cell { font-family: Georgia }\n"),
    ]);
    assert!(
        html.contains("@scope (#fsa1-s0) to (fsa1-cell[data-outside])"),
        "the layer roots at the sheet and stops at a cell past its rect:\n{html}"
    );
    assert_eq!(
        html.matches(" data-outside").count(),
        5,
        "the layer's root is the CONTENT rect A1, and the block widens the rows past it:\n{html}"
    );
}

/// Each cell is emitted EXACTLY once — in the block region whose root reaches it, else in the sheet's
/// own rows — so a partially covered row is split across the two and neither doubles a coordinate.
#[test]
fn a_partially_covered_row_emits_each_of_its_cells_once() {
    let html = doc(SHAPES);
    assert_eq!(
        html.matches("<fsa1-cell").count(),
        12,
        "A1:D3 is twelve cells however its roots divide it:\n{html}"
    );
    for at in ["A1", "B2", "C3", "D1"] {
        assert_eq!(
            html.matches(&format!("data-ref=\"{at}\"")).count(),
            if at == "B2" { 3 } else { 1 },
            "{at} is emitted once (B2 is also NAMED twice in its own prelude):\n{html}"
        );
    }
    let rows = html.rsplit("</fsa1-region>").next().unwrap_or_default();
    assert!(
        rows.contains("data-ref=\"C1\"") && !rows.contains("data-ref=\"D1\""),
        "the sheet's own rows hold what no block covers, and nothing else:\n{html}"
    );
}

/// Rows span the ROOT and the viewport decides only what is VISIBLE: a cut root still emits every
/// row and cell of itself, the ones outside carrying `hidden` and NOTHING else, so a region-relative
/// `nth-child` cannot move with a viewport.
#[test]
fn a_viewport_that_cuts_a_root_hides_cells_rather_than_dropping_them() {
    let region = Rect {
        min_col: 0,
        min_row: 0,
        max_col: 1,
        max_row: 2,
    };
    let html = scoped(SHAPES, ViewScope::Region(0, region));
    assert_eq!(
        html.matches("<fsa1-cell").count(),
        12,
        "the stated region is A1:D3 whatever the viewport spans:\n{html}"
    );
    assert_eq!(
        html.matches("<fsa1-cell hidden></fsa1-cell>").count(),
        6,
        "column C and column D are outside it, and carry nothing but `hidden`:\n{html}"
    );
    assert!(
        !html.contains("data-ref=\"C1\""),
        "a hidden cell states no coordinate:\n{html}"
    );
}

/// A size belongs to the AXIS, which is where an authored one BINDS: the sheet states one track per
/// viewport column behind the label track, and the cell filling it is `width: 100%` inline —
/// unbeatable, so a `width` that reached a cell could never overrule the run the model resolved.
#[test]
fn an_axis_size_states_the_grid_track_and_never_the_cell() {
    let html = doc(&[
        ("A1:B2", "1\t2\n3\t4"),
        (
            "A1:B2.css",
            "  fsa1-row:last-child fsa1-cell { height: 22.5pt }\n  fsa1-cell:last-child { width: 14.5ch }\n",
        ),
    ]);
    assert!(
        html.contains("grid-template-columns:auto auto 14.5ch")
            && html.contains("grid-template-rows:auto auto 22.5pt"),
        "each run states its own track, the leading one standing for the labels:\n{html}"
    );
    assert_eq!(
        html.matches("width:100%;height:100%").count(),
        4 + 5,
        "every cell and head fills the track it sits in:\n{html}"
    );
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

/// The two carriers must not disagree about a cell: the ASCII table preserves an embedded newline
/// and padding, so the HTML one may not let a browser collapse them.
#[test]
fn an_embedded_newline_and_padding_survive_the_html_carrier() {
    let html = doc(&[("A1", "line one\\nline two")]);
    assert!(
        html.contains("white-space: pre;"),
        "cells preserve their own whitespace, and `pre` is the model's `nowrap`:\n{html}"
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
        ViewScope::Workbook,
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

/// `MAX_VIEWPORT_CELLS` is "a refusal, never a crash", and `view` honours it by declining to widen
/// for a cover that would breach it. The grid honours the SAME bound: a raw union of the cover it
/// refused emits one `<fsa1-cell>` per coordinate, which IS the crash.
#[test]
fn a_cover_the_viewport_refused_does_not_widen_the_grid() {
    let html = figured(
        &[("A1:B2", "x\ty\n3\t4")],
        &[(
            // 26 columns x 40_000 rows is 1.04M cells, past the 1M bound.
            "Sheet1/A1:Z40000.json",
            r#"{"width":360,"height":190,"data":{"name":"A1:B2"},"mark":"bar"}"#,
        )],
        ViewScope::Workbook,
    );
    let cells = html.matches("<fsa1-cell").count();
    assert!(
        cells <= 16,
        "the sheet holds four cells; the refused cover must not widen it, but {cells} were emitted"
    );
}

/// A `<script>` is a raw-text element, so a cell spelling `</script>` inside a bound spec would end
/// it and turn the rest of the document into markup.
#[test]
fn a_cell_cannot_close_the_spec_script_it_rides_in() {
    let html = figured(
        &[("A1:A2", "h\n</script><img src=x>")],
        &[("Sheet1/f.json", r#"{"data":{"name":"A1:A2"},"mark":"bar"}"#)],
        ViewScope::Workbook,
    );
    assert!(
        !html.contains("<img src=x>"),
        "the cell may never become markup:\n{html}"
    );
    assert!(html.contains(r"\u003c/script>"), "escaped instead:\n{html}");
}

/// `format-spec.md`: "the filename is the placement and the size, and the figure fills exactly the
/// cells it names". So the sheet's grid reaches those cells even where no file holds one, the figure
/// is a grid item over them rather than a block after them, and its own declared size is replaced by
/// the container the cells resolve to — a figure at 360px would overflow the four columns it fills.
#[test]
fn a_range_form_figure_fills_the_cells_its_name_states() {
    let html = figured(
        &[("A1:B2", "x\ty\n3\t4")],
        &[(
            "Sheet1/D2:E3.json",
            r#"{"width":360,"height":190,"data":{"name":"A1:B2"},"mark":"bar"}"#,
        )],
        ViewScope::Workbook,
    );
    let (sheet, tail) = html
        .split_once("</fsa1-sheet>")
        .expect("the sheet closes once");
    assert!(
        sheet.contains("<figure class=\"fsa1-fig\"") && !tail.contains("<figure"),
        "the figure is drawn IN the sheet, not after it:\n{html}"
    );
    assert!(
        sheet.contains("grid-row:3/5;grid-column:5/7"),
        "D2:E3, offset by the label row and the gutter column:\n{html}"
    );
    assert!(
        sheet.contains(r#""width":"container","height":"container""#)
            && sheet.contains(r#""autosize":{"type":"fit","contains":"padding"}"#),
        "the cells are its box, so the spec's own size is replaced:\n{html}"
    );
    assert!(
        html.contains("grid-template-columns:auto auto auto auto auto auto")
            && sheet.contains("data-ref=\"E1\""),
        "the grid reaches E, and the cells the figure covers are still addressable:\n{html}"
    );
}

/// The NAME form with no sidecar beside it is "placed by the writer" (`format-spec.md`), so it
/// states no cells to fill and nothing decides its size but its own spec. Only a figure that says
/// where it sits is placed.
#[test]
fn a_figure_stating_no_placement_still_follows_the_sheet_at_its_own_size() {
    let html = figured(
        &[("A1:B2", "x\ty\n3\t4")],
        &[(
            "Sheet1/floats.json",
            r#"{"width":360,"height":190,"data":{"name":"A1:B2"},"mark":"bar"}"#,
        )],
        ViewScope::Workbook,
    );
    let (sheet, tail) = html
        .split_once("</fsa1-sheet>")
        .expect("the sheet closes once");
    assert!(
        !sheet.contains("<figure") && tail.contains("<figure class=\"fsa1-fig\">"),
        "an unplaced figure follows the sheet, and carries no grid area:\n{html}"
    );
    assert!(
        tail.contains(r#""width":360,"height":190"#) && !html.contains(r#""width":"container""#),
        "at the size its own spec asked for:\n{html}"
    );
    assert!(
        html.contains("grid-template-columns:auto auto auto;"),
        "and widens no sheet: exactly the gutter and A-B, so the `;` must follow:\n{html}"
    );
}
