// Concern: grades a written chart against the figure it came from | Non-concern: writing it (fsa1-xlsx), spelling a figure (figure_body.rs) | IO: (chart XML, a Figure) -> Ok, else the difference
//! Whether Excel can hold a figure is decided by WRITING the chart and reading it back through the
//! same reader an unpack uses — one definition of "representable", derived from the mapping rather
//! than maintained beside it. There is deliberately no second classifier of "too fancy": a chart
//! reading back as the figure it came from ships, and one that does not is dropped and named.

use fsa1_model::{Binding, Figure, Rect, Workbook};
use serde_json::{Map, Value as Json};

use crate::figure_body::{BookTable, ChartTable, spell};
use fsa1_xlsx::{CHART_PART, parse_chart};

/// `Ok` where the chart part `xml` reads back as the figure it was written from, and otherwise the
/// ONE sentence naming the difference — which is what an author simplifies their spec against.
pub fn chart_restates_figure(
    xml: &str,
    wb: &Workbook,
    sheet: u32,
    figure: &Figure,
) -> Result<(), String> {
    let table = BookTable { wb, sheet };
    let chart = parse_chart(CHART_PART.to_string(), table.tab().to_string(), xml)
        .map_err(|e| format!("the chart written from it does not read back: {e}"))?;
    let content = table.content();
    let (_, body) = spell(&chart, &table, content, &[])?;
    let derived: Json = serde_json::from_str(&body)
        .map_err(|e| format!("the chart written from it reads back as no spec: {e}"))?;
    same(&figure.spec, &derived, table.tab())
}

/// The mark, the series' bindings and the encodings — the three facts a chart carries. A `$schema`
/// beside them is not a difference: a chart states no schema, so it can contradict none.
fn same(want: &Json, got: &Json, tab: &str) -> Result<(), String> {
    let (want, got) = (layers(want), layers(got));
    if want.len() != got.len() {
        return Err(format!(
            "it states {} layer(s) and the chart written from it reads back as {}",
            want.len(),
            got.len()
        ));
    }
    for (at, (want, got)) in want.iter().zip(&got).enumerate() {
        // A binding is a REFERENCE: `A1:B4` and `Sheet1!A1:B4` on Sheet1 are one range spelled twice.
        let bound = (
            reference(want.get("data"), tab),
            reference(got.get("data"), tab),
        );
        let differs = match bound {
            (Some(want), Some(got)) => want != got,
            // Neither is a binding this crate can resolve, so the objects themselves are the fact.
            _ => want.get("data") != got.get("data"),
        };
        if differs {
            return Err(format!(
                "its layer {at}'s data is {}, and the chart written from it reads back as {}",
                show(want.get("data")),
                show(got.get("data"))
            ));
        }
        for key in ["mark", "encoding"] {
            if want.get(key) != got.get(key) {
                return Err(format!(
                    "its layer {at}'s {key} is {}, and the chart written from it reads back as {}",
                    show(want.get(key)),
                    show(got.get(key))
                ));
            }
        }
    }
    Ok(())
}

/// A layer's `data` as the RANGE it addresses, resolved against the tab the figure sits in. `None`
/// where it names no parseable binding, which compares by the object itself so a difference is still
/// a difference.
fn reference(data: Option<&Json>, tab: &str) -> Option<(String, Rect)> {
    let text = data?.get("name")?.as_str()?;
    let binding = Binding::parse(text).ok()?;
    Some((binding.tab.unwrap_or_else(|| tab.to_string()), binding.rect))
}

/// A spec's layers in document order: the `layer` array where it states one, else the spec itself,
/// which is how [`spell`] writes a lone layer.
fn layers(spec: &Json) -> Vec<&Map<String, Json>> {
    let Some(root) = spec.as_object() else {
        return Vec::new();
    };
    match root.get("layer").and_then(Json::as_array) {
        Some(items) => items.iter().filter_map(Json::as_object).collect(),
        None => vec![root],
    }
}

fn show(value: Option<&Json>) -> String {
    value.map_or_else(|| "absent".to_string(), Json::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fsa1_model::Workbook;

    fn book() -> Workbook {
        Workbook::from_tabs(&[(
            "Sheet1",
            &[("A1:B4", "Region\tUnits\nNorth\t12\nSouth\t9\nEast\t15")],
        )])
        .expect("the tab loads")
    }

    /// One bar chart part, handed over by the crate that spells one.
    const BAR: &str = fsa1_xlsx::BAR_CHART_PART;

    fn figure(spec: &str) -> Figure {
        Figure::parse("Sheet1/f.json", spec).expect("the spec parses")
    }

    /// The membership test in both directions over ONE chart part: the figure it was written from
    /// reads back, and one differing in any of the three facts a chart carries does not.
    #[test]
    fn a_chart_restates_the_figure_it_came_from_and_no_other() {
        let wb = book();
        let same = r#"{"mark":"bar","data":{"name":"Sheet1!A1:B4"},
            "encoding":{"x":{"field":"Region","type":"nominal"},
                        "y":{"field":"Units","type":"quantitative"}}}"#;
        assert_eq!(chart_restates_figure(BAR, &wb, 0, &figure(same)), Ok(()));

        for (spec, needle) in [
            (
                r#"{"mark":"line","data":{"name":"Sheet1!A1:B4"},
                    "encoding":{"x":{"field":"Region","type":"nominal"},
                                "y":{"field":"Units","type":"quantitative"}}}"#,
                "mark",
            ),
            (
                r#"{"mark":"bar","data":{"name":"Sheet1!A1:A4"},
                    "encoding":{"x":{"field":"Region","type":"nominal"},
                                "y":{"field":"Units","type":"quantitative"}}}"#,
                "data",
            ),
            (
                r#"{"mark":"bar","data":{"name":"Sheet1!A1:B4"},
                    "encoding":{"x":{"field":"Region","type":"ordinal"},
                                "y":{"field":"Units","type":"quantitative"}}}"#,
                "encoding",
            ),
            (
                r#"{"layer":[{"mark":"bar","data":{"name":"Sheet1!A1:B4"},
                    "encoding":{"x":{"field":"Region","type":"nominal"},
                                "y":{"field":"Units","type":"quantitative"}}},
                    {"mark":"bar","data":{"name":"Sheet1!A1:B4"},"encoding":{}}]}"#,
                "layer(s)",
            ),
        ] {
            let why = chart_restates_figure(BAR, &wb, 0, &figure(spec))
                .expect_err("this figure is not what the chart states");
            assert!(why.contains(needle), "{why}");
        }
    }

    /// A `$schema` is not a difference: a chart states none, so it can contradict none.
    #[test]
    fn a_schema_key_beside_the_three_facts_is_no_difference() {
        let wb = book();
        let spec = r#"{"$schema":"https://vega.github.io/schema/vega-lite/v5.json",
            "mark":"bar","data":{"name":"Sheet1!A1:B4"},
            "encoding":{"x":{"field":"Region","type":"nominal"},
                        "y":{"field":"Units","type":"quantitative"}}}"#;
        assert_eq!(chart_restates_figure(BAR, &wb, 0, &figure(spec)), Ok(()));
    }
}
