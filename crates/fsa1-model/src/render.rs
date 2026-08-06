// Concern: spells a viewport's cells as display strings under a RenderMode | Non-concern: drawing the table, computing the values | IO: (&Workbook, sheet, Rect, RenderMode) -> RenderGrid

use fsa1_ast::a1::{format_column, parse_a1};
use fsa1_ast::{ErrKind, Value, format_value, num_to_text};

use crate::format::Format;
use crate::grid::Cell;
use crate::overlap::Rect;
use crate::workbook::Workbook;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMode {
    Values,
    /// A formula shows its `=…` text, but a literal still shows its value.
    Functions,
    /// The default: a literal plain, a formula as `<value> ← =<formula>`, reusing the exact `Values`
    /// spelling and `Functions` source text rather than re-deriving either.
    Combined,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderRow {
    pub row_label: String,
    pub cells: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderGrid {
    pub col_labels: Vec<String>,
    pub rows: Vec<RenderRow>,
}

/// A viewport is user-supplied and `render` allocates a string per cell, so without this bound a
/// syntactically-valid selector like `A1:A4294967295` drives the process into an allocation abort.
/// Far above any drawable ASCII table, so only a pathological selector reaches it.
pub const MAX_VIEWPORT_CELLS: u64 = 1_000_000;

/// Shown at every coordinate of an array-formula region but its top-left anchor, so a view never
/// implies each coordinate holds its own formula.
const ARRAY_CONTINUATION_MARK: &str = "^";

/// U+2190, single-spaced. Parentheses are deliberately not the delimiter: a formula's own `(...)`
/// would collide with them.
const COMBINED_ARROW: &str = " ← ";

/// The single composition point, so the arrow, the spacing, and the value-then-source order live in
/// exactly one place. Both arguments are already spelled; this only joins them.
pub fn combined_cell(value: &str, source: &str) -> String {
    format!("{value}{COMBINED_ARROW}{source}")
}

/// `u64` so the product of two `u32` spans cannot overflow.
pub fn viewport_cell_count(vp: Rect) -> u64 {
    let rows = u64::from(vp.max_row - vp.min_row) + 1;
    let cols = u64::from(vp.max_col - vp.min_col) + 1;
    rows * cols
}

/// The whole viewport is demanded in ONE batched [`Workbook::values_at`], so a dependency shared by
/// several viewport cells is computed once. The caller must bound `viewport` to
/// [`MAX_VIEWPORT_CELLS`] first — a string per cell is materialized here.
pub fn render(wb: &Workbook, sheet: u32, viewport: Rect, mode: RenderMode) -> RenderGrid {
    let col_labels: Vec<String> = (viewport.min_col..=viewport.max_col)
        .map(format_column)
        .collect();

    // Indexed row-major, to fill the grid below.
    let needs_values = matches!(mode, RenderMode::Values | RenderMode::Combined);
    let value_strings: Option<Vec<String>> = needs_values.then(|| {
        let coords: Vec<(u32, u32, u32)> = (viewport.min_row..=viewport.max_row)
            .flat_map(|row| (viewport.min_col..=viewport.max_col).map(move |col| (sheet, col, row)))
            .collect();
        let values = wb.values_at(&coords);
        coords
            .iter()
            .zip(&values)
            .map(|(&(s, c, r), v)| display_value_formatted(v, cell_format(wb, s, c, r)))
            .collect()
    });

    let width = (viewport.max_col - viewport.min_col + 1) as usize;
    let rows = (viewport.min_row..=viewport.max_row)
        .enumerate()
        .map(|(ri, row)| {
            let cells = (viewport.min_col..=viewport.max_col)
                .enumerate()
                .map(|(ci, col)| match &value_strings {
                    Some(vs) if mode == RenderMode::Combined => {
                        combined_text(wb, sheet, col, row, &vs[ri * width + ci])
                    }
                    Some(vs) => vs[ri * width + ci].clone(),
                    None => source_text(wb, sheet, col, row),
                })
                .collect();
            RenderRow {
                row_label: (u64::from(row) + 1).to_string(),
                cells,
            }
        })
        .collect();

    RenderGrid { col_labels, rows }
}

/// A per-cell lookup with no cross-cell sharing; `Values` and `Combined` never route here. A
/// load-error cell shows its RAW source, so an agent sees exactly what to fix.
fn source_text(wb: &Workbook, sheet: u32, col: u32, row: u32) -> String {
    match wb.source_at(sheet, col, row) {
        Some(src) if src.array_continuation => ARRAY_CONTINUATION_MARK.to_string(),
        Some(src) => match src.cell {
            Cell::Formula { src: text, .. } => text.clone(),
            Cell::LoadError { src: text, .. } => text.clone(),
            Cell::Value { format, .. } => {
                display_value_formatted(&wb.value_at(sheet, col, row), *format)
            }
        },
        None => String::new(),
    }
}

/// `value` arrives already spelled by [`render`]'s batched pass, and the source side is the exact
/// text [`source_text`] prints — both reused verbatim, so no cell is spelled twice.  A literal gets
/// no arrow: its value IS its provenance.
fn combined_text(wb: &Workbook, sheet: u32, col: u32, row: u32, value: &str) -> String {
    let Some(src) = wb.source_at(sheet, col, row) else {
        return String::new();
    };
    if src.array_continuation {
        return combined_cell(value, ARRAY_CONTINUATION_MARK);
    }
    match src.cell {
        Cell::Value { .. } => value.to_string(),
        Cell::Formula { src: text, .. } | Cell::LoadError { src: text, .. } => {
            combined_cell(value, text)
        }
    }
}

/// A pure read: the seam through which [`render`] threads a computed value's display format.
fn cell_format(wb: &Workbook, sheet: u32, col: u32, row: u32) -> Option<Format> {
    match wb.source_at(sheet, col, row)?.cell {
        Cell::Value { format, .. } | Cell::Formula { format, .. } => *format,
        Cell::LoadError { .. } => None,
    }
}

/// Renders through fsa1-ast's numFmt engine — the same one `TEXT()` runs on, so FSA1 never
/// re-implements Excel number formatting. A format changes the display only, never the value.
fn display_value_formatted(v: &Value, format: Option<Format>) -> String {
    match format {
        Some(f) => display_value(&format_value(v, &f.code())),
        None => display_value(v),
    }
}

/// The single home for spelling a resolved [`Value`]. The number arm defers to
/// [`fsa1_ast::num_to_text`] — the same General formatter `&`/`TEXT` use — so a grid cell and a
/// concatenation can never disagree. An array is defended against, never placed in a cell.
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

/// The inverse of the literal error lexer.
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

/// Endpoints are ordered, so `G8:A3` and `A3:G8` denote the same rectangle — unlike a filename,
/// where the reversed spelling is a refusal.
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
        assert_eq!(g.rows[0].cells, vec!["1", "2 ← =A1+1", "20 ← =B1*10"]);
    }

    #[test]
    fn combined_mode_spells_an_error_valued_formula_with_its_source() {
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
        let vp = parse_viewport("Z9").unwrap(); // claimed by no file
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
        // Wiring only; fsa1-ast::eval freezes the General spelling table itself.
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
        // 7 cols (A..G) x 6 rows (3..8).
        assert_eq!(viewport_cell_count(parse_viewport("A3:G8").unwrap()), 42);
        assert_eq!(viewport_cell_count(parse_viewport("B2").unwrap()), 1);
        let huge = parse_viewport("A1:A4294967295").unwrap();
        assert_eq!(viewport_cell_count(huge), u64::from(u32::MAX));
        assert!(viewport_cell_count(huge) > MAX_VIEWPORT_CELLS);
    }

    #[test]
    fn formatted_literals_and_formulas_render_formatted_but_compute_on_pure_numbers() {
        let files: &[(&str, &str)] = &[
            ("A1", "$1,234.00"),           // a formatted currency LITERAL
            ("A2", "=A1*2~$#,##0.00"),     // a formatted currency FORMULA over it
            ("A3", "=A2+1"),               // a plain formula reading the formatted formula's value
            ("D1", "2021-05-15~m/d/yyyy"), // a date literal
            ("D2", "=D1+1~m/d/yyyy"),      // a formatted date formula
            ("P1", "12.50%"),              // a percent literal
            ("P2", "=P1*2~0.00%"),         // a formatted percent formula
        ];
        let wb = Workbook::from_tabs(&[("Sheet1", files)]).expect("loads clean");

        let shown = |a1: &str| {
            render(&wb, 0, parse_viewport(a1).unwrap(), RenderMode::Values).rows[0].cells[0].clone()
        };
        assert_eq!(shown("A1"), "$1,234.00");
        assert_eq!(shown("A2"), "$2,468.00");
        assert_eq!(shown("D1"), "5/15/2021");
        assert_eq!(shown("D2"), "5/16/2021");
        assert_eq!(shown("P1"), "12.50%");
        assert_eq!(shown("P2"), "25.00%");

        assert_eq!(wb.value_at(0, 0, 0), Value::Number(1234.0)); // A1
        assert_eq!(wb.value_at(0, 0, 1), Value::Number(2468.0)); // A2 = A1*2
        assert_eq!(wb.value_at(0, 0, 2), Value::Number(2469.0)); // A3 = A2+1
        assert_eq!(wb.value_at(0, 3, 0), Value::Number(44331.0)); // D1 (the serial)
        assert_eq!(wb.value_at(0, 15, 0), Value::Number(0.125)); // P1 (the ratio)
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
