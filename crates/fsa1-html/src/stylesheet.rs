// Concern: interns each distinct CellStyle under one class and spells the <style> block | Non-concern: which cell holds a style, a column's WIDTH (table.rs states it) | IO: (styles, any_width) -> CSS

use std::fmt::Write;

use fsa1_model::{CellStyle, Declaration};

use crate::escape;

/// `border-collapse` hands the browser CSS 2.1 §17.6.2.1 conflict resolution over the edge two
/// adjacent cells share, so each declares only its OWN borders. `pre-wrap` makes a cell's newlines
/// and spaces read as they do in ASCII. `table-layout: fixed` is what makes an authored width BIND —
/// `auto` treats one as a suggestion it may overrule to fit content.
const BASE: &str = "\
table { border-collapse: collapse; font-family: sans-serif; font-size: 10pt }
caption { font-weight: bold; padding: 4px 0; text-align: left }
th, td { border: 1px solid #dddddd; padding: 2px 6px; white-space: pre-wrap }
th { background-color: #f0f0f0; font-weight: normal }
";

/// Every other declaration spells from a typed value — a hex colour, a bounded point size, a closed
/// keyword set — so `font-family` is the only one whose text an author writes. Listed rather than
/// wildcarded: a later variant carrying free text must fail to compile here, not reach the raw-text
/// `<style>` element unescaped.
fn spell_safely(d: &Declaration) -> String {
    match d {
        Declaration::FontFamily(name) => {
            format!("{}: {}", d.property(), escape::css_value(name))
        }
        Declaration::BackgroundColor(_)
        | Declaration::Border { .. }
        | Declaration::Color(_)
        | Declaration::FontSize(_)
        | Declaration::FontStyle(_)
        | Declaration::FontWeight(_)
        | Declaration::Height(_)
        | Declaration::TextAlign(_)
        | Declaration::TextDecoration(_)
        | Declaration::VerticalAlign(_)
        | Declaration::WhiteSpace(_)
        | Declaration::Width(_) => d.spell(),
    }
}

#[derive(Default)]
pub(crate) struct Classes(Vec<CellStyle>);

impl Classes {
    /// First appearance in the document's own cell order fixes a style's index, so one workbook
    /// yields one stylesheet byte for byte. `None` is a style that declares nothing — no class at all
    /// rather than an empty rule.
    pub(crate) fn intern(&mut self, mut style: CellStyle) -> Option<String> {
        // A width belongs to the COLUMN: the <colgroup> states it, and per cell it does nothing.
        style.width = None;
        if style.declarations().is_empty() {
            return None;
        }
        let at = self.0.iter().position(|s| *s == style).unwrap_or_else(|| {
            self.0.push(style);
            self.0.len() - 1
        });
        Some(format!("c{at}"))
    }

    /// `fixed` only where a width is actually authored: it is what makes one BIND, and it also
    /// stops an unwidened column sizing to its content, so a document stating none keeps `auto`.
    pub(crate) fn css(&self, any_width: bool) -> String {
        let mut out = String::from(BASE);
        if any_width {
            out.push_str("table { table-layout: fixed }\n");
        }
        for (at, style) in self.0.iter().enumerate() {
            let spelled: Vec<String> = style.declarations().iter().map(spell_safely).collect();
            let _ = writeln!(out, ".c{at} {{ {} }}", spelled.join("; "));
        }
        out
    }
}
