// Concern: draws a View as ASCII grids or nested nodes, diagnostics as a table, and a trace as an indented chain | Non-concern: computing any value | IO: (&View, &[Diagnostic] or &TraceNode) -> String

use annotated_tree::{CodebaseMap, DirNode, FileNode, Format as TreeFormat, for_format};
use comfy_table::presets::ASCII_FULL;
use comfy_table::{Cell, Table};
use fsa1_model::{
    Diagnostic, FigureView, NameView, Rect, RenderMode, SheetView, TraceNode, View, ViewScope,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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

/// A cell's line reads what it is then what it came from, and a figure's follows.
fn figure_node(f: &FigureView) -> FileNode {
    file_node(entry_name(&f.name).to_string(), &drawn_and_bound(f))
}

/// A figure's second reading, wherever it is shown: the mark it draws, then the ranges it binds. A
/// figure that binds nothing shows the mark and no arrow.
fn drawn_and_bound(f: &FigureView) -> String {
    match f.binds.as_slice() {
        [] => f.kind.clone(),
        binds => format!("{} ← {}", f.kind, binds.join(", ")),
    }
}

/// A figure's name LOCATES it; the basename is what names it.
pub(crate) fn entry_name(name: &str) -> &str {
    name.rsplit_once('/').map_or(name, |(_, e)| e)
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
    cut_blocks(&t.to_string(), &blocks(sheet, region))
}

/// A range-form figure's NAME is its rectangle, so a workbook that loads clean leaves that rectangle
/// empty and ONE block can say which figure it is. Anything else keeps the per-cell marks: erasing a
/// cell holding a value would hide it, and two blocks reaching into one band would clobber each
/// other mid-line. Each rectangle comes back clipped to the drawn region and relative to it.
fn blocks(sheet: &SheetView, region: Rect) -> Vec<(Rect, [String; 2])> {
    let mut out = Vec::new();
    for (i, f) in sheet.figures.iter().enumerate() {
        let Some(cover) = f.cover.filter(|_| f.range_form) else {
            continue;
        };
        let clash =
            sheet.figures.iter().enumerate().any(|(j, other)| {
                j != i && other.cover.is_some_and(|c| c.intersect(&cover).is_some())
            });
        let Some(clip) = cover.intersect(&region).filter(|_| !clash) else {
            continue;
        };
        if all_empty(sheet, clip) {
            out.push((relative(clip, region), label(f)));
        }
    }
    out
}

/// Unreachable through the CLI today: a cell file inside a range figure's rectangle is an overlap,
/// and `address::resolve` refuses on any load diagnostic, so `render` exits before a drawer sees the
/// workbook. The guard stands for the caller that reaches this layer directly, and for the day a
/// value can land under a cover — erasing a cell holding one would hide it.
fn all_empty(sheet: &SheetView, r: Rect) -> bool {
    (r.min_row..=r.max_row).all(|row| {
        (r.min_col..=r.max_col)
            .all(|col| sheet.cell(col, row).is_none_or(|(_, text)| text.is_empty()))
    })
}

fn relative(r: Rect, region: Rect) -> Rect {
    Rect {
        min_col: r.min_col - region.min_col,
        min_row: r.min_row - region.min_row,
        max_col: r.max_col - region.min_col,
        max_row: r.max_row - region.min_row,
    }
}

/// The label is TWO lines, the identity first: what the figure IS, then what it draws. The arrow and
/// the binding separator carry no padding — the block is the one place the label competes for width,
/// and `tree` keeps the spaced spelling where width is free. What does not fit inside the block is
/// the writer's problem, never a column's.
fn label(f: &FigureView) -> [String; 2] {
    let drawn = match f.binds.as_slice() {
        [] => f.kind.clone(),
        binds => format!("{}←{}", f.kind, binds.join(",")),
    };
    [entry_name(&f.name).to_string(), drawn]
}

/// Column boundaries are READ from the table's own top border, never recomputed from widths this
/// file does not own: table column 0 is the row-label gutter, so grid column `c` is boundary `c + 1`.
/// The border is pure ASCII, so a `+` byte offset there IS its DISPLAY column — which is what every
/// other line is then cut at, because comfy-table pads by display width and not by bytes.
fn cut_blocks(table: &str, blocks: &[(Rect, [String; 2])]) -> String {
    let mut lines: Vec<String> = table.split('\n').map(str::to_string).collect();
    let bounds: Vec<usize> = match lines.first() {
        Some(top) => top
            .char_indices()
            .filter(|c| c.1 == '+')
            .map(|c| c.0)
            .collect(),
        None => return table.to_string(),
    };
    let grid_rows = row_lines(&lines);
    for (rect, label) in blocks {
        let (Some(&lo), Some(&hi)) = (
            bounds.get(rect.min_col as usize + 1),
            bounds.get(rect.max_col as usize + 2),
        ) else {
            continue;
        };
        let (Some(head), Some(tail)) = (
            grid_rows.get(rect.min_row as usize),
            grid_rows.get(rect.max_row as usize),
        ) else {
            continue;
        };
        let (Some(&first), Some(&last)) = (head.first(), tail.last()) else {
            continue;
        };
        // Blanking the interior takes its `|`, its `fig` texts and its `---+---` runs at once.
        for (line, a, b) in band(&mut lines, first, last, lo, hi) {
            line.replace_range(a + 1..b, &" ".repeat(hi - lo - 1));
        }
        for edge in [first.saturating_sub(1), last + 1] {
            for (line, a, b) in band(&mut lines, edge, edge, lo, hi) {
                let opened: String = line[a + 1..b].replace('+', "-");
                line.replace_range(a + 1..b, &opened);
            }
        }
        let rows: Vec<usize> = grid_rows[rect.min_row as usize..=rect.max_row as usize]
            .iter()
            .flatten()
            .copied()
            .filter(|&i| lines.get(i).is_some_and(|l| interior(l, lo, hi).is_some()))
            .collect();
        write_label(&mut lines, &rows, lo, hi, label);
    }
    lines.join("\n")
}

/// The ROW axis read off the table's own structure, exactly as the boundaries above are: a cell
/// holding a newline spans several lines, so a grid row is not a line and no stride finds it. A
/// separator closes a row, so a row's lines are the run between two of them — the first run is the
/// header, and the Nth run after it is grid row N.
fn row_lines(lines: &[String]) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut run: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if separator(line) {
            if !run.is_empty() {
                out.push(std::mem::take(&mut run));
            }
        } else {
            run.push(i);
        }
    }
    out.push(run);
    out.into_iter().skip(1).filter(|r| !r.is_empty()).collect()
}

/// Border bytes and nothing else. comfy-table pads every cell with a space, so no line carrying a
/// row's contents is spelled this way — a cell whose own text is `---` still sits inside padding.
fn separator(line: &str) -> bool {
    !line.is_empty() && line.chars().all(|c| matches!(c, '+' | '-' | '=' | '|'))
}

fn band(
    lines: &mut [String],
    first: usize,
    last: usize,
    lo: usize,
    hi: usize,
) -> impl Iterator<Item = (&mut String, usize, usize)> {
    lines
        .iter_mut()
        .skip(first)
        .take(last + 1 - first)
        .filter_map(move |l| interior(l, lo, hi).map(|(a, b)| (l, a, b)))
}

/// Where display columns `lo` and `hi` fall in THIS line's bytes, and only if a border stands at
/// both. Correct positioning lands on borders on every line the band covers, so this is a fail-safe
/// that never fires today: a line it declines is one comfy-table did not draw rectangularly, and
/// leaving it alone beats mangling it. A cell whose own TEXT holds `|` or `+` never misleads it.
fn interior(line: &str, lo: usize, hi: usize) -> Option<(usize, usize)> {
    let (mut a, mut b) = (None, None);
    let mut col = 0;
    for (i, ch) in line.char_indices() {
        match col {
            c if c == lo => a = Some(i),
            c if c == hi => {
                b = Some(i);
                break;
            }
            _ => {}
        }
        col += ch.width().unwrap_or(0);
    }
    let border = |i: usize| line[i..].starts_with(['|', '+']);
    a.zip(b).filter(|&(a, b)| border(a) && border(b))
}

/// The wrapped lines are centred as a GROUP in the block's rows, and each is centred between the two
/// boundaries. An interior under four columns holds no reading worth cutting to, so it holds none.
/// Every span written to is the blank ASCII just laid down, so display columns inside it are bytes
/// and a label of any width replaces exactly the columns it occupies.
fn write_label(lines: &mut [String], rows: &[usize], lo: usize, hi: usize, label: &[String; 2]) {
    let width = hi - lo - 1;
    if rows.is_empty() || width < 4 {
        return;
    }
    let wrapped = wrap(label, width, rows.len());
    let top = (rows.len() - wrapped.len()) / 2;
    for (i, text) in wrapped.iter().enumerate() {
        let line = &mut lines[rows[top + i]];
        let Some((a, _)) = interior(line, lo, hi) else {
            continue;
        };
        let (w, start) = (text.width(), a + 1 + (width - text.width()) / 2);
        line.replace_range(start..start + w, text);
    }
}

/// A label line that fits takes one row. One that does not breaks after its `\u{2190}` FIRST, so the mark
/// takes a row and the ranges start together on the next, then after a `,` — decision 6's tight join
/// leaves no space to break at — and then on whitespace. The rows the two lines make are cut
/// together: what does not fit ends `\u{2026}`, and a piece wider than the interior is cut the same way.
fn wrap(label: &[String; 2], width: usize, rows: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in label {
        if line.width() <= width {
            out.push(line.clone());
            continue;
        }
        for seg in after_arrow(line) {
            fill(seg, width, &mut out);
        }
    }
    let dropped = out.len() > rows;
    out.truncate(rows);
    if dropped && let Some(last) = out.last_mut() {
        last.push('\u{2026}');
    }
    for line in &mut out {
        if line.width() > width {
            *line = clip(line, width - 1);
        }
    }
    out
}

/// Each segment starts its own row, so the arrow break wins over the two beneath it.
fn after_arrow(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(i) = rest.find('\u{2190}') {
        let (head, tail) = rest.split_at(i + '\u{2190}'.len_utf8());
        out.push(head);
        rest = tail;
    }
    if !rest.is_empty() {
        out.push(rest);
    }
    out
}

/// Greedy over the pieces of ONE segment: a comma break rejoins with nothing, a whitespace break
/// with the space it broke at.
fn fill(seg: &str, width: usize, out: &mut Vec<String>) {
    let start = out.len();
    for (spaced, piece) in pieces(seg) {
        let room = out.len() > start;
        let gap = usize::from(spaced);
        match out.last_mut() {
            Some(row) if room && row.width() + gap + piece.width() <= width => {
                if spaced {
                    row.push(' ');
                }
                row.push_str(piece);
            }
            _ => out.push(piece.to_string()),
        }
    }
}

/// The pieces a segment may break between, each flagged with whether whitespace preceded it: words,
/// and within a word every run ending in the `,` that closes it.
fn pieces(seg: &str) -> Vec<(bool, &str)> {
    let mut out = Vec::new();
    for word in seg.split_whitespace() {
        let mut rest = word;
        let mut lead = true;
        while !rest.is_empty() {
            let cut = rest.find(',').map_or(rest.len(), |i| i + 1);
            let (piece, tail) = rest.split_at(cut);
            out.push((lead, piece));
            lead = false;
            rest = tail;
        }
    }
    out
}

/// Cut to a display width, never a character count, and ended `…` — a wide character that would
/// straddle the limit is dropped whole rather than half-drawn.
fn clip(line: &str, width: usize) -> String {
    let mut out = String::new();
    let mut w = 0;
    for ch in line.chars() {
        w += ch.width().unwrap_or(0);
        if w > width {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

/// The verdict form: a severity column, because the caller is judging and its exit code agrees.
pub fn diagnostics_table(diags: &[Diagnostic]) -> String {
    located_table(diags, true)
}

/// The REPORT form: the same rows without the severity column. A code's severity says how bad the
/// finding is, never what the run did about it — so a verb that names a loss it accepted (a `pack`
/// that wrote the file and exits 0) must not print `error` beside work it completed on purpose. The
/// same code keeps its severity for whoever does refuse on it: `pack --strict` and `check --xlsx`.
pub fn findings_table(diags: &[Diagnostic]) -> String {
    located_table(diags, false)
}

fn located_table(diags: &[Diagnostic], severity: bool) -> String {
    let mut table = Table::new();
    table.load_preset(ASCII_FULL);
    let row = |first: Cell, rest: [Cell; 4]| {
        let mut cells = Vec::with_capacity(5);
        if severity {
            cells.push(first);
        }
        cells.extend(rest);
        cells
    };
    table.set_header(row(
        Cell::new("severity"),
        [
            Cell::new("code"),
            Cell::new("location"),
            Cell::new("message"),
            Cell::new("help"),
        ],
    ));

    if diags.is_empty() {
        table.add_row(row(
            Cell::new("ok"),
            [
                Cell::new("none"),
                Cell::new("-"),
                Cell::new("no diagnostics: the workbook is clean"),
                Cell::new("-"),
            ],
        ));
        return table.to_string();
    }

    for d in diags {
        table.add_row(row(
            Cell::new(severity_str(d)),
            [
                Cell::new(d.code.code_str()),
                Cell::new(d.loc.to_string()),
                Cell::new(&d.message),
                Cell::new(d.code.help()),
            ],
        ));
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
