// Concern: the D* built-ins over a record block and a criteria block | Non-concern: the criteria grammar (criteria.rs owns it) | IO: (&mut EvalCtx, &[Expr]) -> Value
use super::*;

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
    // `block` guarantees `cells.len() == rows*cols` and `rows >= 1`, so the first `cols` cells are the header row.
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
    // An error condition cell propagates as the whole result; a BLANK one is a no-op left for `matches` to skip.
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
    /// OR across condition ROWS, AND within a row. A blank cell, or a column whose label named no
    /// database field, imposes no constraint.
    fn matches(&self, record: &[Value]) -> bool {
        // Row 0 holds the labels, so the condition rows start at 1.
        (1..self.rows).any(|r| {
            self.cond_cols.iter().all(|cc| {
                let cell = &self.cells[r * self.cols + cc.crit_col];
                match (cc.db_col, cell) {
                    (_, Value::Blank) | (None, _) => true,
                    (Some(dc), _) => match parse_db_criterion(cell) {
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

pub(crate) fn dsum(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    d_agg(ctx, args, DReduce::Sum, false)
}

pub(crate) fn daverage(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    d_agg(ctx, args, DReduce::Avg, false)
}

pub(crate) fn dcount(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    d_agg(ctx, args, DReduce::Count, true)
}

pub(crate) fn dcounta(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    d_agg(ctx, args, DReduce::CountA, true)
}

pub(crate) fn dmax(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    d_agg(ctx, args, DReduce::Max, false)
}

pub(crate) fn dmin(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    d_agg(ctx, args, DReduce::Min, false)
}

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
