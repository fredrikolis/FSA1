// Concern: escapes author text for the two contexts it lands in — markup, and the raw-text <style> element | Non-concern: what is emitted, the CSS a style spells to | IO: (&str) -> escaped text

/// The document's security boundary: a cell's text and a tab's name are author-controlled, so
/// `<script>alert(1)</script>` in a cell must arrive as characters. `"` is escaped here too, so one
/// function serves both element text and a quoted attribute.
pub(crate) fn text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// The ONE free-text style value, `font-family`, which [`fsa1_model::Declaration::font_family`] lets
/// through nearly whole. An ALLOW-list, because a `<style>` element is raw text where entities never
/// decode: naming the dangerous bytes missed `/` (opens a comment) and `[` (consumes to `]`), each
/// swallowing every later rule. Outside the set becomes a hex escape: an unforeseen byte is no hole.
pub(crate) fn css_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-') {
            out.push(c);
        } else {
            out.push_str(&format!("\\{:x} ", c as u32));
        }
    }
    out
}
