// Concern: locks the block and mark-fallback render --format ascii draws over a range-named figure | Non-concern: a name-form figure's marks | IO: spawns the binary -> stdout + exit status

mod common;

use common::{Fixture, run_err};
use unicode_width::UnicodeWidthStr;

/// A figure whose NAME is its range, over a three-line table it binds. The rectangle is the figure's
/// own occupancy, so a cell file inside it is an overlap the load refuses — which is why the block
/// below can be erased without hiding anything.
fn ranged(tag: &str, range: &str) -> Fixture {
    let fx = Fixture::new(tag);
    fx.file("Sheet1", "A1:B3", "region\tsales\nEast\t15\nWest\t22\n")
        .file(
            "Sheet1",
            &format!("{range}.json"),
            "{\"data\":{\"name\":\"A1:B3\"},\"mark\":\"bar\"}",
        );
    fx
}

/// Fifteen cells each reading `fig` never said which figure it was. One block does, and comfy-table
/// spans no cell — so the cells still draw their marks and the block is cut out of the rendered
/// string, leaving every column the width it already had.
#[test]
fn a_range_named_figure_draws_as_one_labelled_block() {
    let fx = ranged("figure-block", "D2:F6");
    let (code, out, err) = run_err(&["render", fx.path().to_str().unwrap()]);
    assert_eq!(code, 0, "a clean workbook renders:\n{out}{err}");
    assert_eq!(
        out.trim_end(),
        "\
+---+--------+-------+---+-----+-----+-----+
|   | A      | B     | C | D   | E   | F   |
+==========================================+
| 1 | region | sales |   |     |     |     |
|---+--------+-------+---+-----------------|
| 2 | East   | 15    |   |                 |
|---+--------+-------+---+                 |
| 3 | West   | 22    |   |   D2:F6.json    |
|---+--------+-------+---+                 |
| 4 |        |       |   |    bar←A1:B3    |
|---+--------+-------+---+                 |
| 5 |        |       |   |                 |
|---+--------+-------+---+                 |
| 6 |        |       |   |                 |
+---+--------+-------+---+-----------------+",
        "D2:F6 is ONE block: its name, then what it draws, centred inside it:\n{out}"
    );
    assert_eq!(
        out.matches("| fig ").count(),
        0,
        "no cell marks left:\n{out}"
    );

    let tight = ranged("figure-block-tight", "D2:E3");
    let (code, out, err) = run_err(&["render", tight.path().to_str().unwrap()]);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(
        out.trim_end(),
        "\
+---+--------+-------+---+-----+-----+
|   | A      | B     | C | D   | E   |
+====================================+
| 1 | region | sales |   |     |     |
|---+--------+-------+---+-----------|
| 2 | East   | 15    |   |D2:E3.json |
|---+--------+-------+---+           |
| 3 | West   | 22    |   | bar←A1:B3 |
+---+--------+-------+---+-----------+",
        "a two-row block holds both label lines and spills neither:\n{out}"
    );

    // One row for two lines: the NAME is the row that survives, and what it lost reads `…`.
    let one = ranged("figure-block-one-row", "D2:E2");
    let (code, out, err) = run_err(&["render", one.path().to_str().unwrap()]);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(
        out.trim_end(),
        "\
+---+--------+-------+---+-----+-----+
|   | A      | B     | C | D   | E   |
+====================================+
| 1 | region | sales |   |     |     |
|---+--------+-------+---+-----------|
| 2 | East   | 15    |   |D2:E2.json…|
|---+--------+-------+---+-----------|
| 3 | West   | 22    |   |     |     |
+---+--------+-------+---+-----+-----+",
        "the label is cut to the block, never past it:\n{out}"
    );
}

/// comfy-table pads a cell by DISPLAY width, so a boundary read off the pure-ASCII top border is a
/// display COLUMN, and every other line is cut where walking its own characters reaches that column.
/// An accented cell and a wide CJK one left of a block move its bytes without moving its columns.
#[test]
fn a_block_holds_its_columns_left_of_non_ascii_cells() {
    let fx = Fixture::new("figure-block-wide");
    fx.file("Sheet1", "A1:B3", "region\tsales\nZürich\t15\n東京都\t22\n")
        .file(
            "Sheet1",
            "D2:F6.json",
            "{\"data\":{\"name\":\"A1:B3\"},\"mark\":\"bar\"}",
        );
    let (code, out, err) = run_err(&["render", fx.path().to_str().unwrap()]);
    assert_eq!(code, 0, "a clean workbook renders:\n{out}{err}");
    assert_eq!(
        out.trim_end(),
        "\
+---+--------+-------+---+-----+-----+-----+
|   | A      | B     | C | D   | E   | F   |
+==========================================+
| 1 | region | sales |   |     |     |     |
|---+--------+-------+---+-----------------|
| 2 | Zürich | 15    |   |                 |
|---+--------+-------+---+                 |
| 3 | 東京都 | 22    |   |   D2:F6.json    |
|---+--------+-------+---+                 |
| 4 |        |       |   |    bar←A1:B3    |
|---+--------+-------+---+                 |
| 5 |        |       |   |                 |
|---+--------+-------+---+                 |
| 6 |        |       |   |                 |
+---+--------+-------+---+-----------------+",
        "the block draws whole beside a two-byte cell and a three-byte one:\n{out}"
    );
    assert!(
        !out.contains("fig"),
        "and no mark survives inside it:\n{out}"
    );

    let plain = ranged("figure-block-wide-ascii", "D2:F6");
    let (_, ascii, _) = run_err(&["render", plain.path().to_str().unwrap()]);
    let widths = |s: &str| {
        s.trim_end()
            .lines()
            .map(UnicodeWidthStr::width)
            .collect::<Vec<usize>>()
    };
    assert_eq!(
        widths(&out),
        widths(&ascii),
        "every line keeps the DISPLAY width the same table of ASCII cells has:\n{out}"
    );
    assert_ne!(
        out.trim_end().len(),
        ascii.trim_end().len(),
        "which is not its byte length, or the fixture would prove nothing:\n{out}"
    );
}

/// A comfy-table row is not a LINE: a cell holding a newline splits its row across two of them, and
/// every row under it moves down. The block's band is read off the table's own separators, so it
/// still lands on the rows the figure covers — and a taller row simply gives the label more room.
#[test]
fn a_block_lands_on_its_rows_left_of_a_multi_line_cell() {
    let fx = Fixture::new("figure-block-multiline");
    fx.file(
        "Sheet1",
        "A1:B3",
        "top\\nbottom\tsales\nEast\t15\nWest\t22\n",
    )
    .file(
        "Sheet1",
        "D2:F6.json",
        "{\"data\":{\"name\":\"A1:B3\"},\"mark\":\"bar\"}",
    );
    let (code, out, err) = run_err(&["render", fx.path().to_str().unwrap()]);
    assert_eq!(code, 0, "a clean workbook renders:\n{out}{err}");
    assert_eq!(
        out.trim_end(),
        "\
+---+--------+-------+---+-----+-----+-----+
|   | A      | B     | C | D   | E   | F   |
+==========================================+
| 1 | top    | sales |   |     |     |     |
|   | bottom |       |   |     |     |     |
|---+--------+-------+---+-----------------|
| 2 | East   | 15    |   |                 |
|---+--------+-------+---+                 |
| 3 | West   | 22    |   |   D2:F6.json    |
|---+--------+-------+---+                 |
| 4 |        |       |   |    bar←A1:B3    |
|---+--------+-------+---+                 |
| 5 |        |       |   |                 |
|---+--------+-------+---+                 |
| 6 |        |       |   |                 |
+---+--------+-------+---+-----------------+",
        "the two-line row pushes the block down whole, marks and all:\n{out}"
    );
    assert!(
        !out.contains("fig"),
        "so no covered cell is left outside it:\n{out}"
    );
}

/// A figure binds one range per dimension, so the second label line is the one that runs long. It
/// breaks after the `←` first, which starts the ranges together on one row, and falls back to the
/// commas only where they still do not fit. Nothing is dropped while the block has rows left.
#[test]
fn a_label_too_wide_for_the_block_breaks_at_its_arrow_then_its_commas() {
    let fx = Fixture::new("figure-block-layers");
    fx.file("Sheet1", "A1:C9", &"a\tb\tc\n".repeat(9)).file(
        "Sheet1",
        "E1:G9.json",
        "{\"layer\":[{\"data\":{\"name\":\"A1:A9\"},\"mark\":\"line\"},\
         {\"data\":{\"name\":\"B1:B9\"},\"mark\":\"point\"},\
         {\"data\":{\"name\":\"C1:C9\"},\"mark\":\"bar\"}]}",
    );
    let (code, out, err) = run_err(&["render", fx.path().to_str().unwrap()]);
    assert_eq!(code, 0, "a clean workbook renders:\n{out}{err}");
    assert!(
        out.contains("|A1:A9,B1:B9,C1:C9|"),
        "the arrow break starts all three ranges on one row:\n{out}"
    );
    assert!(
        out.contains("point, bar)←") && !out.contains('…'),
        "the mark takes the rows above it and nothing is dropped:\n{out}"
    );
}

/// Two blocks written into overlapping bands would clobber each other mid-line, so a cover reaching
/// into a block returns BOTH figures to the marks they draw today. A name-form cover is the reachable
/// clash: two RANGE forms contest cells, and the load refuses them before any drawer sees them.
#[test]
fn a_second_cover_reaching_into_a_block_returns_both_figures_to_marks() {
    let fx = ranged("figure-block-clash", "D2:F6");
    fx.file(
        "Sheet1",
        "Chart1.json",
        "{\"data\":{\"name\":\"A1:B3\"},\"mark\":\"line\"}",
    )
    .file("Sheet1", "Chart1.css", "  figure { anchor: E4 }\n");
    let (code, out, err) = run_err(&["render", fx.path().to_str().unwrap()]);
    assert_eq!(code, 0, "both figures place cleanly:\n{out}{err}");
    assert!(
        out.matches("| fig ").count() >= 15,
        "every covered cell is back to its own mark:\n{out}"
    );
    assert!(
        !out.contains("D2:F6.json") && !out.contains("bar←A1:B3"),
        "and no block was cut, so no label was written:\n{out}"
    );

    // The SAME fixture without the second cover blocks, so the marks above are a fallback taken.
    let alone = ranged("figure-block-clash-alone", "D2:F6");
    let (code, block, err) = run_err(&["render", alone.path().to_str().unwrap()]);
    assert_eq!(code, 0, "{block}{err}");
    assert!(
        block.contains("D2:F6.json") && block.matches("| fig ").count() == 0,
        "one cover alone draws the block, so the marks above are not the feature switched off:\n{block}"
    );
}

/// A block is a view of the cells in the VIEWPORT, never of the cells the figure claims: a region
/// that misses the figure entirely draws nothing of it, and one that clips it draws the part inside.
#[test]
fn a_block_is_clipped_to_the_viewport_and_absent_from_one_that_misses_it() {
    let fx = ranged("figure-block-clip", "D2:F6");
    let root = fx.path().to_str().unwrap();

    let (code, out, err) = run_err(&["render", &format!("{root}/Sheet1/A1:C3")]);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(
        out.trim_end(),
        "\
+---+--------+-------+---+
|   | A      | B     | C |
+========================+
| 1 | region | sales |   |
|---+--------+-------+---|
| 2 | East   | 15    |   |
|---+--------+-------+---|
| 3 | West   | 22    |   |
+---+--------+-------+---+",
        "no cell of D2:F6 is drawn, so there is no block and no label:\n{out}"
    );

    let (code, out, err) = run_err(&["render", &format!("{root}/Sheet1/A1:E4")]);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(
        out.trim_end(),
        "\
+---+--------+-------+---+-----+-----+
|   | A      | B     | C | D   | E   |
+====================================+
| 1 | region | sales |   |     |     |
|---+--------+-------+---+-----------|
| 2 | East   | 15    |   |D2:F6.json |
|---+--------+-------+---+           |
| 3 | West   | 22    |   | bar←A1:B3 |
|---+--------+-------+---+           |
| 4 |        |       |   |           |
+---+--------+-------+---+-----------+",
        "the block stops at column E, where the viewport does:\n{out}"
    );
}
