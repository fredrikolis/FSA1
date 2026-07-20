// Concern: the RENDER MODEL — turn a `Workbook` viewport (a sheet + a rectangular range) into a plain-data ASCII grid: the column-letter header row, the row-number gutter, and each cell's display string under one of three modes (computed VALUES — demand-driven, only the viewport's cone evaluates; FORMULA text; COMBINED — a literal's value, or a formula's `<value> ← =<formula>` reusing the SAME value+source spellings), plus the A1-range→`Rect` viewport parser, the single `Value`→display-string spelling (numbers, TRUE/FALSE, the `#REF!`-family error text, blank), and the ONE `<value> ← <source>` combined composition (`combined_cell`) reused by the CLI's name rendering | Non-concern: the ASCII table DRAWING itself (charlie-cli owns comfy-table layout/borders — this hands back strings, never glyphs), the demand-driven eval engine (workbook.rs owns the pull), and the lint/diagnostic surface (workbook.rs `lint`) | IO: (a `&Workbook`, a sheet, a viewport `Rect`, a `RenderMode`) -> a `RenderGrid` of strings; (an A1 range `&str`) -> a `Rect`
//! The render model: [`render`] builds a [`RenderGrid`] of display strings for a viewport; the CLI
//! draws it. [`parse_viewport`] turns an `A3:G8` (or `A3`) range string into a [`Rect`].
//!
//! Logic lives here (per `repo-standards.md` "logic in the engine; the CLI is a thin consumer"):
//! the demand-driven viewport evaluation, the `Value` display spelling, and the header/gutter
//! labels are all model concerns. The CLI only lays the returned strings into an ASCII table.

use charlie_ast::a1::{format_column, parse_a1};
use charlie_ast::{ErrKind, Value, num_to_text};

use crate::grid::Cell;
use crate::overlap::Rect;
use crate::workbook::Workbook;

/// What a rendered cell shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMode {
    /// The computed value (demand-driven — only the viewport's dependency cone evaluates).
    Values,
    /// The source text: a formula cell shows its `=…` text; a literal cell shows its value
    /// (Excel's "show formulas" view).
    Functions,
    /// The COMBINED view (the DEFAULT for `render` and `tree`): per cell, a literal shows its value
    /// plain (as [`RenderMode::Values`]); a formula shows `<value> ← =<formula>` — its computed value
    /// AND its authored source in one glance, arrow U+2190 single-spaced; a blank is blank; an error
    /// value keeps its error spelling (still `<err> ← =<formula>` if it came from a formula). Reuses the
    /// EXACT `Values` value spelling and `Functions` source text — no second formatter, no re-parse.
    Combined,
}

/// One rendered row: the gutter label (the 1-based row number) and the viewport cells left→right.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderRow {
    pub row_label: String,
    pub cells: Vec<String>,
}

/// A viewport rendered to strings: the column-letter header row (`A`, `B`, …) and the data rows.
/// The CLI feeds `col_labels` (prefixed with a corner cell) as the table header and each
/// [`RenderRow`] as a table row. No borders/glyphs here — that is the CLI's comfy-table layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderGrid {
    /// The column-letter labels across the viewport (one per column, no gutter corner).
    pub col_labels: Vec<String>,
    pub rows: Vec<RenderRow>,
}

/// The largest viewport (in cells) [`render`] will materialize into a [`RenderGrid`]. A viewport is
/// user-supplied (`--range A1:A4294967295` is a syntactically-valid address pair), and `render`
/// allocates a row/cell string for every cell of the rectangle — an unbounded viewport drives the
/// process into an allocation abort/hang. The CLI checks a requested viewport against this bound and
/// refuses with a located diagnostic (a valid invocation must never crash). Far above any drawable
/// ASCII table (a million cells is already unreadable), so only a pathological `--range` reaches it.
pub const MAX_VIEWPORT_CELLS: u64 = 1_000_000;

/// The `--functions` marker for a GRID5 array-formula region's CONTINUATION cell (any cell but the
/// anchor): the region's single `=formula` renders at the top-left anchor, and each other coordinate
/// shows this caret to signal "filled by the array formula anchored at the region's top-left" without
/// re-printing the formula (VAL1: one array-formula cell spanning its range, not many cells).
const ARRAY_CONTINUATION_MARK: &str = "^";

/// The Combined-mode provenance delimiter: `<value> ← <source>` — U+2190 LEFTWARDS ARROW, single-spaced
/// (` ← `). Parentheses are deliberately NOT the delimiter (a formula's own `(...)` would collide). Its
/// one home; the composition is done ONLY through [`combined_cell`].
const COMBINED_ARROW: &str = " ← ";

/// Compose the [`RenderMode::Combined`] spelling from a cell/name's ALREADY-PRODUCED value and source
/// strings: `<value> ← <source>`. The SINGLE composition point — the grid-cell path ([`combined_text`])
/// AND the CLI's `tree` name rendering both call it, so the arrow, the spacing, and the value-then-source
/// order live in exactly ONE place (HARD RULE 2/4: one combined spelling, never a second formatter). The
/// caller supplies the value spelled by [`display_value`] and the source spelled by the `Functions` path;
/// this only joins them.
pub fn combined_cell(value: &str, source: &str) -> String {
    format!("{value}{COMBINED_ARROW}{source}")
}

/// The cell-count of a viewport rectangle, as `u64` so the product of two `u32` spans cannot
/// overflow. The CLI compares this to [`MAX_VIEWPORT_CELLS`] before calling [`render`].
pub fn viewport_cell_count(vp: Rect) -> u64 {
    let rows = u64::from(vp.max_row - vp.min_row) + 1;
    let cols = u64::from(vp.max_col - vp.min_col) + 1;
    rows * cols
}

/// Build the render grid for `viewport` on `sheet` under `mode`. Demand-driven: in [`RenderMode::Values`]
/// the whole viewport is demanded through the workbook's batched [`Workbook::values_at`], so the
/// viewport's cells accrete into ONE dependency graph (ENG3) — a dependency shared by several viewport
/// cells is computed once — and only that transitive cone evaluates.
///
/// The caller must bound `viewport` to [`MAX_VIEWPORT_CELLS`] (see [`viewport_cell_count`]) before
/// calling — `render` materializes a string per cell, so an unbounded viewport would OOM.
pub fn render(wb: &Workbook, sheet: u32, viewport: Rect, mode: RenderMode) -> RenderGrid {
    let col_labels: Vec<String> = (viewport.min_col..=viewport.max_col)
        .map(format_column)
        .collect();

    // Values AND Combined both demand every viewport cell in ONE plan+evaluate pass so shared
    // dependencies are computed once (ENG3); Combined then prefixes each value with its source
    // provenance. The spelled strings are indexed row-major to fill the grid below.
    let needs_values = matches!(mode, RenderMode::Values | RenderMode::Combined);
    let value_strings: Option<Vec<String>> = needs_values.then(|| {
        let coords: Vec<(u32, u32, u32)> = (viewport.min_row..=viewport.max_row)
            .flat_map(|row| (viewport.min_col..=viewport.max_col).map(move |col| (sheet, col, row)))
            .collect();
        wb.values_at(&coords).iter().map(display_value).collect()
    });

    let width = (viewport.max_col - viewport.min_col + 1) as usize;
    let rows = (viewport.min_row..=viewport.max_row)
        .enumerate()
        .map(|(ri, row)| {
            let cells = (viewport.min_col..=viewport.max_col)
                .enumerate()
                .map(|(ci, col)| match &value_strings {
                    // Combined: `<value> ← <source>` per cell, reusing the batched value string.
                    Some(vs) if mode == RenderMode::Combined => {
                        combined_text(wb, sheet, col, row, &vs[ri * width + ci])
                    }
                    // Values: the batched computed value.
                    Some(vs) => vs[ri * width + ci].clone(),
                    // Functions: the per-cell source lookup (no cross-cell sharing).
                    None => cell_text(wb, sheet, col, row, mode),
                })
                .collect();
            RenderRow {
                // 1-based row number gutter.
                row_label: (u64::from(row) + 1).to_string(),
                cells,
            }
        })
        .collect();

    RenderGrid { col_labels, rows }
}

/// The display string for one cell under `mode`. `Functions` is a per-cell source lookup with no
/// cross-cell sharing; `Values` and `Combined` are batched through the `value_strings` path in
/// [`render`], so their arms here are statically dead (kept total, never a panic, if ever reached).
fn cell_text(wb: &Workbook, sheet: u32, col: u32, row: u32, mode: RenderMode) -> String {
    match mode {
        RenderMode::Functions => match wb.source_at(sheet, col, row) {
            // A GRID5 array-formula region shows its single `=formula` at the ANCHOR (top-left) cell;
            // each CONTINUATION cell shows a caret `^` marker (the formula lives once, at the anchor —
            // VAL1), so the view never implies each coordinate holds its own formula.
            Some(src) if src.array_continuation => ARRAY_CONTINUATION_MARK.to_string(),
            // A formula cell shows its source text; a literal cell shows its value (Excel's Ctrl+`
            // "show formulas" view: formulas as text, literals as their value). GRID6: a load-error
            // cell shows its RAW (unparsed) source text, so an agent sees exactly what to fix — while
            // `--values` shows the located error value it resolves to.
            Some(src) => match src.cell {
                Cell::Formula { src: text, .. } => text.clone(),
                Cell::LoadError { src: text, .. } => text.clone(),
                Cell::Value(_) => display_value(&wb.value_at(sheet, col, row)),
            },
            None => String::new(),
        },
        // `Values` and `Combined` are materialized once, batched, in [`render`] (the `value_strings`
        // branch), so `cell_text` is only ever reached for `Functions`; these arms are statically dead.
        // Rather than panic, spell each the same way its batched path would — total, never a panic (this
        // engine's never-panic convention), and the correct answer if ever reached.
        RenderMode::Values => display_value(&wb.value_at(sheet, col, row)),
        RenderMode::Combined => combined_text(
            wb,
            sheet,
            col,
            row,
            &display_value(&wb.value_at(sheet, col, row)),
        ),
    }
}

/// The [`RenderMode::Combined`] display string for one cell, given `value` — the cell's ALREADY-SPELLED
/// computed value (from the batched `Values` pass in [`render`], so no re-evaluation and the SAME
/// spelling as `--values`). A literal shows just its value (its value IS its provenance — no arrow); a
/// formula (or a GRID6 load-error cell) shows `<value> ← <source>`, where `<source>` is the EXACT text
/// `--functions` prints (the parsed `=…`, or the raw unparsed body) — reused verbatim, never re-parsed
/// (HARD RULE 2). A GRID5 continuation cell shows its spilled value with the `^` array-formula marker as
/// provenance (the same token `--functions` prints — the formula lives once, at the anchor, VAL1). A gap
/// cell (covered by no file) is blank.
fn combined_text(wb: &Workbook, sheet: u32, col: u32, row: u32, value: &str) -> String {
    let Some(src) = wb.source_at(sheet, col, row) else {
        return String::new();
    };
    if src.array_continuation {
        return combined_cell(value, ARRAY_CONTINUATION_MARK);
    }
    match src.cell {
        Cell::Value(_) => value.to_string(),
        Cell::Formula { src: text, .. } | Cell::LoadError { src: text, .. } => {
            combined_cell(value, text)
        }
    }
}

/// The single home for spelling a resolved [`Value`] as a display string: a number in Excel's
/// **General** number format, `TRUE`/`FALSE`, the `#REF!`-family error text, text verbatim, blank as
/// empty. An array (which a placed cell never is, but defended) shows its top-left cell.
///
/// The number case defers to [`charlie_ast::num_to_text`] — the SAME General formatter the `&`/`TEXT`
/// text form uses — so the grid/`charlie-cli eval` display and the concatenation text never diverge:
/// extreme magnitudes render in scientific form (`1E+20`, `1E-09`, `1.23456789012346E+15`) instead of
/// leaking Rust's full-precision `Display`, and `-0.0` canonicalizes to an unsigned `0` (Excel never
/// shows `-0`).
pub fn display_value(v: &Value) -> String {
    match v {
        Value::Number(n) => num_to_text(*n),
        Value::Text(s) => s.clone(),
        Value::Bool(true) => "TRUE".to_string(),
        Value::Bool(false) => "FALSE".to_string(),
        Value::Error(k) => err_text(*k).to_string(),
        Value::Blank => String::new(),
        Value::Array(_, cells) => cells.first().map_or(String::new(), display_value),
    }
}

/// The canonical spreadsheet spelling of an error value (the inverse of the literal error lexer).
fn err_text(k: ErrKind) -> &'static str {
    match k {
        ErrKind::Ref => "#REF!",
        ErrKind::Div0 => "#DIV/0!",
        ErrKind::Value => "#VALUE!",
        ErrKind::Name => "#NAME?",
        ErrKind::Na => "#N/A",
        ErrKind::Null => "#NULL!",
        ErrKind::Num => "#NUM!",
        ErrKind::Spill => "#SPILL!",
        ErrKind::Calc => "#CALC!",
    }
}

/// Parse a viewport range string into a [`Rect`]: `A3:G8` (a rectangle) or `A3` (a single cell).
/// Endpoints are ordered, so `G8:A3` and `A3:G8` denote the same rectangle. Rejects a `$`-anchored,
/// lowercase, or leading-zero address (a viewport is canonical A1, like a filename), and any
/// malformed address — a located, never-panicking refusal returned as a message string.
pub fn parse_viewport(s: &str) -> Result<Rect, String> {
    let (a, b) = match s.split_once(':') {
        Some((a, b)) => (a, b),
        None => (s, s),
    };
    let a = parse_endpoint(a)?;
    let b = parse_endpoint(b)?;
    Ok(Rect {
        min_col: a.0.min(b.0),
        min_row: a.1.min(b.1),
        max_col: a.0.max(b.0),
        max_row: a.1.max(b.1),
    })
}

/// One viewport endpoint address → zero-based `(col, row)`, enforcing canonical A1 form.
fn parse_endpoint(s: &str) -> Result<(u32, u32), String> {
    let addr = parse_a1(s).map_err(|e| format!("bad range address {s:?}: {e:?}"))?;
    if addr.col_abs || addr.row_abs {
        return Err(format!("a viewport address must not use '$': {s:?}"));
    }
    if addr.col_had_lowercase {
        return Err(format!("a viewport column must be uppercase: {s:?}"));
    }
    if addr.row_had_leading_zero {
        return Err(format!(
            "a viewport row must not have a leading zero: {s:?}"
        ));
    }
    Ok((addr.col, addr.row))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wb() -> Workbook {
        // A1=1 (literal), B1==A1+1 (formula), C1==B1*10 (formula); A2:B2 a literal row.
        // A file's content is exactly its grid (GRID1) — no annotation line.
        let f = |b: &str| b.to_string();
        let a1 = f("1");
        let b1 = f("=A1+1");
        let c1 = f("=B1*10");
        let a2 = f("hello\tTRUE");
        Workbook::from_tabs(&[(
            "Sheet1",
            &[
                ("A1", a1.as_str()),
                ("B1", b1.as_str()),
                ("C1", c1.as_str()),
                ("A2:B2", a2.as_str()),
            ],
        )])
        .expect("loads clean")
    }

    #[test]
    fn values_mode_computes_the_cone() {
        let wb = wb();
        let g = render(&wb, 0, parse_viewport("A1:C1").unwrap(), RenderMode::Values);
        assert_eq!(g.col_labels, vec!["A", "B", "C"]);
        assert_eq!(g.rows.len(), 1);
        assert_eq!(g.rows[0].row_label, "1");
        assert_eq!(g.rows[0].cells, vec!["1", "2", "20"]);
    }

    #[test]
    fn functions_mode_shows_formula_text_and_literal_values() {
        let wb = wb();
        let g = render(
            &wb,
            0,
            parse_viewport("A1:C1").unwrap(),
            RenderMode::Functions,
        );
        // A1 is a literal (shows its value); B1/C1 are formulas (show their text).
        assert_eq!(g.rows[0].cells, vec!["1", "=A1+1", "=B1*10"]);
    }

    #[test]
    fn combined_mode_shows_value_then_source_for_formulas_and_plain_for_literals() {
        let wb = wb();
        let g = render(
            &wb,
            0,
            parse_viewport("A1:C1").unwrap(),
            RenderMode::Combined,
        );
        // A1 is a literal → its value plain (no arrow); B1/C1 are formulas → `<value> ← =<formula>`,
        // reusing the SAME value spelling (Values) and source text (Functions) per cell.
        assert_eq!(g.rows[0].cells, vec!["1", "2 ← =A1+1", "20 ← =B1*10"]);
    }

    #[test]
    fn combined_mode_spells_an_error_valued_formula_with_its_source() {
        // A formula that evaluates to an error keeps its error spelling on the value side, and still
        // carries `← =<formula>` (the combined form never drops provenance for a formula).
        let a1 = "=1/0".to_string();
        let wb = Workbook::from_tabs(&[("Sheet1", &[("A1", a1.as_str())])]).expect("loads");
        let g = render(&wb, 0, parse_viewport("A1").unwrap(), RenderMode::Combined);
        assert_eq!(g.rows[0].cells, vec!["#DIV/0! ← =1/0"]);
    }

    #[test]
    fn combined_mode_leaves_a_gap_cell_blank() {
        let wb = wb();
        let g = render(&wb, 0, parse_viewport("Z9").unwrap(), RenderMode::Combined);
        assert_eq!(g.rows[0].cells[0], "");
    }

    #[test]
    fn a_gap_cell_renders_empty_in_every_mode() {
        let wb = wb();
        // Z9 is claimed by no file.
        let vp = parse_viewport("Z9").unwrap();
        for mode in [
            RenderMode::Values,
            RenderMode::Functions,
            RenderMode::Combined,
        ] {
            let g = render(&wb, 0, vp, mode);
            assert_eq!(g.rows[0].cells[0], "");
        }
    }

    #[test]
    fn value_spelling_covers_every_arm() {
        assert_eq!(display_value(&Value::Number(20.0)), "20");
        assert_eq!(display_value(&Value::Number(2.5)), "2.5");
        // ONE discriminating case that the Number arm routes through the General formatter
        // (`num_to_text`) and not Rust's raw `Display`: `1e20` renders scientific `1E+20`, where raw
        // `Display` would spell a 21-digit integer. The exhaustive General spelling table (scientific
        // thresholds, `-0.0`→`0`, 15-sig rounding) is frozen once at its home,
        // charlie-ast::eval `num_to_text_matches_excel_general_format`; this only proves the wiring.
        assert_eq!(display_value(&Value::Number(1e20)), "1E+20");
        assert_eq!(display_value(&Value::Text("hi".into())), "hi");
        assert_eq!(display_value(&Value::Bool(true)), "TRUE");
        assert_eq!(display_value(&Value::Bool(false)), "FALSE");
        assert_eq!(display_value(&Value::Error(ErrKind::Ref)), "#REF!");
        assert_eq!(display_value(&Value::Error(ErrKind::Spill)), "#SPILL!");
        assert_eq!(display_value(&Value::Blank), "");
    }

    #[test]
    fn viewport_orders_endpoints_and_accepts_a_single_cell() {
        let a = parse_viewport("A3:G8").unwrap();
        let b = parse_viewport("G8:A3").unwrap();
        assert_eq!(a, b);
        assert_eq!(
            a,
            Rect {
                min_col: 0,
                min_row: 2,
                max_col: 6,
                max_row: 7
            }
        );
        let one = parse_viewport("B2").unwrap();
        assert_eq!(
            one,
            Rect {
                min_col: 1,
                min_row: 1,
                max_col: 1,
                max_row: 1
            }
        );
    }

    #[test]
    fn viewport_cell_count_is_the_rectangle_area_and_never_overflows() {
        // A3:G8 is 7 cols (A..G) x 6 rows (3..8) = 42 cells.
        assert_eq!(viewport_cell_count(parse_viewport("A3:G8").unwrap()), 42);
        assert_eq!(viewport_cell_count(parse_viewport("B2").unwrap()), 1);
        // A full u32 column span computed in u64 does not overflow and exceeds the render bound.
        let huge = parse_viewport("A1:A4294967295").unwrap();
        assert_eq!(viewport_cell_count(huge), u64::from(u32::MAX));
        assert!(viewport_cell_count(huge) > MAX_VIEWPORT_CELLS);
    }

    #[test]
    fn viewport_rejects_non_canonical_or_malformed_addresses() {
        assert!(parse_viewport("$A$1").is_err());
        assert!(parse_viewport("a1").is_err());
        assert!(parse_viewport("A01").is_err());
        assert!(parse_viewport("1A").is_err());
        assert!(parse_viewport("").is_err());
    }
}
