// Concern: the FS4 NAME abstraction + the representation SEAM (`NameRepr`) — the engine-facing `Name`/`NameTarget`/`NameScope` and the `NameTable` that resolves an identifier (sheet-scoped shadowing a workbook-scoped one) to a cell/range A1 REF or a named-formula/constant EXPR, plus the READER-UNION that BUILDS the table from raw on-disk entries in EITHER representation (a POSIX symlink — bare `<ident>` cell or `<ident>.begin`/`<ident>.end` corners, accepting corner aliases; OR a regular non-A1-named ref-file holding `=ref`/`=expr`/a bare A1 target that catches a degraded symlink) and the LOAD-TIME source rewrite that substitutes each name token in a `=formula` with its resolved A1/expr so the engine only ever sees A1 (ENG1) — a name whose identifier parses as an A1 address, a lone/inverted corner, a scope+ident conflict, or an AMBIGUOUS single-slash `Sheet/Cell` target (never a silently-wrong cross-sheet ref) is a located refusal (CORE2), and an unresolvable name token is left verbatim so it loads as a located `#NAME?` (VAL3/GRID6) | Non-concern: WALKING the filesystem for entries / reading a symlink's target path (workbook.rs `load_dir` owns the fs IO and hands this pre-resolved `(sheet, cell)` targets), splitting a TSV file into fields (grid.rs), and whether the rewritten A1/expr parses/evaluates (charlie-ast) | IO: (raw name entries) -> a `NameTable` + located `Diagnostic`s; (a formula source `&str` + a home sheet) -> the name-resolved source
//! FS4 names: [`Name`], [`NameTarget`], [`NameScope`], [`NameTable`]. A name is normalized to the same
//! table whatever its on-disk representation (GRID3), so the engine sees only the resolved A1/expr.
//!
//! The representation SEAM is the reader-union: [`NameTable::build`] accepts [`RawNameEntry`]s already
//! tagged by form ([`NameRepr::Symlink`] / [`NameRepr::RefFile`]) — the caller (`load_dir`) owns the
//! filesystem detection (symlink vs regular file, and reading a symlink's target). A future
//! all-ref-file writer (Windows) is a drop-in: the reader already understands the ref-file form,
//! including a ref-file that holds a RELATIVE symlink target (`Data/H1`, `../Data/H1`) — the shape a
//! symlink-flattening container (zip) degrades a static-range symlink to.

use crate::diagnostic::{Code, Diagnostic, Loc};
use charlie_ast::a1::parse_a1;

/// The resolved target of a name (GRID3 normalized): a static A1 reference, or a named-formula/constant
/// EXPRESSION. Both are charlie-A1 source fragments the load-time rewrite substitutes into a formula.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameTarget {
    /// A static cell or range, as charlie-A1 text (`B5`, `Sheet1!B5`, `Sheet1!B2:B4`). Substituted
    /// verbatim (it is a self-contained reference token).
    Ref(String),
    /// A named formula / constant — an expression source (`Base*1.05`, `3.14`). Substituted wrapped in
    /// parentheses so it keeps its precedence inside the referencing formula.
    Expr(String),
}

/// A name's scope, by folder placement (FS4): the workbook root (workbook-scoped) or a tab folder
/// (sheet-scoped). A sheet-scoped name shadows a workbook-scoped one of the same identifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameScope {
    Workbook,
    Sheet(String),
}

/// One resolved name: its identifier, scope, and target. The engine-facing abstraction — no on-disk
/// representation leaks past [`NameTable::build`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    pub ident: String,
    pub scope: NameScope,
    pub target: NameTarget,
}

/// The on-disk representation of a name — the SEAM (HARD RULE 5) the reader-union spans. `load_dir`
/// produces one per name entry it finds; the pure builder normalizes both to the same [`Name`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameRepr {
    /// A POSIX symlink, already resolved by the caller to its target `(sheet, cell-A1)`.
    Symlink {
        target_sheet: String,
        target_cell: String,
    },
    /// A regular file whose content is the target ref/expr (or a degraded symlink: a bare A1 target,
    /// or a relative path `Data/H1` / `../Data/H1` a symlink-flattening container collapsed it to).
    RefFile { content: String },
}

/// A raw name entry as the fs reader found it, before normalization: its scope, its on-disk entry name
/// (which may carry a `.begin`/`.end` corner suffix), and its representation form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawNameEntry {
    pub scope: NameScope,
    /// The on-disk entry name (`total`, `Days.begin`, `Rate`).
    pub entry_name: String,
    pub form: NameRepr,
}

/// The workbook's resolved name table: every name, plus the resolution rule (sheet shadows workbook).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NameTable {
    names: Vec<Name>,
}

/// Which corner of a range a `.suffix` denotes. The reader accepts several self-evident aliases
/// (Postel); only `begin`/`end` are documented and written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Corner {
    Begin,
    End,
}

/// Recognize a corner suffix (`begin`/`start`/`tl`/`topleft` = begin; `end`/`br`/`bottomright` = end),
/// case-insensitively. Unknown suffix -> `None` (the whole entry name is the identifier).
fn corner_alias(suffix: &str) -> Option<Corner> {
    match suffix.to_ascii_lowercase().as_str() {
        "begin" | "start" | "tl" | "topleft" => Some(Corner::Begin),
        "end" | "br" | "bottomright" => Some(Corner::End),
        _ => None,
    }
}

/// Split an entry name into `(identifier, optional corner)`. `Days.begin` -> (`Days`, Begin); `Rate` ->
/// (`Rate`, None); `my.name` (unknown suffix) -> (`my.name`, None).
fn split_corner(entry_name: &str) -> (String, Option<Corner>) {
    if let Some((ident, suffix)) = entry_name.rsplit_once('.')
        && let Some(c) = corner_alias(suffix)
    {
        return (ident.to_string(), Some(c));
    }
    (entry_name.to_string(), None)
}

/// Whether `ident` parses as an A1 address (lenient — `a1`/`$A$1` count). A name whose identifier is an
/// A1 address is a located refusal (FS4: a name never collides with a cell's filename).
fn ident_is_a1(ident: &str) -> bool {
    parse_a1(ident).is_ok()
}

/// Whether a regular file's name denotes a CELL/GRID file (FS2) rather than a NAME entry — the routing
/// predicate both loader paths share. A name that carries a `:` is an A1 range ATTEMPT (routed to the
/// filename parser, which may still reject a non-canonical spelling); a colon-free name is a cell iff it
/// parses (leniently) as a single A1 address. Everything else (`Days`, `Rate`, `Days.begin`) is a name.
pub fn is_cell_filename(name: &str) -> bool {
    if name.contains(':') {
        return true;
    }
    parse_a1(name).is_ok()
}

/// Whether a text is a pure A1 cell/range reference (a single cell or `cell:cell`, optionally
/// `$`-anchored / `Sheet!`-qualified). Used to classify a ref-file's content as a [`NameTarget::Ref`]
/// vs an [`NameTarget::Expr`]. Validates the WHOLE ref, not just the address after the last `!`: the
/// `Sheet!` qualifier (when present) must itself be a well-formed sheet token — a bare identifier or a
/// `'…'`-quoted string — so an EXPRESSION that merely ends in a valid address (`1/Sheet!A1`) is never
/// misclassified as a ref, and a quoted-sheet ref (`'Sheet 2'!A1`) is correctly recognized as one. A
/// formula/constant/union address carries one of `,(){}`/space/`#`/`'` — a plain address never does —
/// so those are rejected here (they are an expression, not a ref).
fn is_pure_ref(text: &str) -> bool {
    let (sheet_part, addr) = match text.rsplit_once('!') {
        Some((s, a)) => (Some(s), a),
        None => (None, text),
    };
    // The sheet qualifier, when present, must be a real sheet token — not an arbitrary expression
    // prefix (`1/Sheet`) that happens to sit before a `!`.
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

/// Whether `s` is a well-formed sheet token as it appears before `!` in a ref: a BARE identifier (an
/// ASCII letter then ASCII alphanumerics — the same set [`quote_sheet`] leaves unquoted) or a `'…'`
/// -QUOTED string whose every interior `'` is doubled. Anything else (an unquoted space, a leading
/// digit, an expression prefix, a lone/odd quote) is not a sheet token, so a ref carrying it is an
/// expression, not a pure ref.
fn is_sheet_token(s: &str) -> bool {
    if let Some(inner) = s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        // A quoted sheet: once `''` pairs are removed no lone `'` may remain.
        s.len() >= 2 && !inner.replace("''", "").contains('\'')
    } else {
        let mut cs = s.chars();
        cs.next().is_some_and(|c| c.is_ascii_alphabetic())
            && s.chars().all(|c| c.is_ascii_alphanumeric())
    }
}

/// Strip a `'…'` sheet quoting back to the raw sheet name (undoubling interior `''`), else the name
/// as-is. The inverse of [`quote_sheet`], so a resolved ref can be re-qualified with canonical quoting.
fn unquote_sheet(s: &str) -> String {
    if let Some(inner) = s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        inner.replace("''", "'")
    } else {
        s.to_string()
    }
}

/// A ref-file body: its content trimmed, with any single leading `=` stripped (a degraded symlink has
/// none; the non-POSIX ref-file writer and a hand-written ref carry one).
fn strip_eq(content: &str) -> &str {
    let t = content.trim();
    t.strip_prefix('=').unwrap_or(t).trim()
}

/// The classification of a ref-file body as a possible DEGRADED symlink path (see
/// [`degraded_path_target`]).
enum DegradedPath {
    /// A resolved cross-scope symlink path -> (target sheet, target cell).
    Resolved(String, String),
    /// Not a path form at all — no `/`, a division `A1/B1` (both sides A1), or a `Sheet!Cell` ref.
    /// The caller classifies it by the other rules (a pure ref, or an expression).
    NotAPath,
    /// A single-slash `Sheet/Cell` that is NOT a valid symlink path in this scope yet is not a clean
    /// division either — an AMBIGUOUS target the caller turns into a located refusal (HARD RULE 5),
    /// never a silently-wrong cross-sheet ref.
    Ambiguous,
}

/// Interpret a ref-file body as a DEGRADED symlink whose target crossed a scope boundary — the relative
/// filesystem path the ingest writer emits, that a symlink-flattening container (zip) collapses to file
/// content. SCOPE-AWARE, because the accepted path shape depends on the name's scope:
///
/// * `../Sheet/Cell` — a SHEET-scoped name pointing at another tab (up out of its own folder). Always
///   unambiguous (no formula begins with `../`), and legitimate only from a sheet scope.
/// * `Sheet/Cell` (no `../`) — the relative target of a WORKBOOK-scoped name (the root down into a tab).
///   A single-slash body is ambiguous with an Excel division (`A1/B1`, `Name/A1`): a body whose LEFT is
///   an A1 cell is a division ([`DegradedPath::NotAPath`], never a path); for a WORKBOOK scope a non-A1
///   left is the legit degrade ([`DegradedPath::Resolved`]); for a SHEET scope a bare `Sheet/Cell` (a
///   cross-sheet target that lost its `../`) is [`DegradedPath::Ambiguous`] and must be refused, never
///   silently read as a cross-sheet ref.
///
/// A symlink always targets a single cell (RIGHT component one A1 cell, exactly one folder level). A
/// body carrying an `!` is a sheet-qualified ref, never a path.
fn degraded_path_target(body: &str, scope: &NameScope) -> DegradedPath {
    if body.contains('!') {
        return DegradedPath::NotAPath; // a `Sheet!Cell` ref — classified by `is_pure_ref` instead
    }
    let is_up = body.starts_with("../");
    let rel = body.strip_prefix("../").unwrap_or(body);
    let Some((sheet, cell)) = rel.rsplit_once('/') else {
        return DegradedPath::NotAPath; // no `/`: a bare cell / ref / expr, handled by the caller
    };
    // Exactly one folder level (a tab under the root) with a single A1 coordinate on the right.
    let well_formed =
        !sheet.is_empty() && !sheet.contains('/') && !cell.contains(':') && parse_a1(cell).is_ok();
    if is_up {
        // `../…` is unambiguously a path; only a sheet-scoped name legitimately climbs out of its folder.
        return match scope {
            NameScope::Sheet(_) if well_formed => {
                DegradedPath::Resolved(sheet.to_string(), cell.to_string())
            }
            _ => DegradedPath::Ambiguous,
        };
    }
    if parse_a1(sheet).is_ok() {
        // `A1/B1`: both sides A1 -> a division EXPRESSION, never a path.
        return DegradedPath::NotAPath;
    }
    // `Word/Cell` (non-A1 left): the legit degrade of a WORKBOOK-scoped name; ambiguous under a sheet
    // scope (a sheet-scoped cross-sheet target always carries `../`).
    match scope {
        NameScope::Workbook if well_formed => {
            DegradedPath::Resolved(sheet.to_string(), cell.to_string())
        }
        _ => DegradedPath::Ambiguous,
    }
}

impl NameTable {
    /// An empty table — every lookup misses (in-memory workbooks with no name entries, and the default
    /// no-name rewrite context).
    pub fn empty() -> NameTable {
        NameTable::default()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Build the table from raw entries (the reader-union), collecting located refusals. A name whose
    /// identifier parses as A1, a lone/inverted corner, cross-sheet corners, or a duplicate ident+scope
    /// is a located [`Code::NameRefusal`] (CORE2) — never a silent drop and never a silently-wrong name.
    pub fn build(entries: Vec<RawNameEntry>) -> (NameTable, Vec<Diagnostic>) {
        let mut diags = Vec::new();
        // Group corner entries by (scope, ident) so a `.begin`/`.end` pair becomes one range name.
        let mut table = NameTable::default();
        // Pending range corners: keyed by (scope, ident) -> (begin?, end?).
        let mut corners: Vec<CornerAcc> = Vec::new();

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
        // Finalize the range corners: each needs both a begin and an end, on the same sheet, canonical.
        for acc in corners {
            if let Some(name) = acc.finish(&mut diags) {
                insert_name(&mut table, name, &mut diags);
            }
        }
        (table, diags)
    }

    /// Resolve `ident` for a formula on `sheet`: a sheet-scoped name shadows a workbook-scoped one.
    ///
    /// Matching is CASE-SENSITIVE by deliberate design (not Excel's case-insensitive name lookup): a
    /// charlie name IS a filesystem entry (FS4), and on a case-sensitive filesystem `TaxRate` and
    /// `taxrate` are two distinct entries — two distinct names — so a case-folding lookup would be
    /// ambiguous between legitimately-different files. This does not narrow xlsx import parity in
    /// practice: Excel normalizes a defined name's casing on entry, so an imported workbook's formula
    /// tokens already match the emitted name file's spelling. A token that differs only in case
    /// therefore resolves to a located `#NAME?` (VAL3) rather than the value — a conscious keep.
    fn resolve(&self, ident: &str, sheet: &str) -> Option<&NameTarget> {
        self.names
            .iter()
            .find(|n| n.ident == ident && n.scope == NameScope::Sheet(sheet.to_string()))
            .or_else(|| {
                self.names
                    .iter()
                    .find(|n| n.ident == ident && n.scope == NameScope::Workbook)
            })
            .map(|n| &n.target)
    }

    /// Rewrite the name tokens in a whole TSV file's content, formula field by formula field, resolving
    /// each against `sheet` (the file's tab). Only `=`-prefixed fields are touched; a literal field that
    /// happens to spell a name is untouched. Field boundaries are the raw split (unescaped tab/newline)
    /// the deserializer uses, and a name/A1 substitution contains no escape character, so the rewrite is
    /// lossless. An empty table is a cheap identity.
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

    /// Rewrite one `=formula` field's name tokens to their resolved A1/expr. Cycle-safe (a named-formula
    /// self-reference stops expanding and stays verbatim -> a located `#NAME?`).
    fn rewrite_formula(&self, field: &str, sheet: &str) -> String {
        let body = &field[1..]; // past the leading `=`
        let mut visiting = Vec::new();
        format!("={}", self.rewrite_body(body, sheet, &mut visiting))
    }

    /// Walk a formula body, substituting each bare name identifier with its target. String literals and
    /// `'quoted sheet'` names are copied atomically; an identifier that is a function call (`(` follows),
    /// a sheet qualifier (`!` follows), or the tail after a `!` is never a name. `visiting` guards
    /// against a named-formula expansion cycle.
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
                        // A sheet qualifier LHS, a function call, or the tail of a `Sheet!` ref: never a
                        // defined name — push verbatim (the `!`/`(` flows on through the loop).
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

    /// Substitute one identifier: its resolved A1 (a `Ref`) or parenthesized expr (an `Expr`, resolved
    /// recursively), else the identifier verbatim (an unresolvable name -> a located `#NAME?` at parse).
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

    /// The full name list — for tests and the writer-side round-trip.
    pub fn names(&self) -> &[Name] {
        &self.names
    }
}

/// An accumulator for a range name's two corner symlinks, keyed by (scope, ident).
struct CornerAcc {
    scope: NameScope,
    ident: String,
    begin: Option<(String, String)>, // (sheet, cell) of the begin corner
    end: Option<(String, String)>,
}

impl CornerAcc {
    /// Finalize into a range [`Name`], or push a located refusal for a lone/inverted/cross-sheet corner.
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

/// Fold one corner entry into the matching accumulator (creating it if new), refusing a ref-file corner
/// whose content is not a bare cell.
fn acc_corner(
    corners: &mut Vec<CornerAcc>,
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
    let acc = match corners
        .iter_mut()
        .find(|a| a.ident == ident && a.scope == scope)
    {
        Some(a) => a,
        None => {
            corners.push(CornerAcc {
                scope: scope.clone(),
                ident: ident.clone(),
                begin: None,
                end: None,
            });
            corners.last_mut().expect("just pushed")
        }
    };
    match corner {
        Corner::Begin => acc.begin = Some(target),
        Corner::End => acc.end = Some(target),
    }
}

/// The `(sheet, cell)` a corner entry points at, or `None` when the ref-file content does not resolve
/// to a single cell (an ambiguous single-slash path, a blank/malformed body, or a range) — which
/// [`acc_corner`] turns into a located refusal. A symlink carries it directly; a ref-file corner
/// (degraded symlink) holds a scope-aware relative symlink path (`Data/H1` workbook, `../Data/H1`
/// cross-sheet), an explicit `=Sheet!Cell`/`Sheet!Cell` ref, or a bare `=A1`/`A1` cell resolved
/// against its scope sheet. The leading `=` disambiguates FORM exactly as in [`classify_ref_file`]: a
/// `=`-prefixed body is a formula, never a degraded path, so it never reaches [`degraded_path_target`]
/// (a `=Revenue/A1` corner is refused, NEVER a silently-materialized cross-sheet range — HARD RULE 5).
fn corner_target(scope: &NameScope, form: &NameRepr) -> Option<(String, String)> {
    match form {
        NameRepr::Symlink {
            target_sheet,
            target_cell,
        } => Some((target_sheet.clone(), target_cell.clone())),
        NameRepr::RefFile { content } => {
            let has_eq = content.trim_start().starts_with('=');
            let addr = strip_eq(content);
            // The leading `=` disambiguates FORM exactly as in [`classify_ref_file`]: a degraded symlink
            // corner is a BARE relative path (`Data/H1` / `../Data/H1`), never one carrying `=`, so
            // [`degraded_path_target`] is consulted ONLY for a non-`=` body. A `=`-prefixed corner body
            // (`=Revenue/A1`, `=../Sheet/Cell`) is a formula, NEVER a path — forcing it down the
            // NotAPath branch keeps a `=Revenue/A1` corner from silently materializing a cross-sheet
            // `Revenue!A1` range (HARD RULE 5). Only a legit `=Data!H1` / `=A1` corner resolves there.
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
                        // An explicit `Sheet!Cell` corner: validate the WHOLE ref (sheet token + cell).
                        (is_sheet_token(sheet_part) && !a.contains(':') && parse_a1(a).is_ok())
                            .then(|| (unquote_sheet(sheet_part), a.to_string()))
                    } else if !addr.contains(':') && parse_a1(addr).is_ok() {
                        // A degraded same-sheet symlink: a bare A1 cell against the corner's scope sheet.
                        Some((scope_sheet(scope)?, addr.to_string()))
                    } else {
                        None
                    }
                }
            }
        }
    }
}

/// Build a bare (single-cell / ref-file) name, or push a located refusal and return `None` for a
/// ref-file whose content is an AMBIGUOUS target (a single-slash `Sheet/Cell` that is neither a valid
/// symlink path in this scope nor a division/ref/expr — HARD RULE 5, never a silently-wrong ref). A
/// symlink is always a direct ref; a ref-file's content otherwise classifies into a `Ref`/`Expr` (an
/// unresolvable-looking body becomes an `Expr` that loads as a located `#NAME?`, never a build refusal).
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
            // Validate the target cell at BUILD (like `CornerAcc::finish` does for a range corner): a
            // malformed single-cell symlink target is a LOCATED refusal now, never a silently-wrong ref
            // qualified into a deferred parse error at eval (HARD RULE 3, CORE2).
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

/// Classify a ref-file's content into a target, or `None` when it is an AMBIGUOUS target that must
/// become a located refusal (never a silently-wrong ref/expr).
///
/// The leading `=` disambiguates FORM at ANY scope: a POSIX symlink degrades to a BARE relative path
/// (`Data/H1` workbook-scoped, `../Data/H1` cross-sheet) — never one carrying a `=` — so
/// [`degraded_path_target`] is consulted ONLY for a non-`=` body. A `=`-prefixed body is a
/// formula/expression, so workbook-scoped `=Revenue/A1` is the division Expr (`Revenue` resolved
/// recursively as a name, unresolvable -> a located `#NAME?`), NEVER the cross-sheet ref `Revenue!A1`
/// (HARD RULE 5: never a silently-wrong ref).
///
/// Accepted forms: a non-`=` degraded-symlink relative path (`Data/H1` workbook-scoped, `../Data/H1`
/// cross-sheet); a pure A1 ref (`=A2:A366`, `B5`, `Sheet1!B5`, `'Sheet 2'!B5`, or a non-`=` degraded
/// bare `B5`), re-emitted with canonical sheet quoting and bare/same-sheet refs qualified with the
/// scope sheet -> [`NameTarget::Ref`]; anything else (`=Base*1.05`, `=Revenue/A1`, `3.14`,
/// `1/Sheet!A1`) -> [`NameTarget::Expr`]. A non-`=` single-slash `Sheet/Cell` that is neither a valid
/// path in this scope nor a division -> `None`.
fn classify_ref_file(scope: &NameScope, content: &str) -> Option<NameTarget> {
    let has_eq = content.trim().starts_with('=');
    let body = strip_eq(content);
    // Only a BARE (non-`=`) body may be a degraded static-range/single-cell symlink whose target
    // crossed a scope boundary (it keeps the relative path `Data/H1` / `../Data/H1`); re-qualify it to
    // the same `Sheet!Cell` the live symlink would have resolved to, so a degraded workbook reads
    // equivalently (HARD RULE 5). A `=`-prefixed body is never a path — it falls through to the
    // pure-ref / expression classification below.
    if !has_eq {
        match degraded_path_target(body, scope) {
            DegradedPath::Resolved(sheet, cell) => {
                return Some(NameTarget::Ref(qualify(&sheet, &cell)));
            }
            DegradedPath::Ambiguous => return None,
            DegradedPath::NotAPath => {}
        }
    }
    // A pure A1 ref (at either form) is a `Ref`; everything else is an `Expr`.
    Some(if is_pure_ref(body) {
        ref_target(scope, body)
    } else {
        NameTarget::Expr(body.to_string())
    })
}

/// Build a [`NameTarget::Ref`] from an already-validated pure-ref `body`. A `Sheet!Addr` ref is
/// re-emitted with canonical sheet quoting ([`quote_sheet`]) so a sheet name needing quotes injects the
/// `'…'!` form the lexer round-trips (never a split-at-the-space `#NAME?`); a bare (unqualified) ref is
/// qualified with the scope sheet, so a sheet-scoped name resolves the same from any sheet (matching the
/// symlink form, whose target sheet is always explicit).
fn ref_target(scope: &NameScope, body: &str) -> NameTarget {
    match body.rsplit_once('!') {
        Some((sheet_part, addr)) => NameTarget::Ref(qualify(&unquote_sheet(sheet_part), addr)),
        None => match scope {
            NameScope::Sheet(s) => NameTarget::Ref(qualify(s, body)),
            NameScope::Workbook => NameTarget::Ref(body.to_string()),
        },
    }
}

/// The scope's sheet name, or `None` for a workbook scope (a bare ref there stays unqualified).
fn scope_sheet(scope: &NameScope) -> Option<String> {
    match scope {
        NameScope::Sheet(s) => Some(s.clone()),
        NameScope::Workbook => None,
    }
}

/// Qualify `addr` (a cell or `cell:cell` range) with its target `sheet`, QUOTING the sheet name when it
/// is not a bare identifier — a space (`My Data`, `Sheet 1`) is the ubiquitous case. The rewrite injects
/// this token straight into a `=formula` the charlie lexer then parses, and the lexer reads a
/// space-bearing sheet name only in the `'…'!` quoted form ([`charlie_ast`] `lex_quoted_sheet_name`); an
/// unquoted `My Data!B5` would split at the space and the name would resolve to a located `#NAME?`
/// instead of the value (ENG6/CORE1). Over-quoting a name that would have lexed bare is harmless (the
/// lexer accepts `'Data'!B5` identically), so any non-bare-identifier sheet is quoted.
fn qualify(sheet: &str, addr: &str) -> String {
    format!("{}!{addr}", quote_sheet(sheet))
}

/// A sheet name as a formula token: a bare identifier (an ASCII letter then letters/digits) is used
/// as-is; anything else (a space, punctuation, a leading digit, `_`) is wrapped in `'…'` with any
/// interior `'` doubled — the `'It''s Data'` form the lexer round-trips. Mirrors the ADDRESS builder's
/// `quote_sheet_name` and translate.rs's quoted `sheet_prefix` intent, but conservative (a `_`/`.` name
/// the Excel-parity quoter would leave bare is quoted here, since the charlie lexer's bare-word set is
/// narrower than Excel's and quoting is always safe).
fn quote_sheet(name: &str) -> String {
    let bare = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric());
    if bare {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

/// Insert a name, refusing a duplicate identifier in the same scope (an ambiguous name).
fn insert_name(table: &mut NameTable, name: Name, diags: &mut Vec<Diagnostic>) {
    if table
        .names
        .iter()
        .any(|n| n.ident == name.ident && n.scope == name.scope)
    {
        diags.push(refuse(
            &name.ident,
            format!("name {:?} is defined twice in the same scope", name.ident),
        ));
        return;
    }
    table.names.push(name);
}

/// A located name-representation refusal (CORE2), anchored on the offending entry name.
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
        // Rate expands, and Base inside it expands too (recursive), each parenthesized.
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
        // `insert_name`'s duplicate-ident-in-scope refusal (an ambiguous name) — the last of the
        // build-time refusal branches, alongside the A1-ident, lone-corner, and inverted-corner ones.
        let (t, d) = NameTable::build(vec![
            reffile(sheet("S"), "Rate", "1"),
            reffile(sheet("S"), "Rate", "2"),
        ]);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].code, Code::NameRefusal);
        // The first definition survives; the duplicate is dropped (never silently overwritten).
        assert_eq!(t.rewrite_tsv("=Rate", "S"), "=(1)");
        // The SAME identifier in a DIFFERENT scope is not a duplicate (workbook vs sheet coexist).
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

    #[test]
    fn a_degraded_symlink_reffile_reads_as_a_ref() {
        // A bare A1 content (no `=`) is what a same-sheet symlink degrades to through zip -> read as a ref.
        let (t, d) = NameTable::build(vec![reffile(sheet("S"), "total", "B5")]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(t.rewrite_tsv("=total", "S"), "=S!B5");
    }

    #[test]
    fn a_degraded_workbook_scoped_symlink_relative_path_reads_as_a_ref() {
        // A WORKBOOK-scoped static name's symlink target is the relative path `Data/H1`; degraded to
        // ref-file content it must re-qualify to `Data!H1` (the same ref the live symlink resolved to),
        // not become an Expr. Covers the fixture's `TaxRate` under a symlink-flattening container.
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
        // A sheet-scoped name pointing at another sheet degrades (through a symlink-flattening zip) to a
        // BARE relative path `../Data/H1`, which re-qualifies to `Data!H1`. The non-POSIX ref-file writer
        // emits the same target as a proper `=Sheet!Cell` ref (never a `=`-prefixed PATH — see
        // `an_eq_prefixed_path_body_is_an_expr_never_a_degraded_ref` for why `=` is always a formula).
        let (t, d) = NameTable::build(vec![
            reffile(sheet("S"), "Cross", "../Data/H1"),
            reffile(sheet("S"), "CrossEq", "=Data!H1"),
        ]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(t.rewrite_tsv("=Cross+CrossEq", "S"), "=Data!H1+Data!H1");
    }

    #[test]
    fn a_degraded_range_corner_relative_path_reads_as_a_range() {
        // A workbook-scoped RANGE name's two corner symlinks degrade to relative-path ref-files
        // (`Data/B2`, `Data/B4`); the reader-union re-qualifies each corner and reforms the range.
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
        // `=A1/B1` is a division EXPRESSION, not a `sheet/cell` path (both sides are A1 cells and there is
        // no `../`), so it stays an Expr — the ambiguity the relative-path reader must not swallow.
        let (t, d) = NameTable::build(vec![reffile(sheet("S"), "Ratio", "=A1/B1")]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(t.rewrite_tsv("=Ratio", "S"), "=(A1/B1)");
    }

    #[test]
    fn name_resolution_is_case_sensitive() {
        // A charlie name IS a filesystem entry (FS4): `Rate` and `rate` are two distinct entries, so
        // resolution is case-sensitive by design (not Excel's case-fold) — a differently-cased token
        // does NOT resolve, and stays verbatim to load as a located `#NAME?` (see `resolve`'s note).
        let (t, d) = NameTable::build(vec![reffile(sheet("S"), "Rate", "1")]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(t.rewrite_tsv("=Rate", "S"), "=(1)"); // exact spelling resolves
        assert_eq!(t.rewrite_tsv("=rate", "S"), "=rate"); // different case: unresolved -> #NAME?
    }

    #[test]
    fn an_unresolvable_name_is_left_verbatim() {
        let (t, _) = NameTable::build(vec![reffile(sheet("S"), "Known", "=A1:A2")]);
        // An unknown name stays as its token -> the parser refuses it -> a located #NAME?.
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
        // ENG6/CORE1: a resolved ref whose target sheet needs quoting (a space is the common case) must
        // inject the `'…'!` quoted form the lexer reads as ONE sheet token — an unquoted `My Data!B5`
        // splits at the space and resolves to a located `#NAME?` instead of the value. Every site that
        // qualifies a ref must quote: a bare single-cell symlink, a corner-pair range, a degraded
        // workbook-scoped relative path, and a scope-sheet-qualified bare ref.
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
        // `local` is sheet-scoped on `My Data`, so it resolves on that sheet — qualified with its scope
        // sheet, which likewise needs quoting.
        assert_eq!(t.rewrite_tsv("=local+1", "My Data"), "='My Data'!B7+1");
    }

    #[test]
    fn a_sheet_name_with_an_apostrophe_doubles_the_quote() {
        // The `'It''s Data'` escaping form the lexer round-trips: an interior `'` is doubled.
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
        // A plain sheet name (letters/digits) is injected bare — no over-eager quoting that would churn
        // the common case or the existing corpus.
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
        // degraded_path_target fitness: a SHEET-scoped ref-file `Data/H1` (non-A1 left, bare-A1 right,
        // no `!`, no `../`) is AMBIGUOUS — a cross-sheet target that lost its `../`, or a `Name/cell`
        // division. It must be a LOCATED refusal, NEVER silently the cross-sheet ref `Data!H1`. (A
        // legit sheet-scoped cross-sheet target carries `../`; see the `../Data/H1` test above.)
        let (t, d) = NameTable::build(vec![reffile(sheet("S"), "Bad", "Data/H1")]);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].code, Code::NameRefusal);
        // The name never entered the table, so the token stays verbatim -> a located #NAME? (never the
        // silently-wrong `=Data!H1`).
        assert_eq!(t.rewrite_tsv("=Bad", "S"), "=Bad");
        assert!(t.names().iter().all(|n| n.ident != "Bad"));
        // The very same body under a WORKBOOK scope IS the legit degrade of a workbook name's symlink
        // target (from the root down into a tab) — that stays a ref, proving the split is scope-driven.
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
        // is_pure_ref fitness: the WHOLE ref is validated, not just the address after the last `!`.
        // A quoted sheet part with a special char (`'Sheet 2'!A1`) classifies as a pure REF and is
        // re-emitted with canonical quoting so the lexer reads one sheet token.
        let (t, d) = NameTable::build(vec![reffile(sheet("S"), "Q", "='Sheet 2'!A1")]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(t.rewrite_tsv("=Q*2", "S"), "='Sheet 2'!A1*2"); // ref: injected unparenthesized
        // Vice-versa: an EXPRESSION that merely ENDS in a valid address (`1/Sheet!A1`, sheet part
        // `1/Sheet` is not a sheet token) must NOT be misclassified as a pure ref — that would inject
        // it unparenthesized and change precedence. It stays an Expr, wrapped in parens.
        let (t2, d2) = NameTable::build(vec![reffile(sheet("S"), "E", "=1/Sheet!A1")]);
        assert!(d2.is_empty(), "{d2:?}");
        assert_eq!(t2.rewrite_tsv("=E*2", "S"), "=(1/Sheet!A1)*2"); // expr: parenthesized, precedence kept
        // A bare-identifier sheet ref round-trips unchanged (no over-quoting of the common case).
        let (t3, d3) = NameTable::build(vec![reffile(sheet("S"), "R", "=Sheet1!B2:B4")]);
        assert!(d3.is_empty(), "{d3:?}");
        assert_eq!(t3.rewrite_tsv("=SUM(R)", "S"), "=SUM(Sheet1!B2:B4)");
    }

    #[test]
    fn a_degraded_corner_with_an_ambiguous_path_is_refused() {
        // The corner analogue of the single-slash refusal: a SHEET-scoped `.begin`/`.end` corner
        // whose degraded content is a bare `Data/B2` (no `../`) is ambiguous -> a located refusal, not
        // a silently-wrong cross-sheet corner. Both corners refuse (each is an ambiguous target).
        let (t, d) = NameTable::build(vec![
            reffile(sheet("S"), "r.begin", "Data/B2"),
            reffile(sheet("S"), "r.end", "Data/B4"),
        ]);
        assert!(
            d.iter().all(|x| x.code == Code::NameRefusal) && !d.is_empty(),
            "{d:?}"
        );
        assert!(t.names().iter().all(|n| n.ident != "r")); // no silently-wrong range materialized
    }

    #[test]
    fn an_eq_prefixed_path_body_is_an_expr_never_a_degraded_ref() {
        // The `=` prefix disambiguates FORM at ANY scope (HARD RULE 5): a degraded POSIX symlink is a
        // BARE relative path, never one carrying `=`. So a WORKBOOK-scoped `=Revenue/A1` is the division
        // EXPRESSION (`Revenue` resolved recursively as a name — unresolvable here, so it stays verbatim
        // and loads as a located `#NAME?`), NEVER the silently-wrong cross-sheet ref `Revenue!A1`.
        let (t, d) = NameTable::build(vec![RawNameEntry {
            scope: NameScope::Workbook,
            entry_name: "Div".to_string(),
            form: NameRepr::RefFile {
                content: "=Revenue/A1".to_string(),
            },
        }]);
        assert!(d.is_empty(), "{d:?}");
        // Parenthesized division, Revenue left verbatim (unresolved -> #NAME?) — NOT `=Revenue!A1`.
        assert_eq!(t.rewrite_tsv("=Div", "S"), "=(Revenue/A1)");
        assert!(!t.rewrite_tsv("=Div", "S").contains('!'));

        // The very same body BARE (non-`=`) at a workbook scope IS the legit degraded-symlink path (the
        // root down into a tab) and resolves as the cross-sheet ref — proving the split is `=`-driven.
        let (bt, bd) = NameTable::build(vec![RawNameEntry {
            scope: NameScope::Workbook,
            entry_name: "Path".to_string(),
            form: NameRepr::RefFile {
                content: "Revenue/A1".to_string(),
            },
        }]);
        assert!(bd.is_empty(), "{bd:?}");
        assert_eq!(bt.rewrite_tsv("=Path", "S"), "=Revenue!A1");

        // And a SHEET-scoped bare `Data/H1` (a cross-sheet target that lost its `../`) stays the located
        // refusal — the `=`-form change does not disturb the existing scope-driven ambiguity refusal.
        let (st, sd) = NameTable::build(vec![reffile(sheet("S"), "Amb", "Data/H1")]);
        assert_eq!(sd.len(), 1, "{sd:?}");
        assert_eq!(sd[0].code, Code::NameRefusal);
        assert!(st.names().iter().all(|n| n.ident != "Amb"));
    }

    #[test]
    fn an_eq_prefixed_corner_body_is_refused_never_a_materialized_cross_sheet_range() {
        // The CORNER analogue of `an_eq_prefixed_path_body_is_an_expr_never_a_degraded_ref`: a degraded
        // symlink corner is a BARE relative path, so a `=`-prefixed corner body is a formula, never a
        // path — it must NOT reach `degraded_path_target` (HARD RULE 5). A workbook-scoped
        // `=Revenue/A1`+`=Revenue/A9` corner pair is a located refusal, NOT a silently-materialized
        // cross-sheet `Revenue!A1:A9` range, so `=SUM` over it does not silently resolve.
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
        assert!(t.names().iter().all(|n| n.ident != "r")); // no silently-wrong range materialized
        assert_eq!(t.rewrite_tsv("=SUM(r)", "S"), "=SUM(r)"); // unresolved -> a located #NAME?, never `Revenue!A1:A9`
        assert!(!t.rewrite_tsv("=SUM(r)", "S").contains('!'));

        // Legit `=`-prefixed corners still resolve via the NotAPath classification: an explicit
        // `=Data!H1`/`=Data!H9` pair (rsplit-`!`) and a bare `=A1`/`=A9` pair (parse_a1 vs the scope
        // sheet). The `=`-gate forces NotAPath; the existing corner logic then resolves each.
        let (xt, xd) = NameTable::build(vec![
            reffile(sheet("S"), "x.begin", "=Data!H1"),
            reffile(sheet("S"), "x.end", "=Data!H9"),
            reffile(sheet("S"), "y.begin", "=A1"),
            reffile(sheet("S"), "y.end", "=A9"),
        ]);
        assert!(xd.is_empty(), "{xd:?}");
        assert_eq!(xt.rewrite_tsv("=SUM(x)", "S"), "=SUM(Data!H1:H9)");
        assert_eq!(xt.rewrite_tsv("=SUM(y)", "S"), "=SUM(S!A1:A9)");

        // And a BARE (non-`=`) `../Data/H1`/`../Data/H9` sheet-scoped corner pair still resolves as the
        // cross-sheet range — the `=`-gate leaves the degraded-path corner untouched.
        let (bt, bd) = NameTable::build(vec![
            reffile(sheet("S"), "z.begin", "../Data/H1"),
            reffile(sheet("S"), "z.end", "../Data/H9"),
        ]);
        assert!(bd.is_empty(), "{bd:?}");
        assert_eq!(bt.rewrite_tsv("=SUM(z)", "S"), "=SUM(Data!H1:H9)");
    }

    #[test]
    fn a_malformed_single_cell_symlink_target_is_a_located_refusal() {
        // A bare single-cell symlink whose target cell is malformed (`B` has no row) must be a LOCATED
        // refusal at BUILD (like a range corner is), never a silently-wrong ref qualified into a deferred
        // parse error at eval (HARD RULE 3). The name never enters the table.
        let (t, d) = NameTable::build(vec![symlink(sheet("S"), "bad", "S", "B")]);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].code, Code::NameRefusal);
        assert!(t.names().iter().all(|n| n.ident != "bad"));
        assert_eq!(t.rewrite_tsv("=bad", "S"), "=bad"); // unresolved -> a located #NAME?
    }

    #[test]
    fn a_blank_corner_is_a_located_refusal_never_a_panic() {
        // Blank-corner materialization: a range corner whose ref-file content is BLANK (empty/whitespace)
        // resolves to no cell — it must be a LOCATED refusal (HARD RULE 3), never a panic and never a
        // silently-wrong range. `parse_a1("")` is an Err, so corner_target -> None -> acc_corner refuses.
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
}
