// Concern: the FORMAT FIREWALL — the ONE module that touches calamine — reading a real `.ods` workbook into the format-neutral `SourceBook`: open the file, and for each sheet fuse calamine's cached VALUE range (`worksheet_range`) with its FORMULA range (`worksheet_formula`) into an A1-anchored, row-major rectangle of `SourceCell`s (a formula cell keeps its raw OpenFormula text for `translate`; a value cell maps calamine's `Data` onto charlie's VAL3 value model, a date/time cell becoming an Excel serial via `dates`); every failure (unopenable file, unreadable sheet, unrepresentable error kind, unparseable date, an oversized used range) is a located `IngestError` (CORE2), never a panic | Non-concern: translating the formula grammar (translate.rs), spelling a cell to TSV (serialize.rs), and writing files (lib.rs); calamine/zip/xml stay behind this seam so charlie-model/ast never see them | IO: (a `.ods` path) -> `Result<SourceBook, IngestError>`
//! The calamine-backed ODS reader: [`read_ods`]. calamine is confined here (HARD firewall) — the rest of
//! the crate, and all of charlie-model/ast, work only on the neutral [`SourceBook`].

use std::path::Path;

use calamine::{CellErrorType, Data, Ods, Reader};
use charlie_ast::ErrKind;

use crate::dates::{iso_datetime_to_serial, iso_duration_to_serial};
use crate::error::{ErrorKind, IngestError};
use crate::source::{SheetSource, SourceBook, SourceCell};

/// The largest used-range area (in cells) the reader will materialize before refusing. A real sheet's
/// used range is far below this; a pathological one becomes a located refusal, never an OOM abort.
const MAX_SHEET_CELLS: u64 = 4_000_000;

/// Read a `.ods` file into the neutral [`SourceBook`]. A missing/unreadable file or an unreadable sheet
/// is a located [`IngestError`]; a structurally-fine cell that cannot map to charlie's model (an
/// unknown error kind, an unparseable date) is likewise located at its `sheet!A1`.
pub fn read_ods(path: &Path) -> Result<SourceBook, IngestError> {
    if !path.exists() {
        return Err(IngestError::io(
            ErrorKind::SourceNotFound,
            format!("no such file {:?}", path.display()),
        ));
    }
    let mut wb: Ods<_> = calamine::open_workbook(path).map_err(|e| {
        IngestError::io(
            ErrorKind::SourceIo,
            format!(
                "cannot open {:?} as an OpenDocument spreadsheet: {e}",
                path.display()
            ),
        )
    })?;

    let names = wb.sheet_names().to_vec();
    let mut sheets = Vec::with_capacity(names.len());
    for name in names {
        sheets.push(read_sheet(&mut wb, &name)?);
    }
    Ok(SourceBook { sheets })
}

/// Read one sheet: fuse its value and formula ranges into an A1-anchored rectangle.
fn read_sheet(
    wb: &mut Ods<impl std::io::Read + std::io::Seek>,
    name: &str,
) -> Result<SheetSource, IngestError> {
    let values = wb
        .worksheet_range(name)
        .map_err(|e| IngestError::at_sheet(name, format!("cannot read sheet values: {e}")))?;
    let formulas = wb
        .worksheet_formula(name)
        .map_err(|e| IngestError::at_sheet(name, format!("cannot read sheet formulas: {e}")))?;

    // The used rectangle spans A1..=the furthest non-empty cell of EITHER range (a formula cell always
    // has a cached value too, but taking the max is robust). `end()` is the absolute bottom-right.
    let end = match (values.end(), formulas.end()) {
        (None, None) => {
            return Ok(SheetSource {
                name: name.to_string(),
                rows: 0,
                cols: 0,
                cells: Vec::new(),
            });
        }
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (Some(a), Some(b)) => (a.0.max(b.0), a.1.max(b.1)),
    };
    let rows = end.0 + 1;
    let cols = end.1 + 1;
    if u64::from(rows) * u64::from(cols) > MAX_SHEET_CELLS {
        return Err(IngestError::at_sheet(
            name,
            format!("used range {rows}x{cols} exceeds the {MAX_SHEET_CELLS}-cell import bound"),
        ));
    }

    let mut cells = Vec::with_capacity((rows as usize) * (cols as usize));
    for row in 0..rows {
        for col in 0..cols {
            // A non-empty formula string (calamine returns "" for a non-formula cell) makes this a
            // formula cell; otherwise map the cached value. `get_value` takes ABSOLUTE coordinates.
            let cell = match formulas.get_value((row, col)) {
                Some(f) if !f.is_empty() => SourceCell::Formula(f.clone()),
                _ => match values.get_value((row, col)) {
                    Some(v) => data_to_cell(v, name, col, row)?,
                    None => SourceCell::Blank,
                },
            };
            cells.push(cell);
        }
    }
    Ok(SheetSource {
        name: name.to_string(),
        rows,
        cols,
        cells,
    })
}

/// Map one calamine cached [`Data`] value onto charlie's VAL3 value model. A date/time is converted to
/// an Excel serial (`dates`); an unknown error kind or an unparseable date is a located refusal.
fn data_to_cell(v: &Data, sheet: &str, col: u32, row: u32) -> Result<SourceCell, IngestError> {
    let at = || charlie_ast::a1::format_cell(col, row);
    Ok(match v {
        Data::Empty => SourceCell::Blank,
        Data::Int(i) => SourceCell::Number(*i as f64),
        Data::Float(f) => SourceCell::Number(*f),
        Data::String(s) => SourceCell::Text(s.clone()),
        Data::Bool(b) => SourceCell::Bool(*b),
        Data::DateTime(dt) => SourceCell::DateSerial(dt.as_f64()),
        Data::DateTimeIso(s) => {
            SourceCell::DateSerial(iso_datetime_to_serial(s).ok_or_else(|| {
                IngestError::at_cell(sheet, at(), format!("unparseable ISO date/time {s:?}"))
            })?)
        }
        Data::DurationIso(s) => {
            SourceCell::DateSerial(iso_duration_to_serial(s).ok_or_else(|| {
                IngestError::at_cell(sheet, at(), format!("unparseable ISO duration {s:?}"))
            })?)
        }
        Data::Error(e) => SourceCell::Error(map_error(e).ok_or_else(|| {
            IngestError::at_cell(
                sheet,
                at(),
                format!("no charlie equivalent for error {e:?}"),
            )
        })?),
    })
}

/// Map a calamine [`CellErrorType`] to charlie's [`ErrKind`]. `GettingData` (an async data-load stub)
/// has no charlie value-model equivalent, so it is `None` (a located refusal, not a guessed error).
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
        let cell = |d: &Data| data_to_cell(d, "S", 0, 0).unwrap();
        assert_eq!(cell(&Data::Empty), SourceCell::Blank);
        assert_eq!(cell(&Data::Int(42)), SourceCell::Number(42.0));
        assert_eq!(cell(&Data::Float(-3.5)), SourceCell::Number(-3.5));
        assert_eq!(
            cell(&Data::String("hi".into())),
            SourceCell::Text("hi".into())
        );
        assert_eq!(cell(&Data::Bool(true)), SourceCell::Bool(true));
    }

    #[test]
    fn maps_iso_dates_and_durations_to_serials() {
        let cell = |d: &Data| data_to_cell(d, "S", 0, 0).unwrap();
        assert_eq!(
            cell(&Data::DateTimeIso("2024-01-15".into())),
            SourceCell::DateSerial(45306.0)
        );
        assert_eq!(
            cell(&Data::DurationIso("PT12H".into())),
            SourceCell::DateSerial(0.5)
        );
    }

    #[test]
    fn maps_error_kinds_and_refuses_the_unmappable() {
        assert_eq!(map_error(&CellErrorType::Div0), Some(ErrKind::Div0));
        assert_eq!(map_error(&CellErrorType::NA), Some(ErrKind::Na));
        assert_eq!(map_error(&CellErrorType::Ref), Some(ErrKind::Ref));
        // No charlie equivalent -> None (the caller makes it a located refusal, not a guess).
        assert_eq!(map_error(&CellErrorType::GettingData), None);
        assert_eq!(
            data_to_cell(&Data::Error(CellErrorType::Value), "S", 1, 2).unwrap(),
            SourceCell::Error(ErrKind::Value)
        );
        // The unmappable error is a located refusal at its source cell.
        let err = data_to_cell(&Data::Error(CellErrorType::GettingData), "S", 0, 0).unwrap_err();
        assert_eq!(err.kind, crate::error::ErrorKind::Invalid);
    }

    #[test]
    fn an_unparseable_iso_date_is_a_located_refusal() {
        let err = data_to_cell(&Data::DateTimeIso("nope".into()), "Data", 3, 1).unwrap_err();
        assert_eq!(err.kind, crate::error::ErrorKind::Invalid);
        assert_eq!(err.sheet.as_deref(), Some("Data"));
        assert_eq!(err.cell.as_deref(), Some("D2")); // col 3, row 1 -> D2
    }
}
