// Concern: spells each figure as a <figure> and the one script that mounts it | Non-concern: expanding a spec, the table | IO: (bound specs) -> markup + a script

/// The pinned runtime, inlined so the export stays ONE self-contained file: a CDN `src=` would make
/// a saved page a page that stops drawing the day the network is gone. `build.rs` locates it.
const RUNTIME: &str = include_str!(env!("FSA1_VEGA_BUNDLE_PATH"));

/// Compiles each spec down to Vega and mounts it beside its own caption. One pass over the
/// `<figure>` elements, so a document of any size costs one listener-free run.
const MOUNT: &str = "\
document.querySelectorAll('figure.fsa1-fig').forEach(function(f){\
var spec=JSON.parse(f.querySelector('script.fsa1-spec').textContent);\
var view=new vega.View(vega.parse(vegaLite.compile(spec).spec),\
{renderer:'svg',container:f.querySelector('div.fsa1-fig-view')});\
view.runAsync();});
";

/// The whole figure block, or the EMPTY string where the document holds none — a workbook stating no
/// figure carries no runtime, no `<figure>` and not one extra byte. `figures` is `(name, bound spec)`.
pub(crate) fn block(figures: &[(String, String)]) -> String {
    if figures.is_empty() {
        return String::new();
    }
    let mut out = format!("\n<script>{RUNTIME}</script>");
    for (name, spec) in figures {
        out.push_str(&format!(
            "\n<figure class=\"fsa1-fig\"><figcaption>{caption}</figcaption>\
             <div class=\"fsa1-fig-view\"></div>\
             <script class=\"fsa1-spec\" type=\"application/json\">{spec}</script></figure>",
            caption = crate::escape::text(name),
            spec = script_json(spec),
        ));
    }
    out.push_str(&format!("\n<script>{MOUNT}</script>"));
    out
}

/// A `<script>` is a RAW-TEXT element, so an author's cell text spelling `</script>` inside the spec
/// would end it and turn the remainder into markup. `<` occurs in JSON only inside a string, where
/// `<` is the same character — so escaping every one of them is lossless and closes the hole.
fn script_json(spec: &str) -> String {
    spec.replace('<', "\\u003c")
}
