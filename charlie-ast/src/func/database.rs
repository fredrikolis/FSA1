// Concern: the DATABASE worksheet functions (DSUM DAVERAGE DCOUNT DCOUNTA DGET DMAX DMIN) — the `D*` family, which filter a labelled `database` block (header row + records) by a `criteria` block (a label row + one-or-more condition rows, OR'd across rows and AND'd within a row) and reduce a chosen `field` column over the matching records, matching each condition cell with the DATABASE / advanced-filter criteria grammar (crate::criteria's `parse_db_criterion`: bare text is BEGINS-WITH, a leading `=` is exact) | Non-concern: the registry table + dispatch (func/mod.rs), the CRITERIA grammar itself (criteria.rs owns `Criterion`/`parse_db_criterion`), the `*IF(S)` criteria-aggregation family (criteria_agg.rs), and the shared `block`/`finite_or_num` helpers (func/helpers.rs) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
use super::*;

// Database family. Three arguments, positionally identical across the family:
//   * `database` — a rectangular block whose FIRST ROW is the column labels (headers) and whose
//     remaining rows are the records;
//   * `field` — which column to reduce, given as a 1-based column NUMBER, or a text/reference that
//     names a header (matched case-insensitively). For DCOUNT/DCOUNTA the field may be OMITTED (an
//     empty middle argument, which the parser hands us as a `Blank` literal), meaning "count every
//     matching record" rather than a specific column;
//   * `criteria` — a block whose first row is condition labels (each naming a database column) and
//     whose remaining rows are conditions: a record matches iff it satisfies AT LEAST ONE condition
//     row (OR across rows), and a condition row is satisfied iff EVERY non-blank cell in it matches
//     its column (AND within a row). Each condition cell is matched with the DATABASE criteria grammar
//     (`parse_db_criterion`): bare text is a BEGINS-WITH prefix (criterion `App` matches `Apple`),
//     numbers/operators/wildcards are as elsewhere, and a leading `=` forces exact. A blank condition
//     cell imposes no constraint.
//   KNOWN LIMITATIONS (recorded, defensible scope cuts for a fresh D* landing):
//     * A condition label that names no database column is ignored (accept-under-uncertainty; never a
//       false-reject). Excel instead treats such a label as COMPUTED CRITERIA — a per-record formula —
//       so ignoring it can OVER-accept records vs Excel. Computed criteria is a larger, separate
//       feature; not implemented here.
//     * A non-integer field NUMBER is truncated toward zero (5.9 ⇒ column 5); this is the likely Excel
//       behaviour but rests on hand verification (the `formulas` oracle lacks the D* family), not the
//       lib oracle — low confidence.
// SEMANTICS (an Excel-parity call worth a reviewer's eye): an error value in a MATCHING record's
// field cell propagates for the numeric reducers (DSUM/DAVERAGE/DMAX/DMIN) and for DGET (it is the
// value); the counts (DCOUNT/DCOUNTA) never propagate a data error — DCOUNT counts only numbers,
// DCOUNTA counts every non-blank cell (an error cell included). An error-valued CONDITION cell
// propagates as the whole function's result. DGET yields the single matching field value, `#VALUE!`
// when no record matches, and `#NUM!` when more than one does.

/// The reduction a database aggregation performs over the field column of the matching records.
#[derive(Clone, Copy)]
enum DReduce {
    Sum,
    Avg,
    Min,
    Max,
    /// Count the cells that hold a NUMBER (DCOUNT).
    Count,
    /// Count the NON-BLANK cells (DCOUNTA).
    CountA,
}

/// A materialized `database` argument: the header row and the record rows, all as scalar cells. The
/// column count is `headers.len()`, and every record has that same width.
struct Database {
    /// The header row — the first row of the block.
    headers: Vec<Value>,
    /// The records — every row after the header, each a slice of `headers.len()` cells, row-major.
    records: Vec<Vec<Value>>,
}

/// Materialize the `database` argument into its header row and records, or propagate its error.
fn database(ctx: &mut EvalCtx, e: &Expr) -> Result<Database, ErrKind> {
    let (_, cols, cells) = block(ctx, e)?;
    let cols = cols as usize;
    // `block` guarantees `cells.len() == rows*cols` and `rows >= 1`, so the first `cols` cells are the
    // header row and the rest are whole records.
    let headers = cells[..cols].to_vec();
    let records = cells[cols..]
        .chunks_exact(cols)
        .map(<[Value]>::to_vec)
        .collect();
    Ok(Database { headers, records })
}

/// Which field column a `D*` call reduces: a resolved 0-based column, or `All` (DCOUNT/DCOUNTA with an
/// omitted field — count every matching record rather than one column).
enum Field {
    Col(usize),
    All,
}

/// Resolve the `field` argument against the database headers. A 1-based NUMBER selects a column by
/// position (out of range ⇒ `#VALUE!`); any other value is coerced to text and matched against a
/// header case-insensitively (no match ⇒ `#VALUE!`). When `allow_all` (DCOUNT/DCOUNTA), a `Blank`
/// field means "count all matching records" (`Field::All`). An error propagates.
fn resolve_field(headers: &[Value], v: &Value, allow_all: bool) -> Result<Field, ErrKind> {
    match v {
        Value::Error(k) => Err(*k),
        Value::Blank if allow_all => Ok(Field::All),
        Value::Number(n) => {
            let idx = n.trunc();
            if idx >= 1.0 && (idx as usize) <= headers.len() {
                Ok(Field::Col(idx as usize - 1))
            } else {
                Err(ErrKind::Value)
            }
        }
        other => {
            let want = to_text(other)?;
            headers
                .iter()
                .position(|h| header_eq(h, &want))
                .map(Field::Col)
                .ok_or(ErrKind::Value)
        }
    }
}

/// A header cell equals a wanted field name iff their Excel text forms match case-insensitively (a
/// header that cannot be spelled as text — a genuine multi-cell array — simply never matches).
fn header_eq(header: &Value, want: &str) -> bool {
    to_text(header).is_ok_and(|t| t.eq_ignore_ascii_case(want))
}

/// One condition column of the criteria block: which database column it constrains (`None` ⇒ its
/// label names no database column, so it is ignored) paired with the criteria block column index.
struct CondCol {
    db_col: Option<usize>,
    crit_col: usize,
}

/// The compiled criteria: the condition columns (label→database-column mapping) and the block itself,
/// so a record can be tested row-by-row. Any error-valued condition cell has already propagated.
struct Criteria {
    cond_cols: Vec<CondCol>,
    rows: usize,
    cols: usize,
    cells: Vec<Value>,
}

/// Materialize and compile the `criteria` argument: map each condition column's label to a database
/// column, and eagerly reject an error-valued condition cell (an error criterion propagates). The
/// first block row is the labels; rows below are the OR-combined condition rows.
fn criteria(ctx: &mut EvalCtx, e: &Expr, db: &Database) -> Result<Criteria, ErrKind> {
    let (rows, cols, cells) = block(ctx, e)?;
    let (rows, cols) = (rows as usize, cols as usize);
    let cond_cols = (0..cols)
        .map(|c| CondCol {
            db_col: db.headers.iter().position(|h| header_matches(h, &cells[c])),
            crit_col: c,
        })
        .collect();
    // A condition cell that IS an error propagates as the whole result (Excel: an error criterion is
    // not swallowed). A blank cell is a no-op condition, so it is left for `matches` to skip.
    for cell in &cells[cols.min(cells.len())..] {
        if let Value::Error(k) = cell {
            return Err(*k);
        }
    }
    Ok(Criteria {
        cond_cols,
        rows,
        cols,
        cells,
    })
}

/// Two header cells name the same column iff their Excel text forms match case-insensitively.
fn header_matches(header: &Value, label: &Value) -> bool {
    match (to_text(header), to_text(label)) {
        (Ok(h), Ok(l)) => h.eq_ignore_ascii_case(&l),
        _ => false,
    }
}

impl Criteria {
    /// Whether a record satisfies the criteria: it matches iff at least one CONDITION ROW matches
    /// (OR across rows), and a condition row matches iff every non-blank cell in a mapped column
    /// matches that column's value under the DATABASE grammar (bare text ⇒ begins-with; `=` ⇒ exact),
    /// AND'd within the row. A blank condition cell, or a column whose label named no database field,
    /// imposes no constraint.
    fn matches(&self, record: &[Value]) -> bool {
        // Rows 1..self.rows are the OR-combined condition rows (row 0 is the labels).
        (1..self.rows).any(|r| {
            self.cond_cols.iter().all(|cc| {
                let cell = &self.cells[r * self.cols + cc.crit_col];
                match (cc.db_col, cell) {
                    // A blank condition cell or an unmapped column is no constraint.
                    (_, Value::Blank) | (None, _) => true,
                    // The DATABASE grammar (`parse_db_criterion`), NOT the `*IF(S)` grammar: a bare
                    // text criterion matches BEGINS-WITH (`App` selects `Apple`), and only a leading
                    // `=` forces exact. Numbers, operators, and wildcards are shared.
                    (Some(dc), _) => match parse_db_criterion(cell) {
                        // An error criterion was already rejected in `criteria`; treat a stray one as
                        // a non-match rather than panicking (it cannot occur here).
                        Ok(crit) => crit.matches(&record[dc]),
                        Err(_) => false,
                    },
                }
            })
        })
    }
}

/// Shared body of the aggregating database functions: filter records by the criteria and reduce the
/// `field` column of the matches. `allow_all` (DCOUNT/DCOUNTA) permits an omitted field, counting
/// every matching record.
fn d_agg(ctx: &mut EvalCtx, args: &[Expr], reduce: DReduce, allow_all: bool) -> Value {
    let db = match database(ctx, &args[0]) {
        Ok(d) => d,
        Err(k) => return Value::Error(k),
    };
    let field = match resolve_field(&db.headers, &scalarize(ctx.eval(&args[1])), allow_all) {
        Ok(f) => f,
        Err(k) => return Value::Error(k),
    };
    let crit = match criteria(ctx, &args[2], &db) {
        Ok(c) => c,
        Err(k) => return Value::Error(k),
    };
    let matching = db.records.iter().filter(|r| crit.matches(r));
    match field {
        // An omitted field (count family only) counts the matching records outright.
        Field::All => Value::Number(matching.count() as f64),
        Field::Col(col) => reduce_field(matching.map(|r| &r[col]), reduce),
    }
}

/// Reduce the field cells of the matching records. Numeric reducers propagate an error cell; the
/// counts never do (DCOUNT counts numbers, DCOUNTA counts every non-blank cell). `Avg` over no
/// numbers is `#DIV/0!`; `Min`/`Max` over no numbers is `0` (Excel's empty-database result).
fn reduce_field<'a>(cells: impl Iterator<Item = &'a Value>, reduce: DReduce) -> Value {
    let mut sum = 0.0;
    let mut count: u64 = 0;
    let mut nonblank: u64 = 0;
    let mut extreme: Option<f64> = None;
    for v in cells {
        if !matches!(v, Value::Blank) {
            nonblank += 1;
        }
        match v {
            Value::Error(k) => match reduce {
                // A count never propagates a data error; the numeric reducers do.
                DReduce::Count | DReduce::CountA => {}
                _ => return Value::Error(*k),
            },
            Value::Number(n) => {
                sum += n;
                count += 1;
                extreme = Some(match reduce {
                    DReduce::Min => extreme.map_or(*n, |e| e.min(*n)),
                    DReduce::Max => extreme.map_or(*n, |e| e.max(*n)),
                    _ => *n,
                });
            }
            _ => {}
        }
    }
    match reduce {
        DReduce::Sum => finite_or_num(sum),
        DReduce::Avg => {
            if count == 0 {
                Value::Error(ErrKind::Div0)
            } else {
                finite_or_num(sum / count as f64)
            }
        }
        DReduce::Min | DReduce::Max => Value::Number(extreme.unwrap_or(0.0)),
        DReduce::Count => Value::Number(count as f64),
        DReduce::CountA => Value::Number(nonblank as f64),
    }
}

/// `DSUM(database, field, criteria)` — total the numbers in `field` over the matching records.
pub(crate) fn dsum(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    d_agg(ctx, args, DReduce::Sum, false)
}

/// `DAVERAGE(database, field, criteria)` — mean of the matching numbers; no match is `#DIV/0!`.
pub(crate) fn daverage(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    d_agg(ctx, args, DReduce::Avg, false)
}

/// `DCOUNT(database, field, criteria)` — how many matching records hold a NUMBER in `field`; an
/// omitted field counts every matching record.
pub(crate) fn dcount(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    d_agg(ctx, args, DReduce::Count, true)
}

/// `DCOUNTA(database, field, criteria)` — how many matching records have a NON-BLANK `field`; an
/// omitted field counts every matching record.
pub(crate) fn dcounta(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    d_agg(ctx, args, DReduce::CountA, true)
}

/// `DMAX(database, field, criteria)` — largest matching number; no match is `0`.
pub(crate) fn dmax(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    d_agg(ctx, args, DReduce::Max, false)
}

/// `DMIN(database, field, criteria)` — smallest matching number; no match is `0`.
pub(crate) fn dmin(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    d_agg(ctx, args, DReduce::Min, false)
}

/// `DGET(database, field, criteria)` — the single `field` value of the one matching record. No match
/// is `#VALUE!`; more than one match is `#NUM!`. A matching blank field cell reads as `0` (Excel);
/// an error-valued field cell propagates.
pub(crate) fn dget(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let db = match database(ctx, &args[0]) {
        Ok(d) => d,
        Err(k) => return Value::Error(k),
    };
    let col = match resolve_field(&db.headers, &scalarize(ctx.eval(&args[1])), false) {
        Ok(Field::Col(c)) => c,
        Ok(Field::All) => return Value::Error(ErrKind::Value),
        Err(k) => return Value::Error(k),
    };
    let crit = match criteria(ctx, &args[2], &db) {
        Ok(c) => c,
        Err(k) => return Value::Error(k),
    };
    let mut hits = db.records.iter().filter(|r| crit.matches(r));
    match (hits.next(), hits.next()) {
        (None, _) => Value::Error(ErrKind::Value),
        (Some(_), Some(_)) => Value::Error(ErrKind::Num),
        (Some(rec), None) => match &rec[col] {
            // Excel reads a blank returned cell as 0; every other value is returned as-is.
            Value::Blank => Value::Number(0.0),
            other => other.clone(),
        },
    }
}
