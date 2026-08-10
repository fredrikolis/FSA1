// Concern: emits the assembled parts onto a dest it reserves, replaces or refuses | Non-concern: a part's bytes, the CLI envelope | IO: (Workbook, Overlay, charts, dest, force) -> .xlsx or ExportError

use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use fsa1_model::{Overlay, Workbook};

use crate::chart::{self, Chart};
use crate::package::{self, Part};
use crate::shared_strings::SharedStrings;
use crate::{content_types, doc_props, rels, styles, theme, workbook, worksheet};

/// Why [`crate::write_xlsx`] refused, or failed, to produce the `.xlsx`.
#[derive(Debug)]
pub enum ExportError {
    DestExists(PathBuf),
    DestIsDir(PathBuf),
    Io(std::io::Error),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::DestExists(p) => {
                write!(
                    f,
                    "export destination {:?} already exists; pass --force to overwrite it",
                    p.display()
                )
            }
            // No flag makes a rename replace a directory, so the sibling arm's remedy is not repeated.
            ExportError::DestIsDir(p) => {
                write!(
                    f,
                    "export destination {:?} is a directory; name a file to write instead \
                     (pack replaces a file, never a directory)",
                    p.display()
                )
            }
            ExportError::Io(e) => write!(f, "export I/O failure: {e}"),
        }
    }
}

impl std::error::Error for ExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ExportError::Io(e) => Some(e),
            ExportError::DestExists(_) | ExportError::DestIsDir(_) => None,
        }
    }
}

/// A rename onto a directory fails on every platform this crate targets, so a directory `dest` is
/// refused by name rather than as a raw rename fault — `force` names files. Unforced, `dest` is not
/// tested but RESERVED, by the one syscall that refuses and claims together: nothing takes the name
/// mid-emit, and a symlink is refused, not followed. Forced, the rename replaces what stands there.
pub(crate) fn run(
    workbook: &Workbook,
    overlay: &Overlay,
    charts: &[Chart],
    dest: &Path,
    force: bool,
) -> Result<(), ExportError> {
    if dest.is_dir() {
        return Err(ExportError::DestIsDir(dest.to_path_buf()));
    }
    let temp = temp_sibling(dest)?;
    let reserved = if force {
        false
    } else {
        match File::create_new(dest) {
            Ok(file) => {
                // Windows refuses to rename onto a path a handle is still open on.
                drop(file);
                true
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(ExportError::DestExists(dest.to_path_buf()));
            }
            Err(e) => return Err(ExportError::Io(e)),
        }
    };
    let file = match File::create_new(&temp).map_err(|e| temp_error(&temp, e)) {
        Ok(file) => file,
        // The temp is NOT removed here — this call never created it, and it belongs to whoever did.
        Err(e) => {
            if reserved {
                let _ = fs::remove_file(dest);
            }
            return Err(e);
        }
    };
    let parts = build_parts(workbook, overlay, charts);
    let outcome = package::write_package(&parts, file)
        .map_err(|e| match e {
            zip::result::ZipError::Io(io) => ExportError::Io(io),
            other => ExportError::Io(std::io::Error::other(other.to_string())),
        })
        // Windows refuses to rename a file that still has an open handle, so the sink closes first.
        .and_then(|sink| {
            drop(sink);
            fs::rename(&temp, dest).map_err(ExportError::Io)
        });
    if outcome.is_err() {
        let _ = fs::remove_file(&temp);
        if reserved {
            let _ = fs::remove_file(dest);
        }
    }
    outcome
}

/// `std` attaches no path to an open failure, so the one name the caller cannot deduce — a hidden
/// sibling — is put into the message, with the remedy for the one failure that outlives a run: a
/// pack killed mid-emit leaves the temp, and the next pack drawing that pid trips over it.
fn temp_error(temp: &Path, e: std::io::Error) -> ExportError {
    let remedy = if e.kind() == std::io::ErrorKind::AlreadyExists {
        " — a leftover from an interrupted pack; delete it and pack again"
    } else {
        ""
    };
    ExportError::Io(std::io::Error::new(
        e.kind(),
        format!("temp file {:?}: {e}{remedy}", temp.display()),
    ))
}

/// A sibling of `dest`, so the rename onto it stays within one filesystem and is therefore atomic.
/// The pid separates two processes without a clock or a random source; two packs to one `dest`
/// inside ONE process spell the same name, where the second `create_new` refuses.
fn temp_sibling(dest: &Path) -> Result<PathBuf, ExportError> {
    let Some(name) = dest.file_name() else {
        return Err(ExportError::Io(std::io::Error::other(format!(
            "export destination {:?} names no file to write",
            dest.display()
        ))));
    };
    let mut temp = OsString::from(".");
    temp.push(name);
    temp.push(format!(".fsa1-tmp.{}", std::process::id()));
    Ok(dest.with_file_name(temp))
}

fn build_parts(workbook: &Workbook, overlay: &Overlay, charts: &[Chart]) -> Vec<Part> {
    let names = workbook.sheet_names();
    let sheet_count = names.len();

    let style_table = styles::build(workbook, overlay);
    let drawings = drawings(charts, sheet_count);

    let mut ss = SharedStrings::new();
    let mut worksheet_parts = Vec::with_capacity(sheet_count);
    for i in 0..sheet_count {
        let drawn = drawings.iter().any(|d| d.sheet == i as u32);
        let bytes = worksheet::emit(workbook, overlay, i as u32, &mut ss, &style_table, drawn);
        worksheet_parts.push(Part::new(
            format!("xl/worksheets/sheet{}.xml", i + 1),
            bytes,
        ));
    }

    let mut parts = vec![
        Part::new(
            "[Content_Types].xml",
            content_types::emit(sheet_count, charts.len(), drawings.len()),
        ),
        Part::new("_rels/.rels", rels::emit_package_rels()),
        Part::new(
            "xl/workbook.xml",
            workbook::emit(&names, workbook.name_table().names()),
        ),
        Part::new(
            "xl/_rels/workbook.xml.rels",
            rels::emit_workbook_rels(sheet_count),
        ),
    ];
    parts.extend(worksheet_parts);
    parts.extend([
        Part::new("xl/sharedStrings.xml", ss.emit()),
        Part::new("xl/styles.xml", style_table.bytes),
        Part::new("xl/theme/theme1.xml", theme::emit()),
        Part::new("docProps/core.xml", doc_props::emit_core()),
        Part::new("docProps/app.xml", doc_props::emit_app()),
    ]);
    parts.extend(chart_parts(workbook, charts, &drawings));
    parts
}

/// One drawing per sheet that has a chart, holding the 1-based chart numbers it anchors in the order
/// they were given — which is the order the anchors and the drawing's own `rIdN` follow.
struct Drawing {
    sheet: u32,
    charts: Vec<usize>,
}

fn drawings(charts: &[Chart], sheet_count: usize) -> Vec<Drawing> {
    (0..sheet_count as u32)
        .filter_map(|sheet| {
            let on_sheet: Vec<usize> = charts
                .iter()
                .enumerate()
                .filter(|(_, c)| c.sheet() == sheet)
                .map(|(at, _)| at + 1)
                .collect();
            (!on_sheet.is_empty()).then_some(Drawing {
                sheet,
                charts: on_sheet,
            })
        })
        .collect()
}

/// The chart parts, their drawings, and the two `_rels` that wire a sheet to a drawing and a drawing
/// to its charts. A workbook stating no figure emits none of them and packs exactly as it did.
fn chart_parts(workbook: &Workbook, charts: &[Chart], drawings: &[Drawing]) -> Vec<Part> {
    let mut parts = Vec::new();
    for (at, chart) in charts.iter().enumerate() {
        parts.push(Part::new(
            format!("xl/charts/chart{}.xml", at + 1),
            chart::emit_chart(chart),
        ));
    }
    for (at, drawing) in drawings.iter().enumerate() {
        let number = at + 1;
        // `drawing.charts` holds 1-based chart numbers in the order the anchors follow, which is the order this slice is indexed in.
        let placements: Vec<Option<fsa1_model::Placement>> = drawing
            .charts
            .iter()
            .map(|n| charts[n - 1].placement)
            .collect();
        parts.push(Part::new(
            format!("xl/drawings/drawing{number}.xml"),
            chart::emit_drawing(
                &placements,
                chart::anchor_column(workbook.content_region(drawing.sheet)),
            ),
        ));
        parts.push(Part::new(
            format!("xl/drawings/_rels/drawing{number}.xml.rels"),
            rels::emit_drawing_rels(&drawing.charts),
        ));
        parts.push(Part::new(
            format!("xl/worksheets/_rels/sheet{}.xml.rels", drawing.sheet + 1),
            rels::emit_sheet_rels(number),
        ));
    }
    parts
}
