// Concern: the bivariate regression and correlation built-ins | Non-concern: univariate statistics, distributions | IO: (&mut EvalCtx, &[Expr]) -> Value
use super::*;

/// Gather the numeric `(x, y)` pairs of two array/range arguments in parallel: the two blocks must
/// hold the SAME number of cells (else `#N/A`, Excel); a pair is kept only when BOTH cells are
/// numbers (a text/blank/logical in either drops the pair); an error in either propagates (leftmost).
fn collect_pairs(ctx: &mut EvalCtx, ex: &Expr, ey: &Expr) -> Result<Vec<(f64, f64)>, ErrKind> {
    let (_, _, xs) = block(ctx, ex)?;
    let (_, _, ys) = block(ctx, ey)?;
    if xs.len() != ys.len() {
        return Err(ErrKind::Na);
    }
    let mut out = Vec::new();
    for (a, b) in xs.iter().zip(ys.iter()) {
        if let Value::Error(k) = a {
            return Err(*k);
        }
        if let Value::Error(k) = b {
            return Err(*k);
        }
        if let (Value::Number(x), Value::Number(y)) = (a, b) {
            out.push((*x, *y));
        }
    }
    Ok(out)
}

/// The centered sums of a paired sample: `n`, the means, and `Sxx`/`Syy`/`Sxy`
/// (`Σ(x−x̄)²`, `Σ(y−ȳ)²`, `Σ(x−x̄)(y−ȳ)`) — the shared kernel every regression coefficient reads.
/// `None` on an empty sample.
struct Sums {
    n: usize,
    mx: f64,
    my: f64,
    sxx: f64,
    syy: f64,
    sxy: f64,
}

fn sums(pairs: &[(f64, f64)]) -> Option<Sums> {
    let n = pairs.len();
    if n == 0 {
        return None;
    }
    let nf = n as f64;
    let mx = pairs.iter().map(|p| p.0).sum::<f64>() / nf;
    let my = pairs.iter().map(|p| p.1).sum::<f64>() / nf;
    let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
    for &(x, y) in pairs {
        let (dx, dy) = (x - mx, y - my);
        sxx += dx * dx;
        syy += dy * dy;
        sxy += dx * dy;
    }
    Some(Sums {
        n,
        mx,
        my,
        sxx,
        syy,
        sxy,
    })
}

pub(crate) fn correl_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let pairs = match collect_pairs(ctx, &args[0], &args[1]) {
        Ok(p) => p,
        Err(k) => return Value::Error(k),
    };
    match sums(&pairs) {
        Some(s) if s.sxx > 0.0 && s.syy > 0.0 => finite_or_num(s.sxy / (s.sxx * s.syy).sqrt()),
        _ => Value::Error(ErrKind::Div0),
    }
}

pub(crate) fn rsq_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let pairs = match collect_pairs(ctx, &args[0], &args[1]) {
        Ok(p) => p,
        Err(k) => return Value::Error(k),
    };
    match sums(&pairs) {
        Some(s) if s.sxx > 0.0 && s.syy > 0.0 => finite_or_num(s.sxy * s.sxy / (s.sxx * s.syy)),
        _ => Value::Error(ErrKind::Div0),
    }
}

pub(crate) fn covariance_p_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let pairs = match collect_pairs(ctx, &args[0], &args[1]) {
        Ok(p) => p,
        Err(k) => return Value::Error(k),
    };
    match sums(&pairs) {
        Some(s) => finite_or_num(s.sxy / s.n as f64),
        None => Value::Error(ErrKind::Div0),
    }
}

pub(crate) fn covariance_s_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let pairs = match collect_pairs(ctx, &args[0], &args[1]) {
        Ok(p) => p,
        Err(k) => return Value::Error(k),
    };
    match sums(&pairs) {
        Some(s) if s.n >= 2 => finite_or_num(s.sxy / (s.n as f64 - 1.0)),
        _ => Value::Error(ErrKind::Div0),
    }
}

pub(crate) fn slope_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    // x = args[1] (known_xs), y = args[0] (known_ys).
    let pairs = match collect_pairs(ctx, &args[1], &args[0]) {
        Ok(p) => p,
        Err(k) => return Value::Error(k),
    };
    match sums(&pairs) {
        Some(s) if s.sxx > 0.0 => finite_or_num(s.sxy / s.sxx),
        _ => Value::Error(ErrKind::Div0),
    }
}

pub(crate) fn intercept_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let pairs = match collect_pairs(ctx, &args[1], &args[0]) {
        Ok(p) => p,
        Err(k) => return Value::Error(k),
    };
    match sums(&pairs) {
        Some(s) if s.sxx > 0.0 => finite_or_num(s.my - (s.sxy / s.sxx) * s.mx),
        _ => Value::Error(ErrKind::Div0),
    }
}

pub(crate) fn forecast_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let x0 = match coerce_num(&scalarize(ctx.eval(&args[0]))) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    // x = args[2] (known_xs), y = args[1] (known_ys).
    let pairs = match collect_pairs(ctx, &args[2], &args[1]) {
        Ok(p) => p,
        Err(k) => return Value::Error(k),
    };
    match sums(&pairs) {
        Some(s) if s.sxx > 0.0 => {
            let slope = s.sxy / s.sxx;
            finite_or_num((s.my - slope * s.mx) + slope * x0)
        }
        _ => Value::Error(ErrKind::Div0),
    }
}

/// Ordered numeric cells of an array/range argument (an error propagates; non-numbers are dropped),
/// used by TREND to read `known_ys`/`known_xs` in position order.
fn ordered_numbers(ctx: &mut EvalCtx, e: &Expr) -> Result<Vec<f64>, ErrKind> {
    let (_, _, cells) = block(ctx, e)?;
    let mut out = Vec::new();
    for c in &cells {
        match c {
            Value::Error(k) => return Err(*k),
            Value::Number(n) => out.push(*n),
            _ => {}
        }
    }
    Ok(out)
}

pub(crate) fn trend_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let ys = match ordered_numbers(ctx, &args[0]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    if ys.is_empty() {
        return Value::Error(ErrKind::Ref);
    }
    // known_xs: arg 1 if present, else the counter 1..=n.
    let xs = if args.len() >= 2 {
        match ordered_numbers(ctx, &args[1]) {
            Ok(n) => n,
            Err(k) => return Value::Error(k),
        }
    } else {
        (1..=ys.len()).map(|i| i as f64).collect()
    };
    if xs.len() != ys.len() {
        return Value::Error(ErrKind::Ref);
    }
    let use_const = match opt_bool(ctx, args, 3, true) {
        Ok(b) => b,
        Err(k) => return Value::Error(k),
    };
    let (slope, intercept) = fit_line(&xs, &ys, use_const);

    // new_xs: arg 2 if present (keep its shape), else the known xs shaped like known_ys.
    if args.len() >= 3 {
        let (nr, nc, cells) = match block(ctx, &args[2]) {
            Ok(t) => t,
            Err(k) => return Value::Error(k),
        };
        let out: Vec<Value> = cells
            .iter()
            .map(|c| match c {
                Value::Error(k) => Value::Error(*k),
                Value::Number(x) => finite_or_num(intercept + slope * x),
                _ => Value::Error(ErrKind::Value),
            })
            .collect();
        Value::Array(Shape { rows: nr, cols: nc }, out)
    } else {
        let (yr, yc, _) = match block(ctx, &args[0]) {
            Ok(t) => t,
            Err(k) => return Value::Error(k),
        };
        let out: Vec<Value> = xs
            .iter()
            .map(|x| finite_or_num(intercept + slope * x))
            .collect();
        // Preserve the known_ys shape when it is a clean numeric block; else a column.
        let (rows, cols) = if (yr * yc) as usize == out.len() {
            (yr, yc)
        } else {
            (out.len() as u32, 1)
        };
        Value::Array(Shape { rows, cols }, out)
    }
}

/// Fit `y = intercept + slope·x` by least squares. `use_const` false forces `intercept = 0`
/// (regression through the origin: `slope = Σxy / Σx²`). A zero denominator yields a non-finite
/// slope, which the caller demotes to `#NUM!` per element via [`finite_or_num`].
fn fit_line(xs: &[f64], ys: &[f64], use_const: bool) -> (f64, f64) {
    if use_const {
        let n = xs.len() as f64;
        let mx = xs.iter().sum::<f64>() / n;
        let my = ys.iter().sum::<f64>() / n;
        let (mut sxx, mut sxy) = (0.0, 0.0);
        for (&x, &y) in xs.iter().zip(ys.iter()) {
            sxx += (x - mx) * (x - mx);
            sxy += (x - mx) * (y - my);
        }
        let slope = sxy / sxx;
        (slope, my - slope * mx)
    } else {
        let (mut sxx, mut sxy) = (0.0, 0.0);
        for (&x, &y) in xs.iter().zip(ys.iter()) {
            sxx += x * x;
            sxy += x * y;
        }
        (sxy / sxx, 0.0)
    }
}
