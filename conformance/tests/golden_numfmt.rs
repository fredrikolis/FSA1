// Concern: binds fsa1-ast's format_value to every golden numFmt vector | Non-concern: the Python leg, authoring the vectors | IO: (golden_numfmt.json) -> pass/fail

use std::path::{Path, PathBuf};

use fsa1_ast::{Value, format_value};

fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("serde")
        .join("golden_numfmt.json")
}

struct Vector {
    id: u32,
    format: String,
    value: f64,
    expected: String,
}

/// Exact because the golden file is one vector per line, with no `"` or `\` inside any field.
fn str_field(line: &str, key: &str) -> Option<String> {
    let after = &line[line.find(&format!("\"{key}\""))? + key.len() + 2..];
    let after = &after[after.find(':')? + 1..];
    let start = after.find('"')? + 1;
    let end = after[start..].find('"')? + start;
    Some(after[start..end].to_string())
}

fn num_field(line: &str, key: &str) -> Option<f64> {
    let after = &line[line.find(&format!("\"{key}\""))? + key.len() + 2..];
    let after = &after[after.find(':')? + 1..];
    let end = after.find([',', '}']).unwrap_or(after.len());
    after[..end].trim().parse().ok()
}

fn parse_vectors(text: &str) -> Vec<Vector> {
    text.lines()
        .filter(|l| l.contains("\"format\""))
        .map(|l| Vector {
            id: num_field(l, "id").expect("vector has an id") as u32,
            format: str_field(l, "format").expect("vector has a format code"),
            value: num_field(l, "value").expect("vector has a value"),
            expected: str_field(l, "expected").expect("vector has an expected string"),
        })
        .collect()
}

#[test]
fn fsa1_ast_reproduces_every_ecma376_golden_numfmt_vector() {
    let text = std::fs::read_to_string(golden_path()).expect("golden_numfmt.json must be readable");
    let vectors = parse_vectors(&text);
    assert_eq!(
        vectors.len(),
        12,
        "expected 12 golden numFmt vectors (see golden_numfmt.json header + PROVENANCE.md)"
    );
    for v in &vectors {
        let got = format_value(&Value::Number(v.value), &v.format);
        assert_eq!(
            got,
            Value::Text(v.expected.clone()),
            "vector #{} ({:?}, {}) — fsa1-ast rendered {:?}, ECMA-376 golden is {:?}; fix FSA1, \
             never edit the golden",
            v.id,
            v.format,
            v.value,
            got,
            v.expected,
        );
    }
}
