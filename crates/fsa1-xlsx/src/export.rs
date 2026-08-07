// Concern: drives the part emitters into a fresh dest, refusing an occupied one | Non-concern: any part's bytes, the CLI envelope | IO: (a Workbook, an Overlay, a dest) -> .xlsx or ExportError

use std::fmt;
use std::fs::File;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use fsa1_model::{Overlay, Workbook};

use crate::package::{self, Part};
use crate::shared_strings::SharedStrings;
use crate::{content_types, doc_props, rels, styles, theme, workbook, worksheet};

/// Why [`crate::write_xlsx`] refused, or failed, to produce the `.xlsx`.
#[derive(Debug)]
pub enum ExportError {
    DestExists(PathBuf),
    Io(std::io::Error),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::DestExists(p) => {
                write!(
                    f,
                    "export destination {:?} already exists (CORE3: a materialized artifact must land in a not-already-occupied location)",
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
            ExportError::DestExists(_) => None,
        }
    }
}

pub(crate) fn run(workbook: &Workbook, overlay: &Overlay, dest: &Path) -> Result<(), ExportError> {
    let parts = build_parts(workbook, overlay);
    let file = match File::create_new(dest) {
        Ok(f) => f,
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            return Err(ExportError::DestExists(dest.to_path_buf()));
        }
        Err(e) => return Err(ExportError::Io(e)),
    };
    package::write_package(&parts, file).map_err(|e| match e {
        zip::result::ZipError::Io(io) => ExportError::Io(io),
        other => ExportError::Io(std::io::Error::other(other.to_string())),
    })?;
    Ok(())
}

fn build_parts(workbook: &Workbook, overlay: &Overlay) -> Vec<Part> {
    let names = workbook.sheet_names();
    let sheet_count = names.len();

    let style_table = styles::build(workbook, overlay);

    let mut ss = SharedStrings::new();
    let mut worksheet_parts = Vec::with_capacity(sheet_count);
    for i in 0..sheet_count {
        let bytes = worksheet::emit(workbook, overlay, i as u32, &mut ss, &style_table);
        worksheet_parts.push(Part::new(
            format!("xl/worksheets/sheet{}.xml", i + 1),
            bytes,
        ));
    }

    let mut parts = vec![
        Part::new("[Content_Types].xml", content_types::emit(sheet_count)),
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
    parts
}
