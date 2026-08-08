// Concern: derives the chart a figure states, else why Excel has none, and holds the ONE mark-element table | Non-concern: the part's bytes (chart.rs) | IO: (Figure, tab) -> Chart; mark <-> element
//! The chart type comes from `mark` and `encoding` and from nothing else: a spec whose metadata went
//! stale would otherwise write a chart contradicting itself, and a hand-authored figure carries none
//! to consult. This pass derives only the shapes a chart states exactly, because whether the result
//! is the figure it came from is settled by reading the chart back, never by a second opinion here.

use fsa1_model::{Binding, Figure, Rect, Workbook, quote_sheet};
use serde_json::{Map, Value as Json};

use crate::chart::{Chart, ChartSeries, cell};

/// The Vega-Lite mark each Excel plot element draws, DEFINED ONCE here because this crate writes the
/// element, so the spelling is its fact. It stays private: a caller reads it forward with
/// [`element_for`] to write a chart and backward with [`mark_for`] to read one, and the two
/// directions cannot drift because there is only one table to drift from.
const MARKS: [(&str, &str); 5] = [
    ("bar", "barChart"),
    ("line", "lineChart"),
    ("area", "areaChart"),
    ("point", "scatterChart"),
    ("arc", "pieChart"),
];

/// The Excel plot element a Vega-Lite `mark` draws as, or `None` where Excel has no chart for it.
pub(crate) fn element_for(mark: &str) -> Option<&'static str> {
    MARKS
        .iter()
        .find(|(name, _)| *name == mark)
        .map(|(_, element)| *element)
}

/// The Vega-Lite mark an Excel plot `element` draws, or `None` where it has none.
pub fn mark_for(element: &str) -> Option<&'static str> {
    MARKS
        .iter()
        .find(|(_, name)| *name == element)
        .map(|(mark, _)| *mark)
}

/// The ONE sentence naming a plot `element` [`mark_for`] has no mark for. The element's own spelling
/// is this crate's fact, so a reader is handed the sentence rather than composing it.
pub fn no_mark_reason(element: &str) -> String {
    format!("a <c:{element}> has no Vega-Lite mark")
}

/// The keys a spec may state at the top and still be one chart, and the ones a layer inside it may.
/// `usermeta` is ADVISORY: it may only refine a chart the SPEC has already decided, and is never
/// consulted for the chart's own shape. Nothing this writer emits is refinable yet, so it refines
/// nothing today.
const SPEC_KEYS: [&str; 6] = ["$schema", "title", "usermeta", "mark", "data", "encoding"];
const LAYER_KEYS: [&str; 3] = ["mark", "data", "encoding"];

/// The chart `figure` states over tab `sheet`, or the ONE sentence naming what Excel has no chart
/// for — which is what an author simplifies their spec against.
pub fn chart_for(wb: &Workbook, sheet: u32, figure: &Figure) -> Result<Chart, String> {
    let root = figure
        .spec
        .as_object()
        .ok_or("its spec is not one JSON object, so it states no chart")?;
    let title = match root.get("title") {
        None => None,
        Some(Json::String(text)) => Some(text.clone()),
        Some(other) => {
            return Err(format!(
                "its title is {other}, and an Excel chart's title is text"
            ));
        }
    };
    let layers = layers(root)?;
    let tab = wb
        .sheet_name(sheet)
        .ok_or("the tab it sits in is not in this workbook")?
        .to_string();

    let mut element = None;
    let mut horizontal = false;
    let mut series = Vec::with_capacity(layers.len());
    for layer in layers {
        let (one, swapped) = plot(layer)?;
        match element {
            None => element = Some(one),
            Some(first) if first == one => {}
            Some(first) => {
                return Err(format!(
                    "its layers state unlike marks ({first} and {one}), and one Excel chart draws one"
                ));
            }
        }
        if swapped && one != "barChart" {
            return Err(format!(
                "it plots its category on `y`, which only a bar chart states in Excel and a <c:{one}> \
                 cannot"
            ));
        }
        horizontal = swapped;
        series.push(one_series(wb, sheet, &tab, layer)?);
    }
    let element = element.ok_or("it states no layer at all")?;
    Ok(Chart {
        sheet,
        title,
        element,
        horizontal,
        series,
    })
}

/// A spec's layers in document order: the `layer` array where it states one, else the spec itself.
/// A key beside those is content Excel has no chart for, and is named rather than dropped in silence.
fn layers(root: &Map<String, Json>) -> Result<Vec<&Map<String, Json>>, String> {
    let Some(stated) = root.get("layer") else {
        // A lone layer states its mark and data at the top, beside the keys a whole spec may carry.
        unexpected(root, &SPEC_KEYS)?;
        return Ok(vec![root]);
    };
    unexpected(root, &["$schema", "title", "layer"])?;
    let items = stated
        .as_array()
        .ok_or("its `layer` is not an array of layers")?;
    if items.is_empty() {
        return Err("it states no layer at all".to_string());
    }
    items
        .iter()
        .map(|one| {
            let one = one
                .as_object()
                .ok_or_else(|| "one of its layers is not a JSON object".to_string())?;
            unexpected(one, &LAYER_KEYS)?;
            Ok(one)
        })
        .collect()
}

fn unexpected(object: &Map<String, Json>, allowed: &[&str]) -> Result<(), String> {
    match object.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(key) => Err(format!("it states {key:?}, which no Excel chart expresses")),
        None => Ok(()),
    }
}

/// One layer's plot element, and whether its category sits on `y` — the horizontal bar.
fn plot(layer: &Map<String, Json>) -> Result<(&'static str, bool), String> {
    let mark = layer
        .get("mark")
        .and_then(Json::as_str)
        .ok_or("a layer states no `mark` string")?;
    let element =
        element_for(mark).ok_or_else(|| format!("Excel has no chart for the mark {mark:?}"))?;
    let (_, _, swapped) = axes(layer, element)?;
    Ok((element, swapped))
}

/// The two fields a layer plots and which axis the category sits on. A pie has no axes: the value is
/// the angle and the category is what colours the slice.
fn axes(layer: &Map<String, Json>, element: &str) -> Result<(String, String, bool), String> {
    let encoding = layer
        .get("encoding")
        .and_then(Json::as_object)
        .ok_or("a layer states no `encoding` object")?;
    let channel = |name: &str| -> Result<(String, String), String> {
        let object = encoding
            .get(name)
            .and_then(Json::as_object)
            .ok_or_else(|| format!("its encoding states no `{name}` channel"))?;
        unexpected(object, &["field", "type"])?;
        let read = |key: &str| {
            object
                .get(key)
                .and_then(Json::as_str)
                .map(str::to_string)
                .ok_or_else(|| format!("its `{name}` channel states no {key} string"))
        };
        Ok((read("field")?, read("type")?))
    };
    if element == "pieChart" {
        unexpected(encoding, &["theta", "color"])?;
        return Ok((channel("color")?.0, channel("theta")?.0, false));
    }
    unexpected(encoding, &["x", "y"])?;
    let (x, x_type) = channel("x")?;
    let (y, y_type) = channel("y")?;
    match (x_type.as_str(), y_type.as_str()) {
        (_, "quantitative") => Ok((x, y, false)),
        ("quantitative", _) => Ok((y, x, true)),
        _ => Err(format!(
            "neither of its axes is quantitative ({x_type} against {y_type}), and an Excel series \
             plots numbers"
        )),
    }
}

/// One layer as one `<c:ser>`: the two columns of its bound rectangle that its encoding names, and
/// the header cell of the value column, which is what an Excel series is named by.
fn one_series(
    wb: &Workbook,
    sheet: u32,
    tab: &str,
    layer: &Map<String, Json>,
) -> Result<ChartSeries, String> {
    let element = plot(layer)?.0;
    let (category, quantity, _) = axes(layer, element)?;
    let name = layer
        .get("data")
        .and_then(Json::as_object)
        .ok_or("a layer states no `data` object")?;
    unexpected(name, &["name"])?;
    let text = name
        .get("name")
        .and_then(Json::as_str)
        .ok_or("its `data` names no binding")?;
    let binding = Binding::parse(text)?;
    if binding.tab.as_deref().is_some_and(|named| named != tab) {
        return Err(format!(
            "its binding {text:?} names another tab, and a chart plots the tab it is drawn on"
        ));
    }
    let rect = binding.rect;
    if rect.max_row == rect.min_row {
        return Err(format!(
            "its binding {text:?} is one row, which is a header with nothing under it"
        ));
    }
    let cat_col = column(wb, sheet, rect, &category)?;
    let val_col = column(wb, sheet, rect, &quantity)?;
    let column_ref = |col: u32| {
        format!(
            "{}!{}:{}",
            quote_sheet(tab),
            cell(col, rect.min_row + 1),
            cell(col, rect.max_row)
        )
    };
    Ok(ChartSeries {
        name_ref: format!("{}!{}", quote_sheet(tab), cell(val_col, rect.min_row)),
        cat: column_ref(cat_col),
        val: column_ref(val_col),
    })
}

/// The column of `rect` whose HEADER is `field`. A formula header is refused here rather than
/// written: its content and its value are two spellings of one cell, and a series would plot one
/// while the field named the other.
fn column(wb: &Workbook, sheet: u32, rect: Rect, field: &str) -> Result<u32, String> {
    for col in rect.min_col..=rect.max_col {
        let header = match wb.source_at(sheet, col, rect.min_row).map(|s| s.cell) {
            Some(fsa1_model::Cell::Formula { .. }) => continue,
            _ => fsa1_model::display_value(&wb.value_at(sheet, col, rect.min_row)),
        };
        if header == field {
            return Ok(col);
        }
    }
    Err(format!(
        "no column of the rectangle it binds is headed {field:?}"
    ))
}
