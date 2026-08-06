// Concern: cuts a sheet into blocks by growing and joining rectangles over runs of one cell appearance | Non-concern: reading an appearance, which policy runs | IO: (styled coords) -> Vec<Block>

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::decompose::{Block, StyledCell};

/// What one more region is charged against one more `@scope` rule. Design-time measurement taken
/// elsewhere, NOT reproducible in this repo and governing nothing in it: agreement is nearly flat
/// from 1 to 12, but 1 favours recall and 12 precision, and recall wins under the owner's bar — a
/// spurious boundary is a merge an agent performs later, a missed one is structure never expressed.
const LAMBDA: i64 = 1;

/// Empty strips one expansion may vault. Same measurement, same standing: 2 beats 4 and 8 at every
/// lambda, because a longer vault crosses real boundaries. Neither constant is a flag — a codec
/// ships presets, and lambda trades one error class for the other rather than making a split better,
/// so a caller choosing it would be choosing between errors it has no way to reason about.
const LEAP: u32 = 2;

/// The blocks tile the cells: pairwise disjoint, every stated coordinate covered exactly once,
/// ascending by anchor `(row, col)`. What comes out is addressable structure the author expressed
/// through style and number format; it is not a claim about what any block means.
pub(crate) fn appearance_blocks(cells: &[StyledCell]) -> Vec<Block> {
    let sheet = Appearance::new(cells);
    let mut regions = seed(&sheet);
    loop {
        grow(&sheet, &mut regions);
        if !join(&sheet, &mut regions) {
            break;
        }
    }
    regions
}

/// The signature the slice stated per coordinate. A coordinate the slice never mentions and one it
/// mentions with no signature read alike, which is the whole reason an unoccupied cell costs a rule.
struct Appearance {
    stated: HashMap<(u32, u32), Option<u32>>,
}

impl Appearance {
    fn new(cells: &[StyledCell]) -> Appearance {
        Appearance {
            stated: cells
                .iter()
                .map(|&(col, row, signature)| ((col, row), signature))
                .collect(),
        }
    }

    // A blank cell whose style paints nothing never reaches this seam at all (source.rs::is_occupied), so it reads None here while the encoder could still emit for it — conservative, never wrong.
    fn signature(&self, col: u32, row: u32) -> Option<u32> {
        self.stated.get(&(col, row)).copied().flatten()
    }

    fn mentions(&self, col: u32, row: u32) -> bool {
        self.stated.contains_key(&(col, row))
    }

    /// How many `@scope` selector targets the encoder would emit over `r`, mirroring
    /// [`crate::scope_block`]'s `encode_property` with a signature standing in for a declaration.
    /// A search heuristic, so it models neither that function's model-default suppression nor the
    /// per-property split; what it does reproduce is which cells one rule can still speak for.
    fn rules(&self, r: Block) -> usize {
        let (rows, cols) = (r.rows as usize, r.cols as usize);
        let values: Vec<Option<u32>> = (0..rows * cols)
            .map(|i| self.signature(r.col + (i % cols) as u32, r.row + (i / cols) as u32))
            .collect();
        // One unstated coordinate anywhere suppresses the modal for the whole rectangle: it would assert over cells no finer rule could then take back.
        let modal = if values.contains(&None) {
            None
        } else {
            most_common(&values)
        };
        let col_rules: Vec<Option<u32>> = (0..cols)
            .map(
                |c| match uniform(&values, (0..rows).map(|r| r * cols + c)) {
                    Some(value) if value != modal => value,
                    _ => None,
                },
            )
            .collect();
        let after_columns = |c: usize| col_rules[c].or(modal);
        let row_rules: Vec<Option<u32>> = (0..rows)
            .map(
                |r| match uniform(&values, (0..cols).map(|c| r * cols + c)) {
                    Some(value) if (0..cols).any(|c| after_columns(c) != value) => value,
                    _ => None,
                },
            )
            .collect();
        let leftover = (0..rows * cols)
            .filter(|&i| row_rules[i / cols].or(col_rules[i % cols]).or(modal) != values[i])
            .count();
        usize::from(modal.is_some())
            + col_rules.iter().filter(|v| v.is_some()).count()
            + row_rules.iter().filter(|v| v.is_some()).count()
            + leftover
    }
}

/// The one value every named cell shares, or `None` where they do not all share one.
fn uniform(values: &[Option<u32>], mut cells: impl Iterator<Item = usize>) -> Option<Option<u32>> {
    let first = values[cells.next()?];
    cells.all(|i| values[i] == first).then_some(first)
}

/// The value the most cells carry; ties keep the one appearing first, so reading order decides.
fn most_common(values: &[Option<u32>]) -> Option<u32> {
    let mut counts: HashMap<Option<u32>, (usize, usize)> = HashMap::new();
    for (i, value) in values.iter().enumerate() {
        counts.entry(*value).or_insert((0, i)).0 += 1;
    }
    let (_, (_, first)) = counts
        .iter()
        .max_by_key(|(_, (count, first))| (*count, Reverse(*first)))?;
    values[*first]
}

/// One rectangle per run of identical appearance, each group of like-signed cells covered on its own.
fn seed(sheet: &Appearance) -> Vec<Block> {
    let mut groups: BTreeMap<Option<u32>, BTreeSet<(u32, u32)>> = BTreeMap::new();
    for (&(col, row), &signature) in &sheet.stated {
        groups.entry(signature).or_default().insert((row, col));
    }
    let mut regions: Vec<Block> = groups.into_values().flat_map(cover).collect();
    regions.sort_by_key(|b| (b.row, b.col));
    regions
}

/// Greedy maximal rectangles in row-major order over `cells`, keyed `(row, col)`. Maximal under THIS
/// order, never globally: at each anchor the run of columns is taken first and the rows follow it.
/// Anchors come out strictly ascending, because each pass removes the set's current minimum.
fn cover(mut cells: BTreeSet<(u32, u32)>) -> Vec<Block> {
    let mut out = Vec::new();
    while let Some(&(row, col)) = cells.iter().next() {
        let mut cols = 1;
        while cells.contains(&(row, col + cols)) {
            cols += 1;
        }
        let mut rows = 1;
        while (col..col + cols).all(|c| cells.contains(&(row + rows, c))) {
            rows += 1;
        }
        for r in row..row + rows {
            for c in col..col + cols {
                cells.remove(&(r, c));
            }
        }
        out.push(Block {
            col,
            row,
            cols,
            rows,
        });
    }
    out
}

/// The union of two regions sharing an extent on one axis and touching on the other, or `None` where
/// they are not that pair. Same `(col, cols)` and stacked, or same `(row, rows)` and abutting: those
/// are the only two shapes whose union is again a rectangle holding exactly the pair's coordinates.
fn siblings(a: Block, b: Block) -> Option<Block> {
    if a.col == b.col && a.cols == b.cols && a.row + a.rows == b.row {
        return Some(Block {
            col: a.col,
            row: a.row,
            cols: a.cols,
            rows: a.rows + b.rows,
        });
    }
    if a.row == b.row && a.rows == b.rows && a.col + a.cols == b.col {
        return Some(Block {
            col: a.col,
            row: a.row,
            cols: a.cols + b.cols,
            rows: a.rows,
        });
    }
    None
}

/// One round of sibling unions: every vertical pair keyed `(col, cols)` first, then every horizontal
/// pair keyed `(row, rows)` over the set that phase left, so a union formed vertically is eligible
/// horizontally inside the same round. What [`grow`] cannot reach — two regions of equal extent that
/// no one expansion pays for, because each would have to squash the other — a paying union takes.
fn join(sheet: &Appearance, regions: &mut Vec<Block>) -> bool {
    let vertical = sweep(sheet, regions, |b| (b.col, b.cols));
    let horizontal = sweep(sheet, regions, |b| (b.row, b.rows));
    regions.sort_by_key(|b| (b.row, b.col));
    vertical || horizontal
}

/// One phase: group by `key`, walk each group's anchors ascending by `(row, col)`, and fold
/// consecutive pairs left to right, a formed union standing as the left element of the next
/// comparison. `BTreeMap` fixes the group order, so no hash order reaches the result.
fn sweep(sheet: &Appearance, regions: &mut Vec<Block>, key: fn(Block) -> (u32, u32)) -> bool {
    let mut groups: BTreeMap<(u32, u32), Vec<Block>> = BTreeMap::new();
    for &region in regions.iter() {
        groups.entry(key(region)).or_default().push(region);
    }
    let mut applied = false;
    let mut out: Vec<Block> = Vec::new();
    for mut group in groups.into_values() {
        group.sort_by_key(|b| (b.row, b.col));
        let mut folded: Vec<Block> = Vec::new();
        for region in group {
            match folded.last().copied().and_then(|a| pays(sheet, a, region)) {
                Some(union) => {
                    folded.pop();
                    folded.push(union);
                    applied = true;
                }
                None => folded.push(region),
            }
        }
        out.extend(folded);
    }
    *regions = out;
    applied
}

/// Strict: a union costing exactly what the parts plus the one `LAMBDA` it banks cost buys nothing,
/// and taking it would coarsen for free — the bar [`best_move`] holds an expansion to.
fn pays(sheet: &Appearance, a: Block, b: Block) -> Option<Block> {
    let union = siblings(a, b)?;
    let parts = sheet.rules(a) as i64 + sheet.rules(b) as i64 + LAMBDA;
    (parts > sheet.rules(union) as i64).then_some(union)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    Row,
    Col,
}

/// A region's move: the axis it grows along and the absolute index of the outermost strip it takes.
/// The direction and the strip count both follow from that index against the region's own extent,
/// which is what makes the ordering in [`rank`] total.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Expansion {
    axis: Axis,
    outer: u32,
}

/// One region's chosen move, carried from the pass that computed its gain to the pass that applies
/// it: the extent it was computed for, and the regions whose loss the gain was charged against.
struct Move {
    expander: Block,
    expansion: Expansion,
    losers: Vec<Block>,
    gain: i64,
}

/// Red/black passes: a pass fixes a parity and only anchors of that parity may move, which reduces
/// contention without proving it away — two same-parity anchors are still adjacent when their
/// rectangles differ in extent, so [`stands`] re-checks. Two consecutive silent passes cover both
/// parities against one unchanged region set, which is a fixed point; the potential does the rest.
fn grow(sheet: &Appearance, regions: &mut Vec<Block>) {
    let mut parity = 0;
    let mut silent = 0;
    while silent < 2 {
        let start = regions.clone();
        let moves: Vec<Move> = start
            .iter()
            .filter(|r| (r.col + r.row) % 2 == parity)
            .filter_map(|&r| best_move(sheet, &start, r))
            .collect();
        let mut applied = false;
        for m in moves {
            if !stands(regions, &m) {
                continue;
            }
            apply(regions, &m);
            applied = true;
        }
        silent = if applied { 0 } else { silent + 1 };
        parity ^= 1;
    }
}

/// Both halves are load-bearing. Without the first, a region squashed out of existence earlier in
/// this same pass still has its start-of-pass move applied, breaking the covering and the potential's
/// strict decrease with it. Without the second, a gain charged against one set of losers is applied
/// against another — including a cell that was free when the gain was computed and is now held.
fn stands(regions: &[Block], m: &Move) -> bool {
    let strip = taken(m.expander, m.expansion);
    regions.contains(&m.expander)
        && regions
            .iter()
            .copied()
            .filter(|&r| r != m.expander && overlaps(r, strip))
            .eq(m.losers.iter().copied())
}

fn apply(regions: &mut Vec<Block>, m: &Move) {
    let strip = taken(m.expander, m.expansion);
    let mut next: Vec<Block> = regions
        .iter()
        .copied()
        .filter(|r| *r != m.expander && !m.losers.contains(r))
        .collect();
    next.push(expanded(m.expander, strip));
    for &loser in &m.losers {
        next.extend(retained(loser, strip));
    }
    next.sort_by_key(|b| (b.row, b.col));
    *regions = next;
}

/// The move `region` would make against `regions` as they stand, or `None` where none pays.
fn best_move(sheet: &Appearance, regions: &[Block], region: Block) -> Option<Move> {
    let mut best: Option<Move> = None;
    for axis in [Axis::Row, Axis::Col] {
        for toward_low in [true, false] {
            let Some(expansion) = reach(sheet, region, axis, toward_low) else {
                continue;
            };
            let strip = taken(region, expansion);
            let losers: Vec<Block> = regions
                .iter()
                .copied()
                .filter(|&r| r != region && overlaps(r, strip))
                .collect();
            let gain = gain(sheet, region, strip, &losers);
            if gain <= 0 {
                continue;
            }
            let m = Move {
                expander: region,
                expansion,
                losers,
                gain,
            };
            if best
                .as_ref()
                .is_none_or(|b| rank(region, &m) > rank(region, b))
            {
                best = Some(m);
            }
        }
    }
    best
}

/// Greater gain; then a row expansion before a column one; then the smaller absolute index of the
/// outermost strip taken, which is upward before downward and leftward before rightward; then the
/// fewer empty strips leapt. The last can never decide — the leap count is a function of the region,
/// the axis and that index — and it is written down so the order is visibly total over one region.
fn rank(region: Block, m: &Move) -> (i64, u8, Reverse<u32>, Reverse<u32>) {
    let strips = match m.expansion.axis {
        Axis::Row => taken(region, m.expansion).rows,
        Axis::Col => taken(region, m.expansion).cols,
    };
    let axis = match m.expansion.axis {
        Axis::Row => 1,
        Axis::Col => 0,
    };
    (
        m.gain,
        axis,
        Reverse(m.expansion.outer),
        Reverse(strips - 1),
    )
}

/// The nearest strip outward carrying content, having bridged only empty ones on the way. Walking out
/// and stopping at the first strip with content is what latches the landing: everything vaulted is
/// interior by construction, so a region can never terminate on padding or wander into blank space.
fn reach(sheet: &Appearance, region: Block, axis: Axis, toward_low: bool) -> Option<Expansion> {
    let edge = match (axis, toward_low) {
        (Axis::Row, true) => region.row,
        (Axis::Row, false) => region.row + region.rows - 1,
        (Axis::Col, true) => region.col,
        (Axis::Col, false) => region.col + region.cols - 1,
    };
    for step in 1..=LEAP + 1 {
        let outer = match toward_low {
            true => edge.checked_sub(step).filter(|&i| i >= 1),
            false => edge.checked_add(step),
        }?;
        if occupied_strip(sheet, region, axis, outer) {
            return Some(Expansion { axis, outer });
        }
    }
    None
}

/// A strip is empty when no stated cell lies on it within the region's own span.
fn occupied_strip(sheet: &Appearance, region: Block, axis: Axis, index: u32) -> bool {
    match axis {
        Axis::Row => (region.col..region.col + region.cols).any(|c| sheet.mentions(c, index)),
        Axis::Col => (region.row..region.row + region.rows).any(|r| sheet.mentions(index, r)),
    }
}

/// The whole rows or whole columns the expansion takes, spanning the region's own width or height.
fn taken(region: Block, expansion: Expansion) -> Block {
    let (last_row, last_col) = (region.row + region.rows, region.col + region.cols);
    match expansion.axis {
        Axis::Row if expansion.outer < region.row => Block {
            col: region.col,
            row: expansion.outer,
            cols: region.cols,
            rows: region.row - expansion.outer,
        },
        Axis::Row => Block {
            col: region.col,
            row: last_row,
            cols: region.cols,
            rows: expansion.outer - last_row + 1,
        },
        Axis::Col if expansion.outer < region.col => Block {
            col: expansion.outer,
            row: region.row,
            cols: region.col - expansion.outer,
            rows: region.rows,
        },
        Axis::Col => Block {
            col: last_col,
            row: region.row,
            cols: expansion.outer - last_col + 1,
            rows: region.rows,
        },
    }
}

fn expanded(region: Block, strip: Block) -> Block {
    let col = region.col.min(strip.col);
    let row = region.row.min(strip.row);
    Block {
        col,
        row,
        cols: (region.col + region.cols).max(strip.col + strip.cols) - col,
        rows: (region.row + region.rows).max(strip.row + strip.rows) - row,
    }
}

/// What a squashed region keeps, re-cut by the same greedy cover the seed uses. Its retained
/// coordinates are not in general one rectangle — a narrow strip out of a wider region leaves an L,
/// which re-cuts into two — and the potential charges for every piece through the same lambda term.
fn retained(loser: Block, strip: Block) -> Vec<Block> {
    cover(
        coords(loser)
            .filter(|&(row, col)| !holds(strip, row, col))
            .collect(),
    )
}

/// The decrease in `sum of rules over every region + LAMBDA * region count` this move causes. The
/// potential decomposes as a sum over regions, so the expander and the regions losing cells are
/// exactly the terms that move; a cell in the strip that no region holds is priced on neither side.
fn gain(sheet: &Appearance, region: Block, strip: Block, losers: &[Block]) -> i64 {
    let mut before = sheet.rules(region) as i64 + LAMBDA;
    let mut after = sheet.rules(expanded(region, strip)) as i64 + LAMBDA;
    for &loser in losers {
        before += sheet.rules(loser) as i64 + LAMBDA;
        for piece in retained(loser, strip) {
            after += sheet.rules(piece) as i64 + LAMBDA;
        }
    }
    before - after
}

fn coords(b: Block) -> impl Iterator<Item = (u32, u32)> {
    (b.row..b.row + b.rows).flat_map(move |r| (b.col..b.col + b.cols).map(move |c| (r, c)))
}

fn holds(b: Block, row: u32, col: u32) -> bool {
    (b.row..b.row + b.rows).contains(&row) && (b.col..b.col + b.cols).contains(&col)
}

fn overlaps(a: Block, b: Block) -> bool {
    a.col < b.col + b.cols
        && b.col < a.col + a.cols
        && a.row < b.row + b.rows
        && b.row < a.row + a.rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_probe::block;

    fn blocks(cells: &[StyledCell]) -> Vec<Block> {
        appearance_blocks(cells)
    }

    /// `signatures` row-major from A1, every coordinate stated.
    fn grid(cols: u32, signatures: &[u32]) -> Vec<StyledCell> {
        signatures
            .iter()
            .enumerate()
            .map(|(i, &s)| (i as u32 % cols + 1, i as u32 / cols + 1, Some(s)))
            .collect()
    }

    fn bar(row: u32) -> Vec<StyledCell> {
        (1..=3).map(|col| (col, row, Some(1))).collect()
    }

    #[test]
    fn one_signature_over_a_gapless_rectangle_costs_one_rule() {
        let cells = grid(3, &[4, 4, 4, 4, 4, 4]);
        assert_eq!(Appearance::new(&cells).rules(block(1, 1, 3, 2)), 1);
    }

    /// Knocking B2 out of a uniform 2x2 suppresses the modal, and the encoder is left spelling the
    /// same three cells through a column rule and a row rule instead: 1 becomes 2.
    #[test]
    fn one_unstated_coordinate_suppresses_the_modal_rule() {
        let full = grid(2, &[4, 4, 4, 4]);
        let holed = vec![(1, 1, Some(4)), (2, 1, Some(4)), (1, 2, Some(4))];
        assert_eq!(Appearance::new(&full).rules(block(1, 1, 2, 2)), 1);
        assert_eq!(Appearance::new(&holed).rules(block(1, 1, 2, 2)), 2);
    }

    #[test]
    fn a_rectangle_of_nothing_but_unstated_coordinates_costs_nothing() {
        assert_eq!(Appearance::new(&[]).rules(block(1, 1, 5, 4)), 0);
    }

    /// Counted by hand over the 4x4: signature 7 is modal at 8 cells of 16, so 1. Column B is uniform
    /// at 9 and differs from it, so 1. Row 2 is uniform at 9 while column A resolves to 7 after the
    /// column rules, so 1. D4 carries 5, which the row, the column and the modal all miss, so 1.
    #[test]
    fn a_column_a_row_and_one_stray_cell_cost_one_rule_each() {
        let cells = grid(4, &[7, 9, 7, 7, 9, 9, 9, 9, 7, 9, 7, 7, 7, 9, 7, 5]);
        assert_eq!(Appearance::new(&cells).rules(block(1, 1, 4, 4)), 4);
    }

    /// A T of one signature. Greedy row-major takes the widest run at A1, so A1:C1 goes first and the
    /// column under it is cut short at A2:A4. The globally larger rectangle through A1 is the 1x4
    /// column A1:A4, which this cover never considers — a different reading, and a different policy.
    #[test]
    fn the_seed_cover_reads_row_major_not_globally_maximal() {
        let cells = [
            (1, 1, Some(1)),
            (2, 1, Some(1)),
            (3, 1, Some(1)),
            (1, 2, Some(1)),
            (1, 3, Some(1)),
            (1, 4, Some(1)),
        ];
        assert_eq!(
            seed(&Appearance::new(&cells)),
            vec![block(1, 1, 3, 1), block(1, 2, 1, 3)]
        );
    }

    #[test]
    fn a_region_never_grows_onto_strips_carrying_nothing() {
        assert_eq!(blocks(&bar(1)), vec![block(1, 1, 3, 1)]);
    }

    #[test]
    fn a_region_bridges_one_and_two_empty_strips_but_never_three() {
        for gap in [1, 2] {
            let cells = [bar(1), bar(2 + gap)].concat();
            assert_eq!(
                blocks(&cells),
                vec![block(1, 1, 3, 2 + gap)],
                "gap of {gap}"
            );
        }
        let cells = [bar(1), bar(5)].concat();
        assert_eq!(blocks(&cells), vec![block(1, 1, 3, 1), block(1, 5, 3, 1)]);
    }

    /// B2 can absorb the cell above it or the cell to its left for the same gain, and separately a
    /// cell between two like neighbours can absorb either of them for the same gain. Axis decides the
    /// first — rows before columns — and the lower outermost index decides the second, upward.
    #[test]
    fn a_tied_gain_is_broken_by_axis_then_by_the_lower_outermost_strip() {
        let corner = [(2, 1, Some(1)), (1, 2, Some(2)), (2, 2, Some(3))];
        let sheet = Appearance::new(&corner);
        let b2 = block(2, 2, 1, 1);
        let up = Expansion {
            axis: Axis::Row,
            outer: 1,
        };
        let left = Expansion {
            axis: Axis::Col,
            outer: 1,
        };
        assert_eq!(
            gain(&sheet, b2, taken(b2, up), &[block(2, 1, 1, 1)]),
            gain(&sheet, b2, taken(b2, left), &[block(1, 2, 1, 1)])
        );
        let chosen = best_move(&sheet, &seed(&sheet), b2).expect("both directions pay");
        assert_eq!(chosen.expansion, up);

        let column = [(1, 1, Some(1)), (1, 2, Some(3)), (1, 3, Some(1))];
        let sheet = Appearance::new(&column);
        let a2 = block(1, 2, 1, 1);
        let down = Expansion {
            axis: Axis::Row,
            outer: 3,
        };
        assert_eq!(
            gain(&sheet, a2, taken(a2, up), &[block(1, 1, 1, 1)]),
            gain(&sheet, a2, taken(a2, down), &[block(1, 3, 1, 1)])
        );
        let chosen = best_move(&sheet, &seed(&sheet), a2).expect("both directions pay");
        assert_eq!(chosen.expansion, up);
    }

    #[test]
    fn the_blocks_come_out_ascending_by_anchor() {
        let cells = vec![
            (9, 9, Some(2)),
            (1, 1, Some(1)),
            (5, 40, None),
            (9, 8, Some(2)),
        ];
        let out = blocks(&cells);
        assert!(
            out.len() > 1,
            "the fixture must cut more than once: {out:?}"
        );
        assert!(
            out.windows(2)
                .all(|w| (w[0].row, w[0].col) < (w[1].row, w[1].col)),
            "{out:?}"
        );
    }

    /// `LAMBDA` prices one region, so a merge pays when it costs fewer rules than that. The first
    /// grid's merge is rule-neutral, two either way, and banks the whole `LAMBDA`; the second's costs
    /// one rule, because column B stops being uniform and its rule becomes two leftovers while row 3
    /// gains one. Only at `LAMBDA = 1` does the first merge and the second not.
    #[test]
    fn lambda_is_the_one_region_term_a_rule_neutral_merge_can_buy_and_no_more() {
        assert_eq!(
            blocks(&grid(2, &[1, 1, 0, 0])),
            vec![block(1, 1, 2, 2)],
            "a row of one signature over a row of another coalesces at any LAMBDA above 0"
        );
        assert_eq!(
            blocks(&grid(2, &[1, 2, 1, 2, 0, 0])),
            vec![block(1, 1, 2, 2), block(1, 3, 2, 1)],
            "two signature COLUMNS over a third row hold apart until LAMBDA reaches 2"
        );
    }

    #[test]
    fn a_banded_table_joins_into_one_block() {
        let cells = grid(2, &[1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0]);
        assert_eq!(blocks(&cells), vec![block(1, 1, 2, 6)]);
    }

    #[test]
    fn a_table_banded_by_column_joins_into_one_block() {
        let cells = grid(6, &[1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0]);
        assert_eq!(blocks(&cells), vec![block(1, 1, 6, 2)]);
    }

    /// A header row over eight banded rows: the bands pair off into stacks of two, four and eight
    /// while the header stays one row deep, so the last union is 1-deep against 8-deep. Only a rule
    /// that reads the shared extent rather than the depth can take it.
    #[test]
    fn a_join_merges_siblings_of_unequal_depth() {
        let mut signatures = vec![9, 9, 9];
        for row in 2..=9 {
            signatures.extend([u32::from(row % 2 == 1); 3]);
        }
        assert_eq!(blocks(&grid(3, &signatures)), vec![block(1, 1, 3, 9)]);
    }

    #[test]
    fn a_join_never_bridges_a_blank_strip() {
        let table = |top: u32| -> Vec<StyledCell> {
            (0..8)
                .flat_map(move |i| (1..=2).map(move |col| (col, top + i, Some(i % 2))))
                .collect()
        };
        assert_eq!(
            blocks(&[table(1), table(10)].concat()),
            vec![block(1, 1, 2, 8), block(1, 10, 2, 8)]
        );
    }

    /// Every rectangle of unstated coordinates costs 0, so any legal merge banks the whole lambda and
    /// the sheet coarsens until nothing more can reach. That is why `appearance` is refused on a source
    /// whose FORMAT states no appearance, where every cell reads `None` necessarily; a source whose
    /// format is read and that happens to state nothing is accepted and cut on its occupancy.
    #[test]
    fn a_sheet_stating_no_appearance_coarsens() {
        let cells = vec![(1, 1, None), (1, 3, None)];
        let sheet = Appearance::new(&cells);
        assert_eq!(sheet.rules(block(1, 1, 1, 3)), 0);
        assert_eq!(seed(&sheet).len(), 2);
        assert_eq!(blocks(&cells), vec![block(1, 1, 1, 3)]);
    }
}
