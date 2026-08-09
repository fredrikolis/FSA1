// Concern: finds a workbook's figures on disk and holds them per tab | Non-concern: a spec's own grammar or its binding (figure.rs), drawing one | IO: dir -> Figures

use std::collections::BTreeMap;
use std::path::Path;

use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::figure::{Binding, Figure};
use crate::names::{
    CssEntry, FIGURE_SUFFIX, PRESENTATION_SUFFIX, css_entry, figure_stems, is_figure_entry,
};
use crate::placement::Placement;
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
        let mut entries: Vec<_> = std::fs::read_dir(root)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        let mut tabs: BTreeMap<String, Vec<Figure>> = BTreeMap::new();
        let mut placements: BTreeMap<String, Placement> = BTreeMap::new();
        let mut diags = Vec::new();
        for entry in entries {
            let tab = entry.file_name().to_string_lossy().into_owned();
            if Workbook::is_reserved_entry(&tab) || !entry.file_type()?.is_dir() {
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
