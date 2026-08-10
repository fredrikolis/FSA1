// Concern: one function per verb, naming its target, choosing its drawer and asking what its carrier will not take | Non-concern: parsing the flags | IO: (path + options) -> an outcome or a Refusal

use std::path::{Path, PathBuf};

use fsa1_ingest::{Decomposition, ImportReport};
use fsa1_model::{
    Axis, Diagnostic, Direction, Figure, FigureView, Figures, FormulaOutcome, Overlay, Placement,
    Rect, RenderMode, TraceNode, ViewScope, figure_occupancy, view,
};

use crate::address;
use crate::pack_format::PackFormat;
use crate::present;
use crate::refusal::{Kind, Refusal, bad_arg, fail, refused};
use crate::xlsx_loss::losses;

/// Which drawer finishes a view — the identity of the verb that asked, never one of its options, so
/// it is settled here by [`render`] and [`tree`] and no front end names it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Presenter {
    Table,
    Tree,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Ascii,
    Html,
}

/// A display bound on the finished view, never a narrowing of what was demanded.
pub const TREE_CELL_CAP: u32 = 50;

/// `notes` are advisories about the ANSWER, not failures: a region outside the used area, an empty
/// tab. A CLI puts them on stderr and an MCP server folds them into the text, and neither decision
/// belongs here.
pub struct Rendered {
    pub text: String,
    pub notes: Vec<String>,
    /// True when the view held one empty tab, which an ASCII caller reports instead of drawing.
    pub empty: bool,
}

/// The shared body of [`render`] and [`tree`]: everything the two verbs do the same way, up to the
/// drawer each one IS.
fn view_at(
    target: &str,
    mode: Option<RenderMode>,
    presenter: Presenter,
    format: Format,
    full: bool,
) -> Result<Rendered, Refusal> {
    let resolved = address::resolve(target)?;
    if resolved.workbook.sheet_names().is_empty() {
        let msg = format!(
            "{target:?} has no tabs (a tab is a sub-folder of cell/range files; name one as <workbook>/<tab>)"
        );
        return Err(fail(Kind::Validation, &msg));
    }
    let wb = &resolved.workbook;
    let scope = match (resolved.tab, resolved.region()) {
        (tab, Some(rect)) => ViewScope::Region(tab.unwrap_or(0), rect),
        (Some(sheet), None) => ViewScope::Tab(sheet),
        (None, None) => ViewScope::Workbook,
    };
    // The same demand the path states, in the loaders' vocabulary: a file whose NAME the path never meets is not opened, so it neither draws, nor is named, nor refuses the render.
    let demand = fsa1_model::Scope::new(
        resolved
            .tab
            .and_then(|sheet| wb.sheet_name(sheet))
            .map(str::to_string),
        resolved.region(),
    );
    // `tree` NAMES each figure and the table MARKS the cells it covers, so both presenters open one; `eval` and `trace` reach a different function and still never do.
    let (figures, mut notes) = load_figures_scoped(&resolved.root, &demand)?;
    let needs_axes = figures
        .all()
        .any(|(_, f)| matches!(figures.placement(f), Some(Placement::Box { .. })));
    // HTML DRAWS presentation; ASCII draws none and opens the sidecar only to MEASURE, which a cover stated in CELLS does not need.
    let overlay = match (format, presenter) {
        (Format::Html, _) => Some(load_overlay_scoped(&resolved.root, &demand)?),
        (Format::Ascii, Presenter::Table) if needs_axes => {
            Some(load_overlay_scoped(&resolved.root, &demand)?)
        }
        (Format::Ascii, _) => None,
    };
    // A page draws VALUES and shows a formula in its bar, so a `--mode` has nothing left to pick.
    let mode = match (format, mode) {
        (Format::Html, None) => RenderMode::Values,
        (Format::Html, Some(_)) => {
            let msg = "--format html draws values and shows each formula in its formula bar, so it takes no --mode; drop the flag, or use --format ascii";
            return Err(fail(Kind::Validation, msg));
        }
        (Format::Ascii, mode) => mode.unwrap_or(RenderMode::Combined),
    };

    // ONE answer to "the tab's used region" per carrier, read by both the note below and the viewport `view` builds: HTML DRAWS a style so its takes in every stated region, ASCII draws none so its is CONTENT — opening the overlay to MEASURE a cover makes the table no drawer of presentation.
    let view_overlay = match format {
        Format::Html => overlay.as_ref(),
        Format::Ascii => None,
    };

    if let ViewScope::Region(sheet, rect) = scope
        && let Some(used) =
            view_overlay.map_or_else(|| wb.content_region(sheet), |o| o.stated_region(wb, sheet))
        && rect.intersect(&used).is_none()
    {
        notes.push(format!(
            "region {} lies entirely outside the tab's used region {}",
            rect.label(),
            used.label()
        ));
    }

    // Where each figure sits, in CELLS, so the note below reads the rectangle it just built. Only the ASCII table MEASURES a cover: HTML draws the figure itself, and `tree` has no coordinate plane to occlude.
    let mut placed: Vec<(u32, FigureView)> = Vec::new();
    if let (Presenter::Table, Format::Ascii) = (presenter, format) {
        for (s, tab) in wb.sheet_names().iter().enumerate() {
            let s = s as u32;
            let tab_figures = figures.in_tab(tab);
            if tab_figures.is_empty() {
                continue;
            }
            // Where no overlay was opened, the runs are EMPTY rather than absent -- `Axis::columns(&[])` is the default ruler, every cell its default width, and not a ruler of zero-sized cells.
            let (cols, rows) = overlay.as_ref().map_or_else(
                || (Axis::columns(&[]), Axis::rows(&[])),
                |o| {
                    (
                        Axis::columns(&o.column_widths(wb, s)),
                        Axis::rows(&o.row_heights(wb, s)),
                    )
                },
            );
            for figure in tab_figures {
                let cover = figures
                    .placement(figure)
                    .map(|placement| placement.cover(&cols, &rows));
                placed.push((s, figure_view(figure, cover)));
            }
        }
    } else if let Presenter::Tree = presenter {
        for (s, tab) in wb.sheet_names().iter().enumerate() {
            for figure in figures.in_tab(tab) {
                placed.push((s as u32, figure_view(figure, None)));
            }
        }
    }

    let v = view(wb, view_overlay, scope, mode, &placed).map_err(|msg| bad_arg(&msg))?;

    // A sidecar's bytes ride a raw-text `<style>` UNCHANGED, so text its carrier cannot hold is refused before the document is drawn rather than silently swallowing every later rule inside it.
    if let (Format::Html, Some(overlay)) = (format, overlay.as_ref()) {
        for sheet in &v.sheets {
            for scope in overlay.scopes(wb, sheet.sheet) {
                if let Some((line, col)) = fsa1_html::carrier::unholdable(scope.text) {
                    let msg = format!(
                        "{}:{line}:{col}: from here the <style> element this sidecar's bytes are carried in stops carrying them, so every later rule would be dropped -- close it here, or spell it another way",
                        scope.file
                    );
                    return Err(fail(Kind::Validation, &msg));
                }
            }
        }
    }

    let empty = v.sheets.len() == 1 && v.sheets[0].grid.is_none();
    if empty {
        notes.push(format!(
            "tab {:?} is empty (no cells to render)",
            v.sheets[0].name
        ));
    }

    // A figure is in scope for its TAB, and bound only for the carrier that DRAWS one: ASCII discards the spec, so expanding for it buys a fetch and a JSON build for nothing and reports each fault twice.
    let mut bound: Vec<(String, String)> = Vec::new();
    for sheet in &v.sheets {
        for figure in figures.in_tab(sheet.name) {
            match format {
                Format::Html => {
                    let sheet_id = wb.tab_index(sheet.name).expect("a view names its own tabs");
                    match figure.expand(wb, sheet_id) {
                        Ok(spec) => bound.push((figure.name.clone(), spec.to_string())),
                        Err(diags) => notes.extend(diags.iter().map(Diagnostic::to_string)),
                    }
                }
                // ASCII neither draws nor binds; the presenter below is where it NAMES one.
                Format::Ascii => {}
            }
        }
    }

    let text = match (presenter, format) {
        (Presenter::Table, Format::Ascii) => {
            // ASCII cannot DRAW one, so it names it. An unplaced figure marks nothing and names no range: `pack`'s derived position moves as content grows and is no authored position.
            for sheet in &v.sheets {
                for figure in &sheet.figures {
                    let bindings = match figure.binds.as_slice() {
                        [] => "no range".to_string(),
                        bindings => bindings.join(", "),
                    };
                    let name = &figure.name;
                    notes.push(match figure.cover {
                        Some(rect) => {
                            format!("figure {name} covers {} and binds {bindings}", rect.label())
                        }
                        None => format!("figure {name} has no placement and binds {bindings}"),
                    });
                }
            }
            present::table(&v)
        }
        (Presenter::Table, Format::Html) => {
            let overlay = overlay
                .as_ref()
                .expect("Format::Html loaded the overlay above");
            fsa1_html::document(wb, overlay, &v, &bound)
        }
        (Presenter::Tree, _) => present::tree(&v, if full { u32::MAX } else { TREE_CELL_CAP }),
    };
    Ok(Rendered { text, notes, empty })
}

/// The two spec readings a view carries, plus the cover only a caller that measured one supplies.
/// The FORM comes off the name instead: `figure.name` locates, and `figure_occupancy` reads an entry.
fn figure_view(figure: &Figure, cover: Option<Rect>) -> FigureView {
    let entry = present::entry_name(&figure.name);
    FigureView {
        name: figure.name.clone(),
        kind: figure.kind(),
        binds: figure.bindings(),
        cover,
        range_form: figure_occupancy(entry).is_some(),
    }
}

/// Every parameter `render` has. A struct literal must name every field, so a front end that stops
/// offering an option stops COMPILING instead of quietly passing a literal in its place — which is
/// what nothing here deriving, eliding or building a default is for. The same holds of its six
/// siblings below.
pub struct RenderArgs<'a> {
    pub target: &'a str,
    pub mode: Option<RenderMode>,
    pub format: Format,
}

pub fn render(args: RenderArgs<'_>) -> Result<Rendered, Refusal> {
    view_at(args.target, args.mode, Presenter::Table, args.format, false)
}

/// Every parameter `tree` has. It carries no `format`: ASCII is the only carrier a nested view is
/// drawn in.
pub struct TreeArgs<'a> {
    pub target: &'a str,
    pub mode: Option<RenderMode>,
    pub full: bool,
}

pub fn tree(args: TreeArgs<'_>) -> Result<Rendered, Refusal> {
    view_at(
        args.target,
        args.mode,
        Presenter::Tree,
        Format::Ascii,
        args.full,
    )
}

/// Every parameter `check` has. `format` asks the SECOND question — what an export in that format
/// will not carry — which `None` leaves unasked; the vocabulary is `pack`'s, so a format added there
/// is askable here with no field of its own.
pub struct CheckArgs<'a> {
    pub target: &'a str,
    pub format: Option<PackFormat>,
}

/// A workbook that will not load is itself the finding, so this reads a `Decomposed` rather than a
/// `Resolved`: it must still scope and report against a root that never loaded.
pub fn check(args: CheckArgs<'_>) -> Result<Vec<Diagnostic>, Refusal> {
    let target = args.target;
    let decomposed = address::decompose(target)?;
    let root_display = decomposed.root.display().to_string();
    let scope = fsa1_model::Scope::new(decomposed.tab, decomposed.region);

    match decomposed.loaded {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let msg = format!("no such workbook directory {root_display:?}");
            Err(fail(Kind::NotFound, &msg))
        }
        Err(e) => {
            let msg = format!("cannot read {root_display:?}: {e}");
            Err(fail(Kind::Io, &msg))
        }
        // Best-effort: a bare-filename loc carries no tab, so a scope cannot exclude it on that axis, and with no `Workbook` to resolve against a binding is graded on its SYNTAX and no further.
        Ok(Err(load_diags)) => {
            let (sidecar, _) = sidecar_diags(&decomposed.root, &scope)?;
            let (figure, _) = figure_diags(&decomposed.root, None, &scope)?;
            Ok(in_scope(load_diags, &scope)
                .chain(sidecar)
                .chain(figure)
                .collect())
        }
        Ok(Ok(wb)) => {
            if let Some(name) = scope.tab()
                && wb.tab_index(name).is_none()
            {
                let msg = format!(
                    "no tab named {name:?} in {root_display:?} (tabs: {:?})",
                    wb.sheet_names()
                );
                return Err(fail(Kind::NotFound, &msg));
            }
            // The values first and the sidecars after, the order this verb reports on either branch.
            let mut found = wb.lint_scoped(&scope);
            let (sidecar, overlay) = sidecar_diags(&decomposed.root, &scope)?;
            found.extend(sidecar);
            let (figure, figures) = figure_diags(&decomposed.root, Some(&wb), &scope)?;
            found.extend(figure);
            // A sidecar that would not parse leaves no overlay to grade, but it stops only ITS half — the figure half is asked here whatever the presentation did, and the sidecar's own fault is already in the table above.
            if args.format.is_some() {
                let (_, not_drawn) = crate::charts::charts(&wb, &figures);
                found.extend(losses(&wb, overlay.as_ref(), &not_drawn));
            }
            Ok(found)
        }
    }
}

/// Every parameter `eval` has.
pub struct EvalArgs<'a> {
    pub target: &'a str,
    pub formula: &'a str,
}

/// The outcome is returned whole rather than as a string: a formula that yields `#REF!` EVALUATED,
/// and a front end that scores that as a failure needs to be able to tell it from one that did not.
pub fn eval(args: EvalArgs<'_>) -> Result<FormulaOutcome, Refusal> {
    let EvalArgs { target, formula } = args;
    let resolved = address::resolve(target)?;
    if resolved.workbook.sheet_names().is_empty() {
        let msg = format!("{target:?} has no tabs (a tab is a sub-folder of cell/range files)");
        return Err(fail(Kind::Validation, &msg));
    }
    let (wb, tab) = resolved.as_context()?;
    wb.eval_formula(tab.unwrap_or(0), formula)
        .map_err(|diag| refused(vec![diag]))
}

/// Every parameter `trace` has.
pub struct TraceArgs<'a> {
    pub target: &'a str,
    pub dir: Direction,
    pub depth: Option<u32>,
}

pub fn trace(args: TraceArgs<'_>) -> Result<TraceNode, Refusal> {
    let TraceArgs { target, dir, depth } = args;
    let resolved = address::resolve(target)?;
    if resolved.workbook.sheet_names().is_empty() {
        let msg = format!("{target:?} has no tabs (a tab is a sub-folder of cell/range files)");
        return Err(fail(Kind::Validation, &msg));
    }
    let sheet = resolved.tab.unwrap_or(0);
    let (col, row) = resolved.as_single_cell()?;
    resolved
        .workbook
        .trace(sheet, col, row, dir, depth)
        .map_err(|diag| refused(vec![diag]))
}

pub struct Unpacked {
    pub dest: PathBuf,
    pub report: ImportReport,
}

/// Every parameter `unpack` has.
pub struct UnpackArgs<'a> {
    pub src: &'a Path,
    pub dest: Option<&'a Path>,
    pub decomposition: Option<Decomposition>,
    pub strict: bool,
}

pub fn unpack(args: UnpackArgs<'_>) -> Result<Unpacked, Refusal> {
    let UnpackArgs {
        src,
        dest,
        decomposition,
        strict,
    } = args;
    let dest = match dest {
        Some(d) => d.to_path_buf(),
        None => derive_unpack_dest(src)?,
    };
    let imported = match decomposition {
        Some(d) => fsa1_ingest::import_file_as(src, &dest, strict, d),
        None => fsa1_ingest::import_file(src, &dest, strict),
    };
    imported
        .map(|report| Unpacked { dest, report })
        .map_err(|e| fail(unpack_kind(e.kind), &e.to_string()))
}

pub struct Packed {
    pub dest: PathBuf,
    pub sheets: usize,
    pub charts: usize,
    /// Everything the written file does not carry, located. Empty is the ordinary case, and distinct
    /// from a workbook that states neither presentation nor figures — which loses nothing either way.
    pub losses: Vec<Diagnostic>,
}

/// Every parameter `pack` has. `dest` is taken verbatim when given; `format` governs only the name
/// derived when it is not.
pub struct PackArgs<'a> {
    pub folder: &'a Path,
    pub dest: Option<&'a Path>,
    pub format: PackFormat,
    pub strict: bool,
}

/// `strict` refuses rather than writing a workbook that does not wholly cross, which is the same bar
/// `unpack --strict` sets on the way in. Without it the file is written and the losses are NAMED.
pub fn pack(args: PackArgs<'_>) -> Result<Packed, Refusal> {
    let PackArgs {
        folder,
        dest,
        format,
        strict,
    } = args;
    let dest = match dest {
        Some(d) => d.to_path_buf(),
        None => derive_pack_dest(folder, format.name())?,
    };
    let wb = load(folder)?;
    if wb.sheet_names().is_empty() {
        let msg =
            format!("{folder:?} has no tabs to pack (a tab is a sub-folder of cell/range files)");
        return Err(fail(Kind::Validation, &msg));
    }
    let overlay = load_overlay(folder)?;
    let figures = figures_to_draw(folder)?;
    let (charts, not_drawn) = crate::charts::charts(&wb, &figures);
    let losses = losses(&wb, Some(&overlay), &not_drawn);
    if strict && !losses.is_empty() {
        return Err(Refusal {
            kind: Kind::Validation,
            message: "cannot strictly pack this workbook: remove what an .xlsx cannot carry, or \
                      pack without --strict to write it with these left out"
                .to_string(),
            diagnostics: losses,
        });
    }
    fsa1_xlsx::write_xlsx(&wb, &overlay, &charts, &dest)
        .map(|()| Packed {
            dest,
            sheets: wb.sheet_names().len(),
            charts: charts.len(),
            losses,
        })
        .map_err(|e| fail(pack_kind(&e), &e.to_string()))
}

/// A figure a pack cannot even PARSE is a workbook fault, not a chart Excel has no shape for, so it
/// refuses here rather than being reported as one figure that did not cross — `check` refuses the
/// same file for the same reason.
fn figures_to_draw(path: &Path) -> Result<Figures, Refusal> {
    match Figures::load_dir(path) {
        Err(e) => Err(fail(
            Kind::Io,
            &format!("cannot read {:?}: {e}", path.display()),
        )),
        Ok(Err(diags)) => Err(refused(diags)),
        Ok(Ok(figures)) => Ok(figures),
    }
}

/// The stem only, so the workbook lands in the process CWD rather than beside its source file.
fn derive_unpack_dest(src: &Path) -> Result<PathBuf, Refusal> {
    match src.file_stem() {
        Some(stem) if !stem.is_empty() => Ok(PathBuf::from(stem)),
        _ => Err(bad_arg(&format!(
            "cannot derive a workbook directory name from {:?}; give an explicit <dest-workbook-dir>",
            src.display()
        ))),
    }
}

/// Basename only, so the output lands in the process CWD rather than beside the source folder.
fn derive_pack_dest(folder: &Path, ext: &str) -> Result<PathBuf, Refusal> {
    match folder.file_name() {
        Some(base) => {
            let mut name = base.to_os_string();
            name.push(".");
            name.push(ext);
            Ok(PathBuf::from(name))
        }
        None => Err(bad_arg(&format!(
            "cannot derive an output name from {:?} (name a workbook directory like ./acme-dcf)",
            folder.display()
        ))),
    }
}

fn unpack_kind(kind: fsa1_ingest::ErrorKind) -> Kind {
    use fsa1_ingest::ErrorKind;
    match kind {
        ErrorKind::SourceNotFound => Kind::NotFound,
        ErrorKind::DestConflict => Kind::Conflict,
        ErrorKind::SourceIo | ErrorKind::DestIo => Kind::Io,
        ErrorKind::Invalid => Kind::Validation,
    }
}

fn pack_kind(e: &fsa1_xlsx::ExportError) -> Kind {
    match e {
        fsa1_xlsx::ExportError::DestExists(_) => Kind::Conflict,
        fsa1_xlsx::ExportError::Io(_) => Kind::Io,
    }
}

/// The SECOND load, off the same directory: a verb that draws presentation asks for it, and every
/// other one never opens a sidecar at all.
pub fn load_overlay(path: &Path) -> Result<Overlay, Refusal> {
    load_overlay_scoped(path, &fsa1_model::Scope::unscoped())
}

/// [`load_overlay`] under a demand: a sidecar the demand does not name is neither applied nor able
/// to refuse the render, which is what an out-of-scope fault costs today.
pub fn load_overlay_scoped(path: &Path, demand: &fsa1_model::Scope) -> Result<Overlay, Refusal> {
    match Overlay::load_dir_scoped(path, demand) {
        Err(e) => {
            let msg = format!("cannot read {:?}: {e}", path.display());
            Err(fail(Kind::Io, &msg))
        }
        Ok(Err(diags)) => Err(refused(diags)),
        Ok(Ok(overlay)) => Ok(overlay),
    }
}

/// The THIRD load, off the same directory: a verb that DRAWS a figure asks for it, and every other
/// one never opens a `.json` at all. Unlike [`load_overlay`] a refusal here is a NOTE, because a
/// figure is ADDITIVE: a sidecar that will not parse changes what every cell wears, while a figure
/// that will not parse costs the document that figure and nothing else. `check` grades one.
pub fn load_figures(path: &Path) -> Result<(Figures, Vec<String>), Refusal> {
    load_figures_scoped(path, &fsa1_model::Scope::unscoped())
}

/// [`load_figures`] under a demand: a figure the demand does not name is neither drawn nor noted.
pub fn load_figures_scoped(
    path: &Path,
    demand: &fsa1_model::Scope,
) -> Result<(Figures, Vec<String>), Refusal> {
    match Figures::load_dir_scoped(path, demand) {
        Err(e) => {
            let msg = format!("cannot read {:?}: {e}", path.display());
            Err(fail(Kind::Io, &msg))
        }
        Ok(Err(diags)) => Ok((
            Figures::default(),
            diags.iter().map(Diagnostic::to_string).collect(),
        )),
        Ok(Ok(figures)) => Ok((figures, Vec::new())),
    }
}

pub fn load(path: &Path) -> Result<fsa1_model::Workbook, Refusal> {
    match fsa1_model::Workbook::load_dir(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let msg = format!("no such workbook directory {:?}", path.display());
            Err(fail(Kind::NotFound, &msg))
        }
        Err(e) => {
            let msg = format!("cannot read {:?}: {e}", path.display());
            Err(fail(Kind::Io, &msg))
        }
        Ok(Err(diags)) => Err(refused(diags)),
        Ok(Ok(wb)) => Ok(wb),
    }
}

/// `check` parses presentation to LINT it, so a sidecar's refusals are findings rather than a reason
/// to stop. A directory it cannot READ is neither: a pass that could not run reports no faults and
/// must not be mistaken for one that found none. What LOADED rides out beside them, because the
/// export question is answered off exactly this read and no second one.
fn sidecar_diags(
    root: &Path,
    demand: &fsa1_model::Scope,
) -> Result<(Vec<Diagnostic>, Option<Overlay>), Refusal> {
    match Overlay::load_dir_scoped(root, demand) {
        Err(e) => {
            let msg = format!("cannot read {:?}: {e}", root.display());
            Err(fail(Kind::Io, &msg))
        }
        Ok(Err(diags)) => Ok((diags, None)),
        Ok(Ok(overlay)) => Ok((Vec::new(), Some(overlay))),
    }
}

/// `check` parses a figure to LINT it, so its refusals are findings rather than a reason to stop.
/// What each branch can REACH differs and is not pretended otherwise: with a loadable `wb` the JSON
/// must parse AND every binding must resolve; without one there is nothing to resolve against, so
/// only the JSON and the binding SYNTAX are graded.
fn figure_diags(
    root: &Path,
    wb: Option<&fsa1_model::Workbook>,
    demand: &fsa1_model::Scope,
) -> Result<(Vec<Diagnostic>, Figures), Refusal> {
    let figures = match Figures::load_dir_scoped(root, demand) {
        Err(e) => {
            let msg = format!("cannot read {:?}: {e}", root.display());
            return Err(fail(Kind::Io, &msg));
        }
        Ok(Err(diags)) => return Ok((diags, Figures::default())),
        Ok(Ok(figures)) => figures,
    };
    let Some(wb) = wb else {
        let diags = figures.binding_syntax();
        return Ok((diags, figures));
    };
    let diags: Vec<Diagnostic> = figures
        .all()
        .filter_map(|(tab, figure)| {
            let sheet = wb.tab_index(tab)?;
            figure.expand(wb, sheet).err()
        })
        .flatten()
        .collect();
    Ok((diags, figures))
}

fn in_scope<'a>(
    diags: Vec<fsa1_model::Diagnostic>,
    scope: &'a fsa1_model::scope::Scope,
) -> impl Iterator<Item = fsa1_model::Diagnostic> + 'a {
    diags.into_iter().filter(move |d| {
        let (loc_tab, region) = fsa1_model::scope::loc_target(&d.loc);
        scope.wants(loc_tab, region)
    })
}
