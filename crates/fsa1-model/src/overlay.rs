// Concern: holds a tab's default layer and its blocks, and answers what a coordinate or axis wears | Non-concern: a rule's grammar (presentation.rs), a figure's sidecar (figures.rs) | IO: dir -> Overlay
//! Presentation is off the engine's load path: a [`crate::Workbook`] cannot reach a sidecar, so a
//! value derives from content and references alone (VAL1) as a SHAPE rather than an assertion. The
//! resolvers take the workbook because the gap rule is the grid's: a coordinate no block reaches but
//! a range file covers wears an EMPTY style, and one nothing states wears none.

use std::collections::BTreeMap;
use std::path::Path;

use crate::declaration::{Chars, Points};
use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::filename::{parse_filename, parse_root};
use crate::geometry::{AxisRun, declared_heights, declared_widths};
use crate::names::{CssEntry, css_entry, figure_stems, is_tab_layer, presentation_stem};
use crate::overlap::Rect;
use crate::presentation::{Presentation, parse_rules_located, rules_of};
use crate::sidecar_scope::{
    Sidecar, SidecarScope, area, check_scope_nesting, check_tab_layer, scopes,
};
use crate::style::{CellStyle, resolve};
use crate::workbook::Workbook;

/// Keyed by tab NAME, so the two independent directory walks cannot drift into addressing different
/// tabs by one sheet index; every lookup spends a [`Workbook`] to spell the index it was handed.
#[derive(Clone, Debug, Default)]
pub struct Overlay {
    tabs: BTreeMap<String, TabOverlay>,
}

/// `default` is the tab's own `.css` — no stem, so its root is the tab's CONTENT rect, unioned from
/// the range filenames. It is beneath every block and in no area comparison with one.
#[derive(Clone, Debug, Default)]
struct TabOverlay {
    default: Option<Sidecar>,
    blocks: Vec<Sidecar>,
}

impl Overlay {
    /// The outer `io::Result` reports a filesystem failure and the inner one the sidecars' own
    /// refusals, exactly as [`Workbook::load_dir`] splits them.
    pub fn load_dir(root: &Path) -> std::io::Result<Result<Overlay, Vec<Diagnostic>>> {
        let mut entries: Vec<_> = std::fs::read_dir(root)?.collect::<Result<_, _>>()?;
        // Filename order is the order `Workbook::load_dir` gives its tabs, so a sheet index means the same on both sides without either holding the other.
        entries.sort_by_key(|e| e.file_name());
        let mut tabs: Vec<TabInput> = Vec::new();
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            if Workbook::is_reserved_entry(&name) || !entry.file_type()?.is_dir() {
                continue;
            }
            let (sidecars, content) = read_sidecar_dir(&entry.path())?;
            tabs.push((name, sidecars, content));
        }
        Ok(build(tabs))
    }

    /// `tabs` is a [`Workbook::from_tabs`] tree verbatim: the entries that are not sidecars are the
    /// range files, which are not read here but DO say how far the tab's content reaches.
    pub fn from_tabs(tabs: &[(&str, &[(&str, &str)])]) -> Result<Overlay, Vec<Diagnostic>> {
        let owned = tabs
            .iter()
            .map(|(tab, files)| {
                let mut sidecars = Vec::new();
                let mut content = None;
                // The whole tab's figures first: a `.css` beside `<stem>.json` is that figure's placement whatever its stem spells, and the listing is in no useful order.
                let figures = figure_stems(files.iter().map(|(name, _)| *name));
                for (name, text) in *files {
                    // An `Unrooted` is dropped here and in `read_sidecar_dir`, so it never reaches `read_sidecars` and never becomes a block: `figures.rs` alone judges it.
                    match css_entry(name, &figures) {
                        Some(CssEntry::TabLayer | CssEntry::Root(_)) => {
                            sidecars.push(((*name).to_string(), (*text).to_string()));
                        }
                        Some(CssEntry::Unrooted(_)) => {}
                        None => content = Rect::union(content, range_of(name)),
                    }
                }
                ((*tab).to_string(), sidecars, content)
            })
            .collect();
        build(owned)
    }

    /// Empty for a tab with no sidecar and for a sheet index the workbook does not name.
    fn blocks(&self, wb: &Workbook, sheet: u32) -> &[Sidecar] {
        self.tab(wb, sheet).map_or(&[], |t| t.blocks.as_slice())
    }

    /// The tab's sidecars as [`SidecarScope`]s; empty for a tab holding none.
    pub fn scopes(&self, wb: &Workbook, sheet: u32) -> Vec<SidecarScope<'_>> {
        self.tab(wb, sheet)
            .map_or_else(Vec::new, |tab| scopes(tab.default.as_ref(), &tab.blocks))
    }

    fn tab(&self, wb: &Workbook, sheet: u32) -> Option<&TabOverlay> {
        self.tabs.get(wb.sheet_name(sheet)?)
    }

    /// How far the tab reaches, over BOTH halves: the coordinates its files fill, and the block
    /// roots its sidecars name — a root no range file covers being a style-only region. The one
    /// name for a tab's extent, so a third contributor to it is added here and nowhere else.
    pub fn stated_region(&self, wb: &Workbook, sheet: u32) -> Option<Rect> {
        self.blocks(wb, sheet)
            .iter()
            .fold(wb.content_region(sheet), |acc, block| {
                Rect::union(acc, Some(block.root))
            })
    }

    /// `col` and `row` are zero-based and absolute. `Some` wherever the tab STATES something at the
    /// coordinate — a file covers it, or a block root does, or both — and `None` only where it
    /// states neither; a covered coordinate no block reaches is an EMPTY style, which is a different
    /// fact from a gap. Blocks layer in cascade order, so the narrowest root stands.
    pub fn cell_style(&self, wb: &Workbook, sheet: u32, col: u32, row: u32) -> Option<CellStyle> {
        let mut style: Option<CellStyle> = None;
        for block in self.blocks(wb, sheet) {
            let root = block.root;
            if root.contains(col, row) {
                let matched = resolve(
                    &block.presentation,
                    row - root.min_row + 1,
                    col - root.min_col + 1,
                );
                style.get_or_insert_default().layer(&matched);
            }
        }
        // Settled BEFORE the tab layer: the layer is a DEFAULT, so it may not be what makes a coordinate stated.
        let mut style = match style {
            Some(style) => style,
            None => wb.covers(sheet, col, row).then(CellStyle::default)?,
        };
        if let Some(layer) = self
            .tab(wb, sheet)
            .and_then(|t| t.default.as_ref())
            .filter(|layer| layer.root.contains(col, row))
        {
            let root = layer.root;
            let mut under = resolve(
                &layer.presentation,
                row - root.min_row + 1,
                col - root.min_col + 1,
            );
            under.layer(&style);
            style = under;
        }
        // An axis size belongs to the AXIS: resolved per coordinate, one column renders two widths.
        style.width = axis_size(&self.column_widths(wb, sheet), col);
        style.height = axis_size(&self.row_heights(wb, sheet), row);
        Some(style)
    }

    /// The sheet columns this tab's sidecars size, ascending, disjoint and coalesced — what a
    /// `<col min= max= width=>` run is. Two blocks may size one axis differently and neither is a
    /// fault: the cascade answers it, so the SMALLEST root stands, ties to the later name. An axis
    /// no block sizes is absent rather than defaulted.
    pub fn column_widths(&self, wb: &Workbook, sheet: u32) -> Vec<AxisRun<Chars>> {
        self.axis_runs(wb, sheet, declared_widths)
    }

    /// [`Overlay::column_widths`] on the other axis.
    pub fn row_heights(&self, wb: &Workbook, sheet: u32) -> Vec<AxisRun<Points>> {
        self.axis_runs(wb, sheet, declared_heights)
    }

    /// Two blocks' runs may interleave and part-overlap, so they are merged one axis at a time and
    /// re-coalesced rather than intersected pairwise. The overwrite IS the cascade read on an axis:
    /// blocks come in cascade order, so the last one sizing a given axis is the one that stands.
    fn axis_runs<T: Copy + PartialEq>(
        &self,
        wb: &Workbook,
        sheet: u32,
        declared: fn(Rect, &Presentation) -> Vec<AxisRun<T>>,
    ) -> Vec<AxisRun<T>> {
        let mut sized: BTreeMap<u32, T> = BTreeMap::new();
        // The tab layer first, so any block sizing the same axis is layered over it.
        let tab = self.tab(wb, sheet);
        if let Some(layer) = tab.and_then(|t| t.default.as_ref()) {
            for run in declared(layer.root, &layer.presentation) {
                for axis in run.start..=run.end {
                    sized.insert(axis, run.size);
                }
            }
        }
        for block in self.blocks(wb, sheet) {
            for run in declared(block.root, &block.presentation) {
                for axis in run.start..=run.end {
                    sized.insert(axis, run.size);
                }
            }
        }
        let mut runs: Vec<AxisRun<T>> = Vec::new();
        for (axis, size) in sized {
            match runs.last_mut() {
                Some(run) if run.end + 1 == axis && run.size == size => run.end = axis,
                _ => runs.push(AxisRun {
                    start: axis,
                    end: axis,
                    size,
                }),
            }
        }
        runs
    }
}

fn axis_size<T: Copy>(runs: &[AxisRun<T>], index: u32) -> Option<T> {
    runs.iter()
        .find(|r| index >= r.start && index <= r.end)
        .map(|r| r.size)
}

/// A tab's presentation entries, and how far its range files reach.
type TabInput = (String, Vec<(String, String)>, Option<Rect>);

/// What one tab directory yields: its presentation entries, and its content rect.
type TabEntries = (Vec<(String, String)>, Option<Rect>);

fn build(tabs: Vec<TabInput>) -> Result<Overlay, Vec<Diagnostic>> {
    let mut diags: Vec<Diagnostic> = Vec::new();
    let mut out = BTreeMap::new();
    for (tab, entries, content) in tabs {
        let (layer, sidecars): (Vec<_>, Vec<_>) = entries
            .into_iter()
            // The layer is the SUFFIX alone, so it needs no tab: only the two callers above, which have already dropped every `Unrooted`, separate a root from a placement.
            .partition(|(name, _)| is_tab_layer(name));
        let read = layer.into_iter().next().and_then(|(name, text)| {
            let file = format!("{tab}/{name}");
            let Some(root) = content else {
                diags.push(Diagnostic::new(
                    Code::PresentationSelector,
                    Loc::file(&file),
                    "a tab's own stylesheet counts its indices in the tab's content, and this tab                      states none; name the region on the file instead: <range>.css"
                        .to_string(),
                ));
                return None;
            };
            match parse_rules_located(&file, root, &text) {
                Ok(read) => Some((root, file, text, read)),
                Err(d) => {
                    diags.extend(d);
                    None
                }
            }
        });
        let blocks = read_sidecars(&tab, sidecars, content, &mut diags);
        check_scope_nesting(&blocks, &mut diags);
        // The layer's rules are judged BEFORE they are stripped of their positions, so a refusal on one lands on the line the author wrote it.
        let default = read.map(|(root, file, text, read)| {
            if !blocks.is_empty() {
                check_tab_layer(&file, &read.rules, &mut diags);
            }
            Sidecar {
                root,
                file,
                text,
                presentation: rules_of(read.rules),
                uncarried: read.uncarried,
            }
        });
        out.insert(tab, TabOverlay { default, blocks });
    }
    if diags.is_empty() {
        Ok(Overlay { tabs: out })
    } else {
        Err(diags)
    }
}

/// Sorted, so two sidecars of equal area are cascaded in one order whatever the directory yields.
fn read_sidecar_dir(dir: &Path) -> std::io::Result<TabEntries> {
    let mut out = Vec::new();
    let mut content = None;
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    // The listing is settled BEFORE anything is classified: `Chart1.css` sorts before `Chart1.json`, so the sibling that makes it a placement is not yet in hand mid-walk.
    let mut files: Vec<(String, std::path::PathBuf)> = Vec::new();
    for entry in entries {
        if entry.file_type()?.is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            files.push((name, entry.path()));
        }
    }
    let figures = figure_stems(files.iter().map(|(name, _)| name.as_str()));
    for (name, path) in files {
        match css_entry(&name, &figures) {
            Some(CssEntry::TabLayer | CssEntry::Root(_)) => {
                out.push((name, std::fs::read_to_string(path)?));
            }
            Some(CssEntry::Unrooted(_)) => {}
            None => content = Rect::union(content, range_of(&name)),
        }
    }
    Ok((out, content))
}

/// How far a tab's CONTENT reaches, read off the range filenames alone. A name the parser rejects
/// contributes nothing: the workbook load is what refuses it, and this pass must not refuse it twice.
fn range_of(name: &str) -> Option<Rect> {
    parse_filename(&crate::canonical_range_name(name))
        .ok()
        .map(|parsed| parsed.region)
}

/// The tab's sidecars, read against the root each is NAMED for, then laid in cascade order: widest
/// root first so the narrowest reaching a coordinate is the last layered over it, ties settled by
/// canonical filename. Total over distinct roots, so every coordinate has exactly one winner.
fn read_sidecars(
    tab: &str,
    sidecars: Vec<(String, String)>,
    content: Option<Rect>,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Sidecar> {
    let mut read: Vec<(String, Sidecar)> = Vec::new();
    for (name, text) in sidecars {
        let stem = presentation_stem(&name).expect("the classifier admitted only sidecars");
        let located = format!("{tab}/{name}");
        match parse_root(stem) {
            // An open root clamps to the tab's content, so a tab stating none reaches nothing: a no-op, never a refusal.
            Ok(root) => match root.resolve(content) {
                None => continue,
                Some(region) => match parse_rules_located(&located, region, &text) {
                    // Keyed by the RESOLVED region: contention is settled there, so two names reaching one region are what cannot be ordered.
                    Ok(parsed) => read.push((
                        region.label(),
                        Sidecar {
                            root: region,
                            file: located,
                            text,
                            presentation: rules_of(parsed.rules),
                            uncarried: parsed.uncarried,
                        },
                    )),
                    Err(d) => diags.extend(d),
                },
            },
            Err(d) => diags.push(Diagnostic::new(d.code, Loc::file(&located), d.message)),
        }
    }
    // Two spellings of one root canonicalize alike, so no order separates them. Refused, not ordered.
    let mut seen: Vec<&String> = Vec::new();
    for (key, _) in &read {
        if seen.contains(&key) {
            diags.push(Diagnostic::new(
                Code::DuplicateSidecarRoot,
                Loc::file(&format!("{tab}/{key}")),
                format!(
                    "two sidecars state the presentation of {key}; one root is stated once, however \
                     its name is spelled -- delete or merge the duplicate"
                ),
            ));
        }
        seen.push(key);
    }
    read.sort_by(|(a_key, a), (b_key, b)| {
        area(b.root)
            .cmp(&area(a.root))
            .then_with(|| a_key.cmp(b_key))
    });
    read.into_iter().map(|(_, sidecar)| sidecar).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::declaration::{FontWeight, Rgb, TextAlign};
    use crate::workbook::Workbook;

    const PLUM: Rgb = Rgb {
        r: 0x3f,
        g: 0x04,
        b: 0x21,
    };
    const WHITE: Rgb = Rgb {
        r: 0xff,
        g: 0xff,
        b: 0xff,
    };

    /// One tree, two loads — the shape a verb that draws presentation performs on a directory.
    fn over(files: &[(&str, &str)]) -> (Workbook, Overlay) {
        let wb = Workbook::from_tabs(&[("Sheet1", files)])
            .unwrap_or_else(|d| panic!("{files:?} should load: {d:?}"));
        let overlay = Overlay::from_tabs(&[("Sheet1", files)])
            .unwrap_or_else(|d| panic!("{files:?}'s sidecars should load: {d:?}"));
        (wb, overlay)
    }

    fn refusals(files: &[(&str, &str)]) -> Vec<Diagnostic> {
        Overlay::from_tabs(&[("Sheet1", files)]).expect_err("these sidecars must refuse")
    }

    #[test]
    fn a_covered_coordinate_wears_the_style_its_sidecar_resolves_to() {
        let (wb, overlay) = over(&[
            ("A1:A2", "3\n4"),
            ("A1:A2.css", "  fsa1-cell { text-align: right }\n"),
        ]);
        assert_eq!(
            overlay
                .cell_style(&wb, 0, 0, 0)
                .expect("A1 is styled")
                .text_align,
            Some(TextAlign::Right),
        );
    }

    /// Every fault at once, so an author fixes a sidecar in one pass rather than one refusal a run.
    #[test]
    fn a_refused_sidecar_is_refused_with_every_fault_at_once() {
        let codes: Vec<Code> =
            refusals(&[("A1:A2", "3\n4"), ("A1:A2.css", "  th { color: red }\n")])
                .iter()
                .map(|d| d.code)
                .collect();
        assert_eq!(codes, vec![Code::PresentationSelector]);
    }

    /// A sidecar's NAME is its root, so every MALFORMED root a range file refuses it refuses too,
    /// located on the sidecar. The open forms are the one place the two grammars part: a sidecar
    /// states a region and a grid must FILL one, so `A:A` is a root here and 1,048,576 rows there.
    #[test]
    fn a_sidecar_name_is_refused_for_every_root_a_range_file_would_refuse() {
        for (name, want) in [
            ("A1:A1.css", Code::DegenerateRange),
            ("a1:c3.css", Code::LowercaseColumn),
            ("C3:A1.css", Code::NonCanonicalRange),
            ("A01.css", Code::LeadingZeroRow),
        ] {
            let diags = refusals(&[
                ("A1:C3", "1\t2\t3\n4\t5\t6\n7\t8\t9"),
                (name, "  fsa1-cell { color: #3f0421 }\n"),
            ]);
            assert!(
                diags.iter().any(|d| d.code == want),
                "{name} should earn {want:?}: {diags:?}"
            );
            assert!(
                diags.iter().any(|d| d.loc.to_string().contains(name)),
                "{name}'s refusal must be located on it: {diags:?}"
            );
        }
    }

    /// An open root is spelled two ways on two hosts and reaches one region either way, and its
    /// canonical spelling is enforced exactly as a closed one's is — otherwise four names state one
    /// root and the cascade is handed a contest it cannot settle.
    #[test]
    fn an_open_root_is_one_root_however_it_is_spelled() {
        let (wb, overlay) = over(&[
            ("A1:B2", "1\t2\n3\t4"),
            ("A-A.css", "  fsa1-cell { color: #3f0421 }\n"),
        ]);
        assert!(
            overlay
                .cell_style(&wb, 0, 0, 0)
                .and_then(|s| s.color)
                .is_some(),
            "the portable `-` spelling is the same root as `:`",
        );
        for (name, want) in [
            ("a:a.css", Code::LowercaseColumn),
            ("01:01.css", Code::LeadingZeroRow),
        ] {
            let diags = refusals(&[
                ("A1:B2", "1\t2\n3\t4"),
                (name, "  fsa1-cell { color: #3f0421 }\n"),
            ]);
            assert!(
                diags.iter().any(|d| d.code == want),
                "{name} should earn {want:?}: {diags:?}",
            );
        }
        let diags = refusals(&[
            ("A1:B2", "1\t2\n3\t4"),
            ("A:A.css", "  fsa1-cell { color: #3f0421 }\n"),
            ("A1:A2.css", "  fsa1-cell { font-weight: bold }\n"),
        ]);
        assert!(
            diags.iter().any(|d| d.code == Code::DuplicateSidecarRoot),
            "`A:A` over a two-row tab IS `A1:A2`, and one region is stated once: {diags:?}",
        );
    }

    /// The open forms, which a RANGE file still refuses: the filename grammar parts from the
    /// sidecar's exactly here, and nowhere else.
    #[test]
    fn an_open_root_loads_on_a_sidecar_and_is_still_refused_on_a_range_file() {
        // A column root and a row root CROSS, so each is loaded over its own tree; `over` panics on a refusal, and loading at all is the assertion.
        let grid = ("A1:C3", "1\t2\t3\n4\t5\t6\n7\t8\t9");
        let (wb, overlay) = over(&[grid, ("A:A.css", "  fsa1-cell { width: 30ch }\n")]);
        assert_eq!(
            overlay.cell_style(&wb, 0, 0, 0).and_then(|s| s.width),
            Some(Chars(30.0)),
            "`A:A` clamps to the tab's content and sizes column A",
        );
        let (wb, overlay) = over(&[grid, ("2:2.css", "  fsa1-cell { height: 20pt }\n")]);
        assert_eq!(
            overlay.cell_style(&wb, 0, 2, 1).and_then(|s| s.height),
            Some(Points(20.0)),
            "`2:2` clamps on the other axis",
        );
        for name in ["A:A", "3:3"] {
            let refused = Workbook::from_tabs(&[("Sheet1", &[(name, "1")])])
                .expect_err("a GRID file may not name an open range");
            assert!(
                refused
                    .iter()
                    .any(|d| d.code == Code::WholeColumnRowReserved),
                "{name} as a grid file: {refused:?}",
            );
        }
    }

    /// Two spellings of one root canonicalize alike, so no order separates them: refused, never
    /// ordered.
    #[test]
    fn two_spellings_of_one_root_are_refused_rather_than_ordered() {
        let diags = refusals(&[
            ("A1:C3", "1\t2\t3\n4\t5\t6\n7\t8\t9"),
            ("A1:C3.css", "  fsa1-cell { color: #3f0421 }\n"),
            ("A1-C3.css", "  fsa1-cell { color: #ffffff }\n"),
        ]);
        assert!(
            diags.iter().any(|d| d.code == Code::DuplicateSidecarRoot),
            "{diags:?}"
        );
    }

    /// Decision 3 and decision 4 read against the encoder: every tree `unpack` writes must still
    /// load, its tab layer sizing an axis and its blocks a disjoint covering partition whose only
    /// nesting is the single cell a finer rule cannot reach.
    #[test]
    fn the_trees_the_encoder_writes_still_load() {
        let arial = "  fsa1-cell { font-family: Arial }\n";
        let block = "1\t2\t3\t4\t5\n".repeat(5);
        let block = block.trim_end_matches('\n');
        over(&[
            ("A1:E5", block),
            ("A56:E60", block),
            (".css", "  fsa1-cell:nth-child(2) { width: 13ch }\n"),
            ("A1:E5.css", arial),
            ("A56:E60.css", arial),
        ]);
        over(&[
            ("A1:C3", "1\t2\t3\n4\t5\t6\n7\t8\t9"),
            (
                "A1:C3.css",
                "  fsa1-row:first-child fsa1-cell { font-size: 14pt }\n",
            ),
            ("C2.css", "  fsa1-cell { font-size: 20pt }\n"),
            ("C3.css", "  fsa1-cell { font-size: 20pt }\n"),
        ]);
    }

    /// The contention the filesystem no longer arbitrates: two roots over one coordinate LAYER
    /// property by property, the SMALLER area last and so winning, and neither the overlap nor the
    /// disagreement is a fault. The tree order is reversed here because the cascade is the areas'.
    #[test]
    fn overlapping_sidecars_layer_the_smaller_root_last() {
        let table = (
            "A1:C3.css",
            "  fsa1-cell { color: #3f0421; font-weight: bold }\n",
        );
        let inner = ("B2.css", "  fsa1-cell { color: #ffffff }\n");
        for entries in [[table, inner], [inner, table]] {
            let mut files = vec![("A1:C3", "1\t2\t3\n4\t5\t6\n7\t8\t9")];
            files.extend(entries);
            let (wb, overlay) = over(&files);
            let b2 = overlay.cell_style(&wb, 0, 1, 1).expect("B2 is covered");
            assert_eq!(b2.color, Some(WHITE), "the narrower root wins the property");
            assert_eq!(
                b2.font_weight,
                Some(FontWeight::Bold),
                "and takes back none it does not declare",
            );
            assert_eq!(
                overlay.cell_style(&wb, 0, 0, 0).expect("A1").color,
                Some(PLUM)
            );
        }
    }

    /// Two sidecars sizing one sheet column differently is the cascade's question and not the
    /// filesystem's, so the tab overlays clean and column A renders ONE width.
    #[test]
    fn two_sidecars_sizing_one_column_differently_are_no_fault_at_all() {
        let (wb, overlay) = over(&[
            ("A1:A4", "1\n2\n3\n4"),
            ("A1:A2.css", "  fsa1-cell { width: 10ch }\n"),
            ("A3:A4.css", "  fsa1-cell { width: 12ch }\n"),
        ]);
        assert_eq!(
            overlay.column_widths(&wb, 0),
            vec![AxisRun {
                start: 0,
                end: 0,
                size: Chars(12.0)
            }],
        );
    }

    /// A figure's placement sidecar names no range, so it is no block and no coordinate wears it:
    /// the cascade never sees it, and `figures.rs` alone judges it.
    #[test]
    fn a_figures_sidecar_is_no_block_of_the_cascade() {
        let (wb, overlay) = over(&[("A1", "1"), ("Units.css", "  figure { anchor: D2 }\n")]);
        assert!(overlay.blocks(&wb, 0).is_empty());
        assert_eq!(overlay.stated_region(&wb, 0), Some(Rect::cell(0, 0)));
        assert_eq!(
            overlay.cell_style(&wb, 0, 0, 0),
            Some(CellStyle::default()),
            "A1 is covered by its range file and by nothing else",
        );
    }

    /// A block whose root no file covers is content of its own: it widens `stated_region`, and
    /// every coordinate under it answers `cell_style` even though `source_at` answers for none.
    #[test]
    fn a_style_only_region_is_styled_without_any_file() {
        let (wb, overlay) = over(&[
            ("A1", "1"),
            ("E1:G5.css", "  fsa1-cell { background-color: #00ffff }\n"),
        ]);
        assert_eq!(
            overlay.stated_region(&wb, 0),
            Some(Rect {
                min_col: 0,
                min_row: 0,
                max_col: 6,
                max_row: 4
            }),
        );
        let cyan = Rgb {
            r: 0,
            g: 0xff,
            b: 0xff,
        };
        for (col, row) in [(4, 0), (6, 4), (5, 2)] {
            assert!(wb.source_at(0, col, row).is_none(), "({col},{row})");
            assert_eq!(
                overlay
                    .cell_style(&wb, 0, col, row)
                    .unwrap_or_else(|| panic!("({col},{row}) is under a root"))
                    .background_color,
                Some(cyan),
            );
        }
        assert!(
            overlay.cell_style(&wb, 0, 3, 0).is_none(),
            "D1 is stated by nothing"
        );
    }
}
