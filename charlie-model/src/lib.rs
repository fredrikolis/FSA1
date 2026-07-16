// Concern: charlie-model — the filesystem SPREADSHEET model, exposed as: the filename<->range parser (`filename`), body classification (`body`), the broadcast-conformance validator (`conformance`), the overlap detector (`overlap`), and the single-sourced diagnostic registry (`diagnostic`); `parse_file` ties them into one loaded `ParsedFile`, proving bet B1 (filename<->range encoding + the broadcast-conformance dimension check) | Non-concern: the formula LANGUAGE (charlie-ast owns lex/parse/eval; the model stores a formula body OPAQUE), xlsx serde, and the CLI surface (charlie-cli) | IO: (a filename + file contents) -> `Result<ParsedFile, Diagnostic>`
//! # charlie-model — the filesystem spreadsheet model (W2)
//!
//! **CHARTER.** `charlie-model` owns the on-disk encoding (`FORMAT.md`): a tab is a folder and a
//! cell/range is a file whose *name* declares its A1 region and whose *body* is a literal block or
//! one opaque `=formula`. It is the middle crate of the firewall
//! `charlie-cli -> charlie-model -> charlie-ast`: it depends on `charlie-ast` for the ref/value/
//! shape types and the shared A1 grammar (the one allowed firewall edge), and the AST never learns
//! of the filesystem model (the `["charlie-ast","charlie-model"]` deny edge enforces this).
//!
//! It proves **bet B1**: the filename<->range encoding and the broadcast-conformance dimension
//! check. Everything is a *located refusal* ([`Diagnostic`]) — never a panic, never a silent drop
//! (ast-standards PART 5). W2 does not evaluate formulas (that is W3); a formula body is stored
//! verbatim and its result shape is not yet known, so conformance runs only over literal blocks.
//!
//! The **living authoritative spec** for this encoding layer is `docs/format.md` (the as-built
//! contract, including the six W2 hardening resolutions). `conformance/encoding/FORMAT.md` is the
//! FROZEN provisional snapshot the corpus was authored against — fingerprinted, do not edit it.

pub mod body;
pub mod conformance;
pub mod diagnostic;
pub mod filename;
pub mod overlap;

pub use body::{Body, LiteralBlock, classify_body, lex_literal};
pub use conformance::{Placement, classify_placement, validate_conformance};
pub use diagnostic::{Code, Diagnostic, Loc, Severity};
pub use filename::{FileKind, FileName, parse_filename};
pub use overlap::{Rect, detect_overlaps};

use charlie_ast::Shape;

/// One fully-loaded file: the filename declaration, the classified body, and — for a literal body —
/// the broadcast placement. This is the end-to-end B1 artifact for a single file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedFile {
    pub kind: FileKind,
    pub region: Rect,
    pub declared_shape: Shape,
    pub body: Body,
    /// The §6 placement for a literal body; `None` for a formula body (result shape needs eval, W3).
    pub placement: Option<Placement>,
}

/// Load one file from its name and contents: parse the filename, verify the line-1 annotation
/// (FORMAT §8), classify the body, and — for a literal body — validate broadcast conformance
/// against the declared shape. Never panics; the first violation is returned as a located
/// [`Diagnostic`].
pub fn parse_file(name: &str, contents: &str) -> Result<ParsedFile, Diagnostic> {
    let declared = parse_filename(name)?;

    // Line 1 is the mandatory `# ` annotation (FORMAT §8); the body is everything after it.
    let (line1, rest) = match contents.split_once('\n') {
        Some((first, rest)) => (first, rest),
        None => (contents, ""),
    };
    if !line1.starts_with("# ") {
        return Err(Diagnostic::new(
            Code::MissingAnnotation,
            Loc::body(name, 1, 1),
            "line 1 must be a '# ' annotation (FORMAT §8)".to_string(),
        ));
    }

    let body = classify_body(name, rest)?;
    let placement = match &body {
        Body::Literal(block) => Some(validate_conformance(
            name,
            declared.declared_shape,
            block.shape,
        )?),
        // A formula body is opaque in W2: its result shape is unknown until eval (W3), so no
        // conformance verdict yet.
        Body::Formula(_) => None,
    };

    Ok(ParsedFile {
        kind: declared.kind,
        region: declared.region,
        declared_shape: declared.declared_shape,
        body,
        placement,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use charlie_ast::Value;

    const ANN: &str = "# Concern: x | Non-concern: y | IO: input\n";

    #[test]
    fn loads_a_header_row_exact_match() {
        // FORMAT §10: A1:D1.range, declared 1x4, literal shape 1x4 -> exact.
        let contents = format!("{ANN}Product\tUnit Price\tQty\tLine Total");
        let f = parse_file("A1:D1.range", &contents).unwrap();
        assert_eq!(f.declared_shape, Shape { rows: 1, cols: 4 });
        assert_eq!(f.placement, Some(Placement::Exact));
    }

    #[test]
    fn loads_a_drag_fill_formula_opaque() {
        // FORMAT §10: D2:D6.range with a scalar =formula -> stored opaque, no conformance verdict.
        let contents = format!("{ANN}=B2*C2");
        let f = parse_file("D2:D6.range", &contents).unwrap();
        assert_eq!(f.body, Body::Formula("=B2*C2".to_string()));
        assert_eq!(f.placement, None);
    }

    #[test]
    fn loads_a_row_vector_broadcast_down() {
        // FORMAT §10.1: B2:D4.range (3x3) with a 1x3 literal row vector -> broadcast down.
        let contents = format!("{ANN}0.1\t0.2\t0.3");
        let f = parse_file("B2:D4.range", &contents).unwrap();
        assert_eq!(f.placement, Some(Placement::BroadcastDown));
    }

    #[test]
    fn loads_a_blank_cell() {
        let f = parse_file("A1.cell", "# ann only, no body").unwrap();
        assert_eq!(
            f.body,
            Body::Literal(LiteralBlock {
                shape: Shape { rows: 1, cols: 1 },
                cells: vec![Value::Blank],
            })
        );
        assert_eq!(f.placement, Some(Placement::Fill));
    }

    #[test]
    fn missing_annotation_is_rejected() {
        // First line is data, not a `# ` annotation.
        let d = parse_file("A1:D1.range", "Product\tPrice").unwrap_err();
        assert_eq!(d.code, Code::MissingAnnotation);
    }

    #[test]
    fn non_conforming_literal_body_is_rejected_at_load() {
        // A 2x2 literal into a declared 3x3 -> neither exact nor broadcastable -> #SPILL!-class.
        let contents = format!("{ANN}1\t2\n3\t4");
        let d = parse_file("B2:D4.range", &contents).unwrap_err();
        assert_eq!(d.code, Code::NonConforming);
    }

    #[test]
    fn a_bad_filename_is_rejected_before_the_body() {
        let d = parse_file("g8:a3.range", &format!("{ANN}1\t2")).unwrap_err();
        // Lowercase is caught per-address before the ordering check.
        assert_eq!(d.code, Code::LowercaseColumn);
    }
}
