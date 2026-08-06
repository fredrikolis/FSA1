// Concern: the reader->serializer intermediate — a sheet's cells, styles, widths, heights and merges | Non-concern: reading a concrete format, spelling a cell to TSV | IO: none

use std::collections::BTreeMap;

use fsa1_ast::ErrKind;

pub use fsa1_model::Format;

pub use crate::names::DefinedName;
pub use crate::resolve::Resolution;
pub use crate::xlsx_style::{MergedRegion, StyleTable, XlsxStyle};

#[derive(Clone, Debug, Default, PartialEq)]
pub enum SourceValue {
    #[default]
    Blank,
    Number(f64),
    Text(String),
    Bool(bool),
    Error(ErrKind),
    DateSerial(f64),
    /// A number the engine computes on plus the display format it presents under; only reached when
    /// the value is recoverable from its displayed spelling.
    Formatted {
        value: f64,
        format: Format,
    },
    /// Still in the source dialect (`of:=SUM([.A1:.A2])`); `translate` rewrites it into Excel-A1.
    Formula {
        raw: String,
        format: Option<Format>,
    },
}

/// A styled blank is a real cell: its `style` is the only thing it says, and dropping it loses that.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceCell {
    pub value: SourceValue,
    /// Index into the sheet's [`StyleTable`]; `None` when the source states no style for the cell.
    pub style: Option<u32>,
}

/// The reader always states a cell's style slot, so only a hand-built cell needs this.
#[cfg(test)]
impl SourceCell {
    pub(crate) fn unstyled(value: SourceValue) -> SourceCell {
        SourceCell { value, style: None }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SheetSource {
    pub name: String,
    pub rows: u32,
    pub cols: u32,
    /// Row-major, exactly `rows * cols` entries anchored at A1; every gap is a blank.
    pub cells: Vec<SourceCell>,
    /// The workbook's whole style set — the closed set [`SourceCell::style`] indexes.
    pub styles: StyleTable,
    /// 0-based column -> its width in characters, verbatim as the source states it.
    pub col_widths: BTreeMap<u32, f64>,
    /// 0-based row -> its height in points, verbatim as the source states it.
    pub row_heights: BTreeMap<u32, f64>,
    pub merges: Vec<MergedRegion>,
}

impl SheetSource {
    pub fn cell(&self, col: u32, row: u32) -> Option<&SourceCell> {
        (col < self.cols && row < self.rows).then(|| &self.cells[(row * self.cols + col) as usize])
    }

    pub fn style_at(&self, col: u32, row: u32) -> Option<&XlsxStyle> {
        self.cell(col, row)?
            .style
            .and_then(|index| self.styles.get(index))
    }

    /// A coordinate is occupied when it holds a value OR a style whose look a RULE can put back on a
    /// blank — the sheet's whole content, which a serializer must cover and a bound must reach.
    pub fn is_occupied(&self, col: u32, row: u32) -> bool {
        let Some(cell) = self.cell(col, row) else {
            return false;
        };
        cell.value != SourceValue::Blank
            || cell
                .style
                .is_some_and(|index| crate::scope_block::paints_blank(&self.styles, index))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceBook {
    /// In source order.
    pub sheets: Vec<SheetSource>,
    pub resolution: Resolution,
    pub names: Vec<DefinedName>,
}
