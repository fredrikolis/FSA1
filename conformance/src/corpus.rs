// Concern: loads the `.fixtures` corpus into Fixture records | Non-concern: the literal grammar, grading a fixture | IO: (the formula/ dir) -> Vec<Fixture>

use std::path::{Path, PathBuf};

use fsa1_ast::{Value, parse_a1};

use crate::literal;

#[derive(Debug, Clone)]
pub struct Fixture {
    /// `<.fixtures file stem>/<record name>`, unique corpus-wide.
    pub key: String,
    pub funcs: Vec<String>,
    pub formula: String,
    pub expect: Value,
    /// Context cells as `(sheet, col, row, value)`; `sheet` `None` names the default sheet.
    pub cells: Vec<(Option<String>, u32, u32, Value)>,
    /// The cell the formula is computed in, `(row, col)` to match `eval_at`'s argument order.
    pub at: Option<(u32, u32)>,
}

pub fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("formula")
}

pub fn load_all() -> Result<Vec<Fixture>, String> {
    load_from(&corpus_dir())
}

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

#[derive(Default)]
struct Pending {
    name: Option<String>,
    funcs: Vec<String>,
    formula: Option<String>,
    expect: Option<Value>,
    cells: Vec<(Option<String>, u32, u32, Value)>,
    at: Option<(u32, u32)>,
}

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
    use fsa1_ast::ErrKind;

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
        assert_eq!(fx[0].at, Some((4, 2)), "C5 stored row-first");
        assert!(
            parse_one("c", "[x]\nat: Data!A1\nformula: =ROW()\nexpect: 1\n").is_err(),
            "a sheet-qualified `at:` computing cell is refused"
        );
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
        assert!(
            parse_one("c", "formula: =1\n").is_err(),
            "content before the first `[name]` header"
        );
        assert!(
            parse_one("c", "[x]\nformula: =1\n").is_err(),
            "a record with no `expect:`"
        );
        assert!(
            parse_one("c", "[x]\nexpect: 1\n").is_err(),
            "a record with no `formula:`"
        );
        assert!(
            parse_one("c", "[x]\ncell $A$1: 1\nformula: =A1\nexpect: 1\n").is_err(),
            "a `$`-absolute context cell"
        );
        assert!(
            parse_one("c", "[x]\nbogus: 1\nformula: =1\nexpect: 1\n").is_err(),
            "an unknown record key"
        );
    }
}
