// Concern: draws a View as ASCII grids or nested nodes, diagnostics as a table, and a trace as an indented chain | Non-concern: computing any value | IO: (&View, &[Diagnostic] or &TraceNode) -> String

use annotated_tree::{CodebaseMap, DirNode, FileNode, Format as TreeFormat, for_format};
use comfy_table::presets::ASCII_FULL;
use comfy_table::{Cell, Table};
use fsa1_model::{
    Diagnostic, FigureView, NameView, Rect, RenderMode, SheetView, TraceNode, View, ViewScope,
};

pub fn table(view: &View) -> String {
    let named = view.sheets.len() > 1;
    let mut out: Vec<String> = Vec::with_capacity(view.sheets.len());
    for sheet in &view.sheets {
        let grid = grid_table(sheet);
        out.push(match (named, grid.is_empty()) {
            (true, true) => sheet.name.to_string(),
            (true, false) => format!("{}\n{grid}", sheet.name),
            (false, _) => grid,
        });
    }
    out.join("\n\n")
}

pub fn tree(view: &View, cap: u32) -> String {
    let root = match view.scope {
        ViewScope::Region(..) => match view.sheets.first() {
            Some(s) => {
                // A region is a rectangle of CELLS, so it names no figure.
                let cells = s.region.map(|r| cells_in(s, r, u32::MAX).0);
                dir_node(s.name.to_string(), Vec::new(), cells.unwrap_or_default(), 0)
            }
            None => dir_node(String::new(), Vec::new(), Vec::new(), 0),
        },
        ViewScope::Tab(_) => match view.sheets.first() {
            Some(s) => tab_dir(view, s, cap),
            None => dir_node(String::new(), Vec::new(), Vec::new(), 0),
        },
        ViewScope::Workbook => dir_node(
            String::new(),
            view.sheets.iter().map(|s| tab_dir(view, s, cap)).collect(),
            view.names.iter().map(name_node).collect(),
            0,
        ),
    };
    for_format(TreeFormat::Text, false).render(&CodebaseMap {
        roots: vec![root],
        warnings: Vec::new(),
    })
}

fn tab_dir(view: &View, sheet: &SheetView, cap: u32) -> DirNode {
    let mut files: Vec<FileNode> = Vec::new();
    let mut elided: u32 = 0;
    for fe in &sheet.files {
        if fe.array_formula && view.mode == RenderMode::Functions {
            if let Some((_, text)) = sheet.cell(fe.region.min_col, fe.region.min_row) {
                files.push(file_node(fe.name.to_string(), text));
            }
            continue;
        }
        let (cells, over) = cells_in(sheet, fe.region, cap);
        files.extend(cells);
        elided = elided.saturating_add(over);
    }
    files.extend(sheet.names.iter().map(name_node));
    files.extend(sheet.figures.iter().map(figure_node));
    dir_node(sheet.name.to_string(), Vec::new(), files, elided)
}

/// A cell's line reads what it is then what it came from, and a figure's follows: the mark it draws,
/// then the ranges it binds. A figure that binds nothing shows the mark and no arrow.
fn figure_node(f: &FigureView) -> FileNode {
    let entry = f.name.rsplit_once('/').map_or(f.name.as_str(), |(_, e)| e);
    let text = match f.binds.as_slice() {
        [] => f.kind.clone(),
        binds => format!("{} ← {}", f.kind, binds.join(", ")),
    };
    file_node(entry.to_string(), &text)
}

fn cells_in(sheet: &SheetView, r: Rect, cap: u32) -> (Vec<FileNode>, u32) {
    let mut nodes = Vec::new();
    'rect: for row in r.min_row..=r.max_row {
        for col in r.min_col..=r.max_col {
            if nodes.len() as u64 >= u64::from(cap) {
                break 'rect;
            }
            if let Some((label, text)) = sheet.cell(col, row) {
                nodes.push(file_node(label, text));
            }
        }
    }
    let total = (u64::from(r.max_row - r.min_row) + 1) * (u64::from(r.max_col - r.min_col) + 1);
    let over = (total - nodes.len() as u64).min(u64::from(u32::MAX)) as u32;
    (nodes, over)
}

fn name_node(n: &NameView) -> FileNode {
    file_node(n.ident.clone(), &n.text)
}

fn file_node(name: String, text: &str) -> FileNode {
    FileNode {
        name,
        annotation: (!text.is_empty()).then(|| text.to_string()),
        age_secs: None,
        sidecar: false,
    }
}

fn dir_node(name: String, dirs: Vec<DirNode>, files: Vec<FileNode>, elided_files: u32) -> DirNode {
    DirNode {
        name,
        charter: None,
        deps: None,
        dirs,
        files,
        elided_dirs: 0,
        elided_files,
    }
}

fn covered(sheet: &SheetView, col: u32, row: u32) -> bool {
    sheet
        .figures
        .iter()
        .filter_map(|f| f.cover)
        .any(|rect| rect.contains(col, row))
}

/// A covered cell is marked in the grid itself, because the terminal is the only place an agent
/// laying out a sheet ever sees: `fig` where the cell is empty, `fig! ` prefixed where it is not, so
/// a value an export would hide is distinguishable from blank space the figure merely sits over.
fn grid_table(sheet: &SheetView) -> String {
    let (Some(grid), Some(region)) = (sheet.grid.as_ref(), sheet.region) else {
        return String::new();
    };
    let mut t = Table::new();
    t.load_preset(ASCII_FULL);
    let mut header: Vec<Cell> = Vec::with_capacity(grid.col_labels.len() + 1);
    header.push(Cell::new(""));
    header.extend(grid.col_labels.iter().map(Cell::new));
    t.set_header(header);
    for (r, row) in grid.rows.iter().enumerate() {
        let mut cells: Vec<Cell> = Vec::with_capacity(row.cells.len() + 1);
        cells.push(Cell::new(&row.row_label));
        for (c, text) in row.cells.iter().enumerate() {
            let (col, row) = (region.min_col + c as u32, region.min_row + r as u32);
            cells.push(if !covered(sheet, col, row) {
                Cell::new(text)
            } else if text.is_empty() {
                Cell::new("fig")
            } else {
                Cell::new(format!("fig! {text}"))
            });
        }
        t.add_row(cells);
    }
    t.to_string()
}

pub fn diagnostics_table(diags: &[Diagnostic]) -> String {
    let mut table = Table::new();
    table.load_preset(ASCII_FULL);
    table.set_header(vec![
        Cell::new("severity"),
        Cell::new("code"),
        Cell::new("location"),
        Cell::new("message"),
        Cell::new("help"),
    ]);

    if diags.is_empty() {
        table.add_row(vec![
            Cell::new("ok"),
            Cell::new("none"),
            Cell::new("-"),
            Cell::new("no diagnostics: the workbook is clean"),
            Cell::new("-"),
        ]);
        return table.to_string();
    }

    for d in diags {
        table.add_row(vec![
            Cell::new(severity_str(d)),
            Cell::new(d.code.code_str()),
            Cell::new(d.loc.to_string()),
            Cell::new(&d.message),
            Cell::new(d.code.help()),
        ]);
    }
    table.to_string()
}

fn severity_str(d: &Diagnostic) -> &'static str {
    use fsa1_model::Severity;
    match d.code.severity() {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

/// Past this depth the indent holds still: two columns per level down an unbounded dependency chain
/// makes the text quadratic in depth. Every node still gets its own line.
const MAX_INDENT_LEVEL: usize = 64;

pub fn trace(root: &TraceNode) -> String {
    let mut out = String::new();
    trace_into(root, &mut out);
    out
}

fn trace_into(root: &TraceNode, out: &mut String) {
    let mut stack: Vec<(&TraceNode, usize)> = vec![(root, 0)];
    while let Some((node, depth)) = stack.pop() {
        let tag = match &node.hash {
            Some(h) => h.as_str(),
            None => node.status.as_str(),
        };
        let formula = match &node.formula {
            Some(f) => format!("  {f}"),
            None => String::new(),
        };
        let repeated = if node.repeated { "  (repeated)" } else { "" };
        out.push_str(&format!(
            "{}{}{formula}  -> {}  [{tag}]{repeated}\n",
            "  ".repeat(depth.min(MAX_INDENT_LEVEL)),
            node.cell,
            node.value
        ));
        stack.extend(node.children.iter().rev().map(|c| (c, depth + 1)));
    }
}
