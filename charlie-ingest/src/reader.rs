// Concern: the FORMAT FIREWALL — the ONE module that touches calamine — reading a real `.ods` OR `.xlsx` workbook into the format-neutral `SourceBook`: DISPATCH by extension (a `.ods`/`.xlsx` opened via calamine's `open_workbook_auto` behind ONE code path; any other extension is a located CORE2 refusal), and for each sheet fuse calamine's cached VALUE range (`worksheet_range`) with its FORMULA range (`worksheet_formula`) into an A1-anchored, row-major rectangle of `SourceCell`s — a formula cell keeps its raw source-dialect text for `translate` (ODS `of:=[.A1]`, or xlsx's already-Excel-A1 `A1`), a value cell maps calamine's `Data` onto charlie's VAL3 value model, a date/time cell becoming an Excel serial via `dates`; every failure (unsupported extension, unopenable file, unreadable sheet, unrepresentable error kind, unparseable date, an oversized used range) is a located `IngestError` (CORE2), never a panic | Non-concern: translating the formula grammar (translate.rs — one translator serves BOTH dialects, so the reader is format-specific only in its OPENER), spelling a cell to TSV (serialize.rs), and writing files (lib.rs); calamine/zip/xml stay behind this seam so charlie-model/ast never see them | IO: (a `.ods`/`.xlsx` path) -> `Result<SourceBook, IngestError>`
//! The calamine-backed reader: [`read_file`], opening `.ods` and `.xlsx` behind one code path
//! (`open_workbook_auto`). calamine is confined here (HARD firewall) — the rest of the crate, and all
//! of charlie-model/ast, work only on the neutral [`SourceBook`]. The reader is format-specific ONLY in
//! its opener + extension gate; sheet-fusing, value-mapping, and formula-carrying are format-blind, and
//! `translate` rewrites both dialects (an xlsx formula is already Excel-A1, so translation is a noop
//! beyond prepending `=`; an ODS formula is rewritten from OpenFormula).

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use calamine::{CellErrorType, Data, Reader, Sheets, open_workbook_auto};
use charlie_ast::ErrKind;

use crate::dates::{iso_datetime_to_serial, iso_duration_to_serial};
use crate::error::{ErrorKind, IngestError};
use crate::source::{SheetSource, SourceBook, SourceCell};

/// The largest used-range area (in cells) the reader will materialize before refusing. A real sheet's
/// used range is far below this; a pathological one becomes a located refusal, never an OOM abort.
const MAX_SHEET_CELLS: u64 = 4_000_000;

/// Read a `.ods` or `.xlsx` file into the neutral [`SourceBook`], dispatching by extension. An
/// unsupported extension, a missing/unreadable file, or an unreadable sheet is a located
/// [`IngestError`]; a structurally-fine cell that cannot map to charlie's model (an unknown error kind,
/// an unparseable date) is likewise located at its `sheet!A1`.
pub fn read_file(path: &Path) -> Result<SourceBook, IngestError> {
    // Dispatch by extension: only `.ods`/`.xlsx` are supported. An unknown extension is a located
    // CORE2 refusal here rather than letting `open_workbook_auto` format-sniff (which would try every
    // format and mislabel the failure); the message names the offending path + extension.
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
    // ONE opener for both formats: `open_workbook_auto` picks Ods/Xlsx by extension and hands back a
    // `Sheets` that implements the same `Reader` trait, so every step below is format-blind.
    let mut wb = open_workbook_auto(path).map_err(|e| {
        IngestError::io(
            ErrorKind::SourceIo,
            format!("cannot open {:?} as a spreadsheet: {e}", path.display()),
        )
    })?;

    let names = wb.sheet_names().to_vec();
    let mut sheets = Vec::with_capacity(names.len());
    for name in names {
        sheets.push(read_sheet(&mut wb, &name)?);
    }
    Ok(SourceBook { sheets })
}

/// Read one sheet: fuse its value and formula ranges into an A1-anchored rectangle. Format-blind — it
/// works through the `Reader` trait, so the same code serves ODS and xlsx (and any calamine format).
fn read_sheet(wb: &mut Sheets<BufReader<File>>, name: &str) -> Result<SheetSource, IngestError> {
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

    #[test]
    fn an_unsupported_extension_is_a_located_refusal_not_a_format_sniff() {
        // The extension gate fires before any file open, so it does not depend on the file existing.
        let err = read_file(std::path::Path::new("book.csv")).unwrap_err();
        assert_eq!(err.kind, crate::error::ErrorKind::Invalid);
        assert!(err.message.contains(".ods or .xlsx"), "{}", err.message);
        // A file with no extension is likewise refused (never format-sniffed).
        assert_eq!(
            read_file(std::path::Path::new("noext")).unwrap_err().kind,
            crate::error::ErrorKind::Invalid
        );
    }
}
