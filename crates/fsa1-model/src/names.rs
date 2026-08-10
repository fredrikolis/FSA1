// Concern: what an entry's NAME means — a range, a sidecar, a figure, a defined name — and what it resolves to | Non-concern: finding the entries on disk, evaluating | IO: (entries) -> NameTable

use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::overlap::Rect;
use fsa1_ast::a1::parse_a1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameTarget {
    /// Substituted verbatim: it is a self-contained reference token.
    Ref(String),
    /// Substituted wrapped in parentheses, so it keeps its precedence inside the referencing formula.
    Expr(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameScope {
    Workbook,
    Sheet(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    pub ident: String,
    pub scope: NameScope,
    pub target: NameTarget,
}

/// The SEAM the reader-union spans: the caller owns the filesystem detection, and both forms
/// normalize to the same [`Name`], so an all-ref-file writer is a drop-in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameRepr {
    /// Already resolved by the caller to its target `(sheet, cell-A1)`.
    Symlink {
        target_sheet: String,
        target_cell: String,
    },
    /// The target ref or expr — or a DEGRADED symlink, which a symlink-flattening container (zip)
    /// collapses to a bare A1 target or a relative path (`Data/H1`, `../Data/H1`).
    RefFile { content: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawNameEntry {
    pub scope: NameScope,
    /// As found on disk, so it may carry a `.begin`/`.end` corner suffix: `total`, `Days.begin`.
    pub entry_name: String,
    pub form: NameRepr,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NameTable {
    /// DEFINITION order — what the writer round-trips and the tests read.
    names: Vec<Name>,
    /// Never iterated: definition order lives in `names`.
    index: ScopeIndex,
}

/// Nested (identifier, then scope) rather than keyed on a `(ident, scope)` tuple, so a lookup borrows
/// `&str` and allocates NOTHING — [`NameTable::resolve`] runs once per name token of every formula in
/// the workbook. Nesting also makes shadowing the shape of the data: one identifier owns at most one
/// workbook slot and one slot per sheet, and a lookup tries the sheet slot first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ScopeIndex {
    by_ident: std::collections::HashMap<String, ScopeSlots>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ScopeSlots {
    workbook: Option<usize>,
    sheets: std::collections::HashMap<String, usize>,
}

impl ScopeIndex {
    fn get(&self, ident: &str, scope: &NameScope) -> Option<usize> {
        let slots = self.by_ident.get(ident)?;
        match scope {
            NameScope::Workbook => slots.workbook,
            NameScope::Sheet(s) => slots.sheets.get(s).copied(),
        }
    }

    /// The caller has already established the slot is free.
    fn insert(&mut self, ident: &str, scope: &NameScope, pos: usize) {
        let slots = self.by_ident.entry(ident.to_string()).or_default();
        match scope {
            NameScope::Workbook => slots.workbook = Some(pos),
            NameScope::Sheet(s) => {
                slots.sheets.insert(s.clone(), pos);
            }
        }
    }

    /// The shadowing rule: a sheet-scoped definition wins over a workbook-scoped one.
    fn resolve(&self, ident: &str, sheet: &str) -> Option<usize> {
        let slots = self.by_ident.get(ident)?;
        slots.sheets.get(sheet).copied().or(slots.workbook)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Corner {
    Begin,
    End,
}

/// The aliases are read but never written: only `begin`/`end` are documented.
fn corner_alias(suffix: &str) -> Option<Corner> {
    match suffix.to_ascii_lowercase().as_str() {
        "begin" | "start" | "tl" | "topleft" => Some(Corner::Begin),
        "end" | "br" | "bottomright" => Some(Corner::End),
        _ => None,
    }
}

/// An UNKNOWN suffix is part of the identifier: `my.name` -> (`my.name`, None).
fn split_corner(entry_name: &str) -> (String, Option<Corner>) {
    if let Some((ident, suffix)) = entry_name.rsplit_once('.')
        && let Some(c) = corner_alias(suffix)
    {
        return (ident.to_string(), Some(c));
    }
    (entry_name.to_string(), None)
}

/// Lenient, so `a1` and `$A$1` count: a name must never collide with a cell's filename.
fn ident_is_a1(ident: &str) -> bool {
    parse_a1(ident).is_ok()
}

/// The suffix that makes a range-named entry presentation rather than a grid.
pub const PRESENTATION_SUFFIX: &str = ".css";

/// The scoping root a presentation sidecar is named for, or `None` for any other entry. Asked BEFORE
/// [`is_cell_filename`], which claims every name holding a range separator; the stem still goes
/// through the filename parser, which may reject its spelling.
pub fn presentation_stem(name: &str) -> Option<&str> {
    let stem = name.strip_suffix(PRESENTATION_SUFFIX)?;
    is_cell_filename(stem).then_some(stem)
}

/// The tab's OWN layer: the suffix alone, so no stem and no region — a different kind from a rooted
/// sidecar rather than a degenerate one, which is why it is asked for separately.
pub fn is_tab_layer(name: &str) -> bool {
    name == PRESENTATION_SUFFIX
}

/// What a `.css` entry states in the tab it sits in: the tab's own layer, the range root a rooted
/// sidecar is named for, or the figure beside it. [`CssEntry::Unrooted`] is the placement half —
/// what it says about the figure is [`crate::Figures`]'s question, not this one's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CssEntry<'a> {
    TabLayer,
    Root(&'a str),
    Unrooted(&'a str),
}

/// The tab's figure stems — the `<stem>` of every `<stem>.json` beside the entry being classified.
/// Held as a set because a tab's listing is walked once and asked of many times.
pub type FigureStems = std::collections::BTreeSet<String>;

/// The three kinds, or `None` for a name that is no `.css` at all. TAB-AWARE, and it must be:
/// [`parse_a1`] is deliberately lenient, so `Chart1`, `Q4` and `A1-B2` all read as ranges by NAME
/// while `unpack` writes `chart1.json` routinely. A `.css` whose stem has a `<stem>.json` SIBLING is
/// that figure's placement whatever the stem spells; only without one does the name decide.
pub fn css_entry<'a>(name: &'a str, figures: &FigureStems) -> Option<CssEntry<'a>> {
    let stem = name.strip_suffix(PRESENTATION_SUFFIX)?;
    Some(if stem.is_empty() {
        CssEntry::TabLayer
    } else if figures.contains(stem) {
        CssEntry::Unrooted(stem)
    } else if is_cell_filename(stem) {
        CssEntry::Root(stem)
    } else {
        CssEntry::Unrooted(stem)
    })
}

/// The stems of every figure in one tab's listing — what [`css_entry`] is asked against.
pub fn figure_stems<'a>(names: impl IntoIterator<Item = &'a str>) -> FigureStems {
    names
        .into_iter()
        .filter_map(figure_stem)
        .map(str::to_string)
        .collect()
}

/// Every kind — what a loader asks when it wants every entry that states presentation. Name-only,
/// because the SUFFIX alone settles it: which kind a `.css` is takes a tab, but that it states
/// presentation does not.
pub fn is_presentation_entry(name: &str) -> bool {
    name.strip_suffix(PRESENTATION_SUFFIX).is_some()
}

/// The suffix that makes an entry a figure.
pub const FIGURE_SUFFIX: &str = ".json";

/// The STEM a figure is stated under, or `None` for any other entry. The stem is a name or a range
/// -- [`stem_region`] is what sorts the two -- and the range form is the one that collides with a
/// cell. Neither takes part in the cascade.
pub(crate) fn figure_stem(name: &str) -> Option<&str> {
    let stem = name.strip_suffix(FIGURE_SUFFIX)?;
    (!stem.is_empty()).then_some(stem)
}

/// Excel's grid, zero-based: the last column `XFD` and the last row 1,048,576.
const MAX_COL: u32 = 16_383;
const MAX_ROW: u32 = 1_048_575;

/// The rectangle a STEM states, or `None` where it names something other than a place on the grid.
/// The STRICT test -- [`is_cell_filename`] and `parse_a1` are looser and are not it. The grid bound
/// is what the grammar cannot do: `XFE1` is in-grammar one column past the last, and the bound alone
/// makes it a name. A figure's two forms sort by it, and a sidecar's root reads through it.
pub(crate) fn stem_region(stem: &str) -> Option<Rect> {
    let region = crate::filename::parse_filename(stem).ok()?.region;
    (region.max_col <= MAX_COL && region.max_row <= MAX_ROW).then_some(region)
}

/// What an ENTRY occupies: the rectangle a range-form figure's name states, `None` for a name-form
/// figure -- which floats -- and for anything that is not a figure at all. The one derivation every
/// loader shares, so a tree's occupancy cannot differ by which loader walked it.
pub fn figure_occupancy(name: &str) -> Option<Rect> {
    stem_region(figure_stem(name)?)
}

/// Asked BEFORE the defined-name branch, which would otherwise claim the stem as a name.
pub fn is_figure_entry(name: &str) -> bool {
    figure_stem(name).is_some()
}

/// The routing predicate both loader paths share. A range separator cannot occur in a name, so `:`
/// or a `-` joining two A1 corners makes the file a range ATTEMPT, routed to the filename parser
/// which may still reject the spelling. Both are read on every platform so a `convert`-ed tree loads
/// anywhere; everything else (`Days`, `Days.begin`, `Tax_Rate`) is a name.
pub fn is_cell_filename(name: &str) -> bool {
    if name.contains(crate::RANGE_SEP_POSIX) {
        return true;
    }
    // `-` is a range only where it joins two A1 corners, or two ends of ONE axis.
    if let Some((left, right)) = name.split_once(crate::RANGE_SEP_WINDOWS)
        && ((parse_a1(left).is_ok() && parse_a1(right).is_ok()) || one_axis(left, right))
    {
        return true;
    }
    parse_a1(name).is_ok()
}

/// Both ends of ONE axis: all letters, or all digits. The open range's shape, wherever a name is
/// classified before it is parsed.
pub fn one_axis(left: &str, right: &str) -> bool {
    let alpha = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphabetic());
    let digit = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    (alpha(left) && alpha(right)) || (digit(left) && digit(right))
}

/// Validates the WHOLE ref, not just the address after the last `!`, so an EXPRESSION that merely
/// ends in a valid address (`1/Sheet!A1`) is never misclassified as a ref while a quoted-sheet ref
/// (`'Sheet 2'!A1`) still is one.
fn is_pure_ref(text: &str) -> bool {
    let (sheet_part, addr) = match text.rsplit_once('!') {
        Some((s, a)) => (Some(s), a),
        None => (None, text),
    };
    if let Some(s) = sheet_part
        && !is_sheet_token(s)
    {
        return false;
    }
    if addr.contains(['{', '}', '(', ')', ',', ' ', '#', '!', '\'']) {
        return false;
    }
    match addr.split_once(':') {
        Some((l, r)) => parse_a1(l).is_ok() && parse_a1(r).is_ok(),
        None => parse_a1(addr).is_ok(),
    }
}

/// A bare identifier — the same set [`quote_sheet`] leaves unquoted — or a `'…'`-quoted string.
/// Anything else before a `!` makes its text an expression rather than a ref.
fn is_sheet_token(s: &str) -> bool {
    if let Some(inner) = s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        // Once `''` pairs are removed, no lone `'` may remain.
        s.len() >= 2 && !inner.replace("''", "").contains('\'')
    } else {
        let mut cs = s.chars();
        cs.next().is_some_and(|c| c.is_ascii_alphabetic())
            && s.chars().all(|c| c.is_ascii_alphanumeric())
    }
}

/// The inverse of [`quote_sheet`], so a resolved ref can be re-qualified with canonical quoting.
fn unquote_sheet(s: &str) -> String {
    if let Some(inner) = s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        inner.replace("''", "'")
    } else {
        s.to_string()
    }
}

/// A degraded symlink and the alias file that stands in for one carry no leading `=`; `emit_ref_file`
/// and a hand-written formula ref do.
fn strip_eq(content: &str) -> &str {
    let t = content.trim();
    t.strip_prefix('=').unwrap_or(t).trim()
}

enum DegradedPath {
    Resolved(String, String),
    /// The caller classifies it by the other rules: a pure ref, or an expression.
    NotAPath,
    /// A target the caller must turn into a located refusal, never a silently-wrong cross-sheet ref.
    Ambiguous,
}

/// Reads a ref-file body as the relative filesystem path a symlink-flattening container collapsed a
/// symlink to. SCOPE-AWARE, because a `Sheet/Cell` body is ambiguous with an Excel division: under a
/// WORKBOOK scope a non-A1 left is the legitimate degrade, while a SHEET-scoped name reaching another
/// tab always carries `../`, so the same body there is [`DegradedPath::Ambiguous`].
fn degraded_path_target(body: &str, scope: &NameScope) -> DegradedPath {
    if body.contains('!') {
        return DegradedPath::NotAPath; // a `Sheet!Cell` ref — `is_pure_ref` classifies it
    }
    let is_up = body.starts_with("../");
    let rel = body.strip_prefix("../").unwrap_or(body);
    let Some((sheet, cell)) = rel.rsplit_once('/') else {
        return DegradedPath::NotAPath; // a bare cell, ref, or expr
    };
    let well_formed =
        !sheet.is_empty() && !sheet.contains('/') && !cell.contains(':') && parse_a1(cell).is_ok();
    if is_up {
        // No formula begins `../`, so only a sheet-scoped name legitimately climbs out of its folder.
        return match scope {
            NameScope::Sheet(_) if well_formed => {
                DegradedPath::Resolved(sheet.to_string(), cell.to_string())
            }
            _ => DegradedPath::Ambiguous,
        };
    }
    if parse_a1(sheet).is_ok() {
        // `A1/B1`: both sides A1, so a division EXPRESSION and never a path.
        return DegradedPath::NotAPath;
    }
    match scope {
        NameScope::Workbook if well_formed => {
            DegradedPath::Resolved(sheet.to_string(), cell.to_string())
        }
        _ => DegradedPath::Ambiguous,
    }
}

impl NameTable {
    pub fn empty() -> NameTable {
        NameTable::default()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Every refusal is COLLECTED and located, never a silent drop and never a silently-wrong name.
    pub fn build(entries: Vec<RawNameEntry>) -> (NameTable, Vec<Diagnostic>) {
        let mut diags = Vec::new();
        let mut table = NameTable::default();
        let mut corners = PendingCorners::default();

        for entry in entries {
            let RawNameEntry {
                scope,
                entry_name,
                form,
            } = entry;
            let (ident, corner) = split_corner(&entry_name);
            if ident_is_a1(&ident) {
                diags.push(refuse(
                    &entry_name,
                    format!(
                        "name {ident:?} parses as an A1 address; a name is identified by an \
                         identifier, never an A1 coordinate (rename it)"
                    ),
                ));
                continue;
            }
            match corner {
                Some(c) => acc_corner(&mut corners, scope, ident, c, form, &mut diags),
                None => {
                    if let Some(name) = bare_name(scope, ident, &entry_name, form, &mut diags) {
                        insert_name(&mut table, name, &mut diags);
                    }
                }
            }
        }
        for acc in corners.accs {
            if let Some(name) = acc.finish(&mut diags) {
                insert_name(&mut table, name, &mut diags);
            }
        }
        (table, diags)
    }

    /// CASE-SENSITIVE, unlike Excel: an FSA1 name IS a filesystem entry, so `TaxRate` and `taxrate`
    /// are two distinct files and a case-folding lookup would be ambiguous between them. A token
    /// differing only in case therefore resolves to a located `#NAME?` — a conscious keep.
    pub fn resolve(&self, ident: &str, sheet: &str) -> Option<&NameTarget> {
        self.index
            .resolve(ident, sheet)
            .map(|pos| &self.names[pos].target)
    }

    /// Only `=`-prefixed fields are touched, so a literal that happens to spell a name is left alone.
    /// A substitution contains no escape character, so splitting on raw tabs stays lossless.
    pub fn rewrite_tsv(&self, content: &str, sheet: &str) -> String {
        if self.is_empty() {
            return content.to_string();
        }
        let lines: Vec<String> = content
            .split('\n')
            .map(|line| {
                line.split('\t')
                    .map(|field| {
                        if field.starts_with('=') {
                            self.rewrite_formula(field, sheet)
                        } else {
                            field.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\t")
            })
            .collect();
        lines.join("\n")
    }

    /// Cycle-safe: a named-formula self-reference stops expanding and stays verbatim.
    fn rewrite_formula(&self, field: &str, sheet: &str) -> String {
        let body = &field[1..]; // past the leading `=`
        let mut visiting = Vec::new();
        format!("={}", self.rewrite_body(body, sheet, &mut visiting))
    }

    /// String literals and `'quoted sheet'` names are copied atomically; an identifier followed by
    /// `(` or `!`, or sitting after a `!`, is never a name. `visiting` guards against a cycle.
    fn rewrite_body(&self, body: &str, sheet: &str, visiting: &mut Vec<String>) -> String {
        let mut out = String::with_capacity(body.len());
        let mut chars = body.char_indices().peekable();
        while let Some((_, c)) = chars.next() {
            match c {
                '"' => {
                    out.push('"');
                    // Copy through the closing quote, honouring the `""` escape.
                    while let Some((_, ch)) = chars.next() {
                        out.push(ch);
                        if ch == '"' {
                            if matches!(chars.peek(), Some((_, '"'))) {
                                out.push('"');
                                chars.next();
                            } else {
                                break;
                            }
                        }
                    }
                }
                '\'' => {
                    out.push('\'');
                    while let Some((_, ch)) = chars.next() {
                        out.push(ch);
                        if ch == '\'' {
                            if matches!(chars.peek(), Some((_, '\''))) {
                                out.push('\'');
                                chars.next();
                            } else {
                                break;
                            }
                        }
                    }
                }
                c if c.is_ascii_alphabetic() || c == '_' => {
                    let mut ident = String::new();
                    ident.push(c);
                    while let Some(&(_, nc)) = chars.peek() {
                        if nc.is_ascii_alphanumeric() || nc == '_' || nc == '.' {
                            ident.push(nc);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    let after_bang = out.trim_end().ends_with('!');
                    let next = chars.peek().map(|&(_, c)| c);
                    if next == Some('!') || next == Some('(') || after_bang {
                        out.push_str(&ident);
                    } else {
                        out.push_str(&self.substitute(&ident, sheet, visiting));
                    }
                }
                other => out.push(other),
            }
        }
        out
    }

    /// An unresolvable name is left verbatim, so it becomes a located `#NAME?` at parse.
    fn substitute(&self, ident: &str, sheet: &str, visiting: &mut Vec<String>) -> String {
        if visiting.iter().any(|v| v == ident) {
            return ident.to_string(); // a named-formula cycle: stop; loads as #NAME?
        }
        match self.resolve(ident, sheet) {
            Some(NameTarget::Ref(a1)) => a1.clone(),
            Some(NameTarget::Expr(expr)) => {
                visiting.push(ident.to_string());
                let inner = self.rewrite_body(expr, sheet, visiting);
                visiting.pop();
                format!("({inner})")
            }
            None => ident.to_string(),
        }
    }

    pub fn names(&self) -> &[Name] {
        &self.names
    }
}

/// FIRST-SEEN order is load-bearing: [`NameTable::build`]'s finalize pass walks `accs`, so a
/// lone-corner refusal is emitted in the order the corners appeared. That is why the index sits
/// beside the vec rather than replacing it.
#[derive(Default)]
struct PendingCorners {
    accs: Vec<CornerAcc>,
    index: ScopeIndex,
}

impl PendingCorners {
    fn slot(&mut self, ident: String, scope: NameScope) -> &mut CornerAcc {
        let pos = match self.index.get(&ident, &scope) {
            Some(pos) => pos,
            None => {
                let pos = self.accs.len();
                self.index.insert(&ident, &scope, pos);
                self.accs.push(CornerAcc {
                    scope,
                    ident,
                    begin: None,
                    end: None,
                });
                pos
            }
        };
        &mut self.accs[pos]
    }
}

struct CornerAcc {
    scope: NameScope,
    ident: String,
    begin: Option<(String, String)>, // (sheet, cell)
    end: Option<(String, String)>,
}

impl CornerAcc {
    fn finish(self, diags: &mut Vec<Diagnostic>) -> Option<Name> {
        let entry = format!("{}.begin/.end", self.ident);
        let (Some((bs, bc)), Some((es, ec))) = (self.begin, self.end) else {
            diags.push(refuse(
                &entry,
                format!(
                    "range name {:?} has a lone corner; a range needs BOTH a `.begin` and a `.end`",
                    self.ident
                ),
            ));
            return None;
        };
        if bs != es {
            diags.push(refuse(
                &entry,
                format!(
                    "range name {:?} has corners on different sheets ({bs} vs {es})",
                    self.ident
                ),
            ));
            return None;
        }
        let (Ok(b), Ok(e)) = (parse_a1(&bc), parse_a1(&ec)) else {
            diags.push(refuse(
                &entry,
                format!("range name {:?} has an unparseable corner", self.ident),
            ));
            return None;
        };
        if b.col > e.col || b.row > e.row {
            diags.push(refuse(
                &entry,
                format!(
                    "range name {:?} is inverted: its `.begin` ({bc}) is below or right of its `.end` ({ec})",
                    self.ident
                ),
            ));
            return None;
        }
        Some(Name {
            ident: self.ident,
            scope: self.scope,
            target: NameTarget::Ref(qualify(&bs, &format!("{bc}:{ec}"))),
        })
    }
}

fn acc_corner(
    corners: &mut PendingCorners,
    scope: NameScope,
    ident: String,
    corner: Corner,
    form: NameRepr,
    diags: &mut Vec<Diagnostic>,
) {
    let target = match corner_target(&scope, &form) {
        Some(t) => t,
        None => {
            diags.push(refuse(
                &format!("{ident}.{corner:?}"),
                format!("range corner of {ident:?} does not resolve to a single cell"),
            ));
            return;
        }
    };
    let acc = corners.slot(ident, scope);
    match corner {
        Corner::Begin => acc.begin = Some(target),
        Corner::End => acc.end = Some(target),
    }
}

/// `None` wherever the content does not resolve to a single cell, which [`acc_corner`] turns into a
/// located refusal.
fn corner_target(scope: &NameScope, form: &NameRepr) -> Option<(String, String)> {
    match form {
        NameRepr::Symlink {
            target_sheet,
            target_cell,
        } => Some((target_sheet.clone(), target_cell.clone())),
        NameRepr::RefFile { content } => {
            let has_eq = content.trim_start().starts_with('=');
            let addr = strip_eq(content);
            // A degraded symlink corner is a BARE path, so a `=`-prefixed body is a formula and never a path: forcing it down NotAPath stops `=Revenue/A1` materializing a cross-sheet range.
            let degraded = if has_eq {
                DegradedPath::NotAPath
            } else {
                degraded_path_target(addr, scope)
            };
            match degraded {
                DegradedPath::Resolved(sheet, cell) => Some((sheet, cell)),
                DegradedPath::Ambiguous => None, // -> a located refusal in `acc_corner`
                DegradedPath::NotAPath => {
                    if let Some((sheet_part, a)) = addr.rsplit_once('!') {
                        (is_sheet_token(sheet_part) && !a.contains(':') && parse_a1(a).is_ok())
                            .then(|| (unquote_sheet(sheet_part), a.to_string()))
                    } else if !addr.contains(':') && parse_a1(addr).is_ok() {
                        // A degraded same-sheet symlink, read against the corner's scope sheet.
                        Some((scope_sheet(scope)?, addr.to_string()))
                    } else {
                        None
                    }
                }
            }
        }
    }
}

/// An unresolvable-looking body becomes an `Expr` that loads as a located `#NAME?`, never a build
/// refusal; only an AMBIGUOUS target refuses here.
fn bare_name(
    scope: NameScope,
    ident: String,
    entry_name: &str,
    form: NameRepr,
    diags: &mut Vec<Diagnostic>,
) -> Option<Name> {
    let target = match form {
        NameRepr::Symlink {
            target_sheet,
            target_cell,
        } => {
            // Validated at BUILD, so a malformed target is located now rather than deferred to eval.
            if parse_a1(&target_cell).is_err() {
                diags.push(refuse(
                    entry_name,
                    format!(
                        "name {ident:?} points at a malformed cell {target_cell:?}; a single-cell \
                         symlink target must be one A1 coordinate"
                    ),
                ));
                return None;
            }
            NameTarget::Ref(qualify(&target_sheet, &target_cell))
        }
        NameRepr::RefFile { content } => match classify_ref_file(&scope, &content) {
            Some(t) => t,
            None => {
                diags.push(refuse(
                    entry_name,
                    format!(
                        "name {ident:?} has an ambiguous target {:?}: a single-slash `Sheet/Cell` is \
                         not a valid target here — write a cross-sheet target as `../Sheet/Cell` (or \
                         `Sheet!Cell`), and a same-sheet target as a bare cell",
                        content.trim()
                    ),
                ));
                return None;
            }
        },
    };
    Some(Name {
        ident,
        scope,
        target,
    })
}

/// The leading `=` disambiguates FORM at ANY scope: a POSIX symlink degrades to a BARE relative
/// path, never one carrying `=`, so a `=`-prefixed body is an expression. Workbook-scoped
/// `=Revenue/A1` is therefore the division, never the cross-sheet ref `Revenue!A1`. `None` is
/// reserved for the AMBIGUOUS body its caller must refuse.
fn classify_ref_file(scope: &NameScope, content: &str) -> Option<NameTarget> {
    let has_eq = content.trim().starts_with('=');
    let body = strip_eq(content);
    // Re-qualified to the `Sheet!Cell` the live symlink resolved to, so a degraded workbook reads the same.
    if !has_eq {
        match degraded_path_target(body, scope) {
            DegradedPath::Resolved(sheet, cell) => {
                return Some(NameTarget::Ref(qualify(&sheet, &cell)));
            }
            DegradedPath::Ambiguous => return None,
            DegradedPath::NotAPath => {}
        }
    }
    Some(if is_pure_ref(body) {
        ref_target(scope, body)
    } else {
        NameTarget::Expr(body.to_string())
    })
}

/// A bare ref is qualified with the scope sheet, so a sheet-scoped name resolves the same from any
/// sheet — matching the symlink form, whose target sheet is always explicit.
fn ref_target(scope: &NameScope, body: &str) -> NameTarget {
    match body.rsplit_once('!') {
        Some((sheet_part, addr)) => NameTarget::Ref(qualify(&unquote_sheet(sheet_part), addr)),
        None => match scope {
            NameScope::Sheet(s) => NameTarget::Ref(qualify(s, body)),
            NameScope::Workbook => NameTarget::Ref(body.to_string()),
        },
    }
}

/// `None` for a workbook scope, where a bare ref stays unqualified.
fn scope_sheet(scope: &NameScope) -> Option<String> {
    match scope {
        NameScope::Sheet(s) => Some(s.clone()),
        NameScope::Workbook => None,
    }
}

/// The result is injected straight into a `=formula` the FSA1 lexer then parses, and the lexer
/// reads a space-bearing sheet name only in the `'…'!` form — an unquoted `My Data!B5` would split
/// at the space and resolve to `#NAME?`.
fn qualify(sheet: &str, addr: &str) -> String {
    format!("{}!{addr}", quote_sheet(sheet))
}

/// Deliberately conservative — quoting is always safe, and the FSA1 lexer's bare-word set is
/// narrower than Excel's, so a `_`/`.` name the Excel-parity quoter leaves bare is quoted here.
/// Public because a WRITER spelling a ref must quote by the very rule this reader unquotes by.
pub fn quote_sheet(name: &str) -> String {
    let bare = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric());
    if bare {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

fn insert_name(table: &mut NameTable, name: Name, diags: &mut Vec<Diagnostic>) {
    if table.index.get(&name.ident, &name.scope).is_some() {
        diags.push(refuse(
            &name.ident,
            format!("name {:?} is defined twice in the same scope", name.ident),
        ));
        return;
    }
    table
        .index
        .insert(&name.ident, &name.scope, table.names.len());
    table.names.push(name);
}

/// Anchored on the offending entry name.
fn refuse(entry: &str, message: String) -> Diagnostic {
    Diagnostic::new(Code::NameRefusal, Loc::file(entry), message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(name: &str) -> NameScope {
        NameScope::Sheet(name.to_string())
    }

    fn reffile(scope: NameScope, entry: &str, content: &str) -> RawNameEntry {
        RawNameEntry {
            scope,
            entry_name: entry.to_string(),
            form: NameRepr::RefFile {
                content: content.to_string(),
            },
        }
    }

    fn symlink(scope: NameScope, entry: &str, tsheet: &str, cell: &str) -> RawNameEntry {
        RawNameEntry {
            scope,
            entry_name: entry.to_string(),
            form: NameRepr::Symlink {
                target_sheet: tsheet.to_string(),
                target_cell: cell.to_string(),
            },
        }
    }

    /// The three kinds, so a `.css` naming no range is a kind of its own rather than a defined name.
    #[test]
    fn every_css_entry_classifies_by_its_stem() {
        let none = FigureStems::new();
        assert_eq!(css_entry(".css", &none), Some(CssEntry::TabLayer));
        assert_eq!(css_entry("A1:B2.css", &none), Some(CssEntry::Root("A1:B2")));
        assert_eq!(css_entry("A:A.css", &none), Some(CssEntry::Root("A:A")));
        assert_eq!(css_entry("Q4.css", &none), Some(CssEntry::Root("Q4")));
        assert_eq!(
            css_entry("Units.css", &none),
            Some(CssEntry::Unrooted("Units"))
        );
        assert_eq!(
            css_entry("sales.css", &none),
            Some(CssEntry::Unrooted("sales"))
        );
        assert_eq!(css_entry("Units.json", &none), None);
    }

    /// The SIBLING decides, not the spelling: `parse_a1` is lenient, so `Chart1`, `Q4` and `A1-B2`
    /// all read as ranges by name — and `unpack` writes `chart1.json` for every imported chart.
    #[test]
    fn a_css_beside_its_figure_is_that_figures_placement_whatever_the_stem_looks_like() {
        let figures = figure_stems(["Chart1.json", "Q4.json", "A1-B2.json", "sales1.json"]);
        for stem in ["Chart1", "Q4", "A1-B2", "sales1"] {
            assert_eq!(
                css_entry(&format!("{stem}.css"), &figures),
                Some(CssEntry::Unrooted(stem)),
                "{stem}.css sits beside {stem}.json"
            );
        }
        // The tab layer is the suffix alone, so no figure can claim it.
        assert_eq!(css_entry(".css", &figures), Some(CssEntry::TabLayer));
        // And a root the tab holds no figure for still reads as one.
        assert_eq!(
            css_entry("D5:E6.css", &figures),
            Some(CssEntry::Root("D5:E6"))
        );
        assert_eq!(
            css_entry("Q4.css", &figure_stems(["Other.json"])),
            Some(CssEntry::Root("Q4")),
            "with no Q4.json beside it the name decides, exactly as before",
        );
    }

    #[test]
    fn a_reffile_range_resolves_and_a_formula_sums_it() {
        let (t, d) = NameTable::build(vec![reffile(sheet("S"), "Days", "=A2:A4")]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(t.rewrite_tsv("=SUM(Days)", "S"), "=SUM(S!A2:A4)");
    }

    #[test]
    fn a_symlink_single_cell_and_range() {
        let (t, d) = NameTable::build(vec![
            symlink(sheet("S"), "total", "S", "B5"),
            symlink(sheet("S"), "block.begin", "S", "B2"),
            symlink(sheet("S"), "block.end", "S", "B4"),
        ]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(
            t.rewrite_tsv("=total+SUM(block)", "S"),
            "=S!B5+SUM(S!B2:B4)"
        );
    }

    #[test]
    fn a_named_formula_and_constant_expand_with_parens() {
        let (t, d) = NameTable::build(vec![
            reffile(sheet("S"), "Base", "100"),
            reffile(sheet("S"), "Rate", "=Base*1.05"),
            reffile(sheet("S"), "Pi", "3.14"),
        ]);
        assert!(d.is_empty(), "{d:?}");
        // `Base` inside `Rate` expands too.
        assert_eq!(t.rewrite_tsv("=Rate", "S"), "=((100)*1.05)");
        assert_eq!(t.rewrite_tsv("=Pi*2", "S"), "=(3.14)*2");
    }

    #[test]
    fn sheet_scope_shadows_workbook_scope() {
        let (t, d) = NameTable::build(vec![
            RawNameEntry {
                scope: NameScope::Workbook,
                entry_name: "Rate".to_string(),
                form: NameRepr::RefFile {
                    content: "1".to_string(),
                },
            },
            reffile(sheet("S"), "Rate", "2"),
        ]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(t.rewrite_tsv("=Rate", "S"), "=(2)"); // sheet-scoped wins on S
        assert_eq!(t.rewrite_tsv("=Rate", "Other"), "=(1)"); // workbook-scoped elsewhere
    }

    #[test]
    fn one_identifier_defined_on_two_sheets_is_two_names_not_a_duplicate() {
        let (t, d) = NameTable::build(vec![
            reffile(sheet("S"), "Rate", "1"),
            reffile(sheet("T"), "Rate", "2"),
            symlink(sheet("S"), "Block.begin", "S", "A1"),
            symlink(sheet("S"), "Block.end", "S", "A3"),
            symlink(sheet("T"), "Block.begin", "T", "B1"),
            symlink(sheet("T"), "Block.end", "T", "B9"),
        ]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(t.names().len(), 4);
        assert_eq!(t.rewrite_tsv("=Rate", "S"), "=(1)");
        assert_eq!(t.rewrite_tsv("=Rate", "T"), "=(2)");
        assert_eq!(t.rewrite_tsv("=SUM(Block)", "S"), "=SUM(S!A1:A3)");
        assert_eq!(t.rewrite_tsv("=SUM(Block)", "T"), "=SUM(T!B1:B9)");
        assert_eq!(t.rewrite_tsv("=Rate", "Other"), "=Rate"); // not visible from a third sheet
    }

    #[test]
    fn overlapping_names_over_the_same_cells_are_allowed() {
        let (t, d) = NameTable::build(vec![
            reffile(sheet("S"), "First", "=A1:A3"),
            reffile(sheet("S"), "Same", "=A1:A3"),
        ]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(
            t.rewrite_tsv("=SUM(First)+SUM(Same)", "S"),
            "=SUM(S!A1:A3)+SUM(S!A1:A3)"
        );
    }

    #[test]
    fn an_ident_that_parses_as_a1_is_refused() {
        let (_, d) = NameTable::build(vec![symlink(sheet("S"), "A5", "S", "B5")]);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].code, Code::NameRefusal);
    }

    #[test]
    fn a_name_defined_twice_in_the_same_scope_is_refused() {
        let (t, d) = NameTable::build(vec![
            reffile(sheet("S"), "Rate", "1"),
            reffile(sheet("S"), "Rate", "2"),
        ]);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].code, Code::NameRefusal);
        assert_eq!(t.rewrite_tsv("=Rate", "S"), "=(1)"); // the first survives, never overwritten
        // The same identifier in a DIFFERENT scope is not a duplicate.
        let (t2, d2) = NameTable::build(vec![
            RawNameEntry {
                scope: NameScope::Workbook,
                entry_name: "Rate".to_string(),
                form: NameRepr::RefFile {
                    content: "1".to_string(),
                },
            },
            reffile(sheet("S"), "Rate", "2"),
        ]);
        assert!(d2.is_empty(), "{d2:?}");
        assert_eq!(t2.rewrite_tsv("=Rate", "S"), "=(2)");
    }

    #[test]
    fn a_lone_or_inverted_corner_is_refused() {
        let (_, lone) = NameTable::build(vec![symlink(sheet("S"), "r.begin", "S", "B2")]);
        assert_eq!(lone.len(), 1);
        assert_eq!(lone[0].code, Code::NameRefusal);

        let (_, inv) = NameTable::build(vec![
            symlink(sheet("S"), "r.begin", "S", "B4"),
            symlink(sheet("S"), "r.end", "S", "B2"),
        ]);
        assert_eq!(inv.len(), 1);
        assert_eq!(inv[0].code, Code::NameRefusal);
    }

    /// A bare `B5` body is what a same-sheet symlink degrades to through a zip.
    #[test]
    fn a_degraded_symlink_reffile_reads_as_a_ref() {
        let (t, d) = NameTable::build(vec![reffile(sheet("S"), "total", "B5")]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(t.rewrite_tsv("=total", "S"), "=S!B5");
    }

    #[test]
    fn a_degraded_workbook_scoped_symlink_relative_path_reads_as_a_ref() {
        let (t, d) = NameTable::build(vec![RawNameEntry {
            scope: NameScope::Workbook,
            entry_name: "TaxRate".to_string(),
            form: NameRepr::RefFile {
                content: "Data/H1".to_string(),
            },
        }]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(t.rewrite_tsv("=TaxRate*100", "S"), "=Data!H1*100");
    }

    #[test]
    fn a_degraded_cross_sheet_symlink_relative_path_reads_as_a_ref() {
        // Both spellings of the same target: the degraded path, and what the ref-file writer emits.
        let (t, d) = NameTable::build(vec![
            reffile(sheet("S"), "Cross", "../Data/H1"),
            reffile(sheet("S"), "CrossEq", "=Data!H1"),
        ]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(t.rewrite_tsv("=Cross+CrossEq", "S"), "=Data!H1+Data!H1");
    }

    #[test]
    fn a_degraded_range_corner_relative_path_reads_as_a_range() {
        let (t, d) = NameTable::build(vec![
            RawNameEntry {
                scope: NameScope::Workbook,
                entry_name: "AllQOne.begin".to_string(),
                form: NameRepr::RefFile {
                    content: "Data/B2".to_string(),
                },
            },
            RawNameEntry {
                scope: NameScope::Workbook,
                entry_name: "AllQOne.end".to_string(),
                form: NameRepr::RefFile {
                    content: "Data/B4".to_string(),
                },
            },
        ]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(t.rewrite_tsv("=SUM(AllQOne)", "S"), "=SUM(Data!B2:B4)");
    }

    #[test]
    fn a_division_named_formula_is_not_mistaken_for_a_relative_path() {
        let (t, d) = NameTable::build(vec![reffile(sheet("S"), "Ratio", "=A1/B1")]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(t.rewrite_tsv("=Ratio", "S"), "=(A1/B1)");
    }

    #[test]
    fn name_resolution_is_case_sensitive() {
        let (t, d) = NameTable::build(vec![reffile(sheet("S"), "Rate", "1")]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(t.rewrite_tsv("=Rate", "S"), "=(1)");
        assert_eq!(t.rewrite_tsv("=rate", "S"), "=rate"); // unresolved -> a located #NAME?
    }

    #[test]
    fn an_unresolvable_name_is_left_verbatim() {
        let (t, _) = NameTable::build(vec![reffile(sheet("S"), "Known", "=A1:A2")]);
        assert_eq!(t.rewrite_tsv("=Unknown+1", "S"), "=Unknown+1");
    }

    #[test]
    fn corner_aliases_are_accepted() {
        let (t, d) = NameTable::build(vec![
            symlink(sheet("S"), "r.topleft", "S", "B2"),
            symlink(sheet("S"), "r.br", "S", "C4"),
        ]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(t.rewrite_tsv("=SUM(r)", "S"), "=SUM(S!B2:C4)");
    }

    #[test]
    fn a_target_sheet_with_a_space_is_quoted_in_every_representation() {
        // One case per site that qualifies a ref; all four must quote.
        let (t, d) = NameTable::build(vec![
            symlink(sheet("My Data"), "total", "My Data", "B5"), // bare_name symlink
            symlink(sheet("My Data"), "blk.begin", "My Data", "B2"), // CornerAcc::finish
            symlink(sheet("My Data"), "blk.end", "My Data", "B4"),
            RawNameEntry {
                scope: NameScope::Workbook,
                entry_name: "cross".to_string(),
                form: NameRepr::RefFile {
                    content: "My Data/B5".to_string(), // degraded-path re-qualify
                },
            },
            reffile(sheet("My Data"), "local", "B7"), // scope-sheet-qualified bare ref
        ]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(t.rewrite_tsv("=total*2", "My Data"), "='My Data'!B5*2");
        assert_eq!(
            t.rewrite_tsv("=SUM(blk)", "My Data"),
            "=SUM('My Data'!B2:B4)"
        );
        assert_eq!(t.rewrite_tsv("=cross*2", "S"), "='My Data'!B5*2");
        assert_eq!(t.rewrite_tsv("=local+1", "My Data"), "='My Data'!B7+1");
    }

    #[test]
    fn a_sheet_name_with_an_apostrophe_doubles_the_quote() {
        let (t, d) = NameTable::build(vec![symlink(
            sheet("It's Data"),
            "total",
            "It's Data",
            "B5",
        )]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(t.rewrite_tsv("=total", "It's Data"), "='It''s Data'!B5");
    }

    #[test]
    fn a_bare_identifier_sheet_is_not_quoted() {
        assert_eq!(quote_sheet("Sheet1"), "Sheet1");
        assert_eq!(quote_sheet("Data"), "Data");
        assert_eq!(quote_sheet("My Data"), "'My Data'");
        assert_eq!(quote_sheet("2024"), "'2024'"); // leading digit is not a bare word to the lexer
    }

    #[test]
    fn a_name_inside_a_string_literal_is_not_substituted() {
        let (t, d) = NameTable::build(vec![reffile(sheet("S"), "Rate", "1")]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(
            t.rewrite_tsv(r#"=IF(A1,"Rate",Rate)"#, "S"),
            r#"=IF(A1,"Rate",(1))"#
        );
    }

    #[test]
    fn a_sheet_scoped_single_slash_body_is_a_located_refusal_not_a_wrong_ref() {
        let (t, d) = NameTable::build(vec![reffile(sheet("S"), "Bad", "Data/H1")]);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].code, Code::NameRefusal);
        assert_eq!(t.rewrite_tsv("=Bad", "S"), "=Bad"); // never the silently-wrong `=Data!H1`
        assert!(t.names().iter().all(|n| n.ident != "Bad"));
        // The very same body under a WORKBOOK scope is the legit degrade: the split is scope-driven.
        let (wt, wd) = NameTable::build(vec![RawNameEntry {
            scope: NameScope::Workbook,
            entry_name: "Ok".to_string(),
            form: NameRepr::RefFile {
                content: "Data/H1".to_string(),
            },
        }]);
        assert!(wd.is_empty(), "{wd:?}");
        assert_eq!(wt.rewrite_tsv("=Ok", "S"), "=Data!H1");
    }

    #[test]
    fn a_quoted_sheet_ref_is_a_pure_ref_but_an_expression_ending_in_an_address_is_not() {
        let (t, d) = NameTable::build(vec![reffile(sheet("S"), "Q", "='Sheet 2'!A1")]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(t.rewrite_tsv("=Q*2", "S"), "='Sheet 2'!A1*2"); // ref: injected unparenthesized
        // Misclassifying this as a ref would inject it unparenthesized and change the precedence.
        let (t2, d2) = NameTable::build(vec![reffile(sheet("S"), "E", "=1/Sheet!A1")]);
        assert!(d2.is_empty(), "{d2:?}");
        assert_eq!(t2.rewrite_tsv("=E*2", "S"), "=(1/Sheet!A1)*2");
        let (t3, d3) = NameTable::build(vec![reffile(sheet("S"), "R", "=Sheet1!B2:B4")]);
        assert!(d3.is_empty(), "{d3:?}");
        assert_eq!(t3.rewrite_tsv("=SUM(R)", "S"), "=SUM(Sheet1!B2:B4)");
    }

    #[test]
    fn a_degraded_corner_with_an_ambiguous_path_is_refused() {
        // Both corners refuse: each is independently an ambiguous target.
        let (t, d) = NameTable::build(vec![
            reffile(sheet("S"), "r.begin", "Data/B2"),
            reffile(sheet("S"), "r.end", "Data/B4"),
        ]);
        assert!(
            d.iter().all(|x| x.code == Code::NameRefusal) && !d.is_empty(),
            "{d:?}"
        );
        assert!(t.names().iter().all(|n| n.ident != "r"));
    }

    #[test]
    fn an_eq_prefixed_path_body_is_an_expr_never_a_degraded_ref() {
        let (t, d) = NameTable::build(vec![RawNameEntry {
            scope: NameScope::Workbook,
            entry_name: "Div".to_string(),
            form: NameRepr::RefFile {
                content: "=Revenue/A1".to_string(),
            },
        }]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(t.rewrite_tsv("=Div", "S"), "=(Revenue/A1)"); // not `=Revenue!A1`
        assert!(!t.rewrite_tsv("=Div", "S").contains('!'));

        // The very same body BARE at a workbook scope IS the degraded path: the split is `=`-driven.
        let (bt, bd) = NameTable::build(vec![RawNameEntry {
            scope: NameScope::Workbook,
            entry_name: "Path".to_string(),
            form: NameRepr::RefFile {
                content: "Revenue/A1".to_string(),
            },
        }]);
        assert!(bd.is_empty(), "{bd:?}");
        assert_eq!(bt.rewrite_tsv("=Path", "S"), "=Revenue!A1");

        // The `=` gate leaves the scope-driven ambiguity refusal untouched.
        let (st, sd) = NameTable::build(vec![reffile(sheet("S"), "Amb", "Data/H1")]);
        assert_eq!(sd.len(), 1, "{sd:?}");
        assert_eq!(sd[0].code, Code::NameRefusal);
        assert!(st.names().iter().all(|n| n.ident != "Amb"));
    }

    #[test]
    fn an_eq_prefixed_corner_body_is_refused_never_a_materialized_cross_sheet_range() {
        let (t, d) = NameTable::build(vec![
            RawNameEntry {
                scope: NameScope::Workbook,
                entry_name: "r.begin".to_string(),
                form: NameRepr::RefFile {
                    content: "=Revenue/A1".to_string(),
                },
            },
            RawNameEntry {
                scope: NameScope::Workbook,
                entry_name: "r.end".to_string(),
                form: NameRepr::RefFile {
                    content: "=Revenue/A9".to_string(),
                },
            },
        ]);
        assert!(
            !d.is_empty() && d.iter().all(|x| x.code == Code::NameRefusal),
            "{d:?}"
        );
        assert!(t.names().iter().all(|n| n.ident != "r"));
        assert_eq!(t.rewrite_tsv("=SUM(r)", "S"), "=SUM(r)"); // never `Revenue!A1:A9`
        assert!(!t.rewrite_tsv("=SUM(r)", "S").contains('!'));

        // Legit `=`-prefixed corners still resolve, through the NotAPath classification.
        let (xt, xd) = NameTable::build(vec![
            reffile(sheet("S"), "x.begin", "=Data!H1"),
            reffile(sheet("S"), "x.end", "=Data!H9"),
            reffile(sheet("S"), "y.begin", "=A1"),
            reffile(sheet("S"), "y.end", "=A9"),
        ]);
        assert!(xd.is_empty(), "{xd:?}");
        assert_eq!(xt.rewrite_tsv("=SUM(x)", "S"), "=SUM(Data!H1:H9)");
        assert_eq!(xt.rewrite_tsv("=SUM(y)", "S"), "=SUM(S!A1:A9)");

        // And the `=` gate leaves a BARE degraded-path corner pair untouched.
        let (bt, bd) = NameTable::build(vec![
            reffile(sheet("S"), "z.begin", "../Data/H1"),
            reffile(sheet("S"), "z.end", "../Data/H9"),
        ]);
        assert!(bd.is_empty(), "{bd:?}");
        assert_eq!(bt.rewrite_tsv("=SUM(z)", "S"), "=SUM(Data!H1:H9)");
    }

    #[test]
    fn a_malformed_single_cell_symlink_target_is_a_located_refusal() {
        let (t, d) = NameTable::build(vec![symlink(sheet("S"), "bad", "S", "B")]); // `B` has no row
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].code, Code::NameRefusal);
        assert!(t.names().iter().all(|n| n.ident != "bad"));
        assert_eq!(t.rewrite_tsv("=bad", "S"), "=bad");
    }

    #[test]
    fn a_blank_corner_is_a_located_refusal_never_a_panic() {
        let (t, d) = NameTable::build(vec![
            reffile(sheet("S"), "r.begin", ""),
            reffile(sheet("S"), "r.end", "   "),
        ]);
        assert!(
            !d.is_empty() && d.iter().all(|x| x.code == Code::NameRefusal),
            "{d:?}"
        );
        assert!(t.names().iter().all(|n| n.ident != "r"));
    }

    #[test]
    fn the_range_form_is_a_canonical_in_grid_range_and_nothing_looser() {
        for stem in ["D2:K17", "Q4", "A1-B2", "XFD1048576"] {
            assert!(stem_region(stem).is_some(), "{stem} is the range form");
        }
        for stem in [
            "Chart1",
            "sales1",
            "notes2024",
            "A:A",
            "XFE1",
            "A1048577",
            "a1",
            "A01",
            "A1:A1",
            "G8:A3",
            "Units",
        ] {
            assert!(stem_region(stem).is_none(), "{stem} is the name form");
        }
    }

    #[test]
    fn occupancy_is_the_range_form_under_its_suffix_and_nothing_else() {
        assert_eq!(
            figure_occupancy("D2:K17.json"),
            stem_region("D2:K17"),
            "a range-form figure occupies what its name states"
        );
        for name in ["Chart1.json", ".json", "D2:K17", "D2:K17.css"] {
            assert!(figure_occupancy(name).is_none(), "{name} occupies nothing");
        }
    }
}
