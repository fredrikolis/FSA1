// Concern: one function per verb, naming its target by path and choosing its drawer | Non-concern: parsing the flags behind the options | IO: (a path + options) -> an outcome, or a Refusal

use std::path::{Path, PathBuf};

use fsa1_ingest::{Decomposition, ImportReport};
use fsa1_model::{Diagnostic, Direction, FormulaOutcome, RenderMode, TraceNode, ViewScope, view};

use crate::address;
use crate::present;
use crate::refusal::{Kind, Refusal, bad_arg, fail, refused};

/// Which drawer finishes a view. The MCP surface only ever asks for `Table`; `Tree` exists because
/// the CLI has a verb for it, and both walk the same resolve-and-view path to get there.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Presenter {
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

pub fn view_at(
    target: &str,
    mode: RenderMode,
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

    let mut notes = Vec::new();
    if let ViewScope::Region(sheet, rect) = scope
        && let Some(used) = wb.used_region(sheet)
        && rect.intersect(&used).is_none()
    {
        notes.push(format!(
            "region {} lies entirely outside the tab's used region {}",
            rect.label(),
            used.label()
        ));
    }

    let v = view(wb, scope, mode).map_err(|msg| bad_arg(&msg))?;

    let empty = v.sheets.len() == 1 && v.sheets[0].grid.is_none();
    if empty {
        notes.push(format!(
            "tab {:?} is empty (no cells to render)",
            v.sheets[0].name
        ));
    }

    let text = match (presenter, format) {
        (Presenter::Table, Format::Ascii) => present::table(&v),
        (Presenter::Table, Format::Html) => fsa1_html::document(wb, &v),
        (Presenter::Tree, _) => present::tree(&v, if full { u32::MAX } else { TREE_CELL_CAP }),
    };
    Ok(Rendered { text, notes, empty })
}

pub fn render(target: &str, mode: RenderMode, format: Format) -> Result<Rendered, Refusal> {
    view_at(target, mode, Presenter::Table, format, false)
}

/// A workbook that will not load is itself the finding, so this reads a `Decomposed` rather than a
/// `Resolved`: it must still scope and report against a root that never loaded.
pub fn check(target: &str) -> Result<Vec<Diagnostic>, Refusal> {
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
        // Best-effort: a bare-filename loc carries no tab, so a scope cannot exclude it on that axis.
        Ok(Err(load_diags)) => Ok(load_diags
            .into_iter()
            .filter(|d| {
                let (loc_tab, region) = fsa1_model::scope::loc_target(&d.loc);
                scope.includes(loc_tab, region)
            })
            .collect()),
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
            Ok(wb.lint_scoped(&scope))
        }
    }
}

/// The outcome is returned whole rather than as a string: a formula that yields `#REF!` EVALUATED,
/// and a front end that scores that as a failure needs to be able to tell it from one that did not.
pub fn eval(target: &str, formula: &str) -> Result<FormulaOutcome, Refusal> {
    let resolved = address::resolve(target)?;
    if resolved.workbook.sheet_names().is_empty() {
        let msg = format!("{target:?} has no tabs (a tab is a sub-folder of cell/range files)");
        return Err(fail(Kind::Validation, &msg));
    }
    let (wb, tab) = resolved.as_context()?;
    wb.eval_formula(tab.unwrap_or(0), formula)
        .map_err(|diag| refused(vec![diag]))
}

pub fn trace(target: &str, dir: Direction, depth: Option<u32>) -> Result<TraceNode, Refusal> {
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

pub fn unpack(
    src: &Path,
    dest: Option<&Path>,
    decomposition: Option<Decomposition>,
    strict: bool,
) -> Result<Unpacked, Refusal> {
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
}

pub fn pack(folder: &Path, dest: Option<&Path>, ext: &str) -> Result<Packed, Refusal> {
    let dest = match dest {
        Some(d) => d.to_path_buf(),
        None => derive_pack_dest(folder, ext)?,
    };
    let wb = load(folder)?;
    if wb.sheet_names().is_empty() {
        let msg =
            format!("{folder:?} has no tabs to pack (a tab is a sub-folder of cell/range files)");
        return Err(fail(Kind::Validation, &msg));
    }
    fsa1_xlsx::write_xlsx(&wb, &dest)
        .map(|()| Packed {
            dest,
            sheets: wb.sheet_names().len(),
        })
        .map_err(|e| fail(pack_kind(&e), &e.to_string()))
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
