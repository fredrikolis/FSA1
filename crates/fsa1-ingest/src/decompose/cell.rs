// Concern: cuts a sheet into one block per occupied coordinate, joining nothing | Non-concern: every judgement about which coordinates may share a block | IO: (occupied coords) -> Vec<Block>

use crate::decompose::{Block, StyledCell};

/// The identity cut: it joins nothing, so every other policy's blocks are unions of these. No tree
/// of any size is written with it.
pub fn cell_blocks(cells: &[StyledCell]) -> Vec<Block> {
    cells
        .iter()
        .map(|&(col, row, _)| Block {
            col,
            row,
            cols: 1,
            rows: 1,
        })
        .collect()
}
