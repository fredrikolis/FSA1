// Concern: the named policies a sheet's cells are decomposed by | Non-concern: what makes a cell occupied, naming or spelling a block (serialize.rs) | IO: (coords + signatures) -> Vec<Block>

use std::str::FromStr;

use crate::decompose::appearance::appearance_blocks;
use crate::decompose::cell::cell_blocks;
use crate::decompose::occupancy::occupancy_blocks;
use crate::decompose::{Block, StyledCell};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decomposition {
    Cell,
    Occupancy,
    Appearance,
}

impl Decomposition {
    /// Every decomposition there is, in help-text order.
    pub const ALL: [Decomposition; 3] = [
        Decomposition::Cell,
        Decomposition::Occupancy,
        Decomposition::Appearance,
    ];

    /// The single source of each variant's spelling, which [`Decomposition::from_str`] reads
    /// backward.
    pub fn name(self) -> &'static str {
        match self {
            Decomposition::Occupancy => "occupancy",
            Decomposition::Cell => "cell",
            Decomposition::Appearance => "appearance",
        }
    }

    /// The blocks tile the cells: pairwise disjoint, every coordinate covered exactly once.
    pub(crate) fn blocks(self, cells: &[StyledCell]) -> Vec<Block> {
        match self {
            Decomposition::Occupancy => {
                let coords: Vec<(u32, u32)> =
                    cells.iter().map(|&(col, row, _)| (col, row)).collect();
                occupancy_blocks(&coords)
            }
            Decomposition::Cell => cell_blocks(cells),
            Decomposition::Appearance => appearance_blocks(cells),
        }
    }
}

/// The refusal carries nothing because the caller already holds both halves of what it prints: the
/// word it handed over, and [`Decomposition::ALL`].
impl FromStr for Decomposition {
    type Err = ();

    fn from_str(s: &str) -> Result<Decomposition, ()> {
        Decomposition::ALL
            .into_iter()
            .find(|d| d.name() == s)
            .ok_or(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_probe::{holds, overlaps};

    /// `ALL` is hand-written over a closed enum, so this is what holds it exhaustive: a second
    /// decomposition breaks the match below, and one left out of `ALL` fails the count.
    #[test]
    fn all_lists_every_decomposition_once() {
        let mut seen: Vec<Decomposition> = Vec::new();
        for decomposition in Decomposition::ALL {
            match decomposition {
                Decomposition::Cell | Decomposition::Occupancy | Decomposition::Appearance => {}
            }
            assert!(
                !seen.contains(&decomposition),
                "{decomposition:?} listed twice"
            );
            seen.push(decomposition);
        }
        assert_eq!(seen.len(), 3);
    }

    #[test]
    fn a_name_parses_back_to_the_decomposition_that_spelled_it() {
        for decomposition in Decomposition::ALL {
            assert_eq!(decomposition.name().parse(), Ok(decomposition));
        }
        assert_eq!("occupancy".parse(), Ok(Decomposition::Occupancy));
        assert_eq!("Occupancy".parse::<Decomposition>(), Err(()));
        assert_eq!("".parse::<Decomposition>(), Err(()));
    }

    fn rect(
        cols: std::ops::RangeInclusive<u32>,
        rows: std::ops::RangeInclusive<u32>,
        signature: Option<u32>,
    ) -> Vec<StyledCell> {
        rows.flat_map(|row| cols.clone().map(move |col| (col, row, signature)))
            .collect()
    }

    /// The one battery every decomposition below is graded over. Each case is named for the shape it
    /// states, and the last four state ONE shape four ways so a policy that reads signatures is
    /// graded on them too.
    fn cases() -> Vec<(&'static str, Vec<StyledCell>)> {
        let bed = rect(1..=4, 1..=3, None);
        let stamped = |signature: fn(usize, u32) -> Option<u32>| -> Vec<StyledCell> {
            bed.iter()
                .enumerate()
                .map(|(i, &(col, row, _))| (col, row, signature(i, row)))
                .collect()
        };
        vec![
            ("empty", Vec::new()),
            ("one cell", vec![(3, 7, Some(4))]),
            ("a dense 6x5 rectangle", rect(1..=6, 1..=5, Some(1))),
            (
                "two clusters, a wide column gap between them",
                [rect(1..=3, 1..=3, Some(1)), rect(60..=62, 1..=3, Some(2))].concat(),
            ),
            (
                "two clusters, a wide row gap between them",
                [rect(1..=3, 1..=3, Some(1)), rect(1..=3, 60..=62, Some(2))].concat(),
            ),
            (
                "a sparse scatter",
                vec![
                    (1, 1, None),
                    (9, 4, Some(3)),
                    (40, 900, Some(3)),
                    (2, 5000, None),
                ],
            ),
            (
                "a staircase, no fully-empty row or column",
                (1..=8)
                    .flat_map(|i| [(i, i, Some(0)), (i + 1, i, Some(0))])
                    .collect(),
            ),
            (
                "an L, not itself a rectangle",
                [rect(1..=1, 1..=5, Some(1)), rect(2..=5, 5..=5, Some(1))].concat(),
            ),
            ("a tall, thin extent", rect(1..=2, 1..=40, Some(1))),
            ("a wide, flat extent", rect(1..=40, 1..=2, Some(1))),
            ("one bed, no signature at all", stamped(|_, _| None)),
            ("one bed, one signature throughout", stamped(|_, _| Some(7))),
            (
                "one bed, signatures striped by row",
                stamped(|_, row| Some(row % 2)),
            ),
            (
                "one bed, a distinct signature per cell",
                stamped(|i, _| Some(i as u32)),
            ),
        ]
    }

    /// A block MAY span a coordinate the input never stated — that is a gap it carries, and the price
    /// of a rectangle. What no decomposition may do is overlap another block, or leave a stated
    /// coordinate in none of them.
    #[test]
    fn every_decomposition_covers_every_stated_coordinate_exactly_once() {
        for decomposition in Decomposition::ALL {
            for (case, cells) in cases() {
                let at = format!("{}/{case}", decomposition.name());
                let blocks = decomposition.blocks(&cells);
                for &cell in &cells {
                    let covers = blocks.iter().filter(|&&b| holds(b, cell)).count();
                    assert_eq!(
                        covers, 1,
                        "{at}: {cell:?} lands in {covers} blocks: {blocks:?}"
                    );
                }
                for (i, &a) in blocks.iter().enumerate() {
                    for &b in &blocks[i + 1..] {
                        assert!(!overlaps(a, b), "{at}: {a:?} overlaps {b:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn every_block_every_decomposition_cuts_is_a_rectangle() {
        for decomposition in Decomposition::ALL {
            for (case, cells) in cases() {
                let at = format!("{}/{case}", decomposition.name());
                for b in decomposition.blocks(&cells) {
                    assert!(b.col >= 1 && b.row >= 1, "{at}: {b:?} is not 1-based");
                    assert!(b.cols >= 1 && b.rows >= 1, "{at}: {b:?} has an empty axis");
                    assert!(
                        b.col.checked_add(b.cols).is_some() && b.row.checked_add(b.rows).is_some(),
                        "{at}: {b:?} declares an extent past the end of the grid"
                    );
                }
            }
        }
    }

    /// The empty direction is the contract; the other falls out of covering, and is asserted with it
    /// so one line states where blocks are owed and where they are forbidden.
    #[test]
    fn a_decomposition_yields_blocks_exactly_when_the_case_states_occupancy() {
        for decomposition in Decomposition::ALL {
            for (case, cells) in cases() {
                let blocks = decomposition.blocks(&cells);
                assert_eq!(
                    blocks.is_empty(),
                    cells.is_empty(),
                    "{}/{case}: {} cells cut into {blocks:?}",
                    decomposition.name(),
                    cells.len()
                );
            }
        }
    }

    /// The projection this file performs, asserted where it happens: `Occupancy` is handed
    /// coordinates alone, so the same coordinates must tile identically whatever the source said each
    /// of them looks like — including saying nothing.
    #[test]
    fn signatures_never_move_a_block() {
        let mut coords: Vec<(u32, u32)> =
            (1..=4).flat_map(|r| (1..=4).map(move |c| (c, r))).collect();
        coords.extend((1..=6).map(|i| (60 + i, 900 * i)));
        let stamp = |signature: &dyn Fn(usize) -> Option<u32>| {
            let cells: Vec<StyledCell> = coords
                .iter()
                .enumerate()
                .map(|(i, &(col, row))| (col, row, signature(i)))
                .collect();
            Decomposition::Occupancy.blocks(&cells)
        };
        let bare = stamp(&|_| None);
        assert!(bare.len() > 1, "the fixture must actually split: {bare:?}");
        assert_eq!(bare, stamp(&|_| Some(1)), "one signature everywhere");
        assert_eq!(bare, stamp(&|i| Some(i as u32)), "a different one per cell");
    }

    /// The policy's OWN output, not the tree's: `serialize::sheet_files` sorts what it is handed, so
    /// only the decomposition can be caught reordering itself.
    #[test]
    fn every_decomposition_cuts_the_same_cells_the_same_way_twice() {
        for decomposition in Decomposition::ALL {
            for (case, cells) in cases() {
                assert_eq!(
                    decomposition.blocks(&cells),
                    decomposition.blocks(&cells),
                    "{}/{case}: one slice, two cuts",
                    decomposition.name()
                );
            }
        }
    }
}
