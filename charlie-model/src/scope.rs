// Concern: the check-SCOPING predicate (CLI1) — a `Scope` (an optional tab name + an optional A1 `Rect`) and the rule deciding whether a located diagnostic falls within it, so `charlie-cli check` can report ONLY the diagnostics an agent's own cells carry on an import that ALSO holds pre-existing (GRID6) error cells elsewhere; a diagnostic that resolves to a cell region is in scope iff its region intersects the scope rect AND its tab matches, while a FILE-LEVEL diagnostic (no resolvable region, e.g. a whole-tab overlap) is in scope iff its tab is in scope | Non-concern: DETECTING diagnostics (the loader/lint own that), resolving the TRUE tab of a bare-filename loc (`Workbook::lint_scoped` supplies it — a `Body{file}`/`File{name}` loc alone is ambiguous across tabs), and the A1 range<->`Rect` grammar (render/filename own it) | IO: (a `Scope`, a diagnostic's resolved tab + region) -> a bool; (a `Loc`) -> its (tab, region) as far as the loc alone reveals
//! The `check --tab/--range/--cell` scoping predicate: [`Scope`] and [`loc_target`]. An unscoped
//! `Scope` includes every diagnostic (whole-workbook `check`, unchanged); a scoped one keeps only the
//! diagnostics whose location falls within the given tab/range, so an agent can validate exactly the
//! cells it authored on an import that carries unrelated pre-existing error cells.

use crate::diagnostic::Loc;
use crate::filename::parse_filename;
use crate::overlap::Rect;

/// A validation scope for `check`: an optional tab and an optional A1 rectangle. An unscoped `Scope`
/// (both `None`) includes every diagnostic — the whole-workbook check. A tab-only scope keeps every
/// diagnostic on that tab; a rect scope keeps every diagnostic whose cell region intersects it.
#[derive(Clone, Debug, Default)]
pub struct Scope {
    tab: Option<String>,
    rect: Option<Rect>,
}

impl Scope {
    /// The unscoped scope: includes every diagnostic (whole-workbook check).
    pub fn unscoped() -> Scope {
        Scope::default()
    }

    /// A scope over an optional tab and an optional A1 rectangle.
    pub fn new(tab: Option<String>, rect: Option<Rect>) -> Scope {
        Scope { tab, rect }
    }

    /// Whether any narrowing is in effect (a tab or a rect was given). An unscoped check short-circuits
    /// on this to return the full lint verbatim.
    pub fn is_scoped(&self) -> bool {
        self.tab.is_some() || self.rect.is_some()
    }

    /// The scope's tab, if one was given.
    pub fn tab(&self) -> Option<&str> {
        self.tab.as_deref()
    }

    /// Whether a diagnostic on `tab` (its TRUE tab, if known) covering `region` (its cell region, if its
    /// loc resolves to one) is in scope:
    /// * **tab filter** — a scope tab excludes a diagnostic whose known tab differs; a diagnostic whose
    ///   tab is unknown (a bare-filename loc surfacing from a workbook that would not even load) is not
    ///   excluded on the tab axis (best-effort — the structure is broken anyway).
    /// * **rect filter** — a scope rect keeps a diagnostic whose region intersects it; a FILE-LEVEL
    ///   diagnostic (no region) that passed the tab filter is kept (a whole-tab fault is reported
    ///   whenever its tab is in scope, even under a range scope).
    pub fn includes(&self, tab: Option<&str>, region: Option<Rect>) -> bool {
        if let Some(want) = &self.tab
            && let Some(t) = tab
            && t != want
        {
            return false;
        }
        match (self.rect, region) {
            (Some(scope_rect), Some(r)) => r.intersect(&scope_rect).is_some(),
            // A file-level diagnostic (no cell region) rides on the tab filter alone.
            (Some(_), None) => true,
            // No rect scope: the tab filter (above) is the whole decision.
            (None, _) => true,
        }
    }
}

/// The (tab, A1 region) a diagnostic's [`Loc`] points at, as far as the loc alone reveals: a
/// sheet-qualified loc ([`Loc::TabFile`]/[`Loc::Tab`]) yields its tab; a filename/body loc
/// ([`Loc::File`]/[`Loc::Body`]) yields the file's declared region (parsed best-effort — a malformed
/// filename yields no region) but carries NO tab. `Workbook::lint_scoped` supplies the true tab itself
/// (a bare-filename loc is ambiguous across tabs); the load-failed CLI path uses this directly.
pub fn loc_target(loc: &Loc) -> (Option<&str>, Option<Rect>) {
    match loc {
        Loc::File { name, .. } => (None, region_of(name)),
        Loc::Body { file, .. } => (None, region_of(file)),
        Loc::Tab { tab } => (Some(tab), None),
        Loc::TabFile { tab, name } => (Some(tab), region_of(name)),
    }
}

/// The declared region of a cell/range filename, or `None` if it is not a well-formed closed range.
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
        // A file-level (no-region) diagnostic on the in-scope tab is kept.
        assert!(s.includes(Some("Beta"), None));
        // An unknown-tab diagnostic is not excluded on the tab axis (best-effort, load-failed path).
        assert!(s.includes(None, Some(Rect::cell(3, 3))));
    }

    #[test]
    fn rect_scope_keeps_intersecting_regions_only() {
        // Scope A1:B2 (zero-based cols 0..1, rows 0..1).
        let s = Scope::new(None, Some(rect(0, 0, 1, 1)));
        assert!(s.includes(Some("Sheet1"), Some(Rect::cell(0, 0)))); // A1 in scope
        assert!(!s.includes(Some("Sheet1"), Some(Rect::cell(3, 0)))); // D1 out of scope
        // A file-level diagnostic (no region) is kept under a rect scope.
        assert!(s.includes(Some("Sheet1"), None));
    }

    #[test]
    fn loc_target_reads_tab_and_region() {
        // A sheet-qualified loc yields its tab and the file's region.
        let tabfile = Loc::tab_file("Beta", "B2:C3");
        let (tab, region) = loc_target(&tabfile);
        assert_eq!(tab, Some("Beta"));
        assert_eq!(region, Some(rect(1, 1, 2, 2)));
        // A body loc (a GRID6 load error) yields the file's region but NO tab.
        let body = Loc::body("D1", 1, 2);
        let (tab, region) = loc_target(&body);
        assert_eq!(tab, None);
        assert_eq!(region, Some(Rect::cell(3, 0)));
        // A tab loc has a tab but no region.
        let tabloc = Loc::tab("Alpha");
        let (tab, region) = loc_target(&tabloc);
        assert_eq!(tab, Some("Alpha"));
        assert_eq!(region, None);
    }
}
