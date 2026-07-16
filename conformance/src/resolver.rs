// Concern: the deterministic STUB Resolver built from a fixture's input context — a `HashMap`-backed cell store keyed by (sheet, col, row) for single `Ref` reads across a DEFAULT sheet plus zero or more NAMED sheets, a name→`SheetId` map so a cross-sheet reference resolves, plus a per-range materialized buffer (so `range()` can hand back a BORROWED `ArrayView` over an owned contiguous block, honoring the resolver contract with no `unsafe`); the ranges to materialize are collected from the formula's own `Expr` and pre-resolved to `RangeRef`s, so every `Range` node the evaluator asks for is pre-present | Non-concern: the formula LANGUAGE (charlie-ast owns parse/eval) and the filesystem model (charlie-model owns the real fs-backed Resolver) — this is a test double for grading only | IO: (a fixture's cells + its parsed `Expr`) -> a `charlie_ast::Resolver`
//! The grading stub Resolver. It mirrors the shape charlie-ast's own `#[cfg(test)]` grid uses (borrow
//! a contiguous window out of an owned buffer) but generalizes to arbitrary sub-rectangles: rather
//! than materialize whole rows, it pre-materializes exactly the [`RangeRef`]s the parsed formula
//! names, keyed by the resolved range, so each `range()` call is a borrow into a buffer prepared up
//! front. It backs a default sheet plus any NAMED sheets the fixture's context declares, so a
//! cross-sheet reference (`Data!A1`) resolves name → [`SheetId`] → value/range like the real model.

use std::collections::HashMap;

use charlie_ast::{ArrayView, CellRef, Expr, RangeRef, Resolver, Shape, SheetId, Value};

/// The default sheet's index, and the alias name it also answers to (so `Sheet1!A1` and an
/// unqualified `A1` name the same sheet — matching charlie-ast's own test grid).
const DEFAULT_SHEET: u32 = 0;
const DEFAULT_SHEET_NAME: &str = "Sheet1";

/// A context-backed resolver: single cells in a map keyed by `(sheet_idx, col, row)`, each referenced
/// range pre-materialized and keyed by its resolved [`RangeRef`], plus a sheet-name→id map.
pub struct StubResolver {
    cells: HashMap<(u32, u32, u32), Value>,
    ranges: HashMap<RangeRef, (Shape, Vec<Value>)>,
    /// Sheet name → id. The default sheet is [`DEFAULT_SHEET`], aliased to [`DEFAULT_SHEET_NAME`].
    sheet_ids: HashMap<String, u32>,
    /// A persistent one-cell buffer so a defensively-unexpected `range()` can still return a valid
    /// borrowed view (never reached in practice — every range is collected from the same `Expr`).
    fallback: Vec<Value>,
}

impl StubResolver {
    /// Build a resolver from a fixture's `(sheet, col, row, value)` cells and its parsed `expr`. Named
    /// sheets are assigned ids `1..` (sorted for determinism); the default sheet is id `0`. Every
    /// [`charlie_ast::RangeNode`] in `expr` is resolved (name → id) and materialized row-major from
    /// the addressed sheet's cell store (missing cells → `Blank`); a range naming an unknown sheet is
    /// skipped (the evaluator returns `#REF!` and never asks for it).
    pub fn build(cells: &[(Option<String>, u32, u32, Value)], expr: &Expr) -> StubResolver {
        // Assign sheet ids: the default sheet is 0; each distinct NAMED sheet (other than the default
        // alias) gets the next id, in sorted order for a deterministic mapping.
        let mut names: Vec<&str> = cells
            .iter()
            .filter_map(|(s, ..)| s.as_deref())
            .filter(|n| *n != DEFAULT_SHEET_NAME)
            .collect();
        names.sort_unstable();
        names.dedup();
        let mut sheet_ids: HashMap<String, u32> = HashMap::new();
        sheet_ids.insert(DEFAULT_SHEET_NAME.to_string(), DEFAULT_SHEET);
        for (i, name) in names.iter().enumerate() {
            sheet_ids.insert((*name).to_string(), i as u32 + 1);
        }
        let lookup = |name: &str| sheet_ids.get(name).copied().map(SheetId);

        let cell_map: HashMap<(u32, u32, u32), Value> = cells
            .iter()
            .map(|(s, c, r, v)| {
                let idx = s.as_deref().map_or(DEFAULT_SHEET, |n| {
                    lookup(n).map_or(DEFAULT_SHEET, |SheetId(i)| i)
                });
                ((idx, *c, *r), v.clone())
            })
            .collect();

        let mut ranges = HashMap::new();
        let mut seen = Vec::new();
        collect_ranges(expr, &lookup, &mut seen);
        for rr in seen {
            ranges
                .entry(rr)
                .or_insert_with(|| materialize(rr, &cell_map));
        }

        StubResolver {
            cells: cell_map,
            ranges,
            sheet_ids,
            fallback: vec![Value::Blank],
        }
    }
}

impl Resolver for StubResolver {
    fn value(&self, cell: CellRef) -> Value {
        let idx = cell.sheet.map_or(DEFAULT_SHEET, |SheetId(i)| i);
        self.cells
            .get(&(idx, cell.col, cell.row))
            .cloned()
            .unwrap_or(Value::Blank)
    }

    fn range(&self, range: RangeRef) -> ArrayView<'_> {
        match self.ranges.get(&range) {
            Some((shape, cells)) => ArrayView {
                shape: *shape,
                cells,
            },
            // Unreachable for a parsed formula (all ranges were collected up front); a valid 1×1
            // blank view keeps the contract total rather than panicking on a synthesized range.
            None => ArrayView {
                shape: Shape { rows: 1, cols: 1 },
                cells: &self.fallback,
            },
        }
    }

    fn sheet_id(&self, name: &str) -> Option<SheetId> {
        self.sheet_ids.get(name).copied().map(SheetId)
    }
}

/// Materialize a rectangular range into a row-major contiguous buffer from the cell store, reading
/// the sheet the range's (resolved) [`SheetId`] names.
fn materialize(rr: RangeRef, cells: &HashMap<(u32, u32, u32), Value>) -> (Shape, Vec<Value>) {
    let idx = rr.start.sheet.map_or(DEFAULT_SHEET, |SheetId(i)| i);
    let c0 = rr.start.col.min(rr.end.col);
    let c1 = rr.start.col.max(rr.end.col);
    let r0 = rr.start.row.min(rr.end.row);
    let r1 = rr.start.row.max(rr.end.row);
    let mut buf = Vec::with_capacity(((r1 - r0 + 1) * (c1 - c0 + 1)) as usize);
    for r in r0..=r1 {
        for c in c0..=c1 {
            buf.push(cells.get(&(idx, c, r)).cloned().unwrap_or(Value::Blank));
        }
    }
    (
        Shape {
            rows: r1 - r0 + 1,
            cols: c1 - c0 + 1,
        },
        buf,
    )
}

/// Collect every resolved [`RangeRef`] the tree names (into `out`), walking the whole `Expr` and
/// mapping each range node's sheet NAME to a [`SheetId`] via `lookup` (mirroring the evaluator). A
/// range naming an unknown sheet resolves to `None` and is skipped — the evaluator returns `#REF!`
/// for it and never asks `range()` to read it.
fn collect_ranges(expr: &Expr, lookup: &impl Fn(&str) -> Option<SheetId>, out: &mut Vec<RangeRef>) {
    match expr {
        Expr::Range(rn) => {
            if let Some(rr) = rn.resolve(lookup) {
                out.push(rr);
            }
        }
        Expr::Unary(_, inner) | Expr::ImplicitIntersect(inner) | Expr::SpillRef(inner) => {
            collect_ranges(inner, lookup, out)
        }
        Expr::Binary(_, l, r) => {
            collect_ranges(l, lookup, out);
            collect_ranges(r, lookup, out);
        }
        Expr::Call(_, args) => {
            for a in args {
                collect_ranges(a, lookup, out);
            }
        }
        Expr::Lit(_) | Expr::Ref(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use charlie_ast::{ErrKind, eval, parse};

    #[test]
    fn resolves_cells_and_a_range_over_a_sub_rectangle() {
        // A 2x2 block at B2:C3, plus a stray cell, graded through a real parse+eval.
        let cells = vec![
            (None, 1, 1, Value::Number(1.0)), // B2
            (None, 2, 1, Value::Number(2.0)), // C2
            (None, 1, 2, Value::Number(3.0)), // B3
            (None, 2, 2, Value::Number(4.0)), // C3
        ];
        let expr = parse("=SUM(B2:C3)").unwrap();
        let r = StubResolver::build(&cells, &expr);
        assert_eq!(eval(&expr, &r), Value::Number(10.0));

        // A single-cell reference.
        let expr = parse("=C3").unwrap();
        let r = StubResolver::build(&cells, &expr);
        assert_eq!(eval(&expr, &r), Value::Number(4.0));

        // A missing cell resolves Blank (→ 0 in arithmetic).
        let expr = parse("=Z9+1").unwrap();
        let r = StubResolver::build(&cells, &expr);
        assert_eq!(eval(&expr, &r), Value::Number(1.0));
    }

    #[test]
    fn error_cells_propagate_through_a_range() {
        let cells = vec![
            (None, 0, 0, Value::Number(1.0)),
            (None, 0, 1, Value::Error(ErrKind::Div0)),
            (None, 0, 2, Value::Number(3.0)),
        ];
        let expr = parse("=SUM(A1:A3)").unwrap();
        let r = StubResolver::build(&cells, &expr);
        assert_eq!(eval(&expr, &r), Value::Error(ErrKind::Div0));
    }

    #[test]
    fn cross_sheet_refs_and_ranges_resolve_to_the_named_sheet() {
        // A default-sheet A1=1, plus a named `Data` sheet with A1..A3 = 10,20,30.
        let cells = vec![
            (None, 0, 0, Value::Number(1.0)),
            (Some("Data".to_string()), 0, 0, Value::Number(10.0)),
            (Some("Data".to_string()), 0, 1, Value::Number(20.0)),
            (Some("Data".to_string()), 0, 2, Value::Number(30.0)),
        ];
        // A single cross-sheet ref routes to Data (10), not the default sheet (1).
        let expr = parse("=Data!A1").unwrap();
        let r = StubResolver::build(&cells, &expr);
        assert_eq!(eval(&expr, &r), Value::Number(10.0));

        // A cross-sheet range sums Data's column.
        let expr = parse("=SUM(Data!A1:A3)").unwrap();
        let r = StubResolver::build(&cells, &expr);
        assert_eq!(eval(&expr, &r), Value::Number(60.0));

        // An unknown sheet is #REF!.
        let expr = parse("=Nope!A1").unwrap();
        let r = StubResolver::build(&cells, &expr);
        assert_eq!(eval(&expr, &r), Value::Error(ErrKind::Ref));
    }
}
