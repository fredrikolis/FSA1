// Concern: IngestError — a READ failure carrying the sheet and A1 to blame, plus a stable ErrorKind | Non-concern: a write failure (ExportError in export.rs), CLI exit codes | IO: none

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    SourceNotFound,
    SourceIo,
    DestConflict,
    DestIo,
    /// Structurally readable, but a cell/formula/name/sheet-name cannot map to FSA1's model.
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IngestError {
    pub kind: ErrorKind,
    pub sheet: Option<String>,
    pub cell: Option<String>,
    pub message: String,
}

impl IngestError {
    pub fn io(kind: ErrorKind, message: impl Into<String>) -> IngestError {
        IngestError {
            kind,
            sheet: None,
            cell: None,
            message: message.into(),
        }
    }

    pub fn invalid_at_cell(
        sheet: impl Into<String>,
        cell: impl Into<String>,
        message: impl Into<String>,
    ) -> IngestError {
        IngestError {
            kind: ErrorKind::Invalid,
            sheet: Some(sheet.into()),
            cell: Some(cell.into()),
            message: message.into(),
        }
    }

    pub fn invalid_at_sheet(sheet: impl Into<String>, message: impl Into<String>) -> IngestError {
        IngestError {
            kind: ErrorKind::Invalid,
            sheet: Some(sheet.into()),
            cell: None,
            message: message.into(),
        }
    }
}

impl fmt::Display for IngestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.sheet, &self.cell) {
            (Some(s), Some(c)) => write!(f, "{s}!{c}: {}", self.message),
            (Some(s), None) => write!(f, "sheet {s:?}: {}", self.message),
            (None, _) => write!(f, "{}", self.message),
        }
    }
}

impl std::error::Error for IngestError {}
