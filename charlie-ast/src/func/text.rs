// Concern: the TEXT worksheet functions (CONCAT TEXTJOIN LEFT RIGHT MID LEN FIND SEARCH SUBSTITUTE REPLACE REPT TRIM UPPER LOWER PROPER EXACT VALUE CHAR CODE TEXT) — the string built-ins, coercing every text argument through eval.rs's `to_text` (so the function forms agree with the `&` operator), indexing 1-based by CHARACTER, and TEXT's supported format-code subset (single-sourced in `classify_format`, shared with the parser's `validate_text_format`) | Non-concern: the registry table + dispatch (func/mod.rs), the text-coercion primitive (eval.rs owns `to_text`), and the shared `one_num`/`arg_text` helpers (func/helpers.rs) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
use super::*;

// Text batch v1: CONCAT TEXTJOIN LEFT RIGHT MID LEN FIND SEARCH SUBSTITUTE REPLACE TRIM UPPER LOWER
// TEXT. Every function coerces a text argument through eval.rs's `to_text` (so a number takes its
// GENERAL form, a boolean → TRUE/FALSE, a blank → "", an error PROPAGATES) — the exact rule the `&`
// operator uses, so the function forms and the operator agree. The Excel-semantics calls pinned here,
// each worth a reviewer's eye:
//   * POSITIONS ARE 1-BASED and count CHARACTERS (Unicode scalar values, `char`s — not bytes); an
//     ASCII fixture is byte==char so the distinction is invisible there, but a multi-byte string
//     indexes by char. LEFT/RIGHT/MID CLAMP an out-of-range count to the string's edge (never panic);
//     a NEGATIVE count/start is `#VALUE!`.
//   * FIND is CASE-SENSITIVE with no wildcards; SEARCH is CASE-INSENSITIVE (ASCII fold, matching the
//     rest of the engine's text equality) and honours the `?`(one char) / `*`(any run) wildcards with
//     `~` escaping. Both return the 1-based START position of the match; a miss is `#VALUE!`; an empty
//     needle returns `start_num`. A `start_num` past `len+1` is `#VALUE!`.
//   * SUBSTITUTE replaces the Nth (with `instance_num`) or ALL (without) NON-OVERLAPPING occurrences,
//     CASE-SENSITIVELY; an EMPTY `old_text` returns the text unchanged (Excel); `instance_num < 1` is
//     `#VALUE!`; an Nth that does not exist returns the text unchanged.
//   * REPLACE is POSITIONAL — it splices out `num_chars` chars from `start_num` and inserts `new_text`
//     (clamping a `start_num` past the end to an append, and `num_chars` past the end to "to the end").
//   * TRIM removes leading/trailing ASCII spaces and COLLAPSES interior runs to a single space (Excel
//     TRIM touches only 0x20, never a tab).
//   * TEXT renders a value through a SUPPORTED format-code subset (single-sourced in `classify_format`); an unsupported
//     LITERAL format is refused at PARSE (`validate_text_format` → `unsupported-format`), while a
//     NON-LITERAL (computed) format is accepted and deferred — `text_fn`'s `None` arm returns `#VALUE!`
//     iff the RESOLVED format is unsupported (accept-under-uncertainty, never a false-reject). The
//     1900 date system with Excel's leap-year bug is the epoch call (see `serial_to_ymd`).
/// `CONCAT(text1, …)` — concatenate the text of every datum, FLATTENING ranges row-major (a blank cell
/// contributes `""`, a number its general text); an error at ANY position propagates. This is the
/// function form of the `&` operator, minus the delimiter/skip logic `TEXTJOIN` adds.
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

/// `TEXTJOIN(delimiter, ignore_empty, text1, …)` — join the text of every datum (ranges flattened)
/// with `delimiter`; when `ignore_empty` is TRUE, a piece that renders to `""` (a blank cell or an
/// empty string) is dropped BEFORE joining (so no doubled delimiter). An error in any piece — or in
/// the delimiter / flag — propagates.
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

/// `LEFT(text, [num_chars])` — the leftmost `num_chars` (default 1) characters, clamped to the string
/// length; a negative count is `#VALUE!`.
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

/// `RIGHT(text, [num_chars])` — the rightmost `num_chars` (default 1) characters, clamped; a negative
/// count is `#VALUE!`.
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

/// `MID(text, start_num, num_chars)` — up to `num_chars` characters from the 1-based `start_num`. A
/// `start_num < 1` or a `num_chars < 0` is `#VALUE!`; a `start_num` past the end yields `""`; the take
/// is clamped to the remaining length.
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

/// `LEN(text)` — the number of CHARACTERS in the value's text form (`LEN(TRUE) = 4`, `LEN(12.5) = 4`).
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

/// `FIND(find_text, within_text, [start_num])` — the 1-based char position of the first CASE-SENSITIVE
/// occurrence of `find_text` in `within_text` at/after `start_num`; a miss is `#VALUE!`. An empty
/// `find_text` returns `start_num`. No wildcards.
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

/// `SEARCH(find_text, within_text, [start_num])` — like `FIND` but CASE-INSENSITIVE (ASCII fold) and
/// honouring the `?`(one char) / `*`(any run) wildcards, with `~` escaping a literal `?`/`*`/`~`.
/// Returns the 1-based START position of the first match; a miss is `#VALUE!`.
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

/// Whether `toks` matches a PREFIX of `text` (the match need not consume all of `text` — `SEARCH`
/// finds a substring anchored at the current start). Case-insensitive (ASCII fold) on a literal,
/// matching the engine's text-equality convention.
///
/// ITERATIVE, single-star-backtrack (the classic greedy wildcard matcher, `O(text·pattern)`). A `*`
/// records its position and the text index it began at; on a later mismatch we rewind to that star and
/// let it swallow one more character. This deliberately replaces a recursive `*`-splits-every-way
/// walk, whose branching made a multi-star pattern EXPONENTIAL in time (a ReDoS: `SEARCH("*a*a*…*b",
/// <run of 'a's>)` could run unbounded) — the greedy form only ever advances the saved star's text
/// index, so total work is bounded by `text.len() · toks.len()`.
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
            // Mismatch (or text exhausted for a non-star token): rewind to the last `*` and let it
            // consume one more character; with no `*` to fall back on, the prefix cannot match.
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

/// `SUBSTITUTE(text, old_text, new_text, [instance_num])` — replace the Nth (with `instance_num`) or
/// ALL (without) non-overlapping CASE-SENSITIVE occurrences of `old_text` with `new_text`. An empty
/// `old_text`, or an `instance_num` past the last occurrence, returns `text` unchanged; `instance_num
/// < 1` is `#VALUE!`.
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

/// `REPLACE(old_text, start_num, num_chars, new_text)` — splice out `num_chars` characters starting at
/// the 1-based `start_num` and insert `new_text`. `start_num < 1` or `num_chars < 0` is `#VALUE!`; a
/// `start_num` past the end appends, and `num_chars` past the end deletes to the end.
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

/// `TRIM(text)` — strip leading/trailing ASCII spaces and collapse each interior run of spaces to a
/// single space (Excel TRIM touches only 0x20 — a tab or other whitespace rides through untouched).
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

/// `UPPER(text)` — the value's text form upper-cased (full Unicode case mapping).
pub(crate) fn upper_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match arg_text(ctx, &args[0]) {
        Ok(s) => Value::Text(s.to_uppercase()),
        Err(k) => Value::Error(k),
    }
}

/// `LOWER(text)` — the value's text form lower-cased (full Unicode case mapping).
pub(crate) fn lower_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match arg_text(ctx, &args[0]) {
        Ok(s) => Value::Text(s.to_lowercase()),
        Err(k) => Value::Error(k),
    }
}

/// `REPT(text, number_times)` — `text` repeated `number_times` times (the count truncates toward zero;
/// a negative count is `#VALUE!`). The result is capped at Excel's 32767-character cell limit — a
/// longer result is `#VALUE!` (a located refusal), never an unbounded allocation.
pub(crate) fn rept_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let s = match arg_text(ctx, &args[0]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let n = match count_arg(ctx, &args[1]) {
        Ok(n) => n,
        Err(k) => return Value::Error(k),
    };
    // Guard the length multiply so a huge count never tries to allocate an unbounded string — a
    // `#VALUE!` refusal at/over Excel's 32767-char cap instead.
    match s.chars().count().checked_mul(n) {
        Some(len) if len <= 32_767 => Value::Text(s.repeat(n)),
        _ => Value::Error(ErrKind::Value),
    }
}

/// `PROPER(text)` — capitalize the first letter of every word (a letter that follows a non-letter, or
/// the first character) and lower-case the rest. Full Unicode case mapping; a word boundary is any
/// non-alphabetic character.
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

/// `EXACT(text1, text2)` — whether the two texts are exactly equal, CASE-SENSITIVELY (unlike the `=`
/// operator, which folds case for text). Returns a boolean.
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

/// `VALUE(text)` — convert numeric/date/time text to a number. A `Number` passes through and a `Blank`
/// is `0`; a text string parses through the accepted subset (see [`parse_value_text`]: a decimal with
/// optional `$`/`,`/`%`/accounting-`(…)` decorations, or a `yyyy-mm-dd` / `hh:mm[:ss]` date-time mapped
/// to its serial); anything else — a boolean, unparsable text, or a multi-cell array — is `#VALUE!`. An
/// error propagates.
pub(crate) fn value_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match scalarize(ctx.eval(&args[0])) {
        Value::Number(n) => Value::Number(n),
        Value::Blank => Value::Number(0.0),
        Value::Error(k) => Value::Error(k),
        Value::Text(t) => parse_value_text(&t),
        // A boolean is not numeric text (Excel `VALUE(TRUE)` is `#VALUE!`); a genuinely multi-cell
        // array is collapsed to `Error(Value)` by `scalarize` and caught by the `Error` arm above.
        Value::Bool(_) | Value::Array(..) => Value::Error(ErrKind::Value),
    }
}

/// Parse the text form `VALUE` accepts, trying the two Excel readings in turn: a NUMBER string (a plain
/// decimal with optional leading sign / scientific notation, plus the common money-and-report
/// decorations Excel strips — `$` currency, `,` thousands separators, a trailing `%` scaling by 1/100,
/// and accounting `(…)` parentheses for a negative), then a DATE/TIME string mapped to its Excel serial
/// (`yyyy-mm-dd`, `hh:mm[:ss]`, or the two space-separated). Anything else — including an empty/blank
/// string — is `#VALUE!`. This is a documented SUBSET of Excel's locale-driven VALUE (en-US decorations
/// and ISO-style date/time only); a form outside the subset is a false-NEGATIVE `#VALUE!`, never a
/// wrong number.
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

/// Parse the NUMBER form `VALUE` accepts (see [`parse_value_text`]), returning `None` when `s` is not a
/// number in the accepted subset. Peels the decorations in a fixed order — accounting parentheses,
/// a leading `$`, a trailing `%`, then thousands separators — before an `f64` parse; a rejected
/// grouping (`strip_thousands` returns `None`) or an unparsable core is `None`.
fn parse_number_text(s: &str) -> Option<f64> {
    // Accounting-style negative: a whole `(…)` wrapper negates the enclosed magnitude.
    let (body, negate) = match s.strip_prefix('(').and_then(|b| b.strip_suffix(')')) {
        Some(inner) => (inner.trim(), true),
        None => (s, false),
    };
    // A leading currency symbol (en-US `$`) is stripped. A TRAILING `$` is NOT en-US currency, so
    // `VALUE("5$")` stays outside the subset and falls through to #VALUE! (never a wrong number).
    let body = body.strip_prefix('$').unwrap_or(body).trim_start();
    // A single trailing `%` scales the parsed magnitude by 1/100.
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

/// Remove `,` thousands separators from a numeric body, but ONLY when they form a valid grouping (an
/// optional sign, then a first group of 1–3 digits followed by comma-separated groups of exactly 3, and
/// no comma in the fractional part). A malformed grouping (`"1,00,0"`, `",5"`, a comma in the decimals)
/// returns `None` so it is refused rather than silently misread. A body with no comma passes through
/// unchanged.
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

/// Parse the DATE/TIME form `VALUE` accepts into an Excel serial: a `yyyy-mm-dd` date, an `hh:mm[:ss]`
/// clock time (a day fraction, so `"12:00"` → `0.5`), or the two separated by whitespace (date serial +
/// time fraction). Returns `None` for anything outside that shape.
fn parse_datetime_serial(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    match parts.as_slice() {
        [one] => parse_iso_date(one)
            .map(|d| d as f64)
            .or_else(|| parse_clock(one)),
        [date, time] => Some(parse_iso_date(date)? as f64 + parse_clock(time)?),
        _ => None,
    }
}

/// Parse a `yyyy-mm-dd` date into its Excel serial (1900 system, leap-bug faithful via
/// [`serial_from_ymd`]), validating the month and the day-of-month and gating the result to the valid
/// serial band `[1, MAX_SERIAL]`. `None` for a non-`yyyy-mm-dd` shape or an out-of-range field.
fn parse_iso_date(s: &str) -> Option<i64> {
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
/// out-of-range field (`hh > 23`, `mm`/`ss` `> 59`).
fn parse_clock(s: &str) -> Option<f64> {
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

/// The Windows-1252 (ANSI) code-page characters for the `0x80..=0x9F` band, indexed by `code - 0x80`.
/// This band is the ONLY place the code page Excel's CHAR/CODE use diverges from ISO-8859-1 (Latin-1):
/// `1..=127` and `160..=255` are the Latin-1 identity, while `128..=159` carry these typographic
/// characters (euro, curly quotes, en/em dash, ellipsis, …). The five positions Windows-1252 leaves
/// undefined (`0x81 0x8D 0x8F 0x90 0x9D`) fall back to their C1 control code point (the Latin-1
/// identity), so CHAR/CODE still round-trip them.
const WIN1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{0081}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{008D}', '\u{017D}', '\u{008F}',
    '\u{0090}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{009D}', '\u{017E}', '\u{0178}',
];

/// `CHAR(number)` — the single character whose code is `number` (truncated toward zero) under the
/// Windows-1252 (ANSI) code page Excel's CHAR uses, valid for `1..=255`; anything outside that band is
/// `#VALUE!`. The code maps 1:1 to a Unicode scalar on `1..=127` and `160..=255`; the `128..=159` band
/// maps through [`WIN1252_HIGH`] (so `CHAR(128)="€"`, `CHAR(151)="—"`), matching Excel rather than
/// returning the raw C1 control character.
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
        // `1..=127` and `160..=255` are the Basic-Latin / Latin-1 identity, so `from_u32` is always
        // `Some`; the `None` arm keeps the function total against a synthesized out-of-range value.
        match char::from_u32(code) {
            Some(c) => c,
            None => return Value::Error(ErrKind::Value),
        }
    };
    Value::Text(c.to_string())
}

/// `CODE(text)` — the Windows-1252 (ANSI) code-page byte of the FIRST character of `text`, the inverse
/// of [`char_fn`]; an empty text is `#VALUE!`. A character the code page cannot represent yields `63`
/// (`'?'`), matching Excel (e.g. `CODE("€")=128`, but a non-Latin glyph → `63`).
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

// --- TEXT() and its format-code subset ------------------------------------------------------
// The supported subset is single-sourced HERE in `classify_format`, which BOTH the parser's
// `validate_text_format` (refuse an unsupported LITERAL format up front) and `text_fn` (render a
// resolved format, `#VALUE!` on an unsupported one) consult — so what parses and what renders can
// never drift.
/// The supported TEXT format-code kinds. Anything else classifies to `None` and is refused at parse.
#[derive(Clone, Copy)]
enum Fmt {
    /// `General` — the value's general text form.
    General,
    /// A `0`/`0.00` fixed-decimal mask: `int_min` leading-zero-padded integer digits, `decimals`
    /// fractional places.
    Fixed { int_min: usize, decimals: usize },
    /// A `#,##0`/`#,##0.00` thousands-grouped mask with `decimals` fractional places.
    Thousands { decimals: usize },
    /// A `0%`/`0.00%` percent mask with `decimals` fractional places (value ×100, trailing `%`).
    Percent { decimals: usize },
    /// The `yyyy-mm-dd` date mask (1900 date system — see `serial_to_ymd`).
    DateYmd,
}

/// Classify a TEXT format string into the supported subset, or `None` if unsupported. The ONE source
/// of truth for both the parse-time gate and the render path.
fn classify_format(fmt: &str) -> Option<Fmt> {
    if fmt.eq_ignore_ascii_case("General") {
        return Some(Fmt::General);
    }
    if fmt.eq_ignore_ascii_case("yyyy-mm-dd") {
        return Some(Fmt::DateYmd);
    }
    // Percent: a `0`-mask followed by a single trailing `%`.
    if let Some(mask) = fmt.strip_suffix('%') {
        if let Some((int_min, decimals)) = parse_zero_mask(mask) {
            // The integer part of a percent mask is a plain `0…` run (no grouping).
            if int_min >= 1 {
                return Some(Fmt::Percent { decimals });
            }
        }
        return None;
    }
    // Thousands: the literal `#,##0` integer group, optionally `.0…` fractional places.
    if let Some(rest) = fmt.strip_prefix("#,##0") {
        return parse_decimals(rest).map(|decimals| Fmt::Thousands { decimals });
    }
    // Fixed: a plain `0…`(`.0…`) mask.
    parse_zero_mask(fmt).map(|(int_min, decimals)| Fmt::Fixed { int_min, decimals })
}

/// Parse a `0`-only mask like `0`, `00`, `0.00` into `(int_min_digits, decimals)`. The integer part
/// must be a non-empty run of `0`; an optional `.` introduces a non-empty run of `0` decimals. Any
/// other character (or an empty part) is unsupported (`None`).
fn parse_zero_mask(mask: &str) -> Option<(usize, usize)> {
    let (int_part, frac_part) = match mask.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (mask, None),
    };
    if int_part.is_empty() || !int_part.bytes().all(|b| b == b'0') {
        return None;
    }
    let decimals = match frac_part {
        None => 0,
        Some(f) if !f.is_empty() && f.bytes().all(|b| b == b'0') => f.len(),
        Some(_) => return None,
    };
    Some((int_part.len(), decimals))
}

/// Parse the fractional tail of a thousands mask (`""` → 0 places, or `.0…` → that many), rejecting
/// anything else.
fn parse_decimals(rest: &str) -> Option<usize> {
    if rest.is_empty() {
        return Some(0);
    }
    let frac = rest.strip_prefix('.')?;
    (!frac.is_empty() && frac.bytes().all(|b| b == b'0')).then_some(frac.len())
}

/// `TEXT(value, format)` — render `value` through the supported format subset (see `classify_format`).
/// A LITERAL format was vetted by `validate_text_format` at parse; a NON-LITERAL (computed) format
/// reaches here unvetted, so the `None` arm is a LIVE path — an unsupported RESOLVED format (e.g.
/// `TEXT(A1, B1)` where `B1` is a currency mask) is `#VALUE!`, never a wrong guess (accept-under-
/// uncertainty: the parse-time gate deferred to this eval-time check). An error `value` propagates; a
/// value that a numeric/date format cannot coerce to a number is `#VALUE!`.
pub(crate) fn text_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    let value = scalarize(ctx.eval(&args[0]));
    if let Value::Error(k) = value {
        return Value::Error(k);
    }
    let fmt = match arg_text(ctx, &args[1]) {
        Ok(s) => s,
        Err(k) => return Value::Error(k),
    };
    let Some(kind) = classify_format(&fmt) else {
        return Value::Error(ErrKind::Value);
    };
    match render_format(&value, kind) {
        Ok(s) => Value::Text(s),
        Err(k) => Value::Error(k),
    }
}

/// Render a (non-error) scalar value through a vetted format kind.
fn render_format(value: &Value, kind: Fmt) -> Result<String, ErrKind> {
    match kind {
        Fmt::General => Ok(to_text(value)?),
        Fmt::Fixed { int_min, decimals } => {
            Ok(format_number(coerce_num(value)?, decimals, int_min, false))
        }
        Fmt::Thousands { decimals } => Ok(format_number(coerce_num(value)?, decimals, 1, true)),
        Fmt::Percent { decimals } => {
            Ok(format_number(coerce_num(value)? * 100.0, decimals, 1, false) + "%")
        }
        Fmt::DateYmd => format_date_ymd(coerce_num(value)?),
    }
}

/// Format `n` with `decimals` fractional places (half-away-from-zero), a minimum of `int_min` integer
/// digits (leading-zero padded), and optional thousands grouping. The workhorse behind the fixed /
/// thousands / percent masks.
fn format_number(n: f64, decimals: usize, int_min: usize, grouping: bool) -> String {
    let (neg, mut int_digits, frac_digits) = split_scaled(n, decimals);
    while int_digits.len() < int_min {
        int_digits.insert(0, '0');
    }
    if grouping {
        int_digits = group_thousands(&int_digits);
    }
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    out.push_str(&int_digits);
    if decimals > 0 {
        out.push('.');
        out.push_str(&frac_digits);
    }
    out
}

/// Scale `|n|` by `10^decimals`, round half-away-from-zero, and split back into `(is_negative,
/// integer_digits, fractional_digits)`. The sign is dropped when the rounded magnitude is zero (Excel
/// shows `0.00`, never `-0.00`).
fn split_scaled(n: f64, decimals: usize) -> (bool, String, String) {
    let factor = 10f64.powi(decimals as i32);
    let scaled = (n.abs() * factor).round();
    let neg = n < 0.0 && scaled != 0.0;
    let mut digits = format!("{scaled:.0}");
    while digits.len() < decimals + 1 {
        digits.insert(0, '0');
    }
    let split = digits.len() - decimals;
    let int_digits = digits[..split].to_string();
    let frac_digits = digits[split..].to_string();
    (neg, int_digits, frac_digits)
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

/// Render an Excel date serial as `yyyy-mm-dd`. The integer day is `floor`ed from the serial, then
/// gated to the valid Excel date band `[1, MAX_SERIAL]` (1900-01-01 … 9999-12-31) — the SAME band the
/// sibling reader [`date_serial_arg`] enforces for YEAR/MONTH/DAY/EDATE/DATEDIF. A serial `< 1` (before
/// the 1900 epoch, rather than Excel's fictional `1900-01-00`), a serial past 9999-12-31, or a `NaN`
/// is `#VALUE!` — one located refusal consistent with every other TEXT format failure. The upper gate
/// is load-bearing: without it a large serial (`=TEXT(1e300,"yyyy-mm-dd")`) flows into `serial_to_ymd`
/// → `civil_from_days` and OVERFLOWS `i64` at `z + 719_468` — a panic under overflow-checks, or a
/// silently-wrapped nonsense date in release. The refusal replaces both with the correct located hole.
fn format_date_ymd(serial: f64) -> Result<String, ErrKind> {
    let day = serial.floor();
    if !(1.0..=MAX_SERIAL as f64).contains(&day) {
        return Err(ErrKind::Value);
    }
    let (y, m, d) = serial_to_ymd(day as i64);
    Ok(format!("{y:04}-{m:02}-{d:02}"))
}

/// Unix day index of 1899-12-31 (Excel "serial 0" in the pre-bug half); the shared epoch anchor for
/// both directions of the serial↔date map (`serial_to_ymd` and its inverse `serial_from_ymd`).
pub(crate) const EPOCH_1899_12_31: i64 = -25568;

/// Convert an Excel date serial (integer, `>= 1`) to a proleptic-Gregorian `(year, month, day)` in the
/// **1900 date system, WITH Excel's leap-year bug replicated** (serial 60 is the fictional
/// `1900-02-29`; serials `>= 61` are shifted back one day to skip it, so serial 61 is `1900-03-01`).
/// This epoch/bug fidelity is the load-bearing date call — a real Excel-authored serial round-trips.
pub(crate) fn serial_to_ymd(serial: i64) -> (i64, u32, u32) {
    // The phantom leap day Excel invented has no real civil date; report it verbatim.
    if serial == 60 {
        return (1900, 2, 29);
    }
    // Serials 1..59 add straight through (serial 1 = 1900-01-01); serials > 60 lose one day (the
    // phantom 1900-02-29) so the calendar re-aligns with reality (serial 61 = 1900-03-01).
    let unix_days = if serial < 60 {
        EPOCH_1899_12_31 + serial
    } else {
        EPOCH_1899_12_31 + serial - 1
    };
    civil_from_days(unix_days)
}

/// Howard Hinnant's `civil_from_days`: days since the Unix epoch (1970-01-01) → proleptic-Gregorian
/// `(year, month, day)`. Exact integer arithmetic, valid across the whole date range v1 cares about.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (y + i64::from(m <= 2), m, d)
}

/// Parse-time gate for `TEXT`: refuse ONLY an UNSUPPORTED STRING LITERAL format code (the subset
/// `classify_format` accepts) — a statically-known-wrong format is caught up front rather than mis-rendered at eval. A
/// non-literal format (a reference/computed string, e.g. `TEXT(A1, B1)`) is ACCEPTED and deferred to
/// `text_fn`, which returns `#VALUE!` iff the RESOLVED format turns out unsupported. This is
/// accept-under-uncertainty (ast-standards PART 6): a false-reject is the cardinal sin, so a dynamic
/// format that RESOLVES to a supported code (`B1="0.00"`) — which real Excel accepts and computes —
/// must not be rejected up front; the only deferred gap is a false-*negative* (an unsupported dynamic
/// format becomes eval's `#VALUE!`, not a parse refusal). Registered as `TEXT`'s `validate` row so the
/// check stays registry data, not a hand-fork in the parser.
pub(crate) fn validate_text_format(args: &[Expr], span: Span) -> Result<(), Diag> {
    // Arity (exactly 2) is already checked; guard defensively so a synthesized short call can't panic.
    match args.get(1) {
        // The one static-certainty case: a literal format string that is NOT in the supported subset.
        Some(Expr::Lit(Value::Text(fmt))) if classify_format(fmt).is_none() => Err(Diag::new(
            DiagCode::UnsupportedFormat,
            span,
            format!("TEXT format code {fmt:?} is not in the supported v1 subset"),
        )),
        // A supported literal, OR any non-literal format v1 cannot vet statically: accept and defer to
        // eval's resolved-format `#VALUE!` rather than false-reject a call Excel would compute.
        _ => Ok(()),
    }
}
