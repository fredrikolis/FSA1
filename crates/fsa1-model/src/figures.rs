// Concern: finds a workbook's figures, decides which a demand admits, holds them per tab and where each sits | Non-concern: what a name MEANS, a spec's grammar or binding | IO: (dir, demand) -> Figures

use std::collections::BTreeMap;
use std::path::Path;

use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::figure::{Binding, Figure};
use crate::names::{
    CssEntry, FIGURE_SUFFIX, PRESENTATION_SUFFIX, css_entry, figure_occupancy, figure_stems,
    is_figure_entry, stem_region,
};
use crate::placement::Placement;
use crate::scope::Scope;
use crate::workbook::Workbook;

/// Every figure a workbook holds, keyed by TAB — the shape [`crate::Overlay::load_dir`] has, and
/// needing no [`Workbook`]: a figure's JSON parses without one.
#[derive(Clone, Debug, Default)]
pub struct Figures {
    tabs: BTreeMap<String, Vec<Figure>>,
    /// Keyed by the figure's LOCATED name — `<tab>/<figure>.json` — so a placement and the spec it
    /// places are found by one key however the two files were ordered on disk.
    placements: BTreeMap<String, Placement>,
}

impl Figures {
    /// The outer `io::Result` reports a filesystem failure and the inner one the figures' own
    /// refusals, exactly as [`Workbook::load_dir`] splits them. A ROOT-level `.json` is not read
    /// here: the workbook load is what refuses it, and this pass must not refuse it twice.
    pub fn load_dir(root: &Path) -> std::io::Result<Result<Figures, Vec<Diagnostic>>> {
        Figures::load_dir_scoped(root, &Scope::unscoped())
    }

    /// [`Figures::load_dir`] under a demand. A `<range>.json` states its extent in its NAME, so the
    /// demand answers it unopened while its rectangle is recorded either way; a NAME-form figure
    /// hides its placement in `<name>.css`, which costs a read, so a demand stating a rect opens
    /// neither. BYTES are all a demand withholds: what the NAMES settle it grades regardless.
    pub fn load_dir_scoped(
        root: &Path,
        demand: &Scope,
    ) -> std::io::Result<Result<Figures, Vec<Diagnostic>>> {
        let mut entries: Vec<_> = std::fs::read_dir(root)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        let mut tabs: BTreeMap<String, Vec<Figure>> = BTreeMap::new();
        let mut placements: BTreeMap<String, Placement> = BTreeMap::new();
        let mut diags = Vec::new();
        for entry in entries {
            let tab = entry.file_name().to_string_lossy().into_owned();
            if Workbook::is_reserved_entry(&tab)
                || !entry.file_type()?.is_dir()
                || !demand.wants(Some(&tab), None)
            {
                continue;
            }
            let mut entries: Vec<_> = std::fs::read_dir(entry.path())?.collect::<Result<_, _>>()?;
            entries.sort_by_key(|e| e.file_name());
            // The listing is settled BEFORE anything is classified: `Chart1.css` sorts before `Chart1.json`, so the sibling that makes it a placement is not yet in hand mid-walk.
            let mut files: Vec<(String, std::path::PathBuf)> = Vec::new();
            for f in entries {
                if f.file_type()?.is_file() {
                    files.push((f.file_name().to_string_lossy().into_owned(), f.path()));
                }
            }
            let stems = figure_stems(files.iter().map(|(name, _)| name.as_str()));
            let mut found = Vec::new();
            // Collected rather than resolved in place: `Units.css` sorts before `Units.json`, so the figure a sidecar names may not have been read yet.
            let mut sidecars: Vec<(String, String)> = Vec::new();
            for (name, path) in files {
                if let Some(CssEntry::Unrooted(stem)) = css_entry(&name, &stems) {
                    let located = format!("{tab}/{name}");
                    // A range-form figure already states its placement, so a sidecar over it has nothing left to say and is a refusal rather than a second answer -- one the NAMES settle, so the demand gates it on that figure alone and no read is spent either way.
                    if let Some(rect) = stem_region(stem) {
                        if demand.wants(Some(&tab), Some(rect)) {
                            diags.push(Diagnostic::new(
                                Code::FigureSidecarClash,
                                Loc::file(&located),
                                format!(
                                    "{stem}{FIGURE_SUFFIX} is named for the range {} it fills, so \
                                     {located} contradicts it; delete the sidecar, or rename the \
                                     figure to a name",
                                    rect.label()
                                ),
                            ));
                        }
                        continue;
                    }
                    // A NAME-form figure hides its placement in these very bytes, so a demand stating a rect opens neither it nor them; a stem naming no figure at all states no extent to be excluded by, so it is read and graded under every demand.
                    if stems.contains(stem) && demand.rect().is_some() {
                        continue;
                    }
                    match std::fs::read_to_string(path) {
                        Ok(text) => sidecars.push((stem.to_string(), text)),
                        Err(e) => diags.push(Diagnostic::new(
                            Code::FigurePlacement,
                            Loc::file(&located),
                            format!("{located} cannot be read as text: {e}"),
                        )),
                    }
                    continue;
                }
                if !is_figure_entry(&name) {
                    continue;
                }
                let located = format!("{tab}/{name}");
                // Before the body is read, because the range form's placement is its NAME's: a spec that does not parse still sits exactly where it is named.
                match figure_occupancy(&name) {
                    Some(rect) => {
                        placements.insert(located.clone(), Placement::Cells(rect));
                        if !demand.wants(Some(&tab), Some(rect)) {
                            continue;
                        }
                    }
                    None if demand.rect().is_some() => continue,
                    None => {}
                }
                // Located, not propagated: `?` would refuse the whole workbook naming its root, so one stray binary called `<name>.json` would take the grid down with it.
                let text = match std::fs::read_to_string(path) {
                    Ok(text) => text,
                    Err(e) => {
                        diags.push(Diagnostic::new(
                            Code::FigureSyntax,
                            Loc::file(&located),
                            format!("{located} cannot be read as text: {e}"),
                        ));
                        continue;
                    }
                };
                match Figure::parse(&located, &text) {
                    Ok(figure) => found.push(figure),
                    Err(d) => diags.push(d),
                }
            }
            for (stem, text) in sidecars {
                let located = format!("{tab}/{stem}{PRESENTATION_SUFFIX}");
                if !stems.contains(&stem) {
                    diags.push(Diagnostic::new(
                        Code::UnclaimedSidecar,
                        Loc::file(&located),
                        format!(
                            "{located} names no range, so it places the figure {stem}{FIGURE_SUFFIX} \
                             -- and this tab holds none"
                        ),
                    ));
                    continue;
                }
                match Placement::parse(&located, &text) {
                    Ok(placement) => {
                        placements.insert(format!("{tab}/{stem}{FIGURE_SUFFIX}"), placement);
                    }
                    Err(d) => diags.push(d),
                }
            }
            if !found.is_empty() {
                tabs.insert(tab, found);
            }
        }
        Ok(if diags.is_empty() {
            Ok(Figures { tabs, placements })
        } else {
            Err(diags)
        })
    }

    /// Where `figure` sits, or `None` for one whose tab holds no sidecar naming it — which is the
    /// figure whose position the writer derives.
    pub fn placement(&self, figure: &Figure) -> Option<&Placement> {
        self.placements.get(&figure.name)
    }

    /// Empty for a tab stating no figure.
    pub fn in_tab(&self, tab: &str) -> &[Figure] {
        self.tabs.get(tab).map_or(&[], Vec::as_slice)
    }

    /// Tab order, then filename order — what `check` reports in and a document draws in.
    pub fn all(&self) -> impl Iterator<Item = (&str, &Figure)> {
        self.tabs
            .iter()
            .flat_map(|(tab, figures)| figures.iter().map(move |f| (tab.as_str(), f)))
    }

    /// The SYNTAX half of a binding's check, which is all a branch with no loadable [`Workbook`] can
    /// reach: there is nothing to resolve a reference against, so only its grammar is graded.
    pub fn binding_syntax(&self) -> Vec<Diagnostic> {
        self.all()
            .flat_map(|(_, figure)| {
                figure.bindings().into_iter().filter_map(move |text| {
                    Binding::parse(&text).err().map(|why| {
                        Diagnostic::new(Code::FigureBinding, Loc::file(&figure.name), why)
                    })
                })
            })
            .collect()
    }
}
