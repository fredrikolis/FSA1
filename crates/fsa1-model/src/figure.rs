// Concern: a figure's Vega-Lite spec, the ranges it binds and the document they complete | Non-concern: finding them on disk (figures.rs), drawing one | IO: (name, text) -> Figure; (&Workbook) -> a spec
//! `datasets` is optional in the Vega-Lite schema, so a `<name>.vl.json` on disk is already
//! schema-valid and complete — merely UNBOUND. Expansion is pure addition of one top-level key, and
//! the file an author reads is the file they wrote.

use fsa1_ast::Value;
use fsa1_ast::a1::{format_cell, parse_a1};
use serde_json::{Map, Value as Json};

use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::names::one_axis;
use crate::overlap::Rect;
use crate::render::display_value;
use crate::workbook::Workbook;

/// The top-level key expansion adds, and the one Vega-Lite resolves a `NamedData` against.
const DATASETS: &str = "datasets";

/// One figure: the entry it was read from, and the spec that entry holds.
#[derive(Clone, Debug)]
pub struct Figure {
    /// As it LOCATES — `<tab>/<name>.vl.json` — so every refusal anchors on the file an author edits.
    pub name: String,
    pub spec: Json,
}

impl Figure {
    /// A parse failure carries [`Loc::Body`] from `serde_json`'s own line/column, the only positions
    /// that survive the read: a parsed [`Json`] keeps none.
    pub fn parse(name: &str, text: &str) -> Result<Figure, Diagnostic> {
        match serde_json::from_str::<Json>(text) {
            Ok(spec) => Ok(Figure {
                name: name.to_string(),
                spec,
            }),
            Err(e) => Err(Diagnostic::new(
                Code::FigureSyntax,
                // serde_json reports column 0 where the fault is the input's end; a Loc is 1-based.
                Loc::body(name, e.line().max(1) as u32, e.column().max(1) as u32),
                format!("{name} does not hold a JSON spec: {e}"),
            )),
        }
    }

    /// Every `data.name` the spec states, not just the top-level one: Vega-Lite allows a `data` per
    /// layer, so a figure over two ranges is two `data` objects and needs no mechanism of its own.
    /// In DOCUMENT order: `serde_json`'s `preserve_order` keeps an object's keys as authored, so a
    /// binding's position in the file is the position it is reported and expanded at.
    pub fn bindings(&self) -> Vec<String> {
        let mut out = Vec::new();
        collect_bindings(&self.spec, &mut out);
        out
    }

    /// The bound spec: the file's own JSON with one `datasets` key added, one entry per binding.
    /// `sheet` is the tab the figure sits in, which an unprefixed binding reads against.
    pub fn expand(&self, wb: &Workbook, sheet: u32) -> Result<Json, Vec<Diagnostic>> {
        let bindings = self.bindings();
        if bindings.is_empty() {
            return Ok(self.spec.clone());
        }
        let Some(root) = self.spec.as_object() else {
            return Err(vec![self.refuse(
                "a spec that binds data is one JSON object; this one is not, so there is nowhere \
                 to state its datasets",
            )]);
        };
        let mut diags = Vec::new();
        let mut datasets = Map::new();
        for text in bindings {
            if datasets.contains_key(&text) {
                continue; // one reference stated twice is one dataset, not a contest
            }
            match self.resolve(wb, sheet, &text) {
                Ok(rows) => {
                    datasets.insert(text, Json::Array(rows));
                }
                Err(d) => diags.push(d),
            }
        }
        if !diags.is_empty() {
            return Err(diags);
        }
        let mut bound = root.clone();
        bound.insert(DATASETS.to_string(), Json::Object(datasets));
        Ok(Json::Object(bound))
    }

    /// One binding's table: VALUES, never formulas, keyed by the field names its FIRST ROW holds.
    fn resolve(&self, wb: &Workbook, sheet: u32, text: &str) -> Result<Vec<Json>, Diagnostic> {
        let binding = Binding::parse(text).map_err(|why| self.refuse(&why))?;
        let sheet = match &binding.tab {
            None => sheet,
            Some(tab) => wb.tab_index(tab).ok_or_else(|| {
                self.refuse(&format!(
                    "binding {text:?} names tab {tab:?}, which this workbook does not hold (tabs: \
                     {:?})",
                    wb.sheet_names()
                ))
            })?,
        };
        let rect = binding.rect;
        // A range the tab does not FILL is a typo, not a table of blanks: `A1:D99` over a four-row tab would silently chart ninety-five empty rows, which is exactly the silent miss a NAMED refusal exists to prevent.
        let used = wb.content_region(sheet);
        let fits = used.is_some_and(|u| {
            rect.min_col >= u.min_col
                && rect.min_row >= u.min_row
                && rect.max_col <= u.max_col
                && rect.max_row <= u.max_row
        });
        if !fits {
            return Err(self.refuse(&format!(
                "binding {text:?} reaches past tab {:?}, whose content is {}; name a rectangle the \
                 tab fills",
                wb.sheet_name(sheet).unwrap_or_default(),
                used.map_or("empty".to_string(), |u| u.label()),
            )));
        }
        let keys: Vec<(u32, u32, u32)> = (rect.min_row..=rect.max_row)
            .flat_map(|row| (rect.min_col..=rect.max_col).map(move |col| (sheet, col, row)))
            .collect();
        let values = wb.values_at(&keys);
        let width = (rect.max_col - rect.min_col + 1) as usize;

        let mut fields: Vec<String> = Vec::with_capacity(width);
        for (i, v) in values[..width].iter().enumerate() {
            let at = format_cell(rect.min_col + i as u32, rect.min_row);
            let field = display_value(v);
            if field.is_empty() {
                return Err(self.refuse(&format!(
                    "binding {text:?} has a blank header at {at}; a row keys on its field NAME, and \
                     an object cannot key on nothing"
                )));
            }
            if fields.contains(&field) {
                return Err(self.refuse(&format!(
                    "binding {text:?} repeats the header {field:?} at {at}; a duplicate would \
                     silently drop a column"
                )));
            }
            fields.push(field);
        }

        Ok(values[width..]
            .chunks(width)
            .map(|row| {
                let mut obj = Map::new();
                for (field, v) in fields.iter().zip(row) {
                    obj.insert(field.clone(), json_value(v));
                }
                Json::Object(obj)
            })
            .collect())
    }

    /// Located on the FILE: a parsed [`Json`] keeps no source positions, and re-scanning the text
    /// would point at the FIRST occurrence of a reference rather than the offending one.
    fn refuse(&self, message: &str) -> Diagnostic {
        Diagnostic::new(
            Code::FigureBinding,
            Loc::file(&self.name),
            message.to_string(),
        )
    }
}

/// A cell contributes its VALUE: `=B2*C2` contributes `40`. Blank is `null` so a gap is a gap; an
/// error is its DISPLAY text so a broken cell shows in the chart rather than vanishing from it; a
/// number spells as the integer it displays as, falling back to that text past 2^53 where an
/// integral f64 stops being exact, and for the non-finite values JSON cannot hold at all.
fn json_value(v: &Value) -> Json {
    match v {
        Value::Blank => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Number(n) if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 => {
            Json::Number((*n as i64).into())
        }
        Value::Number(n) => serde_json::Number::from_f64(*n)
            .map_or_else(|| Json::String(display_value(v)), Json::Number),
        Value::Text(s) => Json::String(s.clone()),
        Value::Error(_) | Value::Array(..) => Json::String(display_value(v)),
    }
}

/// A `data` object carrying a string `name` IS a binding; the walk descends everything else.
fn collect_bindings(node: &Json, out: &mut Vec<String>) {
    match node {
        Json::Object(obj) => {
            if let Some(name) = obj
                .get("data")
                .and_then(|d| d.get("name"))
                .and_then(Json::as_str)
            {
                out.push(name.to_string());
            }
            for v in obj.values() {
                collect_bindings(v, out);
            }
        }
        Json::Array(items) => {
            for v in items {
                collect_bindings(v, out);
            }
        }
        _ => {}
    }
}

/// What one `data.name` addresses. A REFERENCE, not a filename: `crate::parse_filename` is the wrong
/// parser and would refuse the right answers, rejecting `A1:A1` and `A:A` because those are illegal
/// FILE names while both are ordinary references.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    /// `None` where the reference names no tab, which reads against the figure's own.
    pub tab: Option<String>,
    pub rect: Rect,
}

impl Binding {
    /// The grammar: an optional `<tab>!` prefix, then one A1 corner or two joined by `:`. A 1x1
    /// binding is legal. A whole-column form is REFUSED and NAMED, so the answer is a refusal rather
    /// than a silent miss. `Err` carries the sentence its caller locates.
    pub fn parse(text: &str) -> Result<Binding, String> {
        let (tab, addr) = match text.rsplit_once('!') {
            Some((sheet, addr)) => (Some(unquote_sheet(sheet)?), addr),
            None => (None, text),
        };
        let corners = match addr.split_once(':') {
            Some((l, r)) => {
                if one_axis(l, r) {
                    return Err(format!(
                        "binding {text:?} names the whole column or row {addr:?}; a figure binds a \
                         CLOSED rectangle, so name both corners (`A1:A100`)"
                    ));
                }
                (corner(text, l)?, corner(text, r)?)
            }
            None => {
                let one = corner(text, addr)?;
                (one, one)
            }
        };
        let ((c0, r0), (c1, r1)) = corners;
        Ok(Binding {
            tab,
            // Normalized, because a REFERENCE may name its corners either way round.
            rect: Rect {
                min_col: c0.min(c1),
                min_row: r0.min(r1),
                max_col: c0.max(c1),
                max_row: r0.max(r1),
            },
        })
    }
}

fn corner(text: &str, part: &str) -> Result<(u32, u32), String> {
    match parse_a1(part) {
        Ok(a) => Ok((a.col, a.row)),
        Err(_) => Err(format!(
            "binding {text:?} is not an A1 reference: {part:?} is not a coordinate (write one \
             corner, or two joined by `:`, optionally prefixed `<tab>!`)"
        )),
    }
}

/// The inverse of [`crate::names::quote_sheet`], so a tab a reference had to quote reads back.
fn unquote_sheet(s: &str) -> Result<String, String> {
    let name = match s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        Some(inner) if s.len() >= 2 => inner.replace("''", "'"),
        _ => s.to_string(),
    };
    if name.is_empty() {
        return Err(format!("a binding's `{s}!` prefix names no tab"));
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tab a figure sits in, and the figure itself parsed against it.
    fn bind(files: &[(&str, &str)], spec: &str) -> Result<Json, Vec<Diagnostic>> {
        let wb = Workbook::from_tabs(&[("Sheet1", files)]).expect("the tab loads");
        Figure::parse("Sheet1/f.vl.json", spec)
            .map_err(|d| vec![d])?
            .expand(&wb, 0)
    }

    fn datasets(value: &Json) -> &Map<String, Json> {
        value[DATASETS].as_object().expect("a datasets key")
    }

    fn refusal(files: &[(&str, &str)], spec: &str) -> Diagnostic {
        let mut diags = bind(files, spec).expect_err("this figure must refuse");
        assert_eq!(diags.len(), 1, "one refusal, not {diags:?}");
        diags.pop().expect("just asserted")
    }

    /// A range resolves to a table whose FIRST ROW is the field names, and whose cells contribute
    /// VALUES: `=B2*C2` contributes `40`, never its own text.
    #[test]
    fn a_range_binds_to_a_table_its_first_row_keys_and_its_values_fill() {
        let bound = bind(
            &[("A1:C3", "item\tqty\ttotal\nnut\t4\t=B2*10\nbolt\t5\t15")],
            r#"{"data":{"name":"A1:C3"},"mark":"bar"}"#,
        )
        .expect("it binds");
        assert_eq!(
            datasets(&bound)["A1:C3"],
            serde_json::json!([
                {"item": "nut", "qty": 4, "total": 40},
                {"item": "bolt", "qty": 5, "total": 15},
            ]),
        );
        // Pure ADDITION: the author's own spec survives beside the key expansion added.
        assert_eq!(bound["mark"], serde_json::json!("bar"));
        assert_eq!(bound["data"], serde_json::json!({"name": "A1:C3"}));
    }

    /// A binding is a REFERENCE, not a filename, so the two forms `parse_filename` refuses part
    /// company here: a 1x1 is an ordinary reference, and a whole column is refused BY NAME rather
    /// than silently missed.
    #[test]
    fn a_one_by_one_reference_binds_and_a_whole_column_is_refused_by_name() {
        let bound = bind(
            &[("A1", "solo")],
            r#"{"data":{"name":"A1:A1"},"mark":"point"}"#,
        )
        .expect("a 1x1 reference is legal");
        assert_eq!(datasets(&bound)["A1:A1"], serde_json::json!([]));

        let d = refusal(
            &[("A1:A2", "h\n1")],
            r#"{"data":{"name":"A:A"},"mark":"bar"}"#,
        );
        assert_eq!(d.code, Code::FigureBinding);
        assert!(d.message.contains("whole column"), "{}", d.message);
        assert_eq!(d.loc, Loc::file("Sheet1/f.vl.json"));
    }

    /// A binding fault carries `Loc::file` and nothing finer: a parsed `Json` keeps no source
    /// positions, and re-scanning the text would point at the FIRST occurrence, not the offending one.
    #[test]
    fn a_binding_off_the_grid_is_one_refusal_located_on_the_file() {
        let d = refusal(
            &[("A1:D4", "a\tb\tc\td\n1\t2\t3\t4\n5\t6\t7\t8\n9\t1\t2\t3")],
            r#"{"data":{"name":"A1:D99"},"mark":"bar"}"#,
        );
        assert_eq!(d.code, Code::FigureBinding);
        assert_eq!(d.loc, Loc::file("Sheet1/f.vl.json"));
    }

    /// An object cannot key on nothing, and a duplicate would silently drop a column.
    #[test]
    fn a_blank_or_duplicate_header_is_one_located_refusal() {
        for grid in ["a\t\t\n1\t2\t3", "a\tb\ta\n1\t2\t3"] {
            let d = refusal(
                &[("A1:C2", grid)],
                r#"{"data":{"name":"A1:C2"},"mark":"bar"}"#,
            );
            assert_eq!(d.code, Code::FigureBinding, "{grid:?}");
            assert_eq!(d.loc, Loc::file("Sheet1/f.vl.json"), "{grid:?}");
        }
    }

    /// A blank cell is `null` so a gap stays a gap, and an error cell is its DISPLAY text so a broken
    /// cell shows in the chart rather than vanishing from it. A numeric header stringifies.
    #[test]
    fn a_blank_is_null_an_error_is_its_text_and_a_numeric_header_stringifies() {
        let bound = bind(
            &[("A1:B3", "2024\tv\nx\t\ny\t=1/0")],
            r#"{"data":{"name":"A1:B3"},"mark":"bar"}"#,
        )
        .expect("it binds");
        assert_eq!(
            datasets(&bound)["A1:B3"],
            serde_json::json!([{"2024": "x", "v": null}, {"2024": "y", "v": "#DIV/0!"}]),
        );
    }

    /// Vega-Lite allows a `data` per layer, so a figure over two ranges is two `data` objects and
    /// needs no mechanism of its own.
    #[test]
    fn every_data_name_is_walked_so_two_layers_are_two_datasets() {
        let bound = bind(
            &[("A1:B2", "x\ty\n1\t2"), ("D1:E2", "p\tq\n3\t4")],
            r#"{"layer":[{"data":{"name":"A1:B2"},"mark":"bar"},
                        {"data":{"name":"D1:E2"},"mark":"line"}]}"#,
        )
        .expect("it binds");
        let sets = datasets(&bound);
        assert_eq!(sets.len(), 2, "{sets:?}");
        assert_eq!(sets["A1:B2"], serde_json::json!([{"x": 1, "y": 2}]));
        assert_eq!(sets["D1:E2"], serde_json::json!([{"p": 3, "q": 4}]));
    }

    /// A JSON parse failure is the one figure fault with a position to carry, so it carries one.
    #[test]
    fn a_body_that_is_not_json_is_refused_on_its_line_and_column() {
        let d = Figure::parse("Sheet1/f.vl.json", "{\n  \"mark\": bar\n}")
            .expect_err("this is not JSON");
        assert_eq!(d.code, Code::FigureSyntax);
        assert_eq!(d.loc, Loc::body("Sheet1/f.vl.json", 2, 11));
    }

    /// The tab prefix, and the tab a workbook does not hold.
    #[test]
    fn a_cross_tab_binding_reads_the_tab_it_names() {
        let wb = Workbook::from_tabs(&[
            ("Sheet1", &[("A1", "1")]),
            ("Orders", &[("A1:B2", "x\ty\n7\t8")]),
        ])
        .expect("both tabs load");
        let figure = Figure::parse(
            "Sheet1/f.vl.json",
            r#"{"data":{"name":"Orders!A1:B2"},"mark":"bar"}"#,
        )
        .expect("it parses");
        let bound = figure.expand(&wb, 0).expect("it binds");
        assert_eq!(
            datasets(&bound)["Orders!A1:B2"],
            serde_json::json!([{"x": 7, "y": 8}]),
        );
        let missing = Figure::parse(
            "Sheet1/f.vl.json",
            r#"{"data":{"name":"Ghost!A1:B2"},"mark":"bar"}"#,
        )
        .expect("it parses");
        let diags = missing.expand(&wb, 0).expect_err("no such tab");
        assert_eq!(diags[0].code, Code::FigureBinding);
    }

    /// A figure's stem is a NAME, so the loader must not let the defined-name branch claim it.
    #[test]
    fn a_figure_entry_is_neither_a_cell_nor_a_defined_name() {
        let wb = Workbook::from_tabs(&[(
            "Sheet1",
            &[("A1", "1"), ("sales.vl.json", "{\"mark\":\"bar\"}")],
        )])
        .expect("the figure is skipped, not refused");
        let names = wb.name_table().names();
        assert!(
            names.is_empty(),
            "the stem is a figure, never a name: {names:?}"
        );
    }
}
