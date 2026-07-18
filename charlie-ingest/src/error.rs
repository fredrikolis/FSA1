// Concern: the ingest LOCATED-REFUSAL type (CORE2) — `IngestError`, every importer failure carrying a human location (which source sheet + which A1 cell, when a cell is to blame) and a stable `kind` a caller maps to an exit code; plus `ErrorKind` (source-missing / source-io / dest-conflict / dest-io / invalid), so `charlie-cli import` branches its process exit without re-inspecting the message | Non-concern: WHAT is being read/written (reader/serialize/lib own that) and how the CLI spells an exit code (charlie-cli owns the envelope) | IO: none — an error value + its Display
//! Located ingest refusals: [`IngestError`], [`ErrorKind`]. Every importer failure is one of these —
//! never a panic, never a silent wrong grid (CORE2).

use std::fmt;

/// The class of an ingest failure — the seam a caller (the CLI) maps to a process exit code without
/// parsing the message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The source `.ods` path does not exist.
    SourceNotFound,
    /// The source could not be opened/read as an OpenDocument spreadsheet.
    SourceIo,
    /// The destination directory exists and is non-empty — refuse rather than clobber.
    DestConflict,
    /// A destination file/dir could not be created or written.
    DestIo,
    /// The source is structurally readable but a cell/formula/name cannot map to charlie's model
    /// (an untranslatable formula, an unrepresentable value, an illegal sheet name, a bad date).
    Invalid,
}

/// One located ingest refusal: its [`ErrorKind`], the source sheet it arose on (if any), the source
/// A1 cell it blames (if any), and a human message. Rendered as `sheet!A1: message` so a diagnostic
/// points at the exact offending source location (CORE2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IngestError {
    pub kind: ErrorKind,
    /// The source sheet name, when the failure is sheet-scoped.
    pub sheet: Option<String>,
    /// The source A1 cell (`B3`), when a single cell is to blame.
    pub cell: Option<String>,
    pub message: String,
}

impl IngestError {
    /// An I/O-class refusal (source or dest), unlocated within the spreadsheet.
    pub fn io(kind: ErrorKind, message: impl Into<String>) -> IngestError {
        IngestError {
            kind,
            sheet: None,
            cell: None,
            message: message.into(),
        }
    }

    /// An `Invalid`-class refusal located at a source cell (`sheet!A1`).
    pub fn at_cell(
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

    /// An `Invalid`-class refusal located at a source sheet (no single cell to blame).
    pub fn at_sheet(sheet: impl Into<String>, message: impl Into<String>) -> IngestError {
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
