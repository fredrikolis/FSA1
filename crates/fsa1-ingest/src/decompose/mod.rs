// Concern: what a block is and the algorithms that cut them, one file per policy | Non-concern: which policy runs (partition.rs), spelling a block (serialize.rs) | IO: (a sheet's cells) -> Vec<Block>

pub(crate) mod appearance;
pub(crate) mod cell;
pub(crate) mod occupancy;

/// One cell a sheet hands the decomposition: its 1-based `(col, row)` and the appearance the source
/// stated for it — that sheet's own xf index, `None` where the source stated none.
pub type StyledCell = (u32, u32, Option<u32>);

/// A rectangle of the sheet, 1-based and inclusive of its anchor; `cols` and `rows` are never zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub col: u32,
    pub row: u32,
    pub cols: u32,
    pub rows: u32,
}

impl Block {
    pub(crate) fn area(self) -> u64 {
        self.cols as u64 * self.rows as u64
    }
}
