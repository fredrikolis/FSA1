// Concern: states which tab and which A1 rectangle a verb demands read | Non-concern: reading anything, classifying a diagnostic's Loc | IO: (tab, Rect or Root) -> bool

use crate::filename::Root;
use crate::overlap::Rect;

#[derive(Clone, Debug, Default)]
pub struct Scope {
    tab: Option<String>,
    rect: Option<Rect>,
}

impl Scope {
    pub fn unscoped() -> Scope {
        Scope::default()
    }

    pub fn new(tab: Option<String>, rect: Option<Rect>) -> Scope {
        Scope { tab, rect }
    }

    pub fn is_scoped(&self) -> bool {
        self.tab.is_some() || self.rect.is_some()
    }

    pub fn tab(&self) -> Option<&str> {
        self.tab.as_deref()
    }

    pub fn rect(&self) -> Option<Rect> {
        self.rect
    }

    /// Both `None` arguments are permissive, not excluding: a caller that cannot say which tab it is
    /// on survives the tab test, and one with no region survives the rect test.
    pub fn wants(&self, tab: Option<&str>, region: Option<Rect>) -> bool {
        if !self.wants_tab(tab) {
            return false;
        }
        match (self.rect, region) {
            (Some(scope_rect), Some(r)) => r.intersect(&scope_rect).is_some(),
            (Some(_), None) => true,
            (None, _) => true,
        }
    }

    /// A sidecar is admitted per AXIS, not per rectangle: a size declared in a root reaches every
    /// sheet column or row that root spans, so `A100:A200.css` can size column A of a demand for
    /// `A1:B5`. An OPEN axis meets every span, which is what admits `A:A` and `1:9` with no case of
    /// their own.
    pub fn wants_root(&self, tab: Option<&str>, root: Root) -> bool {
        if !self.wants_tab(tab) {
            return false;
        }
        let Some(demand) = self.rect else {
            return true;
        };
        let (cols, rows) = axis_spans(root);
        meets(cols, demand.min_col, demand.max_col) || meets(rows, demand.min_row, demand.max_row)
    }

    fn wants_tab(&self, tab: Option<&str>) -> bool {
        match (&self.tab, tab) {
            (Some(want), Some(t)) => t == want,
            _ => true,
        }
    }
}

/// One axis of a root, first to last, or `None` where that axis is OPEN.
type AxisSpan = Option<(u32, u32)>;

/// A root's column span and its row span.
fn axis_spans(root: Root) -> (AxisSpan, AxisSpan) {
    match root {
        Root::Closed(r) => (Some((r.min_col, r.max_col)), Some((r.min_row, r.max_row))),
        Root::Columns { first, last } => (Some((first, last)), None),
        Root::Rows { first, last } => (None, Some((first, last))),
    }
}

fn meets(span: AxisSpan, first: u32, last: u32) -> bool {
    span.is_none_or(|(a, b)| a <= last && first <= b)
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
    fn unscoped_includes_everything() {
        let s = Scope::unscoped();
        assert!(!s.is_scoped());
        assert!(s.wants(Some("Sheet1"), Some(Rect::cell(0, 0))));
        assert!(s.wants(None, None));
    }

    #[test]
    fn tab_scope_excludes_other_tabs_and_keeps_file_level() {
        let s = Scope::new(Some("Beta".to_string()), None);
        assert!(s.is_scoped());
        assert!(s.wants(Some("Beta"), Some(Rect::cell(3, 3))));
        assert!(!s.wants(Some("Alpha"), Some(Rect::cell(3, 3))));
        assert!(s.wants(Some("Beta"), None));
        assert!(s.wants(None, Some(Rect::cell(3, 3))));
    }

    #[test]
    fn rect_scope_keeps_intersecting_regions_only() {
        let s = Scope::new(None, Some(rect(0, 0, 1, 1))); // A1:B2, zero-based
        assert!(s.wants(Some("Sheet1"), Some(Rect::cell(0, 0)))); // A1 in scope
        assert!(!s.wants(Some("Sheet1"), Some(Rect::cell(3, 0)))); // D1 out of scope
        assert!(s.wants(Some("Sheet1"), None));
    }

    /// A root sizes an AXIS, so sharing one is enough to be read: the demand renders the column
    /// `A100:A200` widens even though it renders none of its rows. Disjoint on BOTH axes is the only
    /// exclusion, and an open axis is disjoint from nothing.
    #[test]
    fn a_root_is_wanted_where_it_shares_an_axis_with_the_demand() {
        let s = Scope::new(Some("Sheet1".to_string()), Some(rect(0, 0, 1, 4))); // A1:B5
        assert!(s.wants_root(Some("Sheet1"), Root::Closed(rect(0, 99, 0, 199)))); // A100:A200
        assert!(s.wants_root(Some("Sheet1"), Root::Closed(rect(4, 0, 6, 2)))); // E1:G3
        assert!(!s.wants_root(Some("Sheet1"), Root::Closed(rect(7, 9, 7, 18)))); // H10:H19
        assert!(!s.wants_root(Some("Sheet2"), Root::Closed(rect(0, 0, 1, 1))));
        assert!(s.wants_root(Some("Sheet1"), Root::Columns { first: 7, last: 7 })); // H:H
        assert!(s.wants_root(
            Some("Sheet1"),
            Root::Rows {
                first: 99,
                last: 199
            }
        )); // 100:200
    }
}
