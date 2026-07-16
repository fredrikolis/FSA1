// Concern: the rectangular REGION a file claims (`Rect`, inclusive zero-based corners) and the OVERLAP detector — across a tab's files, find any two whose declared regions intersect and raise a located `Overlap` diagnostic naming BOTH files and the contested cells; precedence is REJECT, never a guessed winner (FORMAT §7) | Non-concern: parsing the filename that produced a `Rect` (filename.rs), and gaps between regions (a gap is Blank, not an error) | IO: (a tab name, its files' `(name, Rect)` claims) -> `Vec<Diagnostic>` (one per intersecting pair)
//! Region geometry and the overlap detector: [`Rect`], [`detect_overlaps`].

use crate::diagnostic::{Code, Diagnostic, Loc};
use charlie_ast::a1::format_cell;

/// A rectangular region of the grid, inclusive on all four corners, zero-based. A single cell is a
/// `Rect` with equal corners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub min_col: u32,
    pub min_row: u32,
    pub max_col: u32,
    pub max_row: u32,
}

impl Rect {
    /// A 1x1 region at `(col,row)`.
    pub fn cell(col: u32, row: u32) -> Rect {
        Rect {
            min_col: col,
            min_row: row,
            max_col: col,
            max_row: row,
        }
    }

    /// Whether the zero-based cell `(col,row)` lies inside this inclusive region. Used by the
    /// demand-driven evaluator to find the single file (overlaps are rejected at load) that covers a
    /// requested cell.
    pub fn contains(&self, col: u32, row: u32) -> bool {
        self.min_col <= col && col <= self.max_col && self.min_row <= row && row <= self.max_row
    }

    /// The intersection of two regions, or `None` if they are disjoint.
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

    /// Render as an A1 region: a single cell (`C2`) or a range (`C2:D3`).
    pub fn label(&self) -> String {
        let top_left = format_cell(self.min_col, self.min_row);
        if self.min_col == self.max_col && self.min_row == self.max_row {
            top_left
        } else {
            format!("{top_left}:{}", format_cell(self.max_col, self.max_row))
        }
    }
}

/// Detect every pair of files in one tab whose declared regions intersect. Each intersecting pair
/// yields one located [`Code::Overlap`] diagnostic naming both files and the contested cells; the
/// precedence rule is REJECT (FORMAT §7), so no winner is chosen. A disjoint set yields an empty
/// vec (gaps are Blank, not errors).
pub fn detect_overlaps(tab: &str, files: &[(String, Rect)]) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for i in 0..files.len() {
        for j in (i + 1)..files.len() {
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
    }
    out
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
        assert!(r.contains(1, 2)); // top-left corner
        assert!(r.contains(3, 4)); // bottom-right corner
        assert!(r.contains(2, 3)); // interior
        assert!(!r.contains(0, 2)); // left of region
        assert!(!r.contains(4, 4)); // right of region
        assert!(!r.contains(2, 5)); // below region
    }

    #[test]
    fn disjoint_regions_do_not_overlap() {
        // A1:B2 and C3:D4 are disjoint -> no diagnostic (a gap is Blank).
        let files = vec![
            ("A1:B2.range".to_string(), rect(0, 0, 1, 1)),
            ("C3:D4.range".to_string(), rect(2, 2, 3, 3)),
        ];
        assert!(detect_overlaps("Orders", &files).is_empty());
    }

    #[test]
    fn touching_but_not_overlapping_is_fine() {
        // A1:B2 and C1:D2 share an edge boundary but no cell (cols 0-1 vs 2-3).
        let files = vec![
            ("A1:B2.range".to_string(), rect(0, 0, 1, 1)),
            ("C1:D2.range".to_string(), rect(2, 0, 3, 1)),
        ];
        assert!(detect_overlaps("Orders", &files).is_empty());
    }

    #[test]
    fn range_and_cell_overlap_names_both_and_the_contested_cell() {
        // The FORMAT §7 worked example: A1:D3.range and C2.cell contest exactly C2.
        let files = vec![
            ("A1:D3.range".to_string(), rect(0, 0, 3, 2)),
            ("C2.cell".to_string(), Rect::cell(2, 1)),
        ];
        let diags = detect_overlaps("Orders", &files);
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.code, Code::Overlap);
        assert!(d.message.contains("A1:D3.range"));
        assert!(d.message.contains("C2.cell"));
        assert!(d.message.contains("contested: C2"));
        assert!(d.message.contains("reject"));
    }

    #[test]
    fn overlapping_ranges_report_the_contested_block() {
        // A1:C3 and B2:D4 contest the B2:C3 block.
        let files = vec![
            ("A1:C3.range".to_string(), rect(0, 0, 2, 2)),
            ("B2:D4.range".to_string(), rect(1, 1, 3, 3)),
        ];
        let diags = detect_overlaps("T", &files);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("contested: B2:C3"));
    }

    #[test]
    fn duplicate_cells_overlap() {
        let files = vec![
            ("C2.cell".to_string(), Rect::cell(2, 1)),
            ("C2.cell".to_string(), Rect::cell(2, 1)),
        ];
        assert_eq!(detect_overlaps("T", &files).len(), 1);
    }
}
