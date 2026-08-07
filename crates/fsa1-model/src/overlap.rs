// Concern: which of a tab's declared regions collide, over the Rect geometry it defines | Non-concern: file contents, picking a winner | IO: (tab, [(name, Rect)]) -> Vec<Diagnostic>

use crate::diagnostic::{Code, Diagnostic, Loc};
use fsa1_ast::a1::format_cell;

/// Zero-based and inclusive on all four corners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub min_col: u32,
    pub min_row: u32,
    pub max_col: u32,
    pub max_row: u32,
}

impl Rect {
    pub fn cell(col: u32, row: u32) -> Rect {
        Rect {
            min_col: col,
            min_row: row,
            max_col: col,
            max_row: row,
        }
    }

    pub fn contains(&self, col: u32, row: u32) -> bool {
        self.min_col <= col && col <= self.max_col && self.min_row <= row && row <= self.max_row
    }

    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let min_col = self.min_col.max(other.min_col);
        let min_row = self.min_row.max(other.min_row);
        let max_col = self.max_col.min(other.max_col);
        let max_row = self.max_row.min(other.max_row);
        if min_col <= max_col && min_row <= max_row {
            Some(Rect {
                min_col,
                min_row,
                max_col,
                max_row,
            })
        } else {
            None
        }
    }

    /// The smallest rectangle covering both, `None` only where neither states one. The geometry a
    /// consumer spanning a tab's CONTENT and its PRESENTATION at once reads, each such consumer
    /// naming the two halves itself.
    pub fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
        let (Some(a), Some(b)) = (a, b) else {
            return a.or(b);
        };
        Some(Rect {
            min_col: a.min_col.min(b.min_col),
            min_row: a.min_row.min(b.min_row),
            max_col: a.max_col.max(b.max_col),
            max_row: a.max_row.max(b.max_row),
        })
    }

    /// `C2` for a single cell, `C2:D3` for a range.
    pub fn label(&self) -> String {
        let top_left = format_cell(self.min_col, self.min_row);
        if self.min_col == self.max_col && self.min_row == self.max_row {
            top_left
        } else {
            format!("{top_left}:{}", format_cell(self.max_col, self.max_row))
        }
    }
}

/// One diagnostic per intersecting PAIR, naming both files and the contested block. The precedence
/// rule is reject: no winner is chosen by ordering, recency, or specificity. A shared boundary with
/// no shared cell is not an overlap, and a gap between regions is Blank rather than a fault.
pub fn detect_overlaps(tab: &str, files: &[(String, Rect)]) -> Vec<Diagnostic> {
    let mut pairs = intersecting_pairs(files);
    // A pair contesting many coordinates collides once per coordinate but is ONE overlap.
    pairs.sort_unstable();
    pairs.dedup();

    let mut out = Vec::with_capacity(pairs.len());
    for (i, j) in pairs {
        let (ref a_name, a_rect) = files[i];
        let (ref b_name, b_rect) = files[j];
        if let Some(contested) = a_rect.intersect(&b_rect) {
            let message = format!(
                "two files claim overlapping cells in tab {tab:?}\n    {a_name}  and  {b_name}\n    contested: {}\n    precedence: none -- reject. Split or delete one file.",
                contested.label(),
            );
            out.push(Diagnostic::new(Code::Overlap, Loc::tab(tab), message));
        }
    }
    out
}

/// A MEMORY ceiling on the coordinate index (one 16-byte entry per declared coordinate, so ~64
/// MiB), not a shape claim: it is a running total over the whole tab, and a tab that exceeds it is
/// not indexed at all but scanned pairwise, which allocates nothing per coordinate.
const INDEX_BUDGET: u64 = 1 << 22;

/// When the run enumeration compacts its pair vec, keeping it near `max(this, 2 x distinct pairs
/// so far)`. This is NOT a bound: the check fires only BETWEEN runs, so one run's whole expansion
/// can land on top of it. The unconditional bound is the `work` counter in [`indexed_pairs`],
/// tested against the pairwise scan's own `n(n-1)/2` before each run is expanded.
const PAIR_COMPACT_CAP: usize = 1 << 20;

fn intersecting_pairs(files: &[(String, Rect)]) -> Vec<(usize, usize)> {
    indexed_pairs(files).unwrap_or_else(|| pairwise_pairs(files))
}

/// The fallback, and the COST UNIT every other path here is measured in, so no input is more than a
/// constant multiple slower than it was before the index existed. Ascending and repeat-free.
fn pairwise_pairs(files: &[(String, Rect)]) -> Vec<(usize, usize)> {
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for i in 0..files.len() {
        for j in (i + 1)..files.len() {
            if files[i].1.intersect(&files[j].1).is_some() {
                pairs.push((i, j));
            }
        }
    }
    pairs
}

/// Stamps every declared coordinate with its file; a coordinate stamped twice is an overlap. Cost
/// is the tab's declared cells rather than the square of its file count. Returns `None`, having
/// emitted nothing, wherever the index would be the SLOWER path — a reversed `Rect`, an area past
/// [`INDEX_BUDGET`] or costlier than the scan, or a run enumeration crossing that cost mid-flight.
fn indexed_pairs(files: &[(String, Rect)]) -> Option<Vec<(usize, usize)>> {
    let ceiling = pairwise_cost(files.len());

    let mut area: u64 = 0;
    for (_, rect) in files {
        area = area.checked_add(declared_area(rect)?)?;
        if area > INDEX_BUDGET {
            return None;
        }
    }
    if index_cost(area) > ceiling {
        return None;
    }

    let mut claims: Vec<(u64, usize)> = Vec::with_capacity(area as usize);
    for (idx, (_, rect)) in files.iter().enumerate() {
        for row in rect.min_row..=rect.max_row {
            for col in rect.min_col..=rect.max_col {
                claims.push((coord_key(col, row), idx));
            }
        }
    }

    // Sorting by `(coordinate, file index)` makes each run's indices ascend, so each pair is `i < j`.
    claims.sort_unstable();
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let mut compact_at = PAIR_COMPACT_CAP;
    let mut work: u64 = 0;
    let mut start = 0;
    while start < claims.len() {
        let mut end = start + 1;
        while end < claims.len() && claims[end].0 == claims[start].0 {
            end += 1;
        }
        let run = (end - start) as u64;
        work = work.saturating_add(run * (run - 1) / 2);
        if work > ceiling {
            return None;
        }
        for a in start..end {
            for b in (a + 1)..end {
                pairs.push((claims[a].1, claims[b].1));
            }
        }
        if pairs.len() >= compact_at {
            pairs.sort_unstable();
            pairs.dedup();
            compact_at = PAIR_COMPACT_CAP.max(pairs.len().saturating_mul(2));
        }
        start = end;
    }
    Some(pairs)
}

fn pairwise_cost(files: usize) -> u64 {
    let n = files as u64;
    n * n.saturating_sub(1) / 2
}

/// In [`pairwise_cost`]'s unit — one per comparison of the sort that dominates the build.
fn index_cost(area: u64) -> u64 {
    area.saturating_mul(area.max(2).ilog2() as u64 + 1)
}

/// Row-major, so a run of equal keys is one cell.
fn coord_key(col: u32, row: u32) -> u64 {
    ((row as u64) << 32) | col as u64
}

/// `None` where there is no countable number — a reversed rect, or an area past `u64`. Neither
/// comes from `parse_filename`, but `Rect` is public, and either sends the tab to the scan.
fn declared_area(rect: &Rect) -> Option<u64> {
    if rect.min_col > rect.max_col || rect.min_row > rect.max_row {
        return None;
    }
    let cols = (rect.max_col - rect.min_col) as u64 + 1;
    let rows = (rect.max_row - rect.min_row) as u64 + 1;
    cols.checked_mul(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(min_col: u32, min_row: u32, max_col: u32, max_row: u32) -> Rect {
        Rect {
            min_col,
            min_row,
            max_col,
            max_row,
        }
    }

    #[test]
    fn contains_is_inclusive_on_all_corners() {
        let r = rect(1, 2, 3, 4); // B3:D5
        assert!(r.contains(1, 2));
        assert!(r.contains(3, 4));
        assert!(r.contains(2, 3));
        assert!(!r.contains(0, 2));
        assert!(!r.contains(4, 4));
        assert!(!r.contains(2, 5));
    }

    #[test]
    fn disjoint_regions_do_not_overlap() {
        let files = vec![
            ("A1:B2".to_string(), rect(0, 0, 1, 1)),
            ("C3:D4".to_string(), rect(2, 2, 3, 3)),
        ];
        assert!(detect_overlaps("Orders", &files).is_empty());
    }

    #[test]
    fn touching_but_not_overlapping_is_fine() {
        let files = vec![
            ("A1:B2".to_string(), rect(0, 0, 1, 1)),
            ("C1:D2".to_string(), rect(2, 0, 3, 1)),
        ];
        assert!(detect_overlaps("Orders", &files).is_empty());
    }

    #[test]
    fn range_and_cell_overlap_names_both_and_the_contested_cell() {
        let files = vec![
            ("A1:D3".to_string(), rect(0, 0, 3, 2)),
            ("C2".to_string(), Rect::cell(2, 1)),
        ];
        let diags = detect_overlaps("Orders", &files);
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.code, Code::Overlap);
        assert!(d.message.contains("A1:D3"));
        assert!(d.message.contains("C2"));
        assert!(d.message.contains("contested: C2"));
        assert!(d.message.contains("reject"));
    }

    #[test]
    fn overlapping_ranges_report_the_contested_block() {
        let files = vec![
            ("A1:C3".to_string(), rect(0, 0, 2, 2)),
            ("B2:D4".to_string(), rect(1, 1, 3, 3)),
        ];
        let diags = detect_overlaps("T", &files);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("contested: B2:C3"));
    }

    #[test]
    fn duplicate_cells_overlap() {
        let files = vec![
            ("C2".to_string(), Rect::cell(2, 1)),
            ("C2".to_string(), Rect::cell(2, 1)),
        ];
        assert_eq!(detect_overlaps("T", &files).len(), 1);
    }

    #[test]
    fn a_pair_contesting_many_cells_is_one_diagnostic() {
        // These two contest 99x99 = 9,801 cells.
        let files = vec![
            ("A1:CV100".to_string(), rect(0, 0, 99, 99)),
            ("B2:CW101".to_string(), rect(1, 1, 100, 100)),
        ];
        let diags = detect_overlaps("T", &files);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("contested: B2:CV100"));
    }

    #[test]
    fn three_mutually_overlapping_files_yield_one_diagnostic_per_pair() {
        let files = vec![
            ("A1:B2".to_string(), rect(0, 0, 1, 1)),
            ("B2:C3".to_string(), rect(1, 1, 2, 2)),
            ("A1:C3".to_string(), rect(0, 0, 2, 2)),
        ];
        let diags = detect_overlaps("T", &files);
        assert_eq!(diags.len(), 3);
        assert!(diags[0].message.contains("A1:B2  and  B2:C3"));
        assert!(diags[0].message.contains("contested: B2"));
        assert!(diags[1].message.contains("A1:B2  and  A1:C3"));
        assert!(diags[1].message.contains("contested: A1:B2"));
        assert!(diags[2].message.contains("B2:C3  and  A1:C3"));
        assert!(diags[2].message.contains("contested: B2:C3"));
    }

    #[test]
    fn a_tab_of_disjoint_single_cell_files_has_no_overlap() {
        let files: Vec<(String, Rect)> = (0..500u32)
            .map(|i| (format!("cell{i}"), Rect::cell(i % 20, i / 20)))
            .collect();
        assert!(detect_overlaps("T", &files).is_empty());
    }

    #[test]
    fn an_array_region_too_large_to_index_is_still_detected() {
        // `A1:XFD1048576` is a legal filename declaring more coordinates than any index can hold.
        let files = vec![
            ("A1:XFD1048576".to_string(), rect(0, 0, 16383, 1048575)),
            ("C3".to_string(), Rect::cell(2, 2)),
        ];
        let diags = detect_overlaps("T", &files);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("A1:XFD1048576  and  C3"));
        assert!(diags[0].message.contains("contested: C3"));
    }

    #[test]
    fn two_regions_too_large_to_index_overlap_once_and_disjoint_ones_not_at_all() {
        let top = rect(0, 0, 16383, 500_000);
        let bottom = rect(0, 500_001, 16383, 1_048_575);
        let disjoint = vec![
            ("A1:XFD500001".to_string(), top),
            ("A500002:XFD1048576".to_string(), bottom),
        ];
        assert!(detect_overlaps("T", &disjoint).is_empty());

        let straddling = vec![
            ("A1:XFD500001".to_string(), top),
            ("A2:XFD1048576".to_string(), rect(0, 1, 16383, 1_048_575)),
        ];
        assert_eq!(detect_overlaps("T", &straddling).len(), 1);
    }

    /// Small shapes reach the scan, not the index, so this drives [`indexed_pairs`] directly.
    #[test]
    fn the_index_and_the_pairwise_scan_agree_on_every_shape() {
        // Consecutive 2x2 regions stepping one row at a time, so neighbours overlap.
        let chain: Vec<(String, Rect)> = (0..200u32)
            .map(|i| (format!("f{i}"), rect(0, i, 1, i + 1)))
            .collect();
        // Three files per coordinate, so every run is a 3-way contest.
        let stacked: Vec<(String, Rect)> = (0..300u32)
            .map(|i| (format!("f{i}"), Rect::cell(i % 100, 0)))
            .collect();
        let disjoint: Vec<(String, Rect)> = (0..500u32)
            .map(|i| (format!("f{i}"), Rect::cell(i % 20, i / 20)))
            .collect();
        for files in [chain, stacked, disjoint] {
            let mut indexed = indexed_pairs(&files).expect("the index accepts this shape");
            indexed.sort_unstable();
            indexed.dedup();
            assert_eq!(indexed, pairwise_pairs(&files));
        }
    }

    /// 1,500 files on one coordinate is 1,124,250 pairs, past [`PAIR_COMPACT_CAP`]. Every one of
    /// them is real and distinct, so compaction may drop none of them.
    #[test]
    fn compacting_a_massively_contested_coordinate_is_lossless() {
        let files: Vec<(String, Rect)> = (0..1_500u32)
            .map(|i| (format!("A1-{i}"), Rect::cell(0, 0)))
            .collect();
        let mut pairs = indexed_pairs(&files).expect("1,500 coordinates is well inside the budget");
        pairs.sort_unstable();
        pairs.dedup();
        assert_eq!(pairs.len(), 1_500 * 1_499 / 2);
    }

    /// The 5-second bound sits 200x above the linear time and 17x below the quadratic one.
    #[test]
    #[ignore = "100,000-file scaling pin: ~25 ms while linear, ~87 s if the pairwise scan returns"]
    fn a_hundred_thousand_disjoint_files_is_not_quadratic() {
        let files: Vec<(String, Rect)> = (0..100_000u32)
            .map(|i| (format!("f{i}"), Rect::cell(i % 128, i / 128)))
            .collect();
        let started = std::time::Instant::now();
        assert!(detect_overlaps("T", &files).is_empty());
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "100,000 disjoint files took {elapsed:?}: the detector is quadratic in files again"
        );
    }

    /// These 150 files declare 3,000,000 coordinates but contest only 11,175 pairs, so the
    /// PRE-FLIGHT check must decline the index — enumerating this shape would cost ~3.2 GiB.
    #[test]
    fn a_tab_where_every_file_overlaps_answers_in_bounded_memory() {
        let files: Vec<(String, Rect)> = (0..150u32)
            .map(|i| (format!("A{}:T{}", i + 1, i + 1000), rect(0, i, 19, i + 999)))
            .collect();
        assert_eq!(detect_overlaps("T", &files).len(), 150 * 149 / 2);
    }
}
