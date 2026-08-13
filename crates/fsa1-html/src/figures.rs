// Concern: spells one figure as a <figure> and the script that mounts every one | Non-concern: expanding a spec, deciding which cells a figure fills | IO: (a bound figure, a grid area) -> markup

use crate::BoundFigure;

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

/// The runtime, every figure no sheet drew, and the one mounting pass — the tail of a document that
/// holds a figure at all. A workbook stating none carries no runtime, no `<figure>` and not one
/// extra byte, which is [`crate::document`]'s call and not this one's. A figure here states no
/// placement, so it keeps the size its own spec asks for and follows the sheets at it.
pub(crate) fn block(after: &[&BoundFigure]) -> String {
    let mut out = format!("\n<script>{RUNTIME}</script>");
    for figure in after {
        out.push_str(&format!("\n{}", element(figure, "", &figure.spec, "")));
    }
    out.push_str(&format!("\n<script>{MOUNT}</script>"));
    out
}

/// A figure drawn IN the cells it names, `area` being the grid placement they resolve to. Those
/// cells are its whole size, which is why the spec is respelled to fill its container and the view
/// is a flex item — the caption takes its line and the drawing takes the rest. `margin: 0` because a
/// page styling `.fsa1-fig` for the figures a document APPENDS would push this one off its cells.
pub(crate) fn filling(figure: &BoundFigure, area: &str) -> String {
    element(
        figure,
        &format!(
            " style=\"{area};width:100%;height:100%;margin:0;display:flex;\
             flex-direction:column;min-width:0;min-height:0\""
        ),
        &filling_cells(&figure.spec),
        " style=\"flex:1;min-height:0\"",
    )
}

fn element(figure: &BoundFigure, style: &str, spec: &str, view: &str) -> String {
    format!(
        "<figure class=\"fsa1-fig\"{style}><figcaption>{caption}</figcaption>\
         <div class=\"fsa1-fig-view\"{view}></div>\
         <script class=\"fsa1-spec\" type=\"application/json\">{spec}</script></figure>",
        caption = crate::escape::text(&figure.name),
        spec = script_json(spec),
    )
}

/// A `<script>` is a RAW-TEXT element, so an author's cell text spelling `</script>` inside the spec
/// would end it and turn the remainder into markup. `<` occurs in JSON only inside a string, where
/// `<` is the same character — so escaping every one of them is lossless and closes the hole.
fn script_json(spec: &str) -> String {
    spec.replace('<', "\\u003c")
}

/// A bound spec respelled for the carrier that draws the figure INSIDE the cells it names: there the
/// cells are the box, so the view's own `width`/`height` are REPLACED — a spec at its declared size
/// would overflow them — and `autosize` fits the drawing, its padding included, to what they resolve
/// to. `expand` yields a JSON object or nothing, so a non-object cannot reach here.
fn filling_cells(spec: &str) -> String {
    let mut root = match serde_json::from_str::<serde_json::Value>(spec) {
        Ok(serde_json::Value::Object(root)) => root,
        _ => unreachable!("a bound spec is a JSON object: Figure::expand yields no other shape"),
    };
    root.insert("width".to_string(), "container".into());
    root.insert("height".to_string(), "container".into());
    root.insert(
        "autosize".to_string(),
        serde_json::json!({"type": "fit", "contains": "padding"}),
    );
    serde_json::Value::Object(root).to_string()
}
