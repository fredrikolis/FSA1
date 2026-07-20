// Concern: the FORMAT FIREWALL — the ONE module that touches calamine — reading a real `.ods` OR `.xlsx` workbook into the format-neutral `SourceBook`: DISPATCH by extension (a `.ods`/`.xlsx` opened via calamine's `open_workbook_auto` behind ONE code path; any other extension is a located CORE2 refusal), and for each sheet fuse calamine's cached VALUE range (`worksheet_range`) with its FORMULA range (`worksheet_formula`) into an A1-anchored, row-major rectangle of `SourceCell`s — a formula cell keeps its raw source-dialect text for `translate` (ODS `of:=[.A1]`, or xlsx's already-Excel-A1 `A1`), a value cell maps calamine's `Data` onto charlie's VAL3 value model, a date/time cell becoming an Excel serial via `dates`; and (for xlsx) BUILD the workbook reference `Resolution` — fusing each table's SHEET from calamine (which navigates the sheet rels) with the scoped `definedName`s + full table `ref`/header/totals/columns from the xlsx parts (`xlsx_meta`, the sibling zip/xml firewall module) so `translate` can resolve names/tables to A1 (HARD RULE 4); every failure (unsupported extension, unopenable file, unreadable sheet, unrepresentable error kind, unparseable date, an oversized used range, unreadable metadata) is a located `IngestError` (CORE2), never a panic | Non-concern: translating the formula grammar (translate.rs — one translator serves BOTH dialects, so the reader is format-specific only in its OPENER), the name/table resolution LOGIC (resolve.rs), reading the name/table metadata parts (xlsx_meta.rs owns zip/quick-xml), spelling a cell to TSV (serialize.rs), and writing files (lib.rs); calamine stays behind this seam (zip/xml behind xlsx_meta's) so charlie-model/ast never see them | IO: (a `.ods`/`.xlsx` path) -> `Result<SourceBook, IngestError>`
//! The calamine-backed reader: [`read_file`], opening `.ods` and `.xlsx` behind one code path
//! (`open_workbook_auto`). calamine is confined here (HARD firewall) — the rest of the crate, and all
//! of charlie-model/ast, work only on the neutral [`SourceBook`]. The reader is format-specific ONLY in
//! its opener + extension gate; sheet-fusing, value-mapping, and formula-carrying are format-blind, and
//! `translate` rewrites both dialects (an xlsx formula is already Excel-A1, so translation is a noop
//! beyond prepending `=`; an ODS formula is rewritten from OpenFormula).

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use calamine::{CellErrorType, Data, Reader, Sheets, open_workbook_auto};
use charlie_ast::ErrKind;

use crate::dates::{iso_datetime_to_serial, iso_duration_to_serial};
use crate::error::{ErrorKind, IngestError};
use crate::names::DefinedName;
use crate::resolve::Resolution;
use crate::source::{SheetSource, SourceBook, SourceCell};
use crate::xlsx_meta;

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
    for name in &names {
        sheets.push(read_sheet(&mut wb, name)?);
    }
    // Build the TABLE resolution (resolved INLINE by `translate`) and collect the workbook's DEFINED
    // NAMES (emitted as on-disk FS4 entries and resolved at LOAD, HARD RULE 2) so the engine only ever
    // sees A1 (HARD RULE 4).
    let (resolution, defined_names) = build_resolution(path, &mut wb, &names)?;
    Ok(SourceBook {
        sheets,
        resolution,
        names: defined_names,
    })
}

/// Build the workbook's [`Resolution`] from the source. For an xlsx this fuses two sources kept behind
/// the format firewall: calamine gives each table's SHEET (it already navigates the sheet rels), and the
/// xlsx parts ([`xlsx_meta`]) give the scoped `definedName`s and each table's full `ref`/header/totals/
/// columns — the extents calamine's high-level seam does not surface. For any other format the result is
/// empty (ODS defined names are left unresolved — they load as `#NAME?` unchanged, never silently wrong).
fn build_resolution(
    path: &Path,
    wb: &mut Sheets<BufReader<File>>,
    sheet_order: &[String],
) -> Result<(Resolution, Vec<DefinedName>), IngestError> {
    let Sheets::Xlsx(x) = wb else {
        return Ok((Resolution::empty(), Vec::new()));
    };
    // table displayName -> its sheet (via calamine). `load_tables` populates a possibly-empty table set;
    // if it errors we simply resolve no tables (their structured refs then load as located #NAME?).
    //
    // Degradation policy (deliberate, HARD RULE 5): a table we cannot MAP to a sheet is dropped, not a
    // refusal — its structured refs stay verbatim and load as a located `#NAME?`, so the import always
    // succeeds (GRID6) and no ref is ever silently wrong. This is the two soft spots below: (1) a
    // `load_tables` error leaves `table_sheet` empty (all tables drop), and (2) a table in `meta.tables`
    // whose xlsx displayName has no key in `table_sheet` is skipped by the `get` at the loop below (a
    // displayName/`table_names_in_sheet` spelling divergence). This is intentionally SOFTER than
    // `xlsx_meta::read_meta(path)?` refusing a malformed metadata PART (a CORE2 structural failure):
    // there the bytes are corrupt, here a lookup simply misses. In practice calamine and `xlsx_meta`
    // read the same displayName, so a divergence would indicate a calamine-vs-part inconsistency rather
    // than a real workbook defect — a located `#NAME?` (visible in `--functions` / `check`) is the
    // right, non-aborting signal for it.
    let mut table_sheet: HashMap<String, String> = HashMap::new();
    if x.load_tables().is_ok() {
        for s in sheet_order {
            for t in x.table_names_in_sheet(s) {
                table_sheet.insert(t.clone(), s.clone());
            }
        }
    }

    let meta = xlsx_meta::read_meta(path)?;
    let mut res = Resolution::empty();
    let mut defined_names = Vec::new();
    for n in meta.names {
        // localSheetId is a 0-based index into the workbook's sheet order. NOTE: `sheet_order` is
        // calamine's `sheet_names()`; this assumes it matches workbook.xml's 0-based sheet indexing. For
        // the common all-worksheet workbook it does. Were calamine ever to omit or reorder sheet types
        // (e.g. chart sheets), a sheet-local name could be attributed to the wrong sheet's scope — a
        // wrong scope only ever narrows/mis-shadows a lookup, so a mis-scoped name's entry lands in the
        // wrong folder and its refs load as #NAME? (HARD RULE 5), never a silently-wrong target.
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
    for t in meta.tables {
        if let Some(sheet) = table_sheet.get(&t.name) {
            res.add_table(
                &t.name,
                sheet,
                t.columns,
                &t.ref_str,
                t.header_rows,
                t.totals_rows,
            );
        }
    }
    Ok((res, defined_names))
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

    // GRID5 SPILL-REGION INGEST IS DEFERRED (to the real-file-corpus milestone), by design.
    // A dynamic-array formula (`=SORT(A1:A3)`) in a real workbook lives in ONE anchor cell (xlsx
    // `<f t="array" ref="C1:C3">`) and SPILLS cached values into the rest of its `ref` range. Mapping
    // that onto a charlie GRID5 range file needs the spill's `ref` extent — but calamine 0.36's
    // high-level `Reader::worksheet_formula` seam this crate uses returns only the anchor's formula
    // TEXT and does NOT surface the array `ref` (its formula metadata distinguishes only Normal /
    // Shared / SharedDerived formulas — legacy drag-fill — with no dynamic-array/spill variant and no
    // spill-range accessor). So the spill's dimensions cannot be cleanly recovered here: the anchor
    // reads as a normal single-cell formula and each spilled coordinate reads as a bare cached VALUE
    // (a per-cell literal), which round-trips as a faithful (if de-spilled) snapshot — never wrong,
    // just not reconstituted as one array-formula region. Forcing a guess (e.g. inferring a `ref` from
    // adjacent cached values) would be unsound, so it is deliberately NOT attempted; real-workbook
    // spill fidelity waits for the real-file-corpus milestone (and a calamine API that exposes the
    // array `ref`, or a direct xlsx-XML read behind this same firewall).
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
