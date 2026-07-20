// Concern: the FORMAT-NEUTRAL intermediate a deserializer produces and the serializer consumes — one sheet's cells as an A1-anchored, row-major rectangle (`SheetSource`), each cell a `SourceCell` (blank / number / text / bool / error / date-serial / raw-formula), the workbook's reference `Resolution` (defined-name map + table geometry, all A1 strings — no calamine/zip/xml types cross here), and the whole book (`SourceBook`); this is the seam that keeps the format reader (calamine) on one side and charlie's TSV writer on the other, so a second format (xlsx) reuses the entire translate/serialize/write half unchanged | Non-concern: reading a concrete format (reader.rs owns calamine) and spelling a cell to TSV (serialize.rs owns that) | IO: none — data types
//! The deserializer↔serializer seam: [`SourceBook`], [`SheetSource`], [`SourceCell`]. A reader fills
//! these from a concrete format; the serializer turns them into charlie grid files. Neither the AST nor
//! the model learns of any format because the format never travels past this neutral shape.

use charlie_ast::ErrKind;

pub use crate::names::DefinedName;
pub use crate::resolve::Resolution;

/// One source cell, already mapped off its concrete format into charlie's value vocabulary (VAL3) plus
/// the one non-value arm — a raw, still-untranslated formula string. A date/time cell arrives as a
/// [`SourceCell::DateSerial`] (the reader converts the format's date representation to charlie's Excel
/// serial, ENG6); a formula arrives as [`SourceCell::Formula`] carrying the source dialect's text for
/// `translate` to rewrite into charlie's Excel-A1 grammar.
#[derive(Clone, Debug, PartialEq)]
pub enum SourceCell {
    /// An empty cell.
    Blank,
    /// A numeric literal.
    Number(f64),
    /// A text literal.
    Text(String),
    /// A boolean literal.
    Bool(bool),
    /// A spreadsheet error value.
    Error(ErrKind),
    /// A date/time-typed cell, already converted to an Excel date serial (charlie dates are serials).
    DateSerial(f64),
    /// A formula cell — the raw source-dialect formula text (e.g. `of:=SUM([.A1:.A2])`), before it is
    /// translated into charlie's Excel-A1 grammar by `translate`.
    Formula(String),
}

/// One sheet as an A1-anchored, row-major rectangle: `rows` × `cols` cells starting at A1, every gap
/// filled with [`SourceCell::Blank`] so the grid fills its range exactly (GRID4). An empty sheet has
/// `rows == 0 && cols == 0` and no cells.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetSource {
    pub name: String,
    pub rows: u32,
    pub cols: u32,
    /// Row-major, `rows * cols` cells.
    pub cells: Vec<SourceCell>,
}

impl SheetSource {
    /// Whether this sheet has no used cells (an empty tab).
    pub fn is_empty(&self) -> bool {
        self.rows == 0 || self.cols == 0
    }
}

/// A whole workbook as neutral sheets, in source order, plus the reference [`Resolution`] (TABLE
/// geometry, resolved INLINE while translating each formula) and the [`DefinedName`]s (FS4 names,
/// EMITTED as on-disk entries and resolved at LOAD, not inline). `resolution`/`names` are empty for a
/// source with no tables/names (or a format not yet resolved).
#[derive(Clone, Debug, PartialEq)]
pub struct SourceBook {
    pub sheets: Vec<SheetSource>,
    pub resolution: Resolution,
    pub names: Vec<DefinedName>,
}
