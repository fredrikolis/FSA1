// Concern: spells the formula bar the document splices in | Non-concern: which cell is focused, what a cell's address or formula IS (table.rs writes both) | IO: none -> markup, CSS and a script

pub(crate) const MARKUP: &str =
    r#"<div id="fx"><span id="fx-ref"></span><span id="fx-src"></span></div>"#;

/// The bar is `position: sticky` so it stays put over a long sheet, exactly as a spreadsheet's does.
pub(crate) const CSS: &str = "\
#fx { position: sticky; top: 0; background: #ffffff; border-bottom: 1px solid #cccccc; \
display: flex; font-family: monospace; font-size: 10pt; gap: 8px; padding: 4px 6px }
#fx-ref { color: #666666; min-width: 6ch }
td:focus { outline: 2px solid #3f6fb7 }
";

/// Reads `data-ref` and `data-formula` off the focused cell and shows its own text where it holds no
/// formula. One listener on the table, so a sheet of any size costs one handler.
pub(crate) const SCRIPT: &str = "\
document.addEventListener('focusin',function(e){\
var c=e.target.closest&&e.target.closest('td');if(!c)return;\
document.getElementById('fx-ref').textContent=c.dataset.ref||'';\
document.getElementById('fx-src').textContent=c.dataset.formula||c.textContent;});
";
