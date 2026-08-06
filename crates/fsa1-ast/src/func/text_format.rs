// Concern: renders a value through an Excel number-format code | Non-concern: the string built-ins, General formatting (eval.rs owns it) | IO: (&Value, &str) -> Value
use super::*;

// ============================ TEXT / FIXED / DOLLAR entry points ============================

pub(crate) fn text_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let value = scalarize(ctx.eval(&args[0]));
    if let Value::Error(k) = value {
        return Value::Error(k);
    }
    let fmt = match arg_text(ctx, &args[1]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    render_text_format(&value, &fmt)
}

/// Render one (already error-free, scalar) value through a format code. The shared core of `text_fn`,
/// factored so the parse-time gate and eval agree on the supported subset. `pub(crate)` so the
/// crate-public [`crate::func::format_value`] façade can expose it to fsa1-model's format-aware
/// render (which passes only the quote-free canonical codes, never a color/condition bracket).
pub(crate) fn render_text_format(value: &Value, fmt: &str) -> Value {
    if fmt.eq_ignore_ascii_case("General") {
        return match to_text(value) {
            Ok(s) => Value::Text(s),
            Err(k) => Value::Error(k),
        };
    }
    if format_unsupported(fmt) {
        return Value::Error(ErrKind::Value);
    }
    let sections = split_sections(fmt);
    if let Value::Text(s) = value {
        return match text_section_index(&sections) {
            Some(i) => Value::Text(render_text_section(&sections[i], s)),
            None => match coerce_num(value) {
                Ok(n) => render_numeric(n, &sections),
                Err(_) => Value::Text(s.clone()),
            },
        };
    }
    match coerce_num(value) {
        Ok(n) => render_numeric(n, &sections),
        Err(k) => Value::Error(k),
    }
}

pub(crate) fn fixed_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let n = match one_num(ctx, &args[0]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let decimals = match opt_num(ctx, args, 1, 2.0) {
        Ok(n) => n.trunc() as i64,
        Err(k) => return Value::Error(k),
    };
    if decimals > MAX_FIXED_DECIMALS {
        return Value::Error(ErrKind::Value);
    }
    let no_commas = match opt_bool(ctx, args, 2, false) {
        Ok(b) => b,
        Err(k) => return Value::Error(k),
    };
    let (value, dec) = round_for_decimals(n, decimals);
    let (neg, mut int_d, frac_d) = split_scaled(value, dec);
    if !no_commas {
        int_d = group_thousands(&int_d);
    }
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    out.push_str(&int_d);
    if dec > 0 {
        out.push('.');
        out.push_str(&frac_d);
    }
    Value::Text(out)
}

pub(crate) fn dollar_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let n = match one_num(ctx, &args[0]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let decimals = match opt_num(ctx, args, 1, 2.0) {
        Ok(n) => n.trunc() as i64,
        Err(k) => return Value::Error(k),
    };
    if decimals > MAX_FIXED_DECIMALS {
        return Value::Error(ErrKind::Value);
    }
    let (value, dec) = round_for_decimals(n, decimals);
    let (neg, int_d, frac_d) = split_scaled(value, dec);
    let mut body = String::from("$");
    body.push_str(&group_thousands(&int_d));
    if dec > 0 {
        body.push('.');
        body.push_str(&frac_d);
    }
    Value::Text(if neg { format!("({body})") } else { body })
}

/// Excel caps `FIXED`/`DOLLAR`'s fractional-place count at 127; a larger `decimals` yields `#VALUE!`
/// (verified against the `formulas` oracle: `FIXED(5,127)="5.000…"` with 127 zeros, `FIXED(5,128)`
/// and `FIXED(0,300)` both `#VALUE!`). The ceiling also bounds [`split_scaled`]'s digit buffer, so an
/// arbitrary user `decimals` can no longer drive its zero-padding into an O(n^2)/overflow blowup.
const MAX_FIXED_DECIMALS: i64 = 127;

/// A NEGATIVE `decimals` rounds to the nearest `10^|decimals|` here and renders 0 places. The
/// exponent is clamped so `10^|decimals|` never overflows to `inf` and poison the rounding to `NaN`.
fn round_for_decimals(n: f64, decimals: i64) -> (f64, usize) {
    if decimals >= 0 {
        (n, decimals as usize)
    } else {
        let f = 10f64.powi((-decimals).min(308) as i32);
        ((n / f).round() * f, 0)
    }
}

// ============================ format-code subset gate ============================

/// The ONE source both the parse-time gate and eval consult, so what parses and what renders can
/// never drift. Unsupported means 5+ sections, or a bracket that is not an elapsed-time token.
fn format_unsupported(fmt: &str) -> bool {
    split_sections(fmt).len() > 4 || !brackets_ok(fmt)
}

/// Whether every bracketed group in `fmt` is a recognised elapsed-time token (`[h]`/`[m]`/`[s]` runs).
/// Skips quoted / backslash-escaped content (a `[` inside `"…"` is a literal, not a group).
fn brackets_ok(fmt: &str) -> bool {
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                for q in chars.by_ref() {
                    if q == '"' {
                        break;
                    }
                }
            }
            '\\' => {
                chars.next();
            }
            '[' => {
                let mut inner = String::new();
                let mut closed = false;
                for q in chars.by_ref() {
                    if q == ']' {
                        closed = true;
                        break;
                    }
                    inner.push(q);
                }
                if !closed
                    || inner.is_empty()
                    || !inner
                        .chars()
                        .all(|c| matches!(c.to_ascii_lowercase(), 'h' | 'm' | 's'))
                {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

/// Split a format code into its `;`-separated sections, honouring quotes (`"a;b"` is one literal) and
/// backslash escapes (`\;` is a literal `;`). Always returns at least one section.
fn split_sections(fmt: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut cur = String::new();
    let mut chars = fmt.chars();
    let mut in_quote = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_quote = !in_quote;
                cur.push(c);
            }
            '\\' => {
                cur.push(c);
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            ';' if !in_quote => sections.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    sections.push(cur);
    sections
}

// ============================ section selection + numeric render ============================

/// From the SECOND section on the value renders in MAGNITUDE — the section supplies its own sign or
/// parentheses — never an automatic minus. An empty chosen section renders as `""`.
fn render_numeric(n: f64, sections: &[String]) -> Value {
    let len = sections.len();
    let (idx, prepend_minus) = if n < 0.0 {
        if len >= 2 { (1, false) } else { (0, true) }
    } else if n == 0.0 && len >= 3 {
        (2, false)
    } else {
        (0, false)
    };
    let section = &sections[idx];
    if section.is_empty() {
        return Value::Text(String::new());
    }
    if is_datetime_section(section) {
        return match render_datetime(section, n) {
            Ok(s) => Value::Text(s),
            Err(k) => Value::Error(k),
        };
    }
    match render_number(section, n.abs(), prepend_minus) {
        Ok(s) => Value::Text(s),
        Err(k) => Value::Error(k),
    }
}

/// A four-section code's 4th section is the text section even without an `@`; otherwise it is the
/// first section carrying an `@`, wherever it sits. `None` passes the text through unchanged.
fn text_section_index(sections: &[String]) -> Option<usize> {
    if sections.len() >= 4 {
        return Some(3);
    }
    sections.iter().position(|s| has_text_placeholder(s))
}

/// Whether a section carries an `@` text placeholder — an `@` outside a quoted run / backslash escape
/// (a `"@"` literal or `\@` is NOT a placeholder).
fn has_text_placeholder(section: &str) -> bool {
    let mut chars = section.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                for q in chars.by_ref() {
                    if q == '"' {
                        break;
                    }
                }
            }
            '\\' => {
                chars.next();
            }
            '@' => return true,
            _ => {}
        }
    }
    false
}

/// Render the text section for a text value: `@` is the input text, `"…"`/`\x` are literals, and
/// every other character is a literal (so `0;;;X` shows `X` for a text value).
fn render_text_section(section: &str, text: &str) -> String {
    let mut out = String::new();
    let mut chars = section.chars();
    while let Some(c) = chars.next() {
        match c {
            '@' => out.push_str(text),
            '"' => {
                for q in chars.by_ref() {
                    if q == '"' {
                        break;
                    }
                    out.push(q);
                }
            }
            '\\' => {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            }
            other => out.push(other),
        }
    }
    out
}

// ============================ number section tokenizer + renderer ============================

/// A token of a NUMBER-format section.
enum Elem {
    /// A digit placeholder: `0` (always a digit), `#` (a digit iff significant), `?` (digit or space).
    Digit(char),
    /// The decimal point `.`.
    Point,
    /// A raw `,` — a grouping separator (between digits) or a `/1000` scaler (trailing), per position.
    Comma,
    /// A `%` — scales the value by 100 and prints literally.
    Percent,
    /// A `/` — introduces the denominator of a fraction.
    Slash,
    /// A scientific-notation marker: `true` for `E+` (always-signed exponent), `false` for `E-`.
    Exp(bool),
    /// Literal text to emit verbatim (a quoted run, an escape, currency, punctuation, spaces).
    Lit(String),
}

/// Tokenize a number-format section into [`Elem`]s: quoted runs / `\x` escapes / `_x` (a space) / `*x`
/// (nothing, in the column-less TEXT context) become literals; `E+`/`E-` become [`Elem::Exp`]; the
/// placeholders/`.`/`,`/`%`/`/` become their own tokens; anything else is a one-char literal.
fn tokenize_number(section: &str) -> Vec<Elem> {
    let mut out = Vec::new();
    let mut chars = section.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '0' | '#' | '?' => out.push(Elem::Digit(c)),
            '.' => out.push(Elem::Point),
            ',' => out.push(Elem::Comma),
            '%' => out.push(Elem::Percent),
            '/' => out.push(Elem::Slash),
            'E' | 'e' => match chars.peek() {
                Some('+') => {
                    chars.next();
                    out.push(Elem::Exp(true));
                }
                Some('-') => {
                    chars.next();
                    out.push(Elem::Exp(false));
                }
                _ => out.push(Elem::Lit(c.to_string())),
            },
            '"' => {
                let mut lit = String::new();
                for q in chars.by_ref() {
                    if q == '"' {
                        break;
                    }
                    lit.push(q);
                }
                out.push(Elem::Lit(lit));
            }
            '\\' => {
                if let Some(n) = chars.next() {
                    out.push(Elem::Lit(n.to_string()));
                }
            }
            '_' => {
                chars.next();
                out.push(Elem::Lit(" ".to_string()));
            }
            '*' => {
                chars.next();
                out.push(Elem::Lit(String::new()));
            }
            other => out.push(Elem::Lit(other.to_string())),
        }
    }
    out
}

/// Render a NUMBER-format section for a non-negative `value`, prepending `-` when `prepend_minus`.
/// Dispatches to the scientific / fraction / standard-decimal sub-renderers by which special tokens
/// the section carries.
fn render_number(section: &str, value: f64, prepend_minus: bool) -> Result<String, ErrKind> {
    let elems = tokenize_number(section);
    let has_exp = elems.iter().any(|e| matches!(e, Elem::Exp(_)));
    let has_slash = elems.iter().any(|e| matches!(e, Elem::Slash));
    let sign = if prepend_minus { "-" } else { "" };
    if has_exp {
        return Ok(format!("{sign}{}", render_scientific(&elems, value)?));
    }
    if has_slash {
        return Ok(format!("{sign}{}", render_fraction(&elems, value)));
    }
    Ok(format!("{sign}{}", render_decimal(&elems, value)))
}

/// The standard fixed/grouped/percent decimal path. Scans placeholders to derive integer/fraction
/// widths, grouping, and the `%`/trailing-`,` scale factors; rounds the scaled magnitude; then emits
/// the section with the digit block dropped in at the placeholders' position (currency/punctuation
/// literals kept in place).
fn render_decimal(elems: &[Elem], value: f64) -> String {
    let last_digit = elems.iter().rposition(|e| matches!(e, Elem::Digit(_)));
    let point_at = elems.iter().position(|e| matches!(e, Elem::Point));
    // Integer placeholders (before the point) and fraction placeholders (after it).
    let mut int_ph: Vec<char> = Vec::new();
    let mut frac_ph: Vec<char> = Vec::new();
    for (i, e) in elems.iter().enumerate() {
        if let Elem::Digit(c) = e {
            match point_at {
                Some(p) if i > p => frac_ph.push(*c),
                _ => int_ph.push(*c),
            }
        }
    }
    // A `,` between two integer digits GROUPS; one after the last digit placeholder SCALES by 1/1000.
    let mut grouping = false;
    let mut scale_commas = 0i32;
    let mut percent = 0i32;
    for (i, e) in elems.iter().enumerate() {
        match e {
            Elem::Comma => match last_digit {
                Some(ld) if i > ld => scale_commas += 1,
                Some(_) => {
                    let before = elems[..i].iter().any(|e| matches!(e, Elem::Digit(_)));
                    let int_end = point_at.unwrap_or(elems.len());
                    let after = i < int_end
                        && elems[i + 1..int_end]
                            .iter()
                            .any(|e| matches!(e, Elem::Digit(_)));
                    if before && after {
                        grouping = true;
                    }
                }
                None => {}
            },
            Elem::Percent => percent += 1,
            _ => {}
        }
    }
    let scaled = value * 10f64.powi(2 * percent) / 1000f64.powi(scale_commas);
    let decimals = frac_ph.len();
    let (_, int_digits, frac_digits) = split_scaled(scaled, decimals);
    let int_str = place_integer(&int_digits, &int_ph, grouping);
    let frac_str = place_fraction(&frac_digits, &frac_ph);

    // The WHOLE integer block lands at the FIRST integer placeholder; later digit tokens are absorbed.
    let mut out = String::new();
    let mut int_done = false;
    for (i, e) in elems.iter().enumerate() {
        match e {
            Elem::Digit(_) => match point_at {
                Some(p) if i > p => {} // fraction digit — emitted with the point
                _ => {
                    if !int_done {
                        out.push_str(&int_str);
                        int_done = true;
                    }
                }
            },
            Elem::Point => {
                if !frac_str.is_empty() {
                    out.push('.');
                    out.push_str(&frac_str);
                }
            }
            Elem::Percent => out.push('%'),
            Elem::Comma => {}
            Elem::Lit(s) => out.push_str(s),
            Elem::Exp(_) | Elem::Slash => {}
        }
    }
    out
}

/// Place the integer digits against their placeholders: pad with leading `0` up to the count of `0`/`?`
/// placeholders, drop a lone `0` when no placeholder forces it (`#`-only), and apply thousands grouping.
fn place_integer(int_digits: &str, int_ph: &[char], grouping: bool) -> String {
    let min = int_ph.iter().filter(|c| **c == '0' || **c == '?').count();
    let mut s = if int_digits == "0" && min == 0 {
        String::new()
    } else {
        int_digits.to_string()
    };
    while s.len() < min {
        s.insert(0, '0');
    }
    if grouping {
        s = group_thousands(&s);
    }
    s
}

/// Place the fraction digits against their placeholders: a `0` placeholder always shows its digit; a
/// `#` past the last significant position is dropped; a `?` past it becomes a space (alignment).
fn place_fraction(frac_digits: &str, frac_ph: &[char]) -> String {
    let digits: Vec<char> = frac_digits.chars().collect();
    // The last position that must appear: any `0` placeholder, or any non-zero digit.
    let mut last_shown: i64 = -1;
    for (i, ph) in frac_ph.iter().enumerate() {
        if *ph == '0' || digits.get(i).is_some_and(|d| *d != '0') {
            last_shown = i as i64;
        }
    }
    let mut out = String::new();
    for (i, ph) in frac_ph.iter().enumerate() {
        if (i as i64) <= last_shown {
            out.push(*digits.get(i).unwrap_or(&'0'));
        } else if *ph == '?' {
            out.push(' ');
        }
    }
    out
}

/// Render a scientific-notation section (`0.00E+00`): normalize `value` so its mantissa carries the
/// integer-placeholder count of leading digits, round the mantissa to the fraction width, format the
/// exponent to its placeholder width with a leading sign (`E+` always, `E-` only for a negative).
fn render_scientific(elems: &[Elem], value: f64) -> Result<String, ErrKind> {
    let exp_at = elems.iter().position(|e| matches!(e, Elem::Exp(_)));
    let exp_at = exp_at.ok_or(ErrKind::Value)?;
    let always_sign = matches!(elems[exp_at], Elem::Exp(true));
    let point_at = elems[..exp_at]
        .iter()
        .position(|e| matches!(e, Elem::Point));
    let mut int_count = 0usize;
    let mut frac_count = 0usize;
    for (i, e) in elems[..exp_at].iter().enumerate() {
        if let Elem::Digit(_) = e {
            match point_at {
                Some(p) if i > p => frac_count += 1,
                _ => int_count += 1,
            }
        }
    }
    let int_count = int_count.max(1);
    let exp_width = elems[exp_at + 1..]
        .iter()
        .filter(|e| matches!(e, Elem::Digit(_)))
        .count()
        .max(1);

    let (mantissa, exponent) = normalize_scientific(value, int_count, frac_count);
    let (_, int_d, frac_d) = split_scaled(mantissa, frac_count);
    let mut out = String::new();
    // Pad the mantissa integer part to `int_count` (leading zeros) — matches Excel's `00.0E+0` forms.
    let mut int_str = int_d;
    while int_str.len() < int_count {
        int_str.insert(0, '0');
    }
    out.push_str(&int_str);
    if frac_count > 0 {
        out.push('.');
        out.push_str(&frac_d);
    }
    out.push('E');
    if exponent < 0 {
        out.push('-');
    } else if always_sign {
        out.push('+');
    }
    out.push_str(&format!("{:0>width$}", exponent.abs(), width = exp_width));
    Ok(out)
}

/// Normalize `value` (>= 0) to `(mantissa, exponent)` with `int_count` mantissa integer digits, then
/// round the mantissa to `frac_count` places, carrying the exponent when rounding overflows the width
/// (`9.99…E9 -> 1.00E10`). `0` normalizes to `(0, 0)`.
fn normalize_scientific(value: f64, int_count: usize, frac_count: usize) -> (f64, i32) {
    if value == 0.0 {
        return (0.0, 0);
    }
    let mut exponent = value.log10().floor() as i32 - (int_count as i32 - 1);
    let mut mantissa = value / 10f64.powi(exponent);
    let factor = 10f64.powi(frac_count as i32);
    mantissa = (mantissa * factor).round() / factor;
    let ceiling = 10f64.powi(int_count as i32);
    if mantissa >= ceiling {
        mantissa /= 10.0;
        mantissa = (mantissa * factor).round() / factor;
        exponent += 1;
    }
    (mantissa, exponent)
}

/// Render a fraction section (`# ?/?`, `?/?`): split into integer / numerator / denominator
/// placeholders, take the whole part when an integer placeholder is present (else an improper
/// fraction), and find the closest numerator/denominator whose denominator fits the placeholder width.
fn render_fraction(elems: &[Elem], value: f64) -> String {
    let slash = elems
        .iter()
        .position(|e| matches!(e, Elem::Slash))
        .unwrap_or(0);
    // Denominator placeholders (after the slash) set the max denominator by their digit count.
    let den_digits = elems[slash + 1..]
        .iter()
        .filter(|e| matches!(e, Elem::Digit(_)))
        .count()
        .max(1);
    // A pathological placeholder run saturates the cap; `best_fraction` is O(log max_den), so a huge cap can neither overflow nor hang.
    let max_den = u32::try_from(den_digits)
        .ok()
        .and_then(|d| 10u64.checked_pow(d))
        .map_or(u64::MAX, |p| p - 1);
    // The numerator is the digit run immediately before the slash; anything before THAT is the integer part.
    let mut num_run_start = slash;
    while num_run_start > 0 && matches!(elems[num_run_start - 1], Elem::Digit(_)) {
        num_run_start -= 1;
    }
    let has_int = elems[..num_run_start]
        .iter()
        .any(|e| matches!(e, Elem::Digit(_)));

    let (whole, frac) = if has_int {
        (value.trunc(), value.fract())
    } else {
        (0.0, value)
    };
    let (mut num, den) = best_fraction(frac, max_den);
    // Emit: integer block (if any), literals, then numerator/slash/denominator.
    let mut out = String::new();
    let mut int_done = false;
    let mut past_slash = false;
    for (i, e) in elems.iter().enumerate() {
        match e {
            Elem::Digit(_) if i >= num_run_start && i < slash => {
                if !past_slash && num != u64::MAX {
                    out.push_str(&num.to_string());
                    num = u64::MAX; // emitted once
                }
            }
            Elem::Slash => {
                out.push('/');
                out.push_str(&den.to_string());
                past_slash = true;
            }
            Elem::Digit(_) if past_slash => {} // denominator emitted with the slash
            Elem::Digit(_) => {
                if has_int && !int_done {
                    out.push_str(&format_whole_or_blank(whole));
                    int_done = true;
                }
            }
            Elem::Lit(s) => out.push_str(s),
            Elem::Point | Elem::Comma | Elem::Percent | Elem::Exp(_) => {}
        }
    }
    out
}

/// The integer part of a fraction: its digits, or `""` when zero (an all-`#` integer placeholder shows
/// nothing for a zero whole part, so `# ?/?` of `0.5` is just the fraction).
fn format_whole_or_blank(whole: f64) -> String {
    if whole == 0.0 {
        String::new()
    } else {
        format!("{}", whole as i64)
    }
}

/// The least-error rational with denominator at most `max_den`, ties to the smaller denominator.
/// Found from the continued-fraction convergents plus the best trailing semiconvergent — the optimal
/// bounded-denominator rational is always one of these — so it reproduces an exhaustive `1..=max_den`
/// scan in O(log max_den). All convergent arithmetic is checked, so it cannot overflow or hang.
fn best_fraction(x: f64, max_den: u64) -> (u64, u64) {
    if !x.is_finite() || max_den == 0 {
        return (x.max(0.0).round() as u64, 1);
    }
    let max = max_den as i128;
    // Convergent recurrence h_n/k_n = a_n·h_{n-1}/k_{n-1} + h_{n-2}/k_{n-2}, seeded at n = -1, -2.
    let (mut h2, mut h1) = (0i128, 1i128);
    let (mut k2, mut k1) = (1i128, 0i128);
    let mut candidates: Vec<(i128, i128)> = Vec::new();
    let mut xr = x;
    for _ in 0..64 {
        if !xr.is_finite() {
            break;
        }
        let a = xr.floor() as i128;
        let hn = a.checked_mul(h1).and_then(|v| v.checked_add(h2));
        let kn = a.checked_mul(k1).and_then(|v| v.checked_add(k2));
        match (hn, kn) {
            (Some(hn), Some(kn)) if kn <= max => {
                h2 = h1;
                h1 = hn;
                k2 = k1;
                k1 = kn;
                candidates.push((h1, k1));
            }
            _ => {
                // Over budget: the closest reachable rational is the largest semiconvergent that fits.
                if k1 > 0 {
                    let a_max = (max - k2) / k1;
                    if a_max >= 1 {
                        candidates.push((a_max * h1 + h2, a_max * k1 + k2));
                    }
                }
                break;
            }
        }
        let frac = xr - a as f64;
        if frac.abs() < 1e-15 {
            break;
        }
        xr = 1.0 / frac;
    }
    // The prior convergent (h1/k1) is always in range and covers the empty/degenerate case.
    candidates.push((h1, k1));
    // Ascending denominator + strictly-better-by-1e-12 mirrors the scan's smaller-denominator tie.
    candidates.retain(|&(n, d)| d >= 1 && d <= max && n >= 0);
    candidates.sort_unstable_by_key(|&(_, d)| d);
    let mut best = (0u64, 1u64);
    let mut best_err = f64::INFINITY;
    for &(n, d) in &candidates {
        let err = (x - n as f64 / d as f64).abs();
        if err + 1e-12 < best_err {
            best_err = err;
            best = (n as u64, d as u64);
        }
    }
    best
}

// ============================ shared number-digit helpers ============================

/// Largest integer an f64 mantissa represents exactly (2^53); past it, `|n| * 10^decimals` can no
/// longer be rounded in the scaled domain without losing digits.
const MAX_EXACT_INT: f64 = 9_007_199_254_740_992.0;

/// Rounds in the SCALED domain only while `|n| * 10^decimals` stays exactly representable; beyond
/// that `|n|`'s own ulp already exceeds `10^-decimals`, so rounding is the identity and `|n|` renders
/// directly. That keeps a large `decimals` bounded — no overflow to `inf`, no quadratic padding.
fn split_scaled(n: f64, decimals: usize) -> (bool, String, String) {
    let abs = n.abs();
    let factor = 10f64.powi(decimals.min(308) as i32);
    let scaled = abs * factor;
    let rounded = if scaled < MAX_EXACT_INT {
        scaled.round() / factor
    } else {
        abs
    };
    let neg = n < 0.0 && rounded != 0.0;
    let rendered = format!("{rounded:.decimals$}");
    match rendered.split_once('.') {
        Some((int_d, frac_d)) => (neg, int_d.to_string(), frac_d.to_string()),
        None => (neg, rendered, String::new()),
    }
}

/// Insert `,` thousands separators into a run of ASCII digits.
fn group_thousands(int_digits: &str) -> String {
    let n = int_digits.len();
    let mut out = String::with_capacity(n + n / 3);
    for (i, ch) in int_digits.chars().enumerate() {
        if i > 0 && (n - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

// ============================ date/time section renderer ============================

const MONTH_ABBR: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTH_FULL: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAY_ABBR: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const DAY_FULL: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// Whether a section is a DATE/TIME format — it carries a `y`/`m`/`d`/`h`/`s` code (or an elapsed
/// `[…]`) outside quotes/escapes. A pure number format (`0.00`, `$#,##0`, `0.00E+00`) has none.
fn is_datetime_section(section: &str) -> bool {
    let mut chars = section.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                for q in chars.by_ref() {
                    if q == '"' {
                        break;
                    }
                }
            }
            '\\' => {
                chars.next();
            }
            'y' | 'Y' | 'd' | 'D' | 'h' | 'H' | 's' | 'S' | 'm' | 'M' | '[' => return true,
            _ => {}
        }
    }
    false
}

/// A date/time token parsed from a mask.
enum Dt {
    Year(usize),
    Month(usize),
    Minute(usize),
    Day(usize),
    Hour(usize),
    Second(usize),
    Elapsed(char, usize),
    AmPm(bool), // true: full "AM"/"PM"; false: "A"/"P"
    Lit(String),
}

/// Render a date/time section for the Excel serial `serial`. A section with a date field (`y`/`d`/month
/// `m`) requires `serial` in `[1, MAX_SERIAL]` (else `#VALUE!`, CORE2 — the same band as YEAR/DAY and
/// the yyyy-mm-dd render). Time fields read the day fraction; month-vs-minute for `m` is resolved by
/// neighbouring `h`/`s` tokens.
fn render_datetime(section: &str, serial: f64) -> Result<String, ErrKind> {
    let tokens = disambiguate_months(tokenize_datetime(section));
    let has_date = tokens
        .iter()
        .any(|t| matches!(t, Dt::Year(_) | Dt::Month(_) | Dt::Day(_)));
    let has_ampm = tokens.iter().any(|t| matches!(t, Dt::AmPm(_)));

    // Time-of-day, rounded to the finest displayed unit so 17:59:59.6 shows as the next minute.
    let frac = serial - serial.floor();
    let units_per_day = if tokens.iter().any(|t| matches!(t, Dt::Second(_))) {
        86_400i64
    } else if tokens.iter().any(|t| matches!(t, Dt::Minute(_))) {
        1_440
    } else {
        24
    };
    let mut total = (frac * units_per_day as f64).round() as i64;
    let mut day_carry = 0i64;
    if total >= units_per_day {
        total -= units_per_day;
        day_carry = 1;
    }
    let (h24, minute, second) = match units_per_day {
        86_400 => (total / 3600, (total % 3600) / 60, total % 60),
        1_440 => (total / 60, total % 60, 0),
        _ => (total, 0, 0),
    };

    let day_serial = serial.floor() as i64 + day_carry;
    // Date fields need a valid civil date; a serial outside the band is a located refusal.
    let (y, mon, dom) = if has_date {
        if !(1..=MAX_SERIAL).contains(&day_serial) {
            return Err(ErrKind::Value);
        }
        serial_to_ymd(day_serial)
    } else {
        (0, 0, 0)
    };
    let dow = ((day_serial - 1).rem_euclid(7)) as usize; // 0 = Sunday (serial 1 = 1900-01-01)

    let mut out = String::new();
    for t in &tokens {
        match t {
            Dt::Year(2) => out.push_str(&format!("{:02}", y.rem_euclid(100))),
            Dt::Year(_) => out.push_str(&format!("{y:04}")),
            Dt::Month(1) => out.push_str(&mon.to_string()),
            Dt::Month(2) => out.push_str(&format!("{mon:02}")),
            Dt::Month(3) => out.push_str(MONTH_ABBR[(mon.max(1) - 1) as usize]),
            Dt::Month(4) => out.push_str(MONTH_FULL[(mon.max(1) - 1) as usize]),
            Dt::Month(_) => out.push(
                MONTH_FULL[(mon.max(1) - 1) as usize]
                    .chars()
                    .next()
                    .unwrap(),
            ),
            Dt::Day(1) => out.push_str(&dom.to_string()),
            Dt::Day(2) => out.push_str(&format!("{dom:02}")),
            Dt::Day(3) => out.push_str(DAY_ABBR[dow]),
            Dt::Day(_) => out.push_str(DAY_FULL[dow]),
            Dt::Hour(w) => out.push_str(&fmt_hour(h24, has_ampm, *w)),
            Dt::Minute(w) => out.push_str(&fmt_unit(minute, *w)),
            Dt::Second(w) => out.push_str(&fmt_unit(second, *w)),
            Dt::AmPm(full) => out.push_str(am_pm(h24, *full)),
            Dt::Elapsed(unit, w) => out.push_str(&fmt_elapsed(serial, *unit, *w)),
            Dt::Lit(s) => out.push_str(s),
        }
    }
    Ok(out)
}

/// Format an hour: 24-hour when no `AM/PM` token is present, else 12-hour (`0`/`12`->`12`). `width` 2
/// zero-pads.
fn fmt_hour(h24: i64, has_ampm: bool, width: usize) -> String {
    let h = if has_ampm {
        let m = h24 % 12;
        if m == 0 { 12 } else { m }
    } else {
        h24
    };
    fmt_unit(h, width)
}

/// Zero-pad `v` to `width` (only widths 1 and 2 occur in masks).
fn fmt_unit(v: i64, width: usize) -> String {
    if width >= 2 {
        format!("{v:02}")
    } else {
        v.to_string()
    }
}

/// The `AM`/`PM` (or `A`/`P`) marker for a 24-hour value.
fn am_pm(h24: i64, full: bool) -> &'static str {
    match (h24 < 12, full) {
        (true, true) => "AM",
        (false, true) => "PM",
        (true, false) => "A",
        (false, false) => "P",
    }
}

/// Elapsed time: total hours/minutes/seconds across the whole `serial` duration, zero-padded to at
/// least `width`. `[h]` counts every hour, `[m]` every minute, `[s]` every second.
fn fmt_elapsed(serial: f64, unit: char, width: usize) -> String {
    let total_sec = (serial * 86_400.0).round() as i64;
    let v = match unit {
        'h' => total_sec / 3600,
        'm' => total_sec / 60,
        _ => total_sec,
    };
    format!("{:0>width$}", v, width = width)
}

/// Tokenize a date/time mask into [`Dt`] tokens: runs of a same letter (`yyyy`, `mm`), `[h]`-style
/// elapsed groups, `AM/PM` / `A/P`, quoted/escaped literals, and punctuation literals. `m` runs are
/// provisionally [`Dt::Month`] and re-tagged by [`disambiguate_months`].
fn tokenize_datetime(section: &str) -> Vec<Dt> {
    let mut out = Vec::new();
    let chars: Vec<char> = section.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let lower = c.to_ascii_lowercase();
        match c {
            '"' => {
                i += 1;
                let mut lit = String::new();
                while i < chars.len() && chars[i] != '"' {
                    lit.push(chars[i]);
                    i += 1;
                }
                i += 1;
                out.push(Dt::Lit(lit));
            }
            '\\' => {
                if i + 1 < chars.len() {
                    out.push(Dt::Lit(chars[i + 1].to_string()));
                }
                i += 2;
            }
            '[' => {
                let start = i + 1;
                let mut j = start;
                while j < chars.len() && chars[j] != ']' {
                    j += 1;
                }
                let unit = chars
                    .get(start)
                    .map(|c| c.to_ascii_lowercase())
                    .unwrap_or('h');
                out.push(Dt::Elapsed(unit, j - start));
                i = j + 1;
            }
            _ if lower == 'a' && matches_ci(&chars, i, "am/pm") => {
                out.push(Dt::AmPm(true));
                i += 5;
            }
            _ if lower == 'a' && matches_ci(&chars, i, "a/p") => {
                out.push(Dt::AmPm(false));
                i += 3;
            }
            'y' | 'Y' | 'm' | 'M' | 'd' | 'D' | 'h' | 'H' | 's' | 'S' => {
                let mut count = 0;
                while i < chars.len() && chars[i].to_ascii_lowercase() == lower {
                    count += 1;
                    i += 1;
                }
                out.push(match lower {
                    'y' => Dt::Year(count),
                    'm' => Dt::Month(count),
                    'd' => Dt::Day(count),
                    'h' => Dt::Hour(count),
                    _ => Dt::Second(count),
                });
            }
            _ => {
                out.push(Dt::Lit(c.to_string()));
                i += 1;
            }
        }
    }
    out
}

/// Whether `pat` (ASCII) matches `chars` at `i`, case-insensitively.
fn matches_ci(chars: &[char], i: usize, pat: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    i + p.len() <= chars.len() && (0..p.len()).all(|k| chars[i + k].to_ascii_lowercase() == p[k])
}

/// Re-tag each provisional [`Dt::Month`] as a [`Dt::Minute`] when it neighbours a time field: the
/// nearest preceding non-literal token is an hour/elapsed-hour, or the nearest following one is a
/// second. This is Excel's `m`=month-vs-minute rule.
fn disambiguate_months(tokens: Vec<Dt>) -> Vec<Dt> {
    let is_hour = |t: &Dt| matches!(t, Dt::Hour(_) | Dt::Elapsed('h', _));
    let is_second = |t: &Dt| matches!(t, Dt::Second(_));
    let mut out = tokens;
    for i in 0..out.len() {
        let Dt::Month(w) = out[i] else { continue };
        let prev = out[..i].iter().rev().find(|t| !matches!(t, Dt::Lit(_)));
        let next = out[i + 1..].iter().find(|t| !matches!(t, Dt::Lit(_)));
        if prev.is_some_and(is_hour) || next.is_some_and(is_second) {
            out[i] = Dt::Minute(w);
        }
    }
    out
}

// ============================ 1900 serial <-> civil date map ============================

/// Unix day index of 1899-12-31 (Excel "serial 0" in the pre-bug half); the shared epoch anchor for
/// both directions of the serial<->date map (`serial_to_ymd` and its inverse `serial_from_ymd`).
pub(crate) const EPOCH_1899_12_31: i64 = -25568;

/// Convert an Excel date serial (integer, `>= 1`) to a proleptic-Gregorian `(year, month, day)` in the
/// **1900 date system, WITH Excel's leap-year bug replicated** (serial 60 is the fictional
/// `1900-02-29`; serials `>= 61` shift back one day to skip it). The load-bearing date call — a real
/// Excel-authored serial round-trips. Shared with `func::date` (its inverse `serial_from_ymd`).
pub(crate) fn serial_to_ymd(serial: i64) -> (i64, u32, u32) {
    if serial == 60 {
        return (1900, 2, 29);
    }
    let unix_days = if serial < 60 {
        EPOCH_1899_12_31 + serial
    } else {
        EPOCH_1899_12_31 + serial - 1
    };
    civil_from_days(unix_days)
}

/// Howard Hinnant's `civil_from_days`: days since the Unix epoch (1970-01-01) -> proleptic-Gregorian
/// `(year, month, day)`. Exact integer arithmetic, valid across the whole date range v1 cares about.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (y + i64::from(m <= 2), m, d)
}

// ============================ parse-time gate ============================

/// Refuses ONLY an unsupported STRING LITERAL; a computed format is accepted and deferred to eval,
/// so an unvettable one is a false-negative rather than a false-reject.
pub(crate) fn validate_text_format(args: &[Expr], span: Span) -> Result<(), Diag> {
    match args.get(1) {
        Some(Expr::Lit(Value::Text(fmt)))
            if !fmt.eq_ignore_ascii_case("General") && format_unsupported(fmt) =>
        {
            Err(Diag::new(
                DiagCode::UnsupportedFormat,
                span,
                format!("TEXT format code {fmt:?} is not in the supported subset"),
            ))
        }
        _ => Ok(()),
    }
}
