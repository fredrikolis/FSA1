// Concern: the STRING-MANIPULATION text worksheet functions (CONCAT CONCATENATE TEXTJOIN LEFT RIGHT MID LEN FIND SEARCH SUBSTITUTE REPLACE REPT TRIM UPPER LOWER PROPER EXACT T CLEAN TEXTBEFORE TEXTAFTER TEXTSPLIT) plus the text<->number/char CONVERTERS (VALUE NUMBERVALUE CHAR CODE UNICHAR UNICODE) — the built-ins coercing every text argument through eval.rs's `to_text` (so the function forms agree with the `&` operator) and indexing 1-based by CHARACTER; VALUE/NUMBERVALUE parse a numeric/date/time-text subset, CHAR/CODE use the Windows-1252 code page, UNICHAR/UNICODE the full Unicode scalar space | Non-concern: the value->text FORMATTING functions + the Excel number-format-code engine (func/text_format.rs owns TEXT/FIXED/DOLLAR and the serial<->date map), the registry table + dispatch (func/mod.rs), the text-coercion primitive (eval.rs owns `to_text`), and the shared `one_num`/`arg_text` helpers (func/helpers.rs) | IO: (`EvalCtx`, the call's unevaluated arg `Expr`s) -> `Value`
use super::*;

// The string built-ins. Every function coerces a text argument through eval.rs's `to_text` (so a
// number takes its GENERAL form, a boolean → TRUE/FALSE, a blank → "", an error PROPAGATES) — the
// exact rule the `&` operator uses, so the function forms and the operator agree. The Excel-semantics
// calls pinned here, each worth a reviewer's eye:
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
//   * TEXTBEFORE/TEXTAFTER split on the Nth (1-based, negative from the end) occurrence of a delimiter,
//     with an optional case-insensitive mode, end-of-text-as-delimiter flag, and if-not-found fallback
//     (default `#N/A`); TEXTSPLIT builds a 2D array by a column (and optional row) delimiter.
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
/// Shared with `func::date` (DATEVALUE/TIMEVALUE parse the SAME ISO date/time subset VALUE accepts,
/// so the one text→serial reading is single-homed here rather than re-derived).
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

// --- Text batch P-parity: T CLEAN TEXTBEFORE TEXTAFTER TEXTSPLIT NUMBERVALUE UNICHAR UNICODE.
//     (CONCATENATE is the legacy alias of CONCAT — its registry row points at `concat_fn`, so there
//     is no separate body.) ---

/// `T(value)` — the value if it is TEXT, else `""` (a number, boolean, or blank yields the empty
/// string). An error propagates (Excel `T(#DIV/0!)` is `#DIV/0!`).
pub(crate) fn t_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match scalarize(ctx.eval(&args[0])) {
        Value::Text(s) => Value::Text(s),
        Value::Error(k) => Value::Error(k),
        _ => Value::Text(String::new()),
    }
}

/// `CLEAN(text)` — strip every non-printable character (Unicode scalar `< 32`, the ASCII control
/// codes Excel's CLEAN removes) from the value's text form.
pub(crate) fn clean_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    match arg_text(ctx, &args[0]) {
        Ok(s) => Value::Text(s.chars().filter(|c| (*c as u32) >= 32).collect()),
        Err(k) => Value::Error(k),
    }
}

/// `TEXTBEFORE(text, delimiter, [instance], [match_mode], [match_end], [if_not_found])` — the text
/// before the `instance`-th occurrence of `delimiter` (see [`text_split_around`]).
pub(crate) fn textbefore_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    text_split_around(ctx, args, true)
}

/// `TEXTAFTER(text, delimiter, [instance], [match_mode], [match_end], [if_not_found])` — the text
/// after the `instance`-th occurrence of `delimiter` (see [`text_split_around`]).
pub(crate) fn textafter_fn(ctx: &mut EvalCtx, args: &[Expr]) -> Value {
    text_split_around(ctx, args, false)
}

/// The shared TEXTBEFORE/TEXTAFTER core. `instance` (default 1) selects the delimiter occurrence,
/// COUNTING FROM THE END when negative; `instance == 0` is `#VALUE!`. `match_mode` 1 folds ASCII case;
/// `match_end` 1 treats the end of `text` as a trailing delimiter. On no match, returns `if_not_found`
/// (eagerly evaluated — an error fallback surfaces even on a hit) or `#N/A`.
///
/// FOLLOW-UP: `delimiter` is read as a single string; Excel also accepts an ARRAY of alternative
/// delimiters (`TEXTBEFORE(a,{",",";"})`). Not yet modelled — the single-delimiter forms are exact.
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
    // `if_not_found` is a plain argument, so Excel evaluates it EAGERLY (TEXTBEFORE is not a lazy
    // IFERROR-family function): an error-valued fallback surfaces even when the delimiter IS found,
    // and it is an argument-evaluation error, so it precedes the body's `instance == 0` refusal.
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

/// `TEXTSPLIT(text, col_delimiter, [row_delimiter], [ignore_empty], [match_mode], [pad_with])` — split
/// `text` into a 2D array by `col_delimiter` (across) and the optional `row_delimiter` (down). With
/// `ignore_empty` TRUE, empty fields (and empty rows) are dropped; ragged rows are padded to the
/// widest with `pad_with` (default `#N/A`). `match_mode` 1 folds ASCII case. An empty result is
/// `#CALC!` (a located refusal, CORE2).
///
/// FOLLOW-UP: each delimiter is read as a single string; Excel also accepts an ARRAY of alternative
/// delimiters (`TEXTSPLIT(a,{",",";"})`). Not yet modelled — the single-delimiter forms are exact.
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

/// `NUMBERVALUE(text, [decimal_separator], [group_separator])` — parse `text` to a number with
/// EXPLICIT separators (defaults `.` / `,`): whitespace is ignored, group separators are stripped, the
/// decimal separator maps to `.`, and each trailing/embedded `%` divides the result by 100. A group
/// separator AFTER the decimal separator, equal decimal/group separators, or an otherwise unparsable
/// core is `#VALUE!`; an empty text is `0`.
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
    // Excel honours only TRAILING percent signs — each divides the result by 100; a '%' anywhere
    // else (embedded or leading) makes the whole text invalid, e.g. NUMBERVALUE("2%5") is #VALUE!.
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

/// `UNICHAR(number)` — the single character whose UNICODE code point is `number` (truncated). Valid
/// for `1..=0x10FFFF` excluding the surrogate range; anything outside (including `0`) is `#VALUE!`.
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

/// `UNICODE(text)` — the UNICODE code point of the FIRST character of `text`; an empty text is
/// `#VALUE!`. The inverse of `UNICHAR`.
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
