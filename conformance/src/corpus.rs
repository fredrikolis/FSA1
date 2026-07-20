// Concern: the FORMULA-FIXTURE corpus — the `Fixture` record (a `category/name` key, the functions it exercises, an input CONTEXT of named cells, a formula string, and an EXTERNALLY-authored EXPECTED value) plus the record grammar (`[name]` blocks with `funcs:`/`formula:`/`expect:`/`cell …:` lines) and the directory loader that globs `formula/*.fixtures` | Non-concern: the per-literal grammar (literal.rs owns `6`/`"t"`/`{..}`) and GRADING a fixture (grade.rs parses+evaluates the formula) — this only reads authored corpus into typed records | IO: (the `formula/` dir on disk) -> Result<Vec<Fixture>, String>
//! The corpus loader. A fixture is a self-contained value probe: an input context, a formula, and
//! the value the formula MUST produce — the expected value authored INDEPENDENTLY of charlie (see
//! `formula/PROVENANCE.md`), so a divergence is always a fact about charlie, never about the oracle.

use std::path::{Path, PathBuf};

use charlie_ast::{Value, parse_a1};

use crate::literal;

/// One value probe: parse+evaluate `formula` against the `cells` context and compare to `expect`.
#[derive(Debug, Clone)]
pub struct Fixture {
    /// The stable `category/name` key (category = the `.fixtures` file stem). Unique corpus-wide.
    pub key: String,
    /// The uppercase function names this fixture exercises, for the coverage ratchet (may be empty
    /// for a pure operator / error-propagation probe).
    pub funcs: Vec<String>,
    /// The formula string, with or without a leading `=`.
    pub formula: String,
    /// The EXPECTED value — the oracle. Authored by hand from the spec, never charlie-produced.
    pub expect: Value,
    /// The input context: `(sheet, col, row, value)` cells the formula's refs/ranges resolve
    /// against. `sheet` is `None` for the default (unqualified) sheet, or `Some(name)` for a cell on
    /// a named sheet (`cell Data!A1: 5`) that a cross-sheet reference resolves.
    pub cells: Vec<(Option<String>, u32, u32, Value)>,
    /// The 0-based `(row, col)` of the cell the formula is COMPUTED IN, when the fixture pins a
    /// computing cell (`at: C5`) — the seam the no-argument `ROW()`/`COLUMN()` forms read. Stored
    /// row-first to match `eval_at(.., row, col)`'s argument order (no swap at the grade callsite).
    /// `None` for the overwhelming majority of fixtures, which are context-free (ad-hoc) probes.
    pub at: Option<(u32, u32)>,
}

/// The corpus directory: `<crate>/formula`, anchored to the manifest dir so it resolves from any cwd.
pub fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("formula")
}

/// Load every fixture from `formula/*.fixtures`, sorted by key. Fail-fast (`Err`) on an unreadable
/// dir, a malformed record, a bad literal, or a duplicate key — a broken corpus is our own invariant.
pub fn load_all() -> Result<Vec<Fixture>, String> {
    load_from(&corpus_dir())
}

/// Load every fixture under a specific directory (the seam the loader tests drive).
pub fn load_from(dir: &Path) -> Result<Vec<Fixture>, String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read corpus dir {}: {e}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "fixtures"))
        .collect();
    files.sort();

    let mut out = Vec::new();
    for path in &files {
        let category = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("bad fixture filename: {}", path.display()))?;
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        parse_file(category, &text, &mut out).map_err(|e| format!("{}: {e}", path.display()))?;
    }

    // Duplicate keys would let one fixture silently mask another — reject.
    let mut keys: Vec<&str> = out.iter().map(|f| f.key.as_str()).collect();
    keys.sort_unstable();
    let before = keys.len();
    keys.dedup();
    if keys.len() != before {
        return Err("duplicate fixture key across the corpus".to_string());
    }

    out.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(out)
}

/// A partially-accumulated record during the line scan.
#[derive(Default)]
struct Pending {
    name: Option<String>,
    funcs: Vec<String>,
    formula: Option<String>,
    expect: Option<Value>,
    cells: Vec<(Option<String>, u32, u32, Value)>,
    at: Option<(u32, u32)>,
}

/// Parse one `.fixtures` file's records into `out`. Grammar (line-based):
/// - `# …` / blank → ignored;
/// - `[name]` → start a new record (flushing the previous);
/// - `funcs: A, B` → the exercised functions (uppercased);
/// - `formula: =…` → the formula (required);
/// - `expect: <literal>` → the EXPECTED value (required);
/// - `cell <A1>: <literal>` → one context cell (canonical A1 only — no `$`/lowercase/leading-zero),
///   optionally sheet-qualified as `cell <Sheet>!<A1>: <literal>` for a cross-sheet reference;
/// - `at: <A1>` → the COMPUTING cell (unqualified canonical A1), so no-argument `ROW()`/`COLUMN()`
///   report that cell's coordinate; omitted for a context-free (ad-hoc) probe;
/// - `note: …` → a free-form author note (ignored by the grader).
fn parse_file(category: &str, text: &str, out: &mut Vec<Fixture>) -> Result<(), String> {
    let mut pending: Option<Pending> = None;

    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let at = |msg: String| format!("line {}: {msg}", lineno + 1);

        if let Some(name) = trimmed.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            if let Some(prev) = pending.take() {
                out.push(finish(category, prev).map_err(&at)?);
            }
            let name = name.trim().to_string();
            if name.is_empty() {
                return Err(at("empty fixture name in `[]`".to_string()));
            }
            pending = Some(Pending {
                name: Some(name),
                ..Pending::default()
            });
            continue;
        }

        let rec = pending
            .as_mut()
            .ok_or_else(|| at("content before the first `[name]` header".to_string()))?;

        if let Some(rest) = trimmed.strip_prefix("cell ") {
            let (addr, lit) = rest
                .split_once(':')
                .ok_or_else(|| at("a `cell` line needs `cell <A1>: <literal>`".to_string()))?;
            let (sheet, col, row) = parse_cell_addr(addr.trim()).map_err(&at)?;
            let value = literal::parse(lit).map_err(&at)?;
            rec.cells.push((sheet, col, row, value));
            continue;
        }

        let (key, val) = trimmed
            .split_once(':')
            .ok_or_else(|| at(format!("expected `key: value`, got {trimmed:?}")))?;
        let val = val.trim();
        match key.trim() {
            "funcs" => {
                rec.funcs = val
                    .split([',', ' '])
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_ascii_uppercase())
                    .collect();
            }
            "formula" => rec.formula = Some(val.to_string()),
            "expect" => rec.expect = Some(literal::parse(val).map_err(&at)?),
            "at" => {
                // The computing cell for the no-argument ROW()/COLUMN() forms — an unqualified
                // canonical A1 (a cross-sheet computing cell is not a v1 concept).
                let (sheet, col, row) = parse_cell_addr(val).map_err(&at)?;
                if sheet.is_some() {
                    return Err(at(
                        "an `at:` computing cell must be an unqualified A1 address".to_string(),
                    ));
                }
                rec.at = Some((row, col));
            }
            "note" => {}
            other => return Err(at(format!("unknown key {other:?}"))),
        }
    }

    if let Some(prev) = pending.take() {
        out.push(finish(category, prev).map_err(|m| format!("fixture: {m}"))?);
    }
    Ok(())
}

/// Turn a completed [`Pending`] into a [`Fixture`], enforcing the required fields.
fn finish(category: &str, p: Pending) -> Result<Fixture, String> {
    let name = p.name.ok_or("missing fixture name")?;
    let formula = p
        .formula
        .ok_or_else(|| format!("fixture {name:?} has no `formula:`"))?;
    let expect = p
        .expect
        .ok_or_else(|| format!("fixture {name:?} has no `expect:`"))?;
    Ok(Fixture {
        key: format!("{category}/{name}"),
        funcs: p.funcs,
        formula,
        expect,
        cells: p.cells,
        at: p.at,
    })
}

/// Parse an optionally sheet-qualified canonical A1 cell address into `(sheet, col, row)`
/// (zero-based col/row), rejecting the non-canonical forms (a context cell is authored, so
/// `$`/lowercase/leading-zero is a corpus error, not a value to keep). A `Sheet!A1` prefix places the
/// cell on a named sheet a cross-sheet reference resolves; the sheet name is unquoted (corpus authors
/// pick names without spaces) and must be non-empty.
fn parse_cell_addr(addr: &str) -> Result<(Option<String>, u32, u32), String> {
    let (sheet, cell_addr) = match addr.split_once('!') {
        Some((name, rest)) => {
            let name = name.trim();
            if name.is_empty() {
                return Err(format!("context cell {addr:?} has an empty sheet name"));
            }
            (Some(name.to_string()), rest.trim())
        }
        None => (None, addr),
    };
    let a = parse_a1(cell_addr).map_err(|e| format!("bad cell address {addr:?}: {e:?}"))?;
    if a.col_abs || a.row_abs {
        return Err(format!("context cell {addr:?} must not use `$`"));
    }
    if a.col_had_lowercase {
        return Err(format!("context cell {addr:?} must be uppercase"));
    }
    if a.row_had_leading_zero {
        return Err(format!(
            "context cell {addr:?} must not have a leading-zero row"
        ));
    }
    Ok((sheet, a.col, a.row))
}

#[cfg(test)]
mod tests {
    use super::*;
    use charlie_ast::ErrKind;

    fn parse_one(category: &str, text: &str) -> Result<Vec<Fixture>, String> {
        let mut out = Vec::new();
        parse_file(category, text, &mut out)?;
        Ok(out)
    }

    #[test]
    fn parses_a_full_record() {
        let text = "\
# a comment
[sum-basic]
funcs: SUM
cell A1: 1
cell A2: 2
cell A3: 3
formula: =SUM(A1:A3)
expect: 6
";
        let fx = parse_one("aggregation", text).unwrap();
        assert_eq!(fx.len(), 1);
        let f = &fx[0];
        assert_eq!(f.key, "aggregation/sum-basic");
        assert_eq!(f.funcs, vec!["SUM"]);
        assert_eq!(f.formula, "=SUM(A1:A3)");
        assert_eq!(f.expect, Value::Number(6.0));
        assert_eq!(f.cells.len(), 3);
        assert_eq!(f.cells[0], (None, 0, 0, Value::Number(1.0)));
    }

    #[test]
    fn parses_sheet_qualified_context_cells() {
        let text = "\
[cross-sheet]
cell A1: 1
cell Data!A1: 42
formula: =Data!A1
expect: 42
";
        let fx = parse_one("crosssheet", text).unwrap();
        assert_eq!(fx.len(), 1);
        let f = &fx[0];
        assert_eq!(f.cells[0], (None, 0, 0, Value::Number(1.0)));
        assert_eq!(
            f.cells[1],
            (Some("Data".to_string()), 0, 0, Value::Number(42.0))
        );
    }

    #[test]
    fn parses_an_at_computing_cell() {
        let text = "\
[row-at-c5]
funcs: ROW
at: C5
formula: =ROW()
expect: 5
";
        let fx = parse_one("lookup", text).unwrap();
        assert_eq!(fx.len(), 1);
        // C5 is 0-based (row 4, col 2) — stored row-first to match `eval_at`'s order.
        assert_eq!(fx[0].at, Some((4, 2)));
        // A sheet-qualified `at:` is refused (the computing cell is a v1 single-sheet notion).
        assert!(parse_one("c", "[x]\nat: Data!A1\nformula: =ROW()\nexpect: 1\n").is_err());
    }

    #[test]
    fn multiple_records_and_error_expect() {
        let text = "\
[a]
formula: =1/0
expect: #DIV/0!
[b]
funcs: ABS
formula: =ABS(-2)
expect: 2
";
        let fx = parse_one("errors", text).unwrap();
        assert_eq!(fx.len(), 2);
        assert_eq!(fx[0].expect, Value::Error(ErrKind::Div0));
        assert_eq!(fx[1].funcs, vec!["ABS"]);
    }

    #[test]
    fn malformed_records_fail_fast() {
        // Content before any header.
        assert!(parse_one("c", "formula: =1\n").is_err());
        // Missing required expect.
        assert!(parse_one("c", "[x]\nformula: =1\n").is_err());
        // Missing required formula.
        assert!(parse_one("c", "[x]\nexpect: 1\n").is_err());
        // A `$` context cell.
        assert!(parse_one("c", "[x]\ncell $A$1: 1\nformula: =A1\nexpect: 1\n").is_err());
        // An unknown key.
        assert!(parse_one("c", "[x]\nbogus: 1\nformula: =1\nexpect: 1\n").is_err());
    }
}
