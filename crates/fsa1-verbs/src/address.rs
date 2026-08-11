// Concern: resolves a <wb>[/<tab>[/<A1>|<name>|<file>]] path to a workbook, tab and region | Non-concern: what a verb does with it | IO: (a path, a demand) -> a target loaded under it, or a refusal
//! The workbook/tab boundary cannot be found by splitting the path on `/`, because the workbook path
//! itself contains `/` (`/tmp/x/Model`, `./demo`). It is found by load-probing each candidate prefix;
//! a trailing component's meaning then follows its position, whether it is a folder, and whether the
//! tab holds a file of that name.

use std::io;
use std::path::{Path, PathBuf};

use fsa1_model::{
    Diagnostic, EntryScope, NameTarget, Rect, Scope, Workbook, entry_scope, figure_stems,
    is_cell_filename, parse_viewport,
};

use crate::refusal::{Kind, Refusal, bad_arg, fail, refused};

/// What a verb wants READ. `Path` is the path's own tab and region, which the loader answers by
/// filename; `Whole` is every file in every tab, which the two directions with no
/// filename-answerable frontier need — a reverse trace inverts the forward map of the files it
/// LOADED, and an ad-hoc formula is not in the workbook, so neither seeds anything.
#[derive(Clone, Copy)]
pub enum Demand {
    Path,
    Whole,
}

/// The path's scope is what loads — its own files and the closure they reference, cross-tab refs
/// included — so a file the path never names is never opened. `root` is carried out because
/// presentation is a SECOND load off the same directory, which a verb that draws it performs itself.
pub struct Resolved {
    pub root: PathBuf,
    pub workbook: Workbook,
    pub tab: Option<u32>,
    region: Option<Rect>,
    /// The scope `workbook` was actually READ under, kept rather than re-derived so the second load
    /// a verb performs asks the identical question.
    demand: Scope,
    /// The raw selector text, kept for a located refusal message.
    selector: Option<String>,
    /// Whether that selector named a FILE. A name-form file settles the tab and states no region, so
    /// without this a verb taking `<wb>[/<tab>]` could not tell it from a bare tab path — and one
    /// written form would then have two behaviours, invisibly to the caller.
    file: bool,
}

/// A decomposition that does not require a successful load, for `check` — which must scope a workbook
/// that fails to load. `loaded` is the ONE content load under the path's own demand, carried out so
/// `check` reuses it.
pub struct Decomposed {
    pub root: PathBuf,
    pub tab: Option<String>,
    pub region: Option<Rect>,
    pub loaded: io::Result<Result<Workbook, Vec<Diagnostic>>>,
}

fn is_not_a_dir(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

fn no_such_name(seg: &str, scope: &str) -> Refusal {
    bad_arg(&format!(
        "{seg:?} is not a canonical A1 cell or range (e.g. B2, A1:D9), and no defined name {seg:?} \
         is in scope (tab {scope:?})"
    ))
}

fn name_not_a_region(seg: &str) -> Refusal {
    bad_arg(&format!(
        "{seg:?} is a named formula/constant, not a cell or range"
    ))
}

fn resolve_name(wb: &Workbook, name: &str, scope: &str) -> Result<(u32, Rect), Refusal> {
    match wb.name_table().resolve(name, scope) {
        None => Err(no_such_name(name, scope)),
        Some(NameTarget::Expr(_)) => Err(name_not_a_region(name)),
        Some(NameTarget::Ref(a1)) => name_ref_to_region(a1, scope, wb),
    }
}

/// `NameTable::build` does not check that a cross-sheet target's sheet exists, so the `tab_index` miss
/// below is reachable, not a build-guaranteed impossibility.
fn name_ref_to_region(a1: &str, scope_tab: &str, wb: &Workbook) -> Result<(u32, Rect), Refusal> {
    let (sheet, addr) = match a1.rsplit_once('!') {
        Some((s, a)) => (Some(unquote_sheet(s)), a),
        None => (None, a1),
    };
    let tab = match &sheet {
        Some(name) => wb.tab_index(name),
        None => wb.tab_index(scope_tab),
    };
    let Some(tab) = tab else {
        return Err(fail(
            Kind::NotFound,
            &format!("defined name target {a1:?} references a tab that does not exist"),
        ));
    };
    let rect = parse_viewport(addr).map_err(|_| {
        bad_arg(&format!(
            "defined name target {a1:?} is not a canonical A1 region"
        ))
    })?;
    Ok((tab, rect))
}

fn unquote_sheet(s: &str) -> String {
    match s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        Some(inner) => inner.replace("''", "'"),
        None => s.to_string(),
    }
}

struct Probe {
    wb_path: PathBuf,
    class: Class,
    loaded: io::Result<Result<Workbook, Vec<Diagnostic>>>,
}

enum Class {
    Root,
    Tab(String),
}

/// A pure `std::fs` read, so a parent whose other tab is broken still counts as workbook-shaped.
/// `file_type()` does not follow symlinks — matching `load_dir` — so a symlink-to-directory is never
/// counted as a tab sub-folder.
fn shaped_and_owns(parent: &Path, base: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return false;
    };
    let mut workbook_shaped = false;
    let mut owns_base = false;
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if Workbook::is_reserved_entry(&name) {
            continue;
        }
        if name == base {
            owns_base = true;
        }
        if !workbook_shaped && subfolder_has_cell_file(&entry.path()) {
            workbook_shaped = true;
        }
    }
    workbook_shaped && owns_base
}

fn subfolder_has_cell_file(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        is_cell_filename(&name)
    })
}

fn parent_base(p: &Path) -> Option<(PathBuf, String)> {
    let base = p.file_name()?.to_string_lossy().into_owned();
    let parent = match p.parent() {
        Some(par) if !par.as_os_str().is_empty() => par.to_path_buf(),
        _ => PathBuf::from("."),
    };
    Some((parent, base))
}

/// The probe reads NO cells: `structure` must settle the workbook root, and resolve a trailing
/// defined name, before the demand is known.
fn probe_scope(p: &Path) -> Probe {
    let loaded = Workbook::load_skeleton(p);
    match &loaded {
        Ok(Ok(wb)) if !wb.sheet_names().is_empty() => Probe {
            wb_path: p.to_path_buf(),
            class: Class::Root,
            loaded,
        },
        Ok(Ok(_)) => as_tab_of_parent(p).unwrap_or(Probe {
            wb_path: p.to_path_buf(),
            class: Class::Root,
            loaded,
        }),
        Ok(Err(_)) if !has_tab_dir(p) => as_tab_of_parent(p).unwrap_or(Probe {
            wb_path: p.to_path_buf(),
            class: Class::Root,
            loaded,
        }),
        _ => Probe {
            wb_path: p.to_path_buf(),
            class: Class::Root,
            loaded,
        },
    }
}

/// The reading a directory takes when a workbook-shaped parent owns it as a tab, which is the one
/// that survives: a tab is not a workbook, so what a load of it as a root said does not stand.
fn as_tab_of_parent(p: &Path) -> Option<Probe> {
    let (parent, base) = parent_base(p)?;
    if !shaped_and_owns(&parent, &base) {
        return None;
    }
    let loaded = Workbook::load_skeleton(&parent);
    Some(Probe {
        wb_path: parent,
        class: Class::Tab(base),
        loaded,
    })
}

/// A workbook root holds its tabs as folders, so one non-reserved subfolder is the whole test.
/// This is what places a load that FAILED: a directory with no tab folder is no root, so a parent
/// owning it as a tab is the reading, and the refusals raised against it as a root were speculative
/// — a name scoped to the workbook there that is well-formed scoped to the sheet is the live case.
fn has_tab_dir(p: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(p) else {
        return false;
    };
    entries.flatten().any(|e| {
        e.file_type().is_ok_and(|ft| ft.is_dir())
            && !Workbook::is_reserved_entry(&e.file_name().to_string_lossy())
    })
}

struct Structure {
    root: PathBuf,
    tab: Option<String>,
    region: Option<Rect>,
    selector: Option<String>,
    /// A peeled final non-A1 segment, resolved to a region by `resolve`/`decompose` — which hold the
    /// loaded workbook the lookup needs.
    name: Option<String>,
    /// Whether the final segment named a FILE the tab holds. A verb whose grammar takes no region
    /// must refuse such a segment whatever scope it states, because a name-form file states none.
    file: bool,
    /// The SKELETON: the listing and name phases, no cells.
    loaded: io::Result<Result<Workbook, Vec<Diagnostic>>>,
}

fn structure(path: &str) -> Result<Structure, Refusal> {
    let p = Path::new(path);
    let probe = probe_scope(p);
    match probe.class {
        Class::Tab(name) => Ok(Structure {
            root: probe.wb_path,
            tab: Some(name),
            region: None,
            selector: None,
            name: None,
            file: false,
            loaded: probe.loaded,
        }),
        Class::Root => match &probe.loaded {
            Ok(_) => Ok(Structure {
                root: probe.wb_path,
                tab: None,
                region: None,
                selector: None,
                name: None,
                file: false,
                loaded: probe.loaded,
            }),
            Err(e) if is_not_a_dir(e) => peel_selector(p),
            Err(e) => Err(fail(
                Kind::Io,
                &format!("cannot read {:?}: {e}", p.display()),
            )),
        },
    }
}

fn peel_selector(p: &Path) -> Result<Structure, Refusal> {
    let Some((prefix, last)) = parent_base(p) else {
        return Err(fail(
            Kind::NotFound,
            &format!("no such workbook directory {:?}", p.display()),
        ));
    };
    let pprobe = probe_scope(&prefix);
    match pprobe.class {
        Class::Tab(name) => finish_selector(pprobe.wb_path, Some(name), pprobe.loaded, &last),
        Class::Root => match &pprobe.loaded {
            Ok(_) => finish_selector(pprobe.wb_path, None, pprobe.loaded, &last),
            Err(e) if is_not_a_dir(e) => tab_position_error(&prefix),
            Err(e) => Err(fail(
                Kind::Io,
                &format!("cannot read {:?}: {e}", prefix.display()),
            )),
        },
    }
}

/// A defined name never parses as canonical A1, and `parse_viewport` refuses a trailing `.css` or
/// `.json`, so the file arm shadows neither neighbour. It answers only where a TAB IS SETTLED: a
/// NAME may fall back to the default tab because a namespace has one, but a filename is a LOCATION,
/// and listing whichever tab sorts first would turn one path's fate on tab ORDER.
fn finish_selector(
    root: PathBuf,
    tab: Option<String>,
    loaded: io::Result<Result<Workbook, Vec<Diagnostic>>>,
    last: &str,
) -> Result<Structure, Refusal> {
    if let Ok(rect) = parse_viewport(last) {
        return Ok(Structure {
            root,
            tab,
            region: Some(rect),
            selector: Some(last.to_string()),
            name: None,
            file: false,
            loaded,
        });
    }
    let held = match (&loaded, &tab) {
        (Ok(Ok(skeleton)), Some(tab_name)) => file_scope(&root, skeleton, tab_name, last)?,
        _ => None,
    };
    match held {
        Some(scope) => Ok(Structure {
            root,
            tab,
            region: match scope {
                EntryScope::Rect(rect) => Some(rect),
                EntryScope::Tab => None,
            },
            selector: Some(last.to_string()),
            name: None,
            file: true,
            loaded,
        }),
        None => Ok(Structure {
            root,
            tab,
            region: None,
            selector: Some(last.to_string()),
            name: Some(last.to_string()),
            file: false,
            loaded,
        }),
    }
}

/// The scope a file named `last` states, in the tab the path already settled. One `read_dir` — the
/// question [`subfolder_has_cell_file`] already asks — opening NO file, so what a path resolves to
/// never turns on any file's bytes. Directories and reserved entries are skipped as `list_tab_dir`
/// skips them; an IO fault refuses as IO rather than folding into `Ok(None)`, "no such entry".
fn file_scope(
    root: &Path,
    wb: &Workbook,
    tab: &str,
    last: &str,
) -> Result<Option<EntryScope>, Refusal> {
    let Some(idx) = wb.tab_index(tab) else {
        return Ok(None);
    };
    let dir = root.join(tab);
    let mut listing: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| cannot_read(&dir, e))? {
        let entry = entry.map_err(|e| cannot_read(&dir, e))?;
        // `file_type` does not follow symlinks, matching `list_tab_dir`.
        if entry
            .file_type()
            .map_err(|e| cannot_read(&dir, e))?
            .is_dir()
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !Workbook::is_reserved_entry(&name) {
            listing.push(name);
        }
    }
    if !listing.iter().any(|name| name == last) {
        return Ok(None);
    }
    let stems = figure_stems(listing.iter().map(String::as_str));
    Ok(entry_scope(last, &stems, wb.content_region(idx)))
}

fn cannot_read(dir: &Path, e: io::Error) -> Refusal {
    fail(Kind::Io, &format!("cannot read {:?}: {e}", dir.display()))
}

fn tab_position_error(prefix: &Path) -> Result<Structure, Refusal> {
    let Some((wb_path, badtab)) = parent_base(prefix) else {
        return Err(fail(
            Kind::NotFound,
            &format!("no such workbook directory {:?}", prefix.display()),
        ));
    };
    let wprobe = probe_scope(&wb_path);
    match wprobe.loaded {
        Ok(Ok(wb)) => Err(fail(
            Kind::NotFound,
            &format!(
                "no tab named {badtab:?} in {:?} (tabs: {:?})",
                wb_path.display(),
                wb.sheet_names()
            ),
        )),
        Ok(Err(diags)) => Err(refused(diags)),
        Err(e) if is_not_a_dir(&e) => Err(fail(
            Kind::NotFound,
            &format!("no such workbook directory {:?}", wb_path.display()),
        )),
        Err(e) => Err(fail(
            Kind::Io,
            &format!("cannot read {:?}: {e}", wb_path.display()),
        )),
    }
}

pub fn decompose(path: &str) -> Result<Decomposed, Refusal> {
    let s = structure(path)?;
    let mut tab = s.tab.clone();
    let mut region = s.region;
    // On a workbook that does not load, the name stays unresolved and `check` surfaces `loaded`.
    if let Some(name) = &s.name
        && let Ok(Ok(skeleton)) = &s.loaded
    {
        let scope = name_scope(skeleton, s.tab.as_deref());
        let (idx, rect) = resolve_name(skeleton, name, &scope)?;
        tab = skeleton
            .sheet_names()
            .get(idx as usize)
            .map(|t| t.to_string());
        region = Some(rect);
    }
    let loaded = Workbook::load_dir_scoped(&s.root, &Scope::new(tab.clone(), region));
    Ok(Decomposed {
        root: s.root,
        tab,
        region,
        loaded,
    })
}

/// An empty Root resolves to an empty `Resolved`; the caller's "has no tabs" guard is what refuses it.
pub fn resolve(path: &str, demand: Demand) -> Result<Resolved, Refusal> {
    let Structure {
        root,
        tab,
        region,
        selector,
        name,
        file,
        loaded,
    } = structure(path)?;
    let skeleton = match loaded {
        Ok(Ok(wb)) => wb,
        Ok(Err(diags)) => return Err(refused(diags)),
        Err(e) if is_not_a_dir(&e) => {
            return Err(fail(
                Kind::NotFound,
                &format!("no such workbook directory {:?}", root.display()),
            ));
        }
        Err(e) => {
            return Err(fail(
                Kind::Io,
                &format!("cannot read {:?}: {e}", root.display()),
            ));
        }
    };
    let (tab, region) = settle(&skeleton, &root, tab.as_deref(), name.as_deref(), region)?;
    let scope = match demand {
        Demand::Whole => Scope::unscoped(),
        Demand::Path => Scope::new(
            tab.and_then(|i| skeleton.sheet_name(i)).map(str::to_string),
            region,
        ),
    };
    Ok(Resolved {
        workbook: load_scoped(&root, &scope)?,
        root,
        tab,
        region,
        demand: scope,
        selector,
        file,
    })
}

/// The tab a bare defined name is looked up against: the path's own, else the workbook's first.
fn name_scope(wb: &Workbook, tab: Option<&str>) -> String {
    tab.map(str::to_string)
        .or_else(|| wb.sheet_names().first().map(|x| x.to_string()))
        .unwrap_or_default()
}

/// The tab and region the path settles on, read off the SKELETON — a trailing defined name must
/// resolve before there is a demand to load cells under.
fn settle(
    wb: &Workbook,
    root: &Path,
    tab: Option<&str>,
    name: Option<&str>,
    region: Option<Rect>,
) -> Result<(Option<u32>, Option<Rect>), Refusal> {
    if let Some(name) = name {
        let scope = name_scope(wb, tab);
        let (idx, rect) = resolve_name(wb, name, &scope)?;
        return Ok((Some(idx), Some(rect)));
    }
    let Some(name) = tab else {
        return Ok((None, region));
    };
    match wb.tab_index(name) {
        Some(idx) => Ok((Some(idx), region)),
        None => Err(fail(
            Kind::NotFound,
            &format!(
                "no tab named {name:?} in {:?} (tabs: {:?})",
                root.display(),
                wb.sheet_names()
            ),
        )),
    }
}

/// The ONE content load: the skeleton settled the structure, and this reads the cells the demand and
/// its closure name. No path loads a workbook's cells twice.
fn load_scoped(root: &Path, scope: &Scope) -> Result<Workbook, Refusal> {
    match Workbook::load_dir_scoped(root, scope) {
        Ok(Ok(wb)) => Ok(wb),
        Ok(Err(diags)) => Err(refused(diags)),
        Err(e) if is_not_a_dir(&e) => Err(fail(
            Kind::NotFound,
            &format!("no such workbook directory {:?}", root.display()),
        )),
        Err(e) => Err(fail(
            Kind::Io,
            &format!("cannot read {:?}: {e}", root.display()),
        )),
    }
}

impl Resolved {
    /// `None` leaves the caller on everything the tab states, presentation included.
    pub fn region(&self) -> Option<Rect> {
        self.region
    }

    /// The demand this resolution states, in the loaders' vocabulary — the very scope the cells were
    /// read under, so presentation and figures answer it identically.
    pub fn demand(&self) -> Scope {
        self.demand.clone()
    }

    pub fn as_single_cell(&self) -> Result<(u32, u32), Refusal> {
        match self.region {
            Some(rect) if rect.min_col == rect.max_col && rect.min_row == rect.max_row => {
                Ok((rect.min_col, rect.min_row))
            }
            Some(rect) => {
                let shown = self.selector.clone().unwrap_or_else(|| rect.label());
                Err(bad_arg(&format!(
                    "trace targets one cell; {shown:?} is a range — give a single cell"
                )))
            }
            // A file stating no extent lands here, not in the range arm, and is still NAMED.
            None => Err(bad_arg(&match self.named_file() {
                Some(file) => format!(
                    "trace targets one cell; {file:?} is a file the tab holds, not one cell"
                ),
                None => "trace needs exactly one cell, e.g. ./budget/Sheet1/C3".to_string(),
            })),
        }
    }

    /// A segment that named a FILE is not a workbook-or-tab path, whichever kind of file it was: the
    /// caller cannot see from the filename whether it states a region, so both refuse alike.
    pub fn as_context(&self) -> Result<(&Workbook, Option<u32>), Refusal> {
        if let Some(file) = self.named_file() {
            return Err(bad_arg(&format!(
                "eval takes <wb> or <wb>/<tab>; {file:?} names a file the tab holds, not a tab"
            )));
        }
        if self.region.is_some() {
            return Err(bad_arg(
                "eval takes <wb> or <wb>/<tab>; it evaluates a formula you supply, not a region",
            ));
        }
        Ok((&self.workbook, self.tab))
    }

    /// The raw text of a final segment that named a file, for a refusal that names what was typed.
    fn named_file(&self) -> Option<&str> {
        self.file.then_some(self.selector.as_deref()).flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);

    impl Tmp {
        fn new(tag: &str) -> Tmp {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let root = std::env::temp_dir()
                .join(format!("FSA1-addr-{tag}-{}-{nanos}", std::process::id()));
            std::fs::create_dir_all(&root).unwrap();
            Tmp(root)
        }
        /// `name` is canonical, with `:`; the host is asked how it spells that on disk.
        fn file(&self, tab: &str, name: &str, body: &str) -> &Tmp {
            let dir = self.0.join(tab);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(fsa1_model::range_file_name(name)), body).unwrap();
            self
        }
        /// A sidecar or figure is addressed by its STEM, so the stem is re-spelled for this host and
        /// the suffix re-attached — `entry` and [`Tmp::at_entry`] must agree or the path misses.
        fn entry(&self, tab: &str, name: &str, body: &str) -> &Tmp {
            let dir = self.0.join(tab);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(spell(name)), body).unwrap();
            self
        }
        /// VERBATIM, for a spelling this host would not itself write.
        fn raw(&self, tab: &str, name: &str, body: &str) -> &Tmp {
            let dir = self.0.join(tab);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(name), body).unwrap();
            self
        }
        fn at_entry(&self, tab: &str, name: &str) -> String {
            self.at(&format!("{tab}/{}", spell(name)))
        }
        fn dir(&self, name: &str) -> &Tmp {
            std::fs::create_dir_all(self.0.join(name)).unwrap();
            self
        }
        fn at(&self, rel: &str) -> String {
            self.0.join(rel).to_str().unwrap().to_string()
        }
        fn root(&self) -> String {
            self.0.to_str().unwrap().to_string()
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn spell(name: &str) -> String {
        fsa1_model::reseparate_entry_name(name, fsa1_model::RANGE_SEP)
            .unwrap_or_else(|| name.to_string())
    }

    fn err_kind<T>(r: Result<T, Refusal>) -> Kind {
        match r {
            Ok(_) => panic!("expected a located refusal, got Ok"),
            Err(r) => r.kind,
        }
    }

    #[test]
    fn whole_workbook_is_root_with_no_tab_or_region() {
        let t = Tmp::new("whole");
        t.file("Model", "A1", "1").file("Other", "A1", "2");
        let r = resolve(&t.root(), Demand::Path).unwrap();
        assert_eq!(r.tab, None, "a bare workbook path selects no explicit tab");
        assert_eq!(r.region(), None);
    }

    #[test]
    fn single_tab_workbook_resolves_the_lone_tab() {
        let t = Tmp::new("single");
        t.file("Model", "A1", "1");
        let r = resolve(&t.at("Model"), Demand::Path).unwrap();
        let idx = r.workbook.tab_index("Model").unwrap();
        assert_eq!(r.tab, Some(idx), "the lone non-empty tab resolves as a Tab");
        assert_eq!(r.region(), None);
    }

    #[test]
    fn wb_slash_b2_is_a_tab_when_b2_is_a_folder() {
        let t = Tmp::new("b2folder");
        t.file("Model", "A1", "1").file("B2", "A1", "9");
        let r = resolve(&t.at("B2"), Demand::Path).unwrap();
        let idx = r.workbook.tab_index("B2").unwrap();
        assert_eq!(r.tab, Some(idx), "a folder named B2 is a tab");
        assert_eq!(r.region(), None, "a tab folder carries no selector");
    }

    #[test]
    fn wb_slash_b2_is_a_selector_on_the_default_tab_when_not_a_folder() {
        let t = Tmp::new("b2sel");
        t.file("Model", "A1", "1");
        let r = resolve(&t.at("B2"), Demand::Path).unwrap();
        assert_eq!(r.tab, None, "a bare selector attaches to the default tab");
        assert_eq!(r.region(), Some(Rect::cell(1, 1)), "B2 is the 1x1 rect");
    }

    #[test]
    fn wb_slash_tab_slash_region_resolves_the_explicit_tab_and_rect() {
        let t = Tmp::new("region");
        t.file("Model", "A1", "1");
        let r = resolve(&t.at("Model/B2:C3"), Demand::Path).unwrap();
        let idx = r.workbook.tab_index("Model").unwrap();
        assert_eq!(r.tab, Some(idx));
        assert_eq!(
            r.region(),
            Some(Rect {
                min_col: 1,
                min_row: 1,
                max_col: 2,
                max_row: 2
            })
        );
    }

    /// A tab folder read AS a root scopes its names to the workbook, where a `../` target is
    /// rightly ambiguous. That refusal is an artefact of the wrong reading and must not escape:
    /// the folder has no tabs of its own, so it is a tab, and the parent resolves the name.
    #[test]
    fn a_tab_dirs_speculative_root_refusal_does_not_escape_the_parent_reading() {
        let t = Tmp::new("spec-refusal");
        t.file("Model", "A1", "header");
        t.file("Cash Flows", "B2", "888");
        std::fs::write(t.at("Model/flow"), "../Cash Flows/B2").expect("write the ref-file alias");
        let r = resolve(&t.at("Model/flow"), Demand::Path)
            .expect("the parent resolves the cross-sheet name");
        let tab = r.tab.expect("the name resolved onto a tab");
        assert_eq!(
            r.workbook.sheet_names()[tab as usize],
            "Cash Flows",
            "resolved across to the target tab, not refused"
        );
    }

    #[test]
    fn broken_root_surfaces_load_diags_and_is_not_a_tab_of_parent() {
        let t = Tmp::new("broken");
        t.file("Model", "A1:D9", "one literal in a 9x4 range");
        let code = err_kind(resolve(&t.root(), Demand::Path));
        assert_eq!(code, Kind::Validation, "a broken root refuses");
    }

    #[test]
    fn empty_root_under_a_plain_parent_is_an_empty_root() {
        let t = Tmp::new("emptyroot");
        t.dir("E");
        let r = resolve(&t.at("E"), Demand::Path).unwrap();
        assert!(
            r.workbook.sheet_names().is_empty(),
            "an empty Root carries no tabs (the command guard fires)"
        );
        assert_eq!(r.tab, None);
    }

    #[test]
    fn empty_tab_of_a_workbook_resolves_as_an_empty_tab() {
        let t = Tmp::new("emptytab");
        t.file("Model", "A1", "1").dir("EmptyTab");
        let r = resolve(&t.at("EmptyTab"), Demand::Path).unwrap();
        let idx = r.workbook.tab_index("EmptyTab").unwrap();
        assert_eq!(
            r.tab,
            Some(idx),
            "an empty tab of a workbook resolves as a Tab"
        );
        assert!(
            !r.workbook.sheet_names().is_empty(),
            "the whole workbook (incl. the non-empty Model) is loaded"
        );
    }

    #[test]
    fn all_empty_tabs_workbook_reads_an_empty_tab_as_an_empty_root() {
        let t = Tmp::new("allempty");
        t.dir("A").dir("B");
        let r = resolve(&t.at("A"), Demand::Path).unwrap();
        assert!(
            r.workbook.sheet_names().is_empty(),
            "an all-empty-tabs workbook's empty tab reads as an empty Root"
        );
    }

    #[test]
    fn named_cell_ref_resolves_to_its_target_rect() {
        let t = Tmp::new("name-cell");
        t.file("Model", "A1", "1").file("Model", "total", "=B5");
        let r = resolve(&t.at("Model/total"), Demand::Path).unwrap();
        let idx = r.workbook.tab_index("Model").unwrap();
        assert_eq!(r.tab, Some(idx), "the name resolves on its scope tab");
        assert_eq!(r.region(), Some(Rect::cell(1, 4)), "B5 is the 1x1 rect");
    }

    #[test]
    fn named_range_resolves_to_its_target_rect() {
        let t = Tmp::new("name-range");
        t.file("Model", "A1", "1").file("Model", "Days", "=A2:A4");
        let r = resolve(&t.at("Model/Days"), Demand::Path).unwrap();
        assert_eq!(
            r.region(),
            Some(Rect {
                min_col: 0,
                min_row: 1,
                max_col: 0,
                max_row: 3
            }),
            "A2:A4 is the range rect"
        );
    }

    #[test]
    fn cross_tab_named_ref_resolves_to_the_other_tab() {
        let t = Tmp::new("name-crosstab");
        t.file("Model", "A1", "1")
            .file("Model", "elsewhere", "=Assumptions!B6")
            .file("Assumptions", "B6", "42");
        let r = resolve(&t.at("Model/elsewhere"), Demand::Path).unwrap();
        let assumptions = r.workbook.tab_index("Assumptions").unwrap();
        assert_eq!(
            r.tab,
            Some(assumptions),
            "the resolved tab is the target tab"
        );
        assert_eq!(r.region(), Some(Rect::cell(1, 5)), "B6 is the 1x1 rect");
    }

    #[test]
    fn named_formula_is_a_bad_args_refusal() {
        let t = Tmp::new("name-expr");
        t.file("Model", "A1", "1")
            .file("Model", "Rate", "=Base*1.05");
        let code = err_kind(resolve(&t.at("Model/Rate"), Demand::Path));
        assert_eq!(
            code,
            Kind::InvalidArguments,
            "a named formula/constant is a bad-args refusal"
        );
    }

    #[test]
    fn unknown_final_non_a1_segment_is_a_bad_args_refusal() {
        let t = Tmp::new("name-unknown");
        t.file("Model", "A1", "1");
        let code = err_kind(resolve(&t.at("Model/nope"), Demand::Path));
        assert_eq!(
            code,
            Kind::InvalidArguments,
            "an unknown final non-A1 segment is a bad-args refusal"
        );
    }

    #[test]
    fn non_final_missing_tab_is_a_no_tab_named_refusal() {
        let t = Tmp::new("notab");
        t.file("Model", "A1", "1");
        let code = err_kind(resolve(&t.at("Nope/A1"), Demand::Path));
        assert_eq!(
            code,
            Kind::NotFound,
            "a non-final missing tab is a not-found refusal"
        );
    }

    #[test]
    fn a_sidecar_path_resolves_to_the_region_its_stem_roots() {
        let t = Tmp::new("file-css");
        t.file("Model", "A1:A5", "1\n2\n3\n4\n5").entry(
            "Model",
            "A1:A5.css",
            "td { color: crimson }\n",
        );
        let r = resolve(&t.at_entry("Model", "A1:A5.css"), Demand::Path).unwrap();
        assert_eq!(r.tab, r.workbook.tab_index("Model"));
        assert_eq!(
            r.region(),
            Some(Rect {
                min_col: 0,
                min_row: 0,
                max_col: 0,
                max_row: 4
            }),
            "the sidecar scopes exactly what its stem spelled bare would"
        );
    }

    /// A name-form figure states no extent, so the honest scope is the whole tab — never a narrower
    /// region that is wrong.
    #[test]
    fn a_name_form_figure_path_resolves_to_the_tab_with_no_region() {
        let t = Tmp::new("file-figure");
        t.file("Model", "A1", "1")
            .entry("Model", "Chart1.json", "{\"mark\":\"bar\"}\n");
        let r = resolve(&t.at_entry("Model", "Chart1.json"), Demand::Path).unwrap();
        assert_eq!(r.tab, r.workbook.tab_index("Model"), "the tab is settled");
        assert_eq!(r.region(), None, "a floating figure names no rectangle");
    }

    /// A filename is a LOCATION, not a namespace lookup, so the ROOT position lists no tab: were it
    /// to list the default one, `wb/A1:A5.css` would resolve against `Orders` and then refuse the
    /// instant a tab sorting ahead of it appeared. A path that turns on tab ORDER is not a path.
    #[test]
    fn a_file_segment_in_the_root_position_refuses_the_same_whatever_tab_sorts_first() {
        let t = Tmp::new("file-root");
        t.file("Orders", "A1:A5", "1\n2\n3\n4\n5").entry(
            "Orders",
            "A1:A5.css",
            "td { color: crimson }\n",
        );
        let path = t.at(&spell("A1:A5.css"));
        for stage in ["one tab", "a tab sorting ahead of it"] {
            let r = match resolve(&path, Demand::Path) {
                Ok(_) => {
                    panic!("a file segment in the root position must not resolve, with {stage}")
                }
                Err(r) => r,
            };
            assert_eq!(r.kind, Kind::InvalidArguments, "with {stage}");
            assert!(
                r.message.contains("is not a canonical A1 cell or range")
                    && r.message.contains("no defined name"),
                "with {stage}, the name arm answers unchanged: {}",
                r.message
            );
            t.file("Alpha", "A1", "9");
        }
    }

    /// `parse_viewport` claims the POSIX spelling first, so a RANGE FILE reaches the file arm only
    /// under the `-` spelling — the one kind of rect the A1 arm would otherwise always answer.
    #[test]
    fn a_dash_spelled_range_file_path_resolves_to_the_rect_its_name_states() {
        let t = Tmp::new("file-range");
        let rows: String = (1..=9).map(|r| format!("{r}\t{r}\t{r}\t{r}\n")).collect();
        t.raw("Model", "A1-D9", &rows);
        let r = resolve(&t.at("Model/A1-D9"), Demand::Path).unwrap();
        assert_eq!(r.tab, r.workbook.tab_index("Model"));
        assert_eq!(
            r.region(),
            Some(Rect {
                min_col: 0,
                min_row: 0,
                max_col: 3,
                max_row: 8
            }),
            "the range file scopes the rectangle its name states"
        );
    }

    #[test]
    fn decompose_carries_the_broken_root_load_out() {
        let t = Tmp::new("decompose");
        t.file("Model", "A1:D9", "one literal in a 9x4 range");
        let d = decompose(&t.root()).unwrap();
        assert!(
            matches!(d.loaded, Ok(Err(_))),
            "a broken root's load result is carried out for check"
        );
        assert_eq!(d.tab, None);
    }
}
