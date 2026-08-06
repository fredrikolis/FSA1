// Concern: reads a .ods/.xlsx into a SourceBook (calamine lives here) | Non-concern: formula translation, name/table geometry | IO: (a path) -> SourceBook

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use calamine::{CellErrorType, Data, Reader, Sheets, open_workbook_auto};
use fsa1_ast::ErrKind;
use fsa1_ast::a1::format_cell;
use fsa1_model::Format;

use crate::dates::{iso_datetime_to_serial, iso_duration_to_serial};
use crate::error::{ErrorKind, IngestError};
use crate::names::DefinedName;
use crate::resolve::Resolution;
use crate::serialize::{effective_literal_format, is_display_exact};
use crate::source::{SheetSource, SourceBook, SourceCell, SourceValue};
use crate::warnings::{Axis, UnpackWarning, axis_run, unowned};
use crate::xlsx_meta::{self, NumFmtMap, RawTable};
use crate::xlsx_style::{self, AxisRun, AxisSize, AxisStyle, StyleTable, Styling};

const MAX_SHEET_CELLS: u64 = 4_000_000;

pub fn read_file(
    path: &Path,
    format_map: Option<&NumFmtMap>,
    warnings: &mut Vec<UnpackWarning>,
) -> Result<SourceBook, IngestError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("ods") | Some("xlsx") => {}
        _ => {
            return Err(IngestError::io(
                ErrorKind::Invalid,
                format!(
                    "cannot import {:?}: unsupported source format (expected a .ods or .xlsx file)",
                    path.display()
                ),
            ));
        }
    }
    if !path.exists() {
        return Err(IngestError::io(
            ErrorKind::SourceNotFound,
            format!("no such file {:?}", path.display()),
        ));
    }
    let mut wb = open_workbook_auto(path).map_err(|e| {
        IngestError::io(
            ErrorKind::SourceIo,
            format!("cannot open {:?} as a spreadsheet: {e}", path.display()),
        )
    })?;

    // .ods carries no styling on the read side: it imports values and formulas and nothing else.
    let styling = match ext.as_deref() {
        Some("xlsx") => Some(xlsx_style::read_styling(path)?),
        _ => None,
    };

    let names = wb.sheet_names().to_vec();
    let mut sheets = Vec::with_capacity(names.len());
    for name in &names {
        let sheet_map = format_map.and_then(|m| m.get(name));
        sheets.push(read_sheet(
            &mut wb,
            name,
            sheet_map,
            styling.as_ref(),
            warnings,
        )?);
    }
    let (resolution, defined_names) = build_resolution(path, &mut wb, &names, warnings)?;
    Ok(SourceBook {
        sheets,
        resolution,
        names: defined_names,
    })
}

/// Fuses two sources kept behind the format firewall: calamine supplies each table's SHEET, and the
/// raw xlsx parts supply the extents calamine's high-level seam hides. Any other format resolves to
/// nothing, so its names and tables load as `#NAME?` rather than as something wrong.
fn build_resolution(
    path: &Path,
    wb: &mut Sheets<BufReader<File>>,
    sheet_order: &[String],
    warnings: &mut Vec<UnpackWarning>,
) -> Result<(Resolution, Vec<DefinedName>), IngestError> {
    let Sheets::Xlsx(x) = wb else {
        return Ok((Resolution::empty(), Vec::new()));
    };
    let mut table_sheet: HashMap<String, String> = HashMap::new();
    let tables_ok = x.load_tables().is_ok();
    if tables_ok {
        for s in sheet_order {
            for t in x.table_names_in_sheet(s) {
                table_sheet.insert(t.clone(), s.clone());
            }
        }
    }

    let meta = xlsx_meta::read_meta(path)?;
    let res = resolve_tables(meta.tables, &table_sheet, tables_ok, warnings);
    let mut defined_names = Vec::new();
    for n in meta.names {
        let scope = n
            .local_sheet_id
            .and_then(|i| sheet_order.get(i as usize))
            .cloned();
        defined_names.push(DefinedName {
            name: n.name,
            scope,
            target: n.target,
        });
    }
    Ok((res, defined_names))
}

/// A table that cannot be mapped to a sheet is DROPPED with a warning, never a refusal: its
/// structured refs then stay verbatim and load as a located `#NAME?`.
fn resolve_tables(
    tables: Vec<RawTable>,
    table_sheet: &HashMap<String, String>,
    tables_ok: bool,
    warnings: &mut Vec<UnpackWarning>,
) -> Resolution {
    let mut res = Resolution::empty();
    for t in tables {
        match table_sheet.get(&t.name) {
            Some(sheet) => res.add_table(
                &t.name,
                sheet,
                t.columns,
                &t.ref_str,
                t.header_rows,
                t.totals_rows,
            ),
            None => {
                let reason = if !tables_ok {
                    "the workbook's table index could not be read; structured refs load as #NAME?"
                } else {
                    "could not map to a sheet (displayName/sheet divergence); structured refs load as #NAME?"
                };
                warnings.push(UnpackWarning::TableDropped {
                    table: t.name,
                    reason: reason.to_string(),
                });
            }
        }
    }
    res
}

/// Two passes over one package: calamine supplies the values and formulas, the styling pass supplies
/// the per-cell style, the widths, the heights and the merges — including for a cell calamine never
/// yields, one holding a style and no value.
fn read_sheet(
    wb: &mut Sheets<BufReader<File>>,
    name: &str,
    format_map: Option<&HashMap<String, Format>>,
    styling: Option<&Styling>,
    warnings: &mut Vec<UnpackWarning>,
) -> Result<SheetSource, IngestError> {
    let values = wb.worksheet_range(name).map_err(|e| {
        IngestError::invalid_at_sheet(name, format!("cannot read sheet values: {e}"))
    })?;
    let formulas = wb.worksheet_formula(name).map_err(|e| {
        IngestError::invalid_at_sheet(name, format!("cannot read sheet formulas: {e}"))
    })?;
    let styles = styling.map(|s| s.styles.clone()).unwrap_or_default();
    let visuals = styling.and_then(|s| s.sheets.get(name));

    // The used rectangle spans A1 to the furthest non-empty cell of EITHER range.
    let (mut rows, mut cols) = match (values.end(), formulas.end()) {
        (None, None) => (0, 0),
        (Some(a), None) => (a.0 + 1, a.1 + 1),
        (None, Some(b)) => (b.0 + 1, b.1 + 1),
        (Some(a), Some(b)) => (a.0.max(b.0) + 1, a.1.max(b.1) + 1),
    };
    let mut style_at: HashMap<(u32, u32), u32> = HashMap::new();
    for &(col, row, index) in visuals.map_or(&[][..], |v| v.styled_cells.as_slice()) {
        style_at.insert((col, row), index);
        // A look the SOURCE draws over a valueless cell extends the sheet whether or not a rule can carry it, so one it cannot is still here to be NAMED.
        if styles.draws_on_blank(index) {
            rows = rows.max(row + 1);
            cols = cols.max(col + 1);
        }
    }
    if u64::from(rows) * u64::from(cols) > MAX_SHEET_CELLS {
        return Err(IngestError::invalid_at_sheet(
            name,
            format!("used range {rows}x{cols} exceeds the {MAX_SHEET_CELLS}-cell import bound"),
        ));
    }

    let col_runs = visuals.map_or(&[][..], |v| v.col_styles.as_slice());
    let row_runs = visuals.map_or(&[][..], |v| v.row_styles.as_slice());
    let by_column = axis_defaults(Axis::Column, col_runs, cols, &styles, name, warnings);
    let by_row = axis_defaults(Axis::Row, row_runs, rows, &styles, name, warnings);

    let mut cells = Vec::with_capacity((rows as usize) * (cols as usize));
    for row in 0..rows {
        for col in 0..cols {
            let format = format_map
                .and_then(|m| m.get(&format_cell(col, row)))
                .copied();
            // calamine returns "" for a non-formula cell.
            let value = match formulas.get_value((row, col)) {
                Some(f) if !f.is_empty() => SourceValue::Formula {
                    raw: f.clone(),
                    format,
                },
                _ => match values.get_value((row, col)) {
                    Some(v) => data_to_value(v, name, col, row, format)?,
                    None => SourceValue::Blank,
                },
            };
            // The .xlsx cascade — the cell's own `s=`, else its row's default, else its column's.
            let default = by_row.get(&row).or_else(|| by_column.get(&col)).copied();
            let style = style_at
                .get(&(col, row))
                .copied()
                .or_else(|| default.filter(|&index| stated_on(&value, &styles, index)));
            cells.push(SourceCell { value, style });
        }
    }
    let col_widths = match visuals {
        Some(v) => within(Axis::Column, &v.col_widths, cols, name, warnings),
        None => BTreeMap::new(),
    };
    let row_heights = match visuals {
        Some(v) => within(Axis::Row, &v.row_heights, rows, name, warnings),
        None => BTreeMap::new(),
    };
    Ok(SheetSource {
        name: name.to_string(),
        rows,
        cols,
        cells,
        styles,
        col_widths,
        row_heights,
        merges: visuals.map(|v| v.merges.clone()).unwrap_or_default(),
    })
}

/// The ONE clipping rule both sheet axes read. An authored statement legally sizes axes the content
/// never reaches — a `<col>` run's `max` may be the last addressable column — and none of those has a
/// range file to sit on, so the tail goes whole to [`unowned`] rather than one line per axis. A size
/// is materialized per axis only INSIDE the extent, which the sheet's own cell bound already holds.
fn within(
    axis: Axis,
    sizes: &[AxisSize],
    extent: u32,
    sheet: &str,
    warnings: &mut Vec<UnpackWarning>,
) -> BTreeMap<u32, f64> {
    let mut inside = BTreeMap::new();
    let mut beyond = Vec::new();
    for run in cover(sizes) {
        for at in run.first..run.last.saturating_add(1).min(extent) {
            inside.insert(at, run.value);
        }
        if run.last >= extent {
            beyond.push((run.first.max(extent), run.last));
        }
    }
    warnings.extend(unowned(axis, sheet, &beyond));
    inside
}

/// Whether an axis's default reaches THIS cell. A valued cell wears it whatever it says; on the blanks
/// the axis also dresses the answer is the occupancy question itself, since a look that needs a glyph
/// shows on none of them and taking it there gives a blank a look `pack` drops — which is exactly how
/// the two legs come to disagree about what a blank is.
fn stated_on(value: &SourceValue, styles: &StyleTable, index: u32) -> bool {
    *value != SourceValue::Blank || crate::scope_block::paints_blank(styles, index)
}

/// The default style each axis states, resolved INSIDE the extent and nowhere else: the statement
/// dresses the axis's whole length, and materializing that is the occupancy inflation `BlankPaint::of`
/// closed — an unbounded axis becoming content. What is left past the extent still SHOWS only where
/// the source draws on a blank cell, so that is what is named.
fn axis_defaults(
    axis: Axis,
    runs: &[AxisStyle],
    extent: u32,
    styles: &StyleTable,
    sheet: &str,
    warnings: &mut Vec<UnpackWarning>,
) -> BTreeMap<u32, u32> {
    let mut inside = BTreeMap::new();
    for run in cover(runs) {
        for at in run.first..run.last.saturating_add(1).min(extent) {
            inside.insert(at, run.value);
        }
        if styles.draws_on_blank(run.value) {
            warnings.push(UnpackWarning::AxisDefaultStyleClipped {
                sheet: sheet.to_string(),
                axis,
                run: axis_run(axis, run.first, run.last),
            });
        }
    }
    inside
}

/// The runs as an ascending DISJOINT cover, a later statement overriding an earlier one where the two
/// overlap — read backward, so the first writer wins and each axis is decided once. Without it a
/// `<cols>` restating the whole axis 20,000 times costs the axis 20,000 times over, which the extent
/// bounds only when the sheet is narrow: the cover is what makes the work the file's own size.
fn cover<T: Copy>(sizes: &[AxisRun<T>]) -> Vec<AxisRun<T>> {
    let mut taken: BTreeMap<u32, u32> = BTreeMap::new();
    let mut out: Vec<AxisRun<T>> = Vec::new();
    for run in sizes.iter().rev() {
        // Descending by start, and disjoint, so the first that ends before the run ends them all.
        let met: Vec<(u32, u32)> = taken
            .range(..=run.last)
            .rev()
            .take_while(|(_, end)| end.saturating_add(1) >= run.first)
            .map(|(&start, &end)| (start, end))
            .collect();
        let mut at = run.first;
        for &(start, end) in met.iter().rev() {
            if start > at {
                out.push(AxisRun {
                    first: at,
                    last: start - 1,
                    value: run.value,
                });
            }
            at = at.max(end.saturating_add(1));
        }
        if at <= run.last {
            out.push(AxisRun {
                first: at,
                last: run.last,
                value: run.value,
            });
        }
        let first = met
            .last()
            .map_or(run.first, |&(start, _)| start.min(run.first));
        let last = met.first().map_or(run.last, |&(_, end)| end.max(run.last));
        for &(start, _) in &met {
            taken.remove(&start);
        }
        taken.insert(first, last);
    }
    out.sort_unstable_by_key(|run| run.first);
    out
}

fn data_to_value(
    v: &Data,
    sheet: &str,
    col: u32,
    row: u32,
    format: Option<Format>,
) -> Result<SourceValue, IngestError> {
    let at = || format_cell(col, row);
    let base = match v {
        Data::Empty => SourceValue::Blank,
        Data::Int(i) => SourceValue::Number(*i as f64),
        Data::Float(f) => SourceValue::Number(*f),
        Data::String(s) => SourceValue::Text(s.clone()),
        Data::Bool(b) => SourceValue::Bool(*b),
        Data::DateTime(dt) => SourceValue::DateSerial(dt.as_f64()),
        Data::DateTimeIso(s) => {
            SourceValue::DateSerial(iso_datetime_to_serial(s).ok_or_else(|| {
                IngestError::invalid_at_cell(
                    sheet,
                    at(),
                    format!("unparseable ISO date/time {s:?}"),
                )
            })?)
        }
        Data::DurationIso(s) => {
            SourceValue::DateSerial(iso_duration_to_serial(s).ok_or_else(|| {
                IngestError::invalid_at_cell(sheet, at(), format!("unparseable ISO duration {s:?}"))
            })?)
        }
        Data::Error(e) => SourceValue::Error(map_error(e).ok_or_else(|| {
            IngestError::invalid_at_cell(sheet, at(), format!("no FSA1 equivalent for error {e:?}"))
        })?),
    };
    Ok(fuse_format(base, format))
}

/// A value literal keeps its display format only when it is recoverable from the displayed spelling;
/// anything else falls back to the General cell, so a lossy import is byte-for-byte unchanged.
fn fuse_format(base: SourceValue, format: Option<Format>) -> SourceValue {
    let (Some(format), SourceValue::Number(n) | SourceValue::DateSerial(n)) = (format, &base)
    else {
        return base;
    };
    let value = *n;
    match effective_literal_format(format, value) {
        Some(Format::Fixed { decimals: 0 }) => base,
        Some(eff) if is_display_exact(value, eff) => SourceValue::Formatted { value, format: eff },
        _ => base,
    }
}

fn map_error(e: &CellErrorType) -> Option<ErrKind> {
    Some(match e {
        CellErrorType::Div0 => ErrKind::Div0,
        CellErrorType::NA => ErrKind::Na,
        CellErrorType::Name => ErrKind::Name,
        CellErrorType::Null => ErrKind::Null,
        CellErrorType::Num => ErrKind::Num,
        CellErrorType::Ref => ErrKind::Ref,
        CellErrorType::Value => ErrKind::Value,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_scalar_values_onto_the_value_model() {
        let cell = |d: &Data| data_to_value(d, "S", 0, 0, None).unwrap();
        assert_eq!(cell(&Data::Empty), SourceValue::Blank);
        assert_eq!(cell(&Data::Int(42)), SourceValue::Number(42.0));
        assert_eq!(cell(&Data::Float(-3.5)), SourceValue::Number(-3.5));
        assert_eq!(
            cell(&Data::String("hi".into())),
            SourceValue::Text("hi".into())
        );
        assert_eq!(cell(&Data::Bool(true)), SourceValue::Bool(true));
    }

    #[test]
    fn maps_iso_dates_and_durations_to_serials() {
        let cell = |d: &Data| data_to_value(d, "S", 0, 0, None).unwrap();
        assert_eq!(
            cell(&Data::DateTimeIso("2024-01-15".into())),
            SourceValue::DateSerial(45306.0)
        );
        assert_eq!(
            cell(&Data::DurationIso("PT12H".into())),
            SourceValue::DateSerial(0.5)
        );
    }

    #[test]
    fn maps_error_kinds_and_refuses_the_unmappable() {
        assert_eq!(map_error(&CellErrorType::Div0), Some(ErrKind::Div0));
        assert_eq!(map_error(&CellErrorType::NA), Some(ErrKind::Na));
        assert_eq!(map_error(&CellErrorType::Ref), Some(ErrKind::Ref));
        assert_eq!(
            map_error(&CellErrorType::GettingData),
            None,
            "no FSA1 equivalent, so the caller refuses rather than guesses"
        );
        assert_eq!(
            data_to_value(&Data::Error(CellErrorType::Value), "S", 1, 2, None).unwrap(),
            SourceValue::Error(ErrKind::Value)
        );
        let err =
            data_to_value(&Data::Error(CellErrorType::GettingData), "S", 0, 0, None).unwrap_err();
        assert_eq!(err.kind, crate::error::ErrorKind::Invalid);
    }

    #[test]
    fn an_unparseable_iso_date_is_a_located_refusal() {
        let err = data_to_value(&Data::DateTimeIso("nope".into()), "Data", 3, 1, None).unwrap_err();
        assert_eq!(err.kind, crate::error::ErrorKind::Invalid);
        assert_eq!(err.sheet.as_deref(), Some("Data"));
        assert_eq!(err.cell.as_deref(), Some("D2"), "col 3, row 1 is D2");
    }

    #[test]
    fn fuse_format_promotes_display_exact_literals_and_falls_back_otherwise() {
        assert_eq!(
            fuse_format(
                SourceValue::Number(1234.0),
                Some(Format::Grouped { decimals: 2 })
            ),
            SourceValue::Formatted {
                value: 1234.0,
                format: Format::Grouped { decimals: 2 }
            }
        );
        assert_eq!(
            fuse_format(
                SourceValue::DateSerial(44331.0),
                Some(Format::Date(fsa1_model::DatePattern::Mdy))
            ),
            SourceValue::Formatted {
                value: 44331.0,
                format: Format::Date(fsa1_model::DatePattern::Mdy)
            },
            "a date serial is always display-exact"
        );
        assert_eq!(
            fuse_format(
                SourceValue::Number(1234.0),
                Some(Format::Accounting {
                    symbol: fsa1_model::CurrencySymbol::Dollar,
                    decimals: 2
                })
            ),
            SourceValue::Formatted {
                value: 1234.0,
                format: Format::Currency {
                    symbol: fsa1_model::CurrencySymbol::Dollar,
                    grouping: true,
                    decimals: 2
                }
            },
            "a non-negative accounting value remaps to its render-equivalent Currency"
        );
        assert_eq!(
            fuse_format(SourceValue::Number(5.0), None),
            SourceValue::Number(5.0)
        );
        assert_eq!(
            fuse_format(
                SourceValue::Number(1234.5678),
                Some(Format::Fixed { decimals: 2 })
            ),
            SourceValue::Number(1234.5678),
            "sub-display precision falls back to General"
        );
        assert_eq!(
            fuse_format(
                SourceValue::Number(-1234.0),
                Some(Format::Accounting {
                    symbol: fsa1_model::CurrencySymbol::Dollar,
                    decimals: 2
                })
            ),
            SourceValue::Number(-1234.0)
        );
        assert_eq!(
            fuse_format(
                SourceValue::Number(5.0),
                Some(Format::Fixed { decimals: 0 })
            ),
            SourceValue::Number(5.0)
        );
        assert_eq!(
            fuse_format(
                SourceValue::Text("hi".into()),
                Some(Format::Percent { decimals: 2 })
            ),
            SourceValue::Text("hi".into())
        );
    }

    #[test]
    fn an_unsupported_extension_is_a_located_refusal_not_a_format_sniff() {
        // The gate fires before any open, so it does not depend on the file existing.
        let err = read_file(std::path::Path::new("book.csv"), None, &mut Vec::new()).unwrap_err();
        assert_eq!(err.kind, crate::error::ErrorKind::Invalid);
        assert!(err.message.contains(".ods or .xlsx"), "{}", err.message);
        assert_eq!(
            read_file(std::path::Path::new("noext"), None, &mut Vec::new())
                .unwrap_err()
                .kind,
            crate::error::ErrorKind::Invalid,
            "a file with no extension is refused too"
        );
    }

    fn read_with_warnings(name: &str) -> (SourceBook, Vec<UnpackWarning>) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let mut warnings = Vec::new();
        let book = read_file(&path, None, &mut warnings)
            .unwrap_or_else(|e| panic!("{name}: {}", e.message));
        (book, warnings)
    }

    fn read_fixture(name: &str) -> SourceBook {
        read_with_warnings(name).0
    }

    /// B2 holds a fill and NOTHING else, so calamine never yields it: it reaches the sheet only
    /// through the styling pass, and it is what column B's extent rests on.
    #[test]
    fn a_styled_blank_survives_with_the_sheet_geometry_around_it() {
        let (book, warnings) = read_with_warnings("visuals.xlsx");
        let sheet = &book.sheets[0];
        assert_eq!((sheet.rows, sheet.cols), (4, 2), "A4 down, B2 across");

        let b2 = sheet.cell(1, 1).expect("B2 is inside the sheet");
        assert_eq!(b2.value, SourceValue::Blank);
        assert!(b2.style.is_some(), "the blank carries its style index");
        assert!(sheet.is_occupied(1, 1), "a filled blank is content");
        assert!(!sheet.is_occupied(1, 0), "B1 says nothing at all");

        let a1 = sheet.style_at(0, 0).expect("A1 is styled");
        assert!(a1.font.bold);
        assert_eq!(
            a1.fill.fg,
            Some(fsa1_model::Rgb {
                r: 255,
                g: 192,
                b: 0
            })
        );
        assert_eq!(
            sheet.styles.normal_font().name.as_deref(),
            Some("Calibri"),
            "the Normal font is reachable for the write leg to drop"
        );

        // The sheet's content stops at column B, so column C's stated width has nothing to sit on.
        assert_eq!(
            sheet.col_widths.get(&2),
            None,
            "column C is past the content"
        );
        assert_eq!(
            warnings.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec![
                "column width for C on sheet Visual dropped: no range file covers column C"
                    .to_string()
            ],
            "and it is named rather than dropped in silence",
        );
        assert_eq!(sheet.row_heights.get(&2), Some(&22.5), "row 3, verbatim");
        assert_eq!(sheet.merges.len(), 1, "{:?}", sheet.merges);
    }

    /// The occupancy guard. A `<col style>` dresses the column all the way down, so materializing it
    /// would make an unbounded axis content — the inflation `BlankPaint::of` closed, arriving through
    /// a second door. Resolving it inside the extent alone mints no coordinate the export cannot carry
    /// back, and what still SHOWS past it is `draws_on_blank`'s question, already asked and now named.
    #[test]
    fn an_axis_default_is_resolved_inside_the_extent_and_only_a_drawn_one_is_named_past_it() {
        let painted = crate::xlsx_style::XlsxStyle {
            fill: crate::xlsx_style::XlsxFill {
                pattern: crate::xlsx_style::FillPattern::Solid,
                fg: Some(fsa1_model::Rgb { r: 0, g: 0, b: 0 }),
                bg: None,
            },
            ..Default::default()
        };
        let bold = crate::xlsx_style::XlsxStyle {
            font: crate::xlsx_style::XlsxFont {
                bold: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let styles = StyleTable::of(
            vec![Default::default(), painted, bold],
            crate::xlsx_style::XlsxFont::default(),
        );
        let blanket = |value| {
            [AxisRun {
                first: 0,
                last: 16_383,
                value,
            }]
        };

        let mut warnings = Vec::new();
        let painting = axis_defaults(Axis::Column, &blanket(1), 3, &styles, "S", &mut warnings);
        assert_eq!(
            painting.into_iter().collect::<Vec<_>>(),
            vec![(0, 1), (1, 1), (2, 1)],
            "16,384 columns are stated and only the sheet's own three are resolved",
        );
        assert_eq!(
            warnings.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec![
                "the fill or border column A:XFD states on sheet S is dropped past the sheet's \
                 extent: no range file covers a cell there"
                    .to_string()
            ],
        );

        let mut quiet = Vec::new();
        let facing = axis_defaults(Axis::Column, &blanket(2), 3, &styles, "S", &mut quiet);
        assert_eq!(facing.len(), 3);
        assert!(
            quiet.is_empty(),
            "a typeface needs a glyph, so a bold column shows nothing on the blanks past the extent",
        );

        let mut empty = Vec::new();
        assert!(
            axis_defaults(Axis::Row, &blanket(1), 0, &styles, "S", &mut empty).is_empty(),
            "a sheet with no extent resolves none of it",
        );
        assert_eq!(empty.len(), 1, "and the whole statement is the loss");
    }

    /// A `<cols>` may state runs that overlap, and the one the sheet shows is the LAST of them —
    /// which is the whole reason the runs are covered before any column is materialized. The cover's
    /// own pieces are then what the tail is folded from, so two of them meeting past the extent are
    /// one loss exactly as two `<col>` runs meeting there are.
    #[test]
    fn overlapping_col_runs_resolve_to_the_last_stated_and_their_tail_to_one_run() {
        let run = |first, last, value| AxisSize { first, last, value };
        let stated = [run(0, 5, 10.0), run(2, 3, 20.0), run(4, 9, 30.0)];

        let mut warnings = Vec::new();
        let widths = within(Axis::Column, &stated, 4, "S", &mut warnings);
        assert_eq!(
            widths.into_iter().collect::<Vec<_>>(),
            vec![(0, 10.0), (1, 10.0), (2, 20.0), (3, 20.0)],
            "columns C and D were restated, and the restatement is what they wear",
        );
        assert_eq!(
            warnings.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec![
                "column width for E:J on sheet S dropped: no range file covers column E:J"
                    .to_string()
            ],
        );

        let mut narrow = Vec::new();
        within(Axis::Column, &stated, 2, "S", &mut narrow);
        assert_eq!(
            narrow.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec![
                "column width for C:J on sheet S dropped: no range file covers column C:J"
                    .to_string()
            ],
            "two cover pieces that meet past the extent are ONE loss",
        );
    }

    /// .ods imports its values and formulas and no appearance at all, which is a stated scope, not a
    /// gap the sheet lies about: its style table is empty rather than absent.
    #[test]
    fn the_ods_path_carries_values_and_no_styling() {
        let book = read_fixture("smoke.ods");
        let sheet = &book.sheets[0];
        assert_eq!(sheet.cells[0].value, SourceValue::Number(10.0));
        assert_eq!(sheet.cells[0].style, None);
        assert!(sheet.styles.is_empty());
        assert!(sheet.col_widths.is_empty() && sheet.row_heights.is_empty());
        assert!(sheet.merges.is_empty());
        assert!(!sheet.is_occupied(1, 1), "no style makes a blank content");
    }

    #[test]
    fn an_unmapped_table_is_dropped_with_a_located_warning() {
        // An xlsx where calamine and xlsx_meta disagree on a table name is impractical to author.
        let table = |name: &str| RawTable {
            name: name.to_string(),
            ref_str: "A1:B4".to_string(),
            header_rows: 1,
            totals_rows: 0,
            columns: vec!["Region".to_string(), "Q1".to_string()],
        };
        let mut w = Vec::new();
        let res = resolve_tables(vec![table("Sales")], &HashMap::new(), true, &mut w);
        assert!(!res.is_table("Sales"), "an unmapped table is not resolved");
        assert_eq!(w.len(), 1);
        assert_eq!(
            w[0],
            UnpackWarning::TableDropped {
                table: "Sales".to_string(),
                reason:
                    "could not map to a sheet (displayName/sheet divergence); structured refs load as #NAME?"
                        .to_string(),
            }
        );

        let mut w2 = Vec::new();
        resolve_tables(vec![table("Sales")], &HashMap::new(), false, &mut w2);
        assert!(
            matches!(
                &w2[0],
                UnpackWarning::TableDropped { table, reason }
                    if table == "Sales" && reason.contains("table index could not be read")
            ),
            "a load_tables failure selects the index-unreadable reason"
        );

        let mut sheet = HashMap::new();
        sheet.insert("Sales".to_string(), "Data".to_string());
        let mut w3 = Vec::new();
        let res = resolve_tables(vec![table("Sales")], &sheet, true, &mut w3);
        assert!(w3.is_empty(), "a mapped table drops nothing");
        assert!(res.is_table("Sales"));
    }
}
