// Concern: docProps/core.xml and app.xml — timestamp-free metadata templates | Non-concern: the relationships, the content-type declaration | IO: () -> the part bytes

const CORE_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    "\n",
    r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" "#,
    r#"xmlns:dcmitype="http://purl.org/dc/dcmitype/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"></cp:coreProperties>"#,
);

const APP_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    "\n",
    r#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><Application>"#,
    r#"fsa1-cli</Application></Properties>"#,
);

pub(crate) fn emit_core() -> Vec<u8> {
    CORE_XML.as_bytes().to_vec()
}

pub(crate) fn emit_app() -> Vec<u8> {
    APP_XML.as_bytes().to_vec()
}
