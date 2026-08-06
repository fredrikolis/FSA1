// Concern: xl/sharedStrings.xml — interning each distinct text value at a stable index | Non-concern: the <c> element, non-text values | IO: (text values) -> the part bytes + an index each

use std::collections::HashMap;

use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};

const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const INFALLIBLE: &str = "writing XML to an in-memory buffer is infallible";

pub(crate) struct SharedStrings {
    entries: Vec<String>,
    index: HashMap<String, usize>,
    reference_count: u64,
}

impl SharedStrings {
    pub(crate) fn new() -> Self {
        SharedStrings {
            entries: Vec::new(),
            index: HashMap::new(),
            reference_count: 0,
        }
    }

    pub(crate) fn intern(&mut self, text: &str) -> usize {
        self.reference_count += 1;
        if let Some(&i) = self.index.get(text) {
            return i;
        }
        let i = self.entries.len();
        self.entries.push(text.to_string());
        self.index.insert(text.to_string(), i);
        i
    }

    pub(crate) fn emit(&self) -> Vec<u8> {
        let mut w = Writer::new(Vec::new());
        w.write_event(Event::Decl(BytesDecl::new(
            "1.0",
            Some("UTF-8"),
            Some("yes"),
        )))
        .expect(INFALLIBLE);
        let mut sst = BytesStart::new("sst");
        sst.push_attribute(("xmlns", NS_MAIN));
        let count = self.reference_count.to_string();
        let unique = self.entries.len().to_string();
        sst.push_attribute(("count", count.as_str()));
        sst.push_attribute(("uniqueCount", unique.as_str()));
        w.write_event(Event::Start(sst)).expect(INFALLIBLE);
        for value in &self.entries {
            w.write_event(Event::Start(BytesStart::new("si")))
                .expect(INFALLIBLE);
            let mut t = BytesStart::new("t");
            t.push_attribute(("xml:space", "preserve"));
            w.write_event(Event::Start(t)).expect(INFALLIBLE);
            w.write_event(Event::Text(BytesText::new(value)))
                .expect(INFALLIBLE);
            w.write_event(Event::End(BytesEnd::new("t")))
                .expect(INFALLIBLE);
            w.write_event(Event::End(BytesEnd::new("si")))
                .expect(INFALLIBLE);
        }
        w.write_event(Event::End(BytesEnd::new("sst")))
            .expect(INFALLIBLE);
        w.into_inner()
    }
}
