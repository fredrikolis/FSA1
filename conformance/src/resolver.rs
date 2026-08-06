// Concern: resolves a fixture's context cells for the evaluator | Non-concern: the formula language, any real backing store | IO: (cells, Expr) -> StubResolver

use std::collections::HashMap;

use fsa1_ast::{ArrayView, CellRef, Expr, RangeRef, Resolver, Shape, SheetId, Value};

const DEFAULT_SHEET: u32 = 0;
const DEFAULT_SHEET_NAME: &str = "Sheet1";

pub struct StubResolver {
    cells: HashMap<(u32, u32, u32), Value>,
    ranges: HashMap<RangeRef, (Shape, Vec<Value>)>,
    sheet_ids: HashMap<String, u32>,
    /// Owns the cell an unregistered `range()` hands back — an [`ArrayView`] must borrow from somewhere.
    fallback: Vec<Value>,
}

impl StubResolver {
    pub fn build(cells: &[(Option<String>, u32, u32, Value)], expr: &Expr) -> StubResolver {
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
            None => ArrayView {
                shape: Shape { rows: 1, cols: 1 },
                cells: &self.fallback,
            },
        }
    }

    fn sheet_id(&self, name: &str) -> Option<SheetId> {
        self.sheet_ids.get(name).copied().map(SheetId)
    }

    fn now_serial(&self) -> f64 {
        fsa1_ast::PINNED_NOW_SERIAL
    }
}

fn materialize(rr: RangeRef, cells: &HashMap<(u32, u32, u32), Value>) -> (Shape, Vec<Value>) {
    let rr = rr.normalized();
    let idx = rr.start.sheet.map_or(DEFAULT_SHEET, |SheetId(i)| i);
    let (c0, c1, r0, r1) = (rr.start.col, rr.end.col, rr.start.row, rr.end.row);
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
    use fsa1_ast::{ErrKind, eval, parse};

    #[test]
    fn resolves_cells_and_a_range_over_a_sub_rectangle() {
        let cells = vec![
            (None, 1, 1, Value::Number(1.0)),
            (None, 2, 1, Value::Number(2.0)),
            (None, 1, 2, Value::Number(3.0)),
            (None, 2, 2, Value::Number(4.0)),
        ];
        let expr = parse("=SUM(B2:C3)").unwrap();
        let r = StubResolver::build(&cells, &expr);
        assert_eq!(eval(&expr, &r), Value::Number(10.0));

        let expr = parse("=C3").unwrap();
        let r = StubResolver::build(&cells, &expr);
        assert_eq!(eval(&expr, &r), Value::Number(4.0));

        let expr = parse("=Z9+1").unwrap();
        let r = StubResolver::build(&cells, &expr);
        assert_eq!(
            eval(&expr, &r),
            Value::Number(1.0),
            "a missing cell resolves Blank, which is 0 in arithmetic"
        );
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
        let cells = vec![
            (None, 0, 0, Value::Number(1.0)),
            (Some("Data".to_string()), 0, 0, Value::Number(10.0)),
            (Some("Data".to_string()), 0, 1, Value::Number(20.0)),
            (Some("Data".to_string()), 0, 2, Value::Number(30.0)),
        ];
        let expr = parse("=Data!A1").unwrap();
        let r = StubResolver::build(&cells, &expr);
        assert_eq!(
            eval(&expr, &r),
            Value::Number(10.0),
            "a cross-sheet ref routes to Data, not the default sheet"
        );

        let expr = parse("=SUM(Data!A1:A3)").unwrap();
        let r = StubResolver::build(&cells, &expr);
        assert_eq!(eval(&expr, &r), Value::Number(60.0));

        let expr = parse("=Nope!A1").unwrap();
        let r = StubResolver::build(&cells, &expr);
        assert_eq!(
            eval(&expr, &r),
            Value::Error(ErrKind::Ref),
            "an unknown sheet name is #REF!"
        );
    }
}
