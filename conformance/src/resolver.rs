// Concern: the deterministic STUB Resolver built from a fixture's input context — a `HashMap`-backed cell store (for single `Ref` reads) plus a per-range materialized buffer (so `range()` can hand back a BORROWED `ArrayView` over an owned contiguous block, honoring the resolver contract with no `unsafe`); the ranges to materialize are collected from the formula's own `Expr`, so every `Range` node the evaluator asks for is pre-present | Non-concern: the formula LANGUAGE (charlie-ast owns parse/eval) and the filesystem model (charlie-model owns the real fs-backed Resolver) — this is a test double for grading only | IO: (a fixture's cells + its parsed `Expr`) -> a `charlie_ast::Resolver`
//! The grading stub Resolver. It mirrors the shape charlie-ast's own `#[cfg(test)]` grid uses (borrow
//! a contiguous window out of an owned buffer) but generalizes to arbitrary sub-rectangles: rather
//! than materialize whole rows, it pre-materializes exactly the [`RangeRef`]s the parsed formula
//! names, keyed by the range, so each `range()` call is a borrow into a buffer prepared up front.

use std::collections::HashMap;

use charlie_ast::{ArrayView, CellRef, Expr, RangeRef, Resolver, Shape, Value};

/// A context-backed resolver: single cells in a map, each referenced range pre-materialized.
pub struct StubResolver {
    cells: HashMap<(u32, u32), Value>,
    ranges: HashMap<RangeRef, (Shape, Vec<Value>)>,
    /// A persistent one-cell buffer so a defensively-unexpected `range()` can still return a valid
    /// borrowed view (never reached in practice — every range is collected from the same `Expr`).
    fallback: Vec<Value>,
}

impl StubResolver {
    /// Build a resolver from a fixture's `(col,row,value)` cells and its parsed `expr`. Every
    /// [`RangeRef`] in `expr` is materialized row-major from the cell store (missing cells → `Blank`).
    pub fn build(cells: &[(u32, u32, Value)], expr: &Expr) -> StubResolver {
        let cell_map: HashMap<(u32, u32), Value> = cells
            .iter()
            .map(|(c, r, v)| ((*c, *r), v.clone()))
            .collect();

        let mut ranges = HashMap::new();
        let mut seen = Vec::new();
        collect_ranges(expr, &mut seen);
        for rr in seen {
            ranges
                .entry(rr)
                .or_insert_with(|| materialize(rr, &cell_map));
        }

        StubResolver {
            cells: cell_map,
            ranges,
            fallback: vec![Value::Blank],
        }
    }
}

impl Resolver for StubResolver {
    fn value(&self, cell: CellRef) -> Value {
        self.cells
            .get(&(cell.col, cell.row))
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

    fn sheet_id(&self, _name: &str) -> Option<charlie_ast::SheetId> {
        // Cross-sheet references are a parse-time refusal in charlie-ast, so a graded formula never
        // resolves a sheet name; there are no named sheets in a fixture context.
        None
    }
}

/// Materialize a rectangular range into a row-major contiguous buffer from the cell store.
fn materialize(rr: RangeRef, cells: &HashMap<(u32, u32), Value>) -> (Shape, Vec<Value>) {
    let c0 = rr.start.col.min(rr.end.col);
    let c1 = rr.start.col.max(rr.end.col);
    let r0 = rr.start.row.min(rr.end.row);
    let r1 = rr.start.row.max(rr.end.row);
    let mut buf = Vec::with_capacity(((r1 - r0 + 1) * (c1 - c0 + 1)) as usize);
    for r in r0..=r1 {
        for c in c0..=c1 {
            buf.push(cells.get(&(c, r)).cloned().unwrap_or(Value::Blank));
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

/// Collect every [`RangeRef`] the tree names (into `out`), walking the whole `Expr`.
fn collect_ranges(expr: &Expr, out: &mut Vec<RangeRef>) {
    match expr {
        Expr::Range(rr) => out.push(*rr),
        Expr::Unary(_, inner) | Expr::ImplicitIntersect(inner) | Expr::SpillRef(inner) => {
            collect_ranges(inner, out)
        }
        Expr::Binary(_, l, r) => {
            collect_ranges(l, out);
            collect_ranges(r, out);
        }
        Expr::Call(_, args) => {
            for a in args {
                collect_ranges(a, out);
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
            (1, 1, Value::Number(1.0)), // B2
            (2, 1, Value::Number(2.0)), // C2
            (1, 2, Value::Number(3.0)), // B3
            (2, 2, Value::Number(4.0)), // C3
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
            (0, 0, Value::Number(1.0)),
            (0, 1, Value::Error(ErrKind::Div0)),
            (0, 2, Value::Number(3.0)),
        ];
        let expr = parse("=SUM(A1:A3)").unwrap();
        let r = StubResolver::build(&cells, &expr);
        assert_eq!(eval(&expr, &r), Value::Error(ErrKind::Div0));
    }
}
