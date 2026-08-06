// Concern: narrows a check to one tab and/or one A1 rectangle | Non-concern: producing the diagnostics it filters | IO: (&Loc) -> (tab, Rect); (tab, Rect) -> bool

use crate::diagnostic::Loc;
use crate::filename::parse_filename;
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

    /// Both `None` arguments are permissive, not excluding: a diagnostic whose tab is unknown
    /// survives the tab filter, and one with no region survives the rect filter.
    pub fn includes(&self, tab: Option<&str>, region: Option<Rect>) -> bool {
        if let Some(want) = &self.tab
            && let Some(t) = tab
            && t != want
        {
            return false;
        }
        match (self.rect, region) {
            (Some(scope_rect), Some(r)) => r.intersect(&scope_rect).is_some(),
            (Some(_), None) => true,
            (None, _) => true,
        }
    }
}

/// What the loc ALONE reveals: a bare-filename loc is ambiguous across tabs, so it yields no tab
/// even though the diagnostic has one. `Workbook::lint_scoped` supplies the true tab itself.
pub fn loc_target(loc: &Loc) -> (Option<&str>, Option<Rect>) {
    match loc {
        Loc::File { name, .. } => (None, region_of(name)),
        Loc::Body { file, .. } => (None, region_of(file)),
        Loc::Tab { tab } => (Some(tab), None),
        Loc::TabFile { tab, name } => (Some(tab), region_of(name)),
    }
}

fn region_of(name: &str) -> Option<Rect> {
    parse_filename(name).ok().map(|f| f.region)
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
        assert!(s.includes(Some("Sheet1"), Some(Rect::cell(0, 0))));
        assert!(s.includes(None, None));
    }

    #[test]
    fn tab_scope_excludes_other_tabs_and_keeps_file_level() {
        let s = Scope::new(Some("Beta".to_string()), None);
        assert!(s.is_scoped());
        assert!(s.includes(Some("Beta"), Some(Rect::cell(3, 3))));
        assert!(!s.includes(Some("Alpha"), Some(Rect::cell(3, 3))));
        assert!(s.includes(Some("Beta"), None));
        assert!(s.includes(None, Some(Rect::cell(3, 3))));
    }

    #[test]
    fn rect_scope_keeps_intersecting_regions_only() {
        let s = Scope::new(None, Some(rect(0, 0, 1, 1))); // A1:B2, zero-based
        assert!(s.includes(Some("Sheet1"), Some(Rect::cell(0, 0)))); // A1 in scope
        assert!(!s.includes(Some("Sheet1"), Some(Rect::cell(3, 0)))); // D1 out of scope
        assert!(s.includes(Some("Sheet1"), None));
    }

    #[test]
    fn loc_target_reads_tab_and_region() {
        let tabfile = Loc::tab_file("Beta", "B2:C3");
        let (tab, region) = loc_target(&tabfile);
        assert_eq!(tab, Some("Beta"));
        assert_eq!(region, Some(rect(1, 1, 2, 2)));
        let body = Loc::body("D1", 1, 2);
        let (tab, region) = loc_target(&body);
        assert_eq!(tab, None);
        assert_eq!(region, Some(Rect::cell(3, 0)));
        let tabloc = Loc::tab("Alpha");
        let (tab, region) = loc_target(&tabloc);
        assert_eq!(tab, Some("Alpha"));
        assert_eq!(region, None);
    }
}
