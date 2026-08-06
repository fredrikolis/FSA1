// Concern: cuts a sheet's occupancy into blocks at its widest fully-empty rows and columns | Non-concern: a cell's appearance, which policy runs (partition.rs) | IO: (occupied coords) -> Vec<Block>

use crate::decompose::Block;

/// Area a block may spend per occupied cell before a split is worth it. Measured over the corpus:
/// 16 splits nothing (the worst sheet is 14.9x); 8 splits 3 sheets, cutting TSV fields 6,621,392 ->
/// 3,952,350; 4 costs 7 more files for no gain; 2 drops 16 column widths. Asymmetric — too high only
/// writes blank fields, too low LOSES geometry (blocks shrink until an axis sits in no block at all).
const WASTE_BUDGET: u64 = 8;

/// Below a 16x16 no split pays for itself, whatever the occupancy.
const FLOOR_AREA: u64 = 256;

/// The blocks tile the occupancy: pairwise disjoint, every coordinate covered exactly once.
/// `occupied` is the set of 1-based `(col, row)` coordinates in any order; empty yields no blocks.
pub(crate) fn occupancy_blocks(occupied: &[(u32, u32)]) -> Vec<Block> {
    if occupied.is_empty() {
        return Vec::new();
    }
    let mut coords = occupied.to_vec();
    let mut blocks = Vec::new();
    let mut pending = vec![(0usize, coords.len())];
    while let Some((lo, hi)) = pending.pop() {
        let span = &mut coords[lo..hi];
        let block = bounding_box(span);
        match split_cut(span, block) {
            None => blocks.push(block),
            Some(cut) => {
                let mid = lo + hoist_low_side(span, cut);
                pending.push((mid, hi));
                pending.push((lo, mid));
            }
        }
    }
    blocks
}

#[derive(Clone, Copy, Debug)]
enum Axis {
    Row,
    Col,
}

impl Axis {
    fn key(self, (col, row): (u32, u32)) -> u32 {
        match self {
            Axis::Row => row,
            Axis::Col => col,
        }
    }
}

/// `last_low` is the last occupied index kept on the low side; the gap after it is discarded.
#[derive(Clone, Copy, Debug)]
struct Cut {
    axis: Axis,
    last_low: u32,
}

fn budget(occupancy: usize) -> u64 {
    FLOOR_AREA.max(WASTE_BUDGET * occupancy as u64)
}

/// A cut lands only on a fully-unoccupied line, so an occupancy with no empty row AND no empty column
/// — a staircase — is NOT splittable, however far over budget it is. Accepted, and the same asymmetry:
/// the block only writes blank fields, while a cut that ignored the gaps would lose geometry.
fn split_cut(span: &[(u32, u32)], block: Block) -> Option<Cut> {
    if block.area() <= budget(span.len()) {
        return None;
    }
    match (widest_gap(span, Axis::Row), widest_gap(span, Axis::Col)) {
        (Some((rows, row_cut)), Some((cols, col_cut))) => {
            Some(if cols > rows { col_cut } else { row_cut })
        }
        (Some((_, cut)), None) | (None, Some((_, cut))) => Some(cut),
        (None, None) => None,
    }
}

/// The longest run of fully-unoccupied lines between two occupied ones — interior by construction,
/// since a bounding box has no unoccupied boundary. Ties keep the lowest index.
fn widest_gap(span: &[(u32, u32)], axis: Axis) -> Option<(u32, Cut)> {
    let mut keys: Vec<u32> = span.iter().map(|&c| axis.key(c)).collect();
    keys.sort_unstable();
    keys.dedup();
    let mut widest: Option<(u32, Cut)> = None;
    for pair in keys.windows(2) {
        let gap = pair[1] - pair[0] - 1;
        if gap > 0 && widest.is_none_or(|(seen, _)| gap > seen) {
            widest = Some((
                gap,
                Cut {
                    axis,
                    last_low: pair[0],
                },
            ));
        }
    }
    widest
}

fn hoist_low_side(span: &mut [(u32, u32)], cut: Cut) -> usize {
    let mut low = 0;
    for i in 0..span.len() {
        if cut.axis.key(span[i]) <= cut.last_low {
            span.swap(low, i);
            low += 1;
        }
    }
    low
}

fn bounding_box(span: &[(u32, u32)]) -> Block {
    debug_assert!(!span.is_empty(), "a gap leaves occupancy on both sides");
    let (mut min_col, mut max_col) = (u32::MAX, 0);
    let (mut min_row, mut max_row) = (u32::MAX, 0);
    for &(col, row) in span {
        min_col = min_col.min(col);
        max_col = max_col.max(col);
        min_row = min_row.min(row);
        max_row = max_row.max(row);
    }
    Block {
        col: min_col,
        row: min_row,
        cols: max_col - min_col + 1,
        rows: max_row - min_row + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_probe::{block, holds, overlaps};
    use crate::decompose::StyledCell;

    /// The signature every fixture states is projected away here exactly as `Decomposition::blocks`
    /// projects it, so each fixture states one and none of them agree —
    /// `partition::tests::signatures_never_move_a_block` is what holds the value irrelevant.
    fn decomposed(cells: &[StyledCell]) -> Vec<Block> {
        let coords: Vec<(u32, u32)> = cells.iter().map(|&(col, row, _)| (col, row)).collect();
        occupancy_blocks(&coords)
    }

    fn total_area(blocks: &[Block]) -> u64 {
        blocks.iter().map(|b| b.area()).sum()
    }

    fn assert_tiles(occupied: &[StyledCell], blocks: &[Block]) {
        for &c in occupied {
            let covers = blocks.iter().filter(|&&b| holds(b, c)).count();
            assert_eq!(covers, 1, "{c:?} is covered {covers} times, want exactly 1");
        }
        for (i, &a) in blocks.iter().enumerate() {
            for &b in &blocks[i + 1..] {
                assert!(!overlaps(a, b), "{a:?} overlaps {b:?}");
            }
        }
    }

    fn rect(
        cols: std::ops::RangeInclusive<u32>,
        rows: std::ops::RangeInclusive<u32>,
        signature: Option<u32>,
    ) -> Vec<StyledCell> {
        rows.flat_map(|r| cols.clone().map(move |c| (c, r, signature)))
            .collect()
    }

    fn sorted(mut blocks: Vec<Block>) -> Vec<Block> {
        blocks.sort_by_key(|b| (b.row, b.col));
        blocks
    }

    #[test]
    fn empty_occupancy_yields_no_blocks() {
        assert_eq!(decomposed(&[]), Vec::new());
    }

    #[test]
    fn a_lone_coordinate_yields_a_1x1_block() {
        assert_eq!(decomposed(&[(3, 7, Some(4))]), vec![block(3, 7, 1, 1)]);
    }

    #[test]
    fn a_stray_cell_never_stretches_a_block_across_the_sheet() {
        for stray in [(26, 50_000, None), (1, 1_048_576, Some(11))] {
            let occupied = vec![(1, 1, Some(0)), (2, 1, None), stray];
            let blocks = decomposed(&occupied);
            assert_tiles(&occupied, &blocks);
            assert_eq!(blocks.len(), 2, "{blocks:?}");
            assert_eq!(
                total_area(&blocks),
                3,
                "{stray:?} must cost its own cell, not a whole declared range: {blocks:?}"
            );
        }
    }

    #[test]
    fn a_dense_sheet_under_the_budget_is_not_split() {
        let occupied = rect(1..=10, 1..=10, Some(2));
        assert_eq!(decomposed(&occupied), vec![block(1, 1, 10, 10)]);
    }

    /// Area 80 against a budget of 560, so the empty row 3 is cheaper to carry than to split.
    #[test]
    fn a_title_over_a_table_stays_one_block() {
        let mut occupied = vec![(1, 1, Some(3)), (1, 2, None)];
        occupied.extend(rect(1..=4, 4..=20, Some(5)));
        assert_eq!(decomposed(&occupied), vec![block(1, 1, 4, 20)]);
    }

    #[test]
    fn an_equal_row_and_column_gap_splits_the_rows() {
        let occupied = vec![(1, 1, None), (20, 1, Some(6)), (1, 20, Some(7))];
        let blocks = sorted(decomposed(&occupied));
        assert_tiles(&occupied, &blocks);
        assert_eq!(blocks, vec![block(1, 1, 20, 1), block(1, 20, 1, 1)]);
    }

    #[test]
    fn equal_gaps_on_one_axis_split_at_the_lowest_index() {
        let occupied = vec![(1, 1, Some(1)), (1, 201, Some(2)), (1, 401, None)];
        let blocks = sorted(decomposed(&occupied));
        assert_tiles(&occupied, &blocks);
        assert_eq!(blocks, vec![block(1, 1, 1, 1), block(1, 201, 1, 201)]);
    }

    /// PINS the gap-free corner of [`split_cut`]: a 300-cell diagonal occupies every row and every
    /// column, so no line is free to cut at and the whole staircase stays ONE block — 90,000 fields
    /// for 300 values, area 300 per occupied cell against a budget of 8. The accepted price.
    #[test]
    fn a_gap_free_staircase_is_one_block_however_far_over_budget() {
        let occupied: Vec<StyledCell> = (1..=300).map(|i| (i, i, Some(i))).collect();
        assert_eq!(decomposed(&occupied), vec![block(1, 1, 300, 300)]);
    }

    #[test]
    fn scattered_clusters_are_tiled_by_their_own_blocks() {
        let mut occupied = rect(1..=4, 1..=4, Some(8));
        occupied.extend(rect(60..=64, 3..=6, None));
        occupied.extend(rect(2..=3, 900..=902, Some(9)));
        occupied.push((70, 5000, Some(0)));
        let blocks = decomposed(&occupied);
        assert_tiles(&occupied, &blocks);
        assert_eq!(blocks.len(), 4, "{blocks:?}");
    }
}
