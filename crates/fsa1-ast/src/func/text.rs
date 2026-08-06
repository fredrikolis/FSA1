// Concern: the string built-ins and the text->number/serial readers | Non-concern: number formatting (text_format.rs owns it) | IO: (&mut EvalCtx, &[Expr]) -> Value
use super::*;

pub(crate) fn concat_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let mut out = String::new();
    for a in args {
        match ctx.eval(a) {
            Value::Array(_, cells) => {
                for c in &cells {
                    match to_text(c) {
                        Ok(s) => out.push_str(&s),
                        Err(k) => return Value::Error(k),
                    }
                }
            }
            other => match to_text(&other) {
                Ok(s) => out.push_str(&s),
                Err(k) => return Value::Error(k),
            },
        }
    }
    Value::Text(out)
}

pub(crate) fn textjoin_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let delim = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let ignore_empty = match coerce_bool(&ctx.eval(&args[1])) {
        Ok(b) => b,
        Err(k) => return Value::Error(k),
    };
    let mut pieces: Vec<String> = Vec::new();
    let push = |s: String, out: &mut Vec<String>| {
        if !(ignore_empty && s.is_empty()) {
            out.push(s);
        }
    };
    for a in &args[2..] {
        match ctx.eval(a) {
            Value::Array(_, cells) => {
                for c in &cells {
                    match to_text(c) {
                        Ok(s) => push(s, &mut pieces),
                        Err(k) => return Value::Error(k),
                    }
                }
            }
            other => match to_text(&other) {
                Ok(s) => push(s, &mut pieces),
                Err(k) => return Value::Error(k),
            },
        }
    }
    Value::Text(pieces.join(&delim))
}

/// Evaluate an argument to a NON-NEGATIVE character count, truncating a fractional value toward zero
/// (Excel) and mapping a negative to `#VALUE!`. A huge value saturates on the `as usize` cast and is
/// clamped by the caller against the string length.
fn count_arg(ctx: &mut EvalCtx, e: &Expr) -> Result<usize, ErrKind> {
    let n = one_num(ctx, e)?.trunc();
    if n < 0.0 {
        return Err(ErrKind::Value);
    }
    Ok(n as usize)
}

pub(crate) fn left_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let s = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let n = if args.len() == 2 {
        match count_arg(ctx, &args[1]) {
            Ok(n) => n,
            Err(k) => return Value::Error(k),
        }
    } else {
        1
    };
    let chars: Vec<char> = s.chars().collect();
    let take = n.min(chars.len());
    Value::Text(chars[..take].iter().collect())
}

pub(crate) fn right_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let s = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let n = if args.len() == 2 {
        match count_arg(ctx, &args[1]) {
            Ok(n) => n,
            Err(k) => return Value::Error(k),
        }
    } else {
        1
    };
    let chars: Vec<char> = s.chars().collect();
    let take = n.min(chars.len());
    Value::Text(chars[chars.len() - take..].iter().collect())
}

pub(crate) fn mid_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let s = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let start = match one_num(ctx, &args[1]) {
        Ok(x) => x.trunc(),
        Err(k) => return Value::Error(k),
    };
    let count = match count_arg(ctx, &args[2]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    if start < 1.0 {
        return Value::Error(ErrKind::Value);
    }
    let chars: Vec<char> = s.chars().collect();
    let start_idx = (start as usize) - 1;
    if start_idx >= chars.len() {
        return Value::Text(String::new());
    }
    let take = count.min(chars.len() - start_idx);
    Value::Text(chars[start_idx..start_idx + take].iter().collect())
}

pub(crate) fn len_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match arg_text(ctx, &args[0]) {
        Ok(s) => Value::Number(s.chars().count() as f64),
        Err(k) => Value::Error(k),
    }
}

/// Shared start-position resolution for `FIND`/`SEARCH`: evaluate the optional `start_num` (default
/// 1), reject `< 1` as `#VALUE!`, and return the 0-based start index. `hay_len` is the haystack's char
/// count; a start past `len + 1` is `#VALUE!` (Excel), while `len + 1` itself is legal (it only
/// matches an empty needle at the end).
fn find_start(ctx: &mut EvalCtx, args: &[Expr], hay_len: usize) -> Result<usize, ErrKind> {
    let start = if args.len() == 3 {
        one_num(ctx, &args[2])?.trunc()
    } else {
        1.0
    };
    if start < 1.0 {
        return Err(ErrKind::Value);
    }
    let idx = (start as usize) - 1;
    if idx > hay_len {
        return Err(ErrKind::Value);
    }
    Ok(idx)
}

pub(crate) fn find_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let needle = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let hay = match arg_text(ctx, &args[1]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let hay_chars: Vec<char> = hay.chars().collect();
    let start_idx = match find_start(ctx, args, hay_chars.len()) {
        Ok(i) => i,
        Err(k) => return Value::Error(k),
    };
    let needle_chars: Vec<char> = needle.chars().collect();
    match find_sub(&hay_chars, &needle_chars, start_idx) {
        Some(i) => Value::Number((i + 1) as f64),
        None => Value::Error(ErrKind::Value),
    }
}

/// The first index `>= from` at which `needle` occurs verbatim (case-sensitive) in `hay`. An empty
/// needle matches at `from` when `from <= hay.len()`.
fn find_sub(hay: &[char], needle: &[char], from: usize) -> Option<usize> {
    if needle.is_empty() {
        return (from <= hay.len()).then_some(from);
    }
    if needle.len() > hay.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

pub(crate) fn search_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let pattern = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let hay = match arg_text(ctx, &args[1]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let hay_chars: Vec<char> = hay.chars().collect();
    let start_idx = match find_start(ctx, args, hay_chars.len()) {
        Ok(i) => i,
        Err(k) => return Value::Error(k),
    };
    let toks = parse_wild(&pattern);
    for p in start_idx..=hay_chars.len() {
        if wild_prefix(&toks, &hay_chars[p..]) {
            return Value::Number((p + 1) as f64);
        }
    }
    Value::Error(ErrKind::Value)
}

/// A wildcard-pattern token for `SEARCH`.
enum Wild {
    /// `*` — any run of characters (including empty).
    Star,
    /// `?` — exactly one character.
    Any,
    /// A literal character (case-folded on compare).
    Lit(char),
}

/// Tokenize a `SEARCH` pattern: `*`/`?` are wildcards, `~` escapes the next char to a literal (a
/// trailing `~` is itself a literal `~`).
fn parse_wild(pattern: &str) -> Vec<Wild> {
    let mut toks = Vec::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => toks.push(Wild::Star),
            '?' => toks.push(Wild::Any),
            '~' => toks.push(Wild::Lit(chars.next().unwrap_or('~'))),
            other => toks.push(Wild::Lit(other)),
        }
    }
    toks
}

/// ITERATIVE single-star backtracking, deliberately NOT a recursive splits-every-way walk: that form
/// is exponential on a multi-star pattern (a ReDoS), while this one is bounded by
/// `text.len() * toks.len()`. Matches a PREFIX only — `SEARCH` anchors at the current start.
fn wild_prefix(toks: &[Wild], text: &[char]) -> bool {
    let n = text.len();
    let mut ti = 0; // text cursor
    let mut pi = 0; // pattern cursor
    // The last `*` we passed and the text index it started matching at (for backtracking).
    let mut star: Option<(usize, usize)> = None;
    while pi < toks.len() {
        match &toks[pi] {
            Wild::Star => {
                star = Some((pi, ti));
                pi += 1;
            }
            Wild::Any if ti < n => {
                pi += 1;
                ti += 1;
            }
            Wild::Lit(c) if ti < n && text[ti].eq_ignore_ascii_case(c) => {
                pi += 1;
                ti += 1;
            }
            // Rewind to the last `*` and let it consume one more character; with none, no match.
            _ => match star {
                Some((sp, st)) if st < n => {
                    ti = st + 1;
                    star = Some((sp, st + 1));
                    pi = sp + 1;
                }
                _ => return false,
            },
        }
    }
    // Pattern fully consumed — a prefix matched; any leftover `text` is fine (this is a prefix match).
    true
}

pub(crate) fn substitute_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let text = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let old = match arg_text(ctx, &args[1]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let new = match arg_text(ctx, &args[2]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    if old.is_empty() {
        return Value::Text(text);
    }
    if args.len() == 4 {
        let n = match one_num(ctx, &args[3]) {
            Ok(x) => x.trunc(),
            Err(k) => return Value::Error(k),
        };
        if n < 1.0 {
            return Value::Error(ErrKind::Value);
        }
        let target = n as usize;
        let mut result = String::new();
        let mut last = 0;
        let mut count = 0usize;
        for (idx, m) in text.match_indices(&old) {
            count += 1;
            if count == target {
                result.push_str(&text[last..idx]);
                result.push_str(&new);
                last = idx + m.len();
                break;
            }
        }
        result.push_str(&text[last..]);
        Value::Text(result)
    } else {
        Value::Text(text.replace(&old, &new))
    }
}

pub(crate) fn replace_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let old = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let start = match one_num(ctx, &args[1]) {
        Ok(x) => x.trunc(),
        Err(k) => return Value::Error(k),
    };
    let num = match count_arg(ctx, &args[2]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    let new = match arg_text(ctx, &args[3]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    if start < 1.0 {
        return Value::Error(ErrKind::Value);
    }
    let chars: Vec<char> = old.chars().collect();
    let start_idx = ((start as usize) - 1).min(chars.len());
    let take = num.min(chars.len() - start_idx);
    let mut result: String = chars[..start_idx].iter().collect();
    result.push_str(&new);
    result.extend(chars[start_idx + take..].iter());
    Value::Text(result)
}

pub(crate) fn trim_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match arg_text(ctx, &args[0]) {
        Ok(s) => {
            let collapsed = s
                .split(' ')
                .filter(|w| !w.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            Value::Text(collapsed)
        }
        Err(k) => Value::Error(k),
    }
}

pub(crate) fn upper_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match arg_text(ctx, &args[0]) {
        Ok(s) => Value::Text(s.to_uppercase()),
        Err(k) => Value::Error(k),
    }
}

pub(crate) fn lower_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match arg_text(ctx, &args[0]) {
        Ok(s) => Value::Text(s.to_lowercase()),
        Err(k) => Value::Error(k),
    }
}

pub(crate) fn rept_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let s = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let n = match count_arg(ctx, &args[1]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    // Checked, so a huge count refuses at the character cap rather than allocating an unbounded string.
    match s.chars().count().checked_mul(n) {
        Some(len) if len <= 32_767 => Value::Text(s.repeat(n)),
        _ => Value::Error(ErrKind::Value),
    }
}

pub(crate) fn proper_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let s = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let mut out = String::with_capacity(s.len());
    let mut prev_alpha = false;
    for c in s.chars() {
        if c.is_alphabetic() {
            if prev_alpha {
                out.extend(c.to_lowercase());
            } else {
                out.extend(c.to_uppercase());
            }
            prev_alpha = true;
        } else {
            out.push(c);
            prev_alpha = false;
        }
    }
    Value::Text(out)
}

pub(crate) fn exact_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let a = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let b = match arg_text(ctx, &args[1]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    Value::Bool(a == b)
}

pub(crate) fn value_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match scalarize(ctx.eval(&args[0])) {
        Value::Number(n) => Value::Number(n),
        Value::Blank => Value::Number(0.0),
        Value::Error(k) => Value::Error(k),
        Value::Text(t) => parse_value_text(&t),
        Value::Bool(_) | Value::Array(..) => Value::Error(ErrKind::Value),
    }
}

/// A deliberate SUBSET of the locale-driven original: en-US decorations and ISO-style date/time only.
/// A form outside the subset is a false-NEGATIVE `#VALUE!`, never a wrong number.
fn parse_value_text(t: &str) -> Value {
    let s = t.trim();
    if s.is_empty() {
        return Value::Error(ErrKind::Value);
    }
    if let Some(n) = parse_number_text(s) {
        return Value::Number(n);
    }
    if let Some(serial) = parse_datetime_serial(s) {
        return Value::Number(serial);
    }
    Value::Error(ErrKind::Value)
}

/// Decorations peel in a FIXED order — accounting parentheses, a leading `$`, a trailing `%`, then
/// thousands separators — before the `f64` parse.
fn parse_number_text(s: &str) -> Option<f64> {
    let (body, negate) = match s.strip_prefix('(').and_then(|b| b.strip_suffix(')')) {
        Some(inner) => (inner.trim(), true),
        None => (s, false),
    };
    // A LEADING `$` only: a trailing one is not en-US currency and stays outside the subset.
    let body = body.strip_prefix('$').unwrap_or(body).trim_start();
    let (body, scale) = match body.strip_suffix('%') {
        Some(pct) => (pct.trim_end(), 0.01),
        None => (body, 1.0),
    };
    let cleaned = strip_thousands(body)?;
    match cleaned.parse::<f64>() {
        Ok(n) if n.is_finite() => {
            let v = n * scale;
            Some(if negate { -v } else { v })
        }
        _ => None,
    }
}

/// Only a VALID grouping is stripped: 1-3 digits then comma-separated groups of exactly 3, with no
/// comma in the fraction. A malformed grouping is `None`, so it refuses rather than silently misreads.
fn strip_thousands(s: &str) -> Option<String> {
    if !s.contains(',') {
        return Some(s.to_string());
    }
    let (sign, rest) = match s.strip_prefix(['+', '-']) {
        Some(r) => (&s[..1], r),
        None => ("", s),
    };
    let (int_part, frac_part) = match rest.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (rest, None),
    };
    if frac_part.is_some_and(|f| f.contains(',')) {
        return None;
    }
    let groups: Vec<&str> = int_part.split(',').collect();
    if groups.len() < 2 {
        return None; // the comma was not a group separator in the integer part.
    }
    for (i, g) in groups.iter().enumerate() {
        let len_ok = if i == 0 {
            (1..=3).contains(&g.len())
        } else {
            g.len() == 3
        };
        if !len_ok || !g.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    let mut out = String::from(sign);
    out.extend(groups);
    if let Some(f) = frac_part {
        out.push('.');
        out.push_str(f);
    }
    Some(out)
}

/// The ONE ISO date/time reading, behind the crate-public [`crate::func::parse_iso_serial`] façade.
pub(crate) fn parse_datetime_serial(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    match parts.as_slice() {
        [one] => parse_iso_date(one)
            .map(|d| d as f64)
            .or_else(|| parse_clock(one)),
        [date, time] => Some(parse_iso_date(date)? as f64 + parse_clock(time)?),
        _ => None,
    }
}

/// `None` for a non-`yyyy-mm-dd` shape, an out-of-range field, or a serial outside the valid band.
pub(crate) fn parse_iso_date(s: &str) -> Option<i64> {
    let mut it = s.split('-');
    let (y, m, d) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() {
        return None;
    }
    let y: i64 = y.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    let d: u32 = d.parse().ok()?;
    if !(1..=9999).contains(&y) || !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) {
        return None;
    }
    let serial = serial_from_ymd(y, m, d);
    (1..=MAX_SERIAL).contains(&serial).then_some(serial)
}

/// Parse an `hh:mm[:ss]` clock time into a day fraction in `[0, 1)`. `None` for a non-clock shape or an
/// out-of-range field (`hh > 23`, `mm`/`ss` `> 59`). Shared with `func::date` (TIMEVALUE/DATEVALUE).
pub(crate) fn parse_clock(s: &str) -> Option<f64> {
    let mut it = s.split(':');
    let h: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let sec: i64 = match it.next() {
        Some(x) => x.parse().ok()?,
        None => 0,
    };
    if it.next().is_some() {
        return None;
    }
    if !(0..=23).contains(&h) || !(0..=59).contains(&m) || !(0..=59).contains(&sec) {
        return None;
    }
    Some((h * 3600 + m * 60 + sec) as f64 / 86_400.0)
}

/// Indexed by `code - 0x80`. This band is the ONLY place Windows-1252 diverges from Latin-1; the five
/// positions it leaves undefined keep their C1 code point so CHAR/CODE still round-trip them.
const WIN1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{0081}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{008D}', '\u{017D}', '\u{008F}',
    '\u{0090}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{009D}', '\u{017E}', '\u{0178}',
];

pub(crate) fn char_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let n = match one_num(ctx, &args[0]) {
        Ok(x) => x.trunc(),
        Err(k) => return Value::Error(k),
    };
    if !(1.0..=255.0).contains(&n) {
        return Value::Error(ErrKind::Value);
    }
    let code = n as u32;
    let c = if (128..=159).contains(&code) {
        WIN1252_HIGH[(code - 128) as usize]
    } else {
        match char::from_u32(code) {
            Some(c) => c,
            None => return Value::Error(ErrKind::Value),
        }
    };
    Value::Text(c.to_string())
}

pub(crate) fn code_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let s = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    match s.chars().next() {
        Some(c) => Value::Number(char_to_win1252(c) as f64),
        None => Value::Error(ErrKind::Value),
    }
}

/// Map a character to its Windows-1252 (ANSI) code-page byte — the inverse of the CHAR mapping. The
/// `0x00..=0x7F` and `0xA0..=0xFF` ranges are the Latin-1 identity; the `0x80..=0x9F` typographic band
/// is found in [`WIN1252_HIGH`]. A character outside the code page returns `63` (`'?'`), as Excel's
/// CODE does.
fn char_to_win1252(c: char) -> u32 {
    let cp = c as u32;
    if cp <= 0x7F || (0xA0..=0xFF).contains(&cp) {
        return cp;
    }
    match WIN1252_HIGH.iter().position(|&h| h == c) {
        Some(i) => 128 + i as u32,
        None => 63,
    }
}

pub(crate) fn t_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match scalarize(ctx.eval(&args[0])) {
        Value::Text(s) => Value::Text(s),
        Value::Error(k) => Value::Error(k),
        _ => Value::Text(String::new()),
    }
}

pub(crate) fn clean_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match arg_text(ctx, &args[0]) {
        Ok(s) => Value::Text(s.chars().filter(|c| (*c as u32) >= 32).collect()),
        Err(k) => Value::Error(k),
    }
}

pub(crate) fn textbefore_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    text_split_around(ctx, args, true)
}

pub(crate) fn textafter_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    text_split_around(ctx, args, false)
}

/// A NEGATIVE `instance` counts delimiter occurrences from the end; `0` is `#VALUE!`. `delimiter` is
/// read as a single string — an ARRAY of alternative delimiters is not modelled.
fn text_split_around(ctx: &mut EvalCtx, args: &[Expr], before: bool) -> Value {
    let text = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let delim = match arg_text(ctx, &args[1]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let instance = match opt_num(ctx, args, 2, 1.0) {
        Ok(n) => n.trunc() as i64,
        Err(k) => return Value::Error(k),
    };
    let ci = match opt_num(ctx, args, 3, 0.0) {
        Ok(n) => n != 0.0,
        Err(k) => return Value::Error(k),
    };
    let match_end = match opt_num(ctx, args, 4, 0.0) {
        Ok(n) => n != 0.0,
        Err(k) => return Value::Error(k),
    };
    // EAGER, not lazy: an error-valued fallback surfaces even on a hit, and precedes the body's own refusals.
    let if_not_found = match args.get(5) {
        Some(e) => match scalarize(ctx.eval(e)) {
            Value::Error(k) => return Value::Error(k),
            v => Some(v),
        },
        None => None,
    };
    if instance == 0 {
        return Value::Error(ErrKind::Value);
    }
    let text_chars: Vec<char> = text.chars().collect();
    let delim_chars: Vec<char> = delim.chars().collect();
    match split_around(&text_chars, &delim_chars, instance, ci, match_end, before) {
        Some(s) => Value::Text(s),
        None => if_not_found.unwrap_or(Value::Error(ErrKind::Na)),
    }
}

/// Compute the TEXTBEFORE/TEXTAFTER result, or `None` when the requested occurrence does not exist.
/// An EMPTY delimiter matches once at the very start (before = `""`, after = the whole text).
fn split_around(
    text: &[char],
    delim: &[char],
    instance: i64,
    ci: bool,
    match_end: bool,
    before: bool,
) -> Option<String> {
    if delim.is_empty() {
        return (instance == 1 || instance == -1).then(|| {
            if before {
                String::new()
            } else {
                text.iter().collect()
            }
        });
    }
    let mut occ = delimiter_occurrences(text, delim, ci);
    if match_end {
        occ.push((text.len(), text.len()));
    }
    let count = occ.len() as i64;
    let idx = if instance > 0 {
        instance - 1
    } else {
        count + instance
    };
    if idx < 0 || idx >= count {
        return None;
    }
    let (start, end) = occ[idx as usize];
    Some(if before {
        text[..start].iter().collect()
    } else {
        text[end..].iter().collect()
    })
}

/// The `(start, end)` char spans of every non-overlapping occurrence of `delim` in `text`, left to
/// right. `ci` folds ASCII case.
fn delimiter_occurrences(text: &[char], delim: &[char], ci: bool) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + delim.len() <= text.len() {
        if (0..delim.len()).all(|k| char_eq(text[i + k], delim[k], ci)) {
            out.push((i, i + delim.len()));
            i += delim.len();
        } else {
            i += 1;
        }
    }
    out
}

/// Character equality, optionally ASCII-case-insensitive (the engine's text-fold convention).
fn char_eq(a: char, b: char, ci: bool) -> bool {
    if ci {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

pub(crate) fn textsplit_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let text = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let col_delim = match arg_text(ctx, &args[1]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let row_delim = match args.get(2) {
        Some(e) => match arg_text(ctx, e) {
            Ok(s) => Some(s),
            Err(k) => return Value::Error(k),
        },
        None => None,
    };
    let ignore_empty = match opt_bool(ctx, args, 3, false) {
        Ok(b) => b,
        Err(k) => return Value::Error(k),
    };
    let ci = match opt_num(ctx, args, 4, 0.0) {
        Ok(n) => n != 0.0,
        Err(k) => return Value::Error(k),
    };
    let pad = match args.get(5) {
        Some(e) => scalarize(ctx.eval(e)),
        None => Value::Error(ErrKind::Na),
    };

    let text_chars: Vec<char> = text.chars().collect();
    let col_chars: Vec<char> = col_delim.chars().collect();
    let row_lines: Vec<Vec<char>> = match &row_delim {
        Some(rd) if !rd.is_empty() => {
            let rd_chars: Vec<char> = rd.chars().collect();
            split_segments(&text_chars, &rd_chars, ci)
                .into_iter()
                .map(|s| s.chars().collect())
                .collect()
        }
        _ => vec![text_chars],
    };
    let mut grid: Vec<Vec<String>> = row_lines
        .iter()
        .map(|line| split_segments(line, &col_chars, ci))
        .collect();
    if ignore_empty {
        for row in &mut grid {
            row.retain(|c| !c.is_empty());
        }
        grid.retain(|r| !r.is_empty());
    }
    let cols = grid.iter().map(|r| r.len()).max().unwrap_or(0);
    if grid.is_empty() || cols == 0 {
        return Value::Error(ErrKind::Calc);
    }
    let rows = grid.len();
    let mut cells = Vec::with_capacity(rows * cols);
    for row in grid {
        for c in 0..cols {
            cells.push(match row.get(c) {
                Some(s) => Value::Text(s.clone()),
                None => pad.clone(),
            });
        }
    }
    Value::Array(
        Shape {
            rows: rows as u32,
            cols: cols as u32,
        },
        cells,
    )
}

/// Split `text` into segments on every non-overlapping occurrence of `delim` (an empty `delim` yields
/// the whole text as one segment). `ci` folds ASCII case.
fn split_segments(text: &[char], delim: &[char], ci: bool) -> Vec<String> {
    if delim.is_empty() {
        return vec![text.iter().collect()];
    }
    let mut out = Vec::new();
    let mut seg = String::new();
    let mut i = 0;
    while i < text.len() {
        if i + delim.len() <= text.len()
            && (0..delim.len()).all(|k| char_eq(text[i + k], delim[k], ci))
        {
            out.push(std::mem::take(&mut seg));
            i += delim.len();
        } else {
            seg.push(text[i]);
            i += 1;
        }
    }
    out.push(seg);
    out
}

pub(crate) fn numbervalue_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let raw = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let dec = match sep_arg(ctx, args, 1, '.') {
        Ok(c) => c,
        Err(k) => return Value::Error(k),
    };
    let grp = match sep_arg(ctx, args, 2, ',') {
        Ok(c) => c,
        Err(k) => return Value::Error(k),
    };
    if dec == grp {
        return Value::Error(ErrKind::Value);
    }
    let no_ws: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    if no_ws.is_empty() {
        return Value::Number(0.0);
    }
    // Only TRAILING percent signs count; a `%` anywhere else invalidates the whole text.
    let trimmed = no_ws.trim_end_matches('%');
    let percents = (no_ws.len() - trimmed.len()) as i32;
    if trimmed.contains('%') {
        return Value::Error(ErrKind::Value);
    }
    let body: String = trimmed.to_string();
    // A group separator may not appear in the fractional part (right of the decimal separator).
    if let Some(dp) = body.find(dec)
        && body[dp + dec.len_utf8()..].contains(grp)
    {
        return Value::Error(ErrKind::Value);
    }
    let cleaned: String = body
        .chars()
        .filter(|c| *c != grp)
        .map(|c| if c == dec { '.' } else { c })
        .collect();
    match cleaned.parse::<f64>() {
        Ok(n) if n.is_finite() => Value::Number(n / 100f64.powi(percents)),
        _ => Value::Error(ErrKind::Value),
    }
}

/// The first character of an optional separator argument, or `default` when the call omits it (an
/// empty separator string also falls back to `default`).
fn sep_arg(ctx: &mut EvalCtx, args: &[Expr], idx: usize, default: char) -> Result<char, ErrKind> {
    match args.get(idx) {
        Some(e) => Ok(arg_text(ctx, e)?.chars().next().unwrap_or(default)),
        None => Ok(default),
    }
}

pub(crate) fn unichar_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let n = match one_num(ctx, &args[0]) {
        Ok(x) => x.trunc(),
        Err(k) => return Value::Error(k),
    };
    if n < 1.0 {
        return Value::Error(ErrKind::Value);
    }
    // `n as u32` saturates a huge value to `u32::MAX`, which `from_u32` then rejects (`None`).
    match char::from_u32(n as u32) {
        Some(c) => Value::Text(c.to_string()),
        None => Value::Error(ErrKind::Value),
    }
}

pub(crate) fn unicode_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let s = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    match s.chars().next() {
        Some(c) => Value::Number(c as u32 as f64),
        None => Value::Error(ErrKind::Value),
    }
}
