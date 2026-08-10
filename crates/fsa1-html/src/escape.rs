// Concern: escapes author text for the one context it lands in — markup, as text or as a quoted attribute | Non-concern: what is emitted, a sidecar's bytes | IO: (&str) -> escaped text

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
