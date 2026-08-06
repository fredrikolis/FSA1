// Concern: the block predicates a decomposition test grades a cut with | Non-concern: cutting a block (decompose/), what a test concludes from an answer | IO: (blocks, a stated cell) -> bool

use crate::decompose::{Block, StyledCell};

pub fn block(col: u32, row: u32, cols: u32, rows: u32) -> Block {
    Block {
        col,
        row,
        cols,
        rows,
    }
}

pub fn holds(b: Block, (col, row, _): StyledCell) -> bool {
    (b.col..b.col + b.cols).contains(&col) && (b.row..b.row + b.rows).contains(&row)
}

pub fn overlaps(a: Block, b: Block) -> bool {
    a.col < b.col + b.cols
        && b.col < a.col + a.cols
        && a.row < b.row + b.rows
        && b.row < a.row + a.rows
}
