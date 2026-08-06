// Concern: encodes one Cell as a <c> element, stamping its s= index | Non-concern: row framing, building the style table, interning a string | IO: (a Cell) -> <c> bytes

use std::io::Write;

use fsa1_ast::a1::format_cell;
use fsa1_ast::{ErrKind, Value};
use fsa1_model::{Cell, CellStyle, Rect, load_error_value};
use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};

use crate::shared_strings::SharedStrings;
use crate::styles::StyleTable;

/// One cell as its row loop found it: what it holds, the style in force on it, and the region it
/// anchors as an array formula.
pub(crate) struct Sited<'a> {
    pub col: u32,
    pub cell: &'a Cell,
    pub style: CellStyle,
    pub array_ref: Option<Rect>,
}

pub(crate) fn write_cell<W: Write>(
    w: &mut Writer<W>,
    row: u32,
    sited: &Sited<'_>,
    ss: &mut SharedStrings,
    styles: &StyleTable,
) -> std::io::Result<()> {
    let Sited {
        col,
        cell,
        style,
        array_ref,
    } = sited;
    let a1 = format_cell(*col, row);
    let style_index = styles.index_of(style, cell);
    match cell {
        Cell::Value {
            value: Value::Number(n),
            ..
        } => {
            write_value_cell(w, &a1, style_index, None, &n.to_string())?;
        }
        Cell::Value {
            value: Value::Text(text),
            ..
        } => {
            let idx = ss.intern(text);
            write_value_cell(w, &a1, style_index, Some("s"), &idx.to_string())?;
        }
        Cell::Value {
            value: Value::Bool(b),
            ..
        } => {
            write_value_cell(w, &a1, style_index, Some("b"), if *b { "1" } else { "0" })?;
        }
        Cell::Value {
            value: Value::Error(k),
            ..
        } => {
            write_value_cell(w, &a1, style_index, Some("e"), err_text(*k))?;
        }
        // A blank carrying a look is the one cell whose whole content is its `s=`; wearing the default look it has none, and states nothing at all.
        Cell::Value {
            value: Value::Blank,
            ..
        } => {
            if let Some(s) = style_index {
                let mut c = BytesStart::new("c");
                c.push_attribute(("r", a1.as_str()));
                c.push_attribute(("s", s.to_string().as_str()));
                w.write_event(Event::Empty(c))?;
            }
        }
        Cell::Value {
            value: Value::Array(_, _),
            ..
        } => {}
        Cell::Formula { src, .. } => {
            let body = src.strip_prefix('=').unwrap_or(src);
            let mut c = BytesStart::new("c");
            c.push_attribute(("r", a1.as_str()));
            let sattr = style_index.map(|idx| idx.to_string());
            if let Some(s) = sattr.as_deref() {
                c.push_attribute(("s", s));
            }
            w.write_event(Event::Start(c))?;
            let mut f = BytesStart::new("f");
            let range;
            if let Some(rect) = array_ref {
                range = range_ref(*rect);
                f.push_attribute(("t", "array"));
                f.push_attribute(("ref", range.as_str()));
            }
            w.write_event(Event::Start(f))?;
            w.write_event(Event::Text(BytesText::new(body)))?;
            w.write_event(Event::End(BytesEnd::new("f")))?;
            w.write_event(Event::End(BytesEnd::new("c")))?;
        }
        Cell::LoadError { diag, .. } => {
            if let Value::Error(k) = load_error_value(diag) {
                write_value_cell(w, &a1, style_index, Some("e"), err_text(k))?;
            }
        }
    }
    Ok(())
}

fn write_value_cell<W: Write>(
    w: &mut Writer<W>,
    a1: &str,
    s: Option<u32>,
    ty: Option<&str>,
    v: &str,
) -> std::io::Result<()> {
    let mut c = BytesStart::new("c");
    c.push_attribute(("r", a1));
    let sattr = s.map(|idx| idx.to_string());
    if let Some(s) = sattr.as_deref() {
        c.push_attribute(("s", s));
    }
    if let Some(t) = ty {
        c.push_attribute(("t", t));
    }
    w.write_event(Event::Start(c))?;
    w.write_event(Event::Start(BytesStart::new("v")))?;
    w.write_event(Event::Text(BytesText::new(v)))?;
    w.write_event(Event::End(BytesEnd::new("v")))?;
    w.write_event(Event::End(BytesEnd::new("c")))?;
    Ok(())
}

fn range_ref(rect: Rect) -> String {
    let tl = format_cell(rect.min_col, rect.min_row);
    if rect.min_col == rect.max_col && rect.min_row == rect.max_row {
        tl
    } else {
        format!("{tl}:{}", format_cell(rect.max_col, rect.max_row))
    }
}

fn err_text(k: ErrKind) -> &'static str {
    match k {
        ErrKind::Ref => "#REF!",
        ErrKind::Div0 => "#DIV/0!",
        ErrKind::Value => "#VALUE!",
        ErrKind::Name => "#NAME?",
        ErrKind::Na => "#N/A",
        ErrKind::Null => "#NULL!",
        ErrKind::Num => "#NUM!",
        ErrKind::Spill => "#SPILL!",
        ErrKind::Calc => "#CALC!",
    }
}
