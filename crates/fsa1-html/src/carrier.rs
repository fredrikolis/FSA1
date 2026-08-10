// Concern: locates where a sidecar's text leaves open what its <style> must close | Non-concern: CSS grammar, what a refusal says (fsa1-verbs/ops.rs) | IO: (&str) -> Option<(line, col)>

/// A sidecar's bytes ride a raw-text `<style>` WHOLE, so the page paints what `Overlay::cell_style`
/// resolves only while the text closes every context it opens: left mid-`/*`, mid-quote or inside an
/// unbalanced `[`/`(`, every later rule reads as more of it, and `</style` ends the element outright
/// wherever it sits. Nothing else is located, no CSS being parsed. Line and column are 1-based.
pub fn unholdable(text: &str) -> Option<(u32, u32)> {
    /// A context the text has entered and not yet left.
    enum Open {
        Comment,
        Quote(char),
        Bracket(char),
    }

    let bytes = text.as_bytes();
    let mut open: Vec<(Open, u32, u32)> = Vec::new();
    let (mut line, mut col) = (1u32, 1u32);
    // The second byte of a two-byte token, or the character an escape spoke for: already accounted.
    let mut eaten = false;

    for (at, c) in text.char_indices() {
        let rest = &bytes[at..];
        if c == '<' && rest.len() >= 7 && rest[..7].eq_ignore_ascii_case(b"</style") {
            return Some((line, col));
        }
        if eaten {
            eaten = false;
        } else {
            match open.last() {
                Some((Open::Comment, ..)) => {
                    if c == '*' && rest.starts_with(b"*/") {
                        open.pop();
                        eaten = true;
                    }
                }
                Some((Open::Quote(q), ..)) => {
                    if c == '\\' {
                        eaten = true;
                    } else if c == *q {
                        open.pop();
                    }
                }
                _ => match c {
                    '/' if rest.starts_with(b"/*") => {
                        open.push((Open::Comment, line, col));
                        eaten = true;
                    }
                    '"' | '\'' => open.push((Open::Quote(c), line, col)),
                    '[' | '(' => open.push((Open::Bracket(c), line, col)),
                    ']' if matches!(open.last(), Some((Open::Bracket('['), ..))) => {
                        open.pop();
                    }
                    ')' if matches!(open.last(), Some((Open::Bracket('('), ..))) => {
                        open.pop();
                    }
                    _ => {}
                },
            }
        }
        if c == '\n' {
            (line, col) = (line + 1, 1);
        } else {
            col += 1;
        }
    }
    // The OUTERMOST context still open: closing it is what puts the later rules back on the page.
    open.first().map(|&(_, line, col)| (line, col))
}

#[cfg(test)]
mod tests {
    use super::unholdable;

    /// Every spelling that ends the element, wherever the author put it: the case is irrelevant, and
    /// so is what follows the sequence — whitespace ends it exactly as `>` does. A CSS comment is no
    /// shelter, the element ending before any CSS is read.
    #[test]
    fn every_spelling_of_the_closing_sequence_is_located() {
        assert_eq!(unholdable("</style>"), Some((1, 1)));
        assert_eq!(
            unholdable("  fsa1-cell { color: #ff0000 }\nAr</STYLE>ial"),
            Some((2, 3))
        );
        assert_eq!(unholdable("x\n\n</style\n"), Some((3, 1)));
        assert_eq!(unholdable("</styles"), Some((1, 1)));
        assert_eq!(
            unholdable("  fsa1-cell { font-family: Arial</STYLE }\n"),
            Some((1, 33))
        );
        assert_eq!(unholdable("/* </style> */"), Some((1, 4)));
    }

    /// `font-family` is free text and reaches the `<style>` raw, so it is where an unclosed context
    /// escapes a declaration. Each of these swallows the rule after it, and the column named is the
    /// opener the author has to close.
    #[test]
    fn a_context_left_open_is_located_where_it_opened() {
        let later = "\n  fsa1-cell:last-child { background-color: #00ff00 }\n";
        for (open, col) in [("/*evil", 33), ("[evil", 33), ("\"evil", 33), ("(evil", 33)] {
            let text = format!("  fsa1-cell {{ font-family: Arial{open} }}{later}");
            assert_eq!(unholdable(&text), Some((1, col)), "{text:?} is unholdable");
        }
    }

    /// The near misses an over-eager scan would take: each is holdable text a `<style>` carries to
    /// the browser whole, and refusing one would refuse a sidecar the model accepts. A context that
    /// CLOSES is carried however it nests, which is why no byte is banned outright.
    #[test]
    fn a_near_miss_is_holdable() {
        for holdable in [
            "Arial",
            "Times New Roman",
            "Ar<ial",
            "Ar/ial",
            "Arial/*fine*/",
            "Arial/*[*/",
            "\"Ar[ial\"",
            "\"a\\\"b\"",
            "url(fine)",
            "fsa1-cell[hidden] { display: none }",
            "</styl",
            "< /style>",
            "</ style>",
        ] {
            assert_eq!(unholdable(holdable), None, "{holdable:?} is holdable");
        }
    }
}
