// Concern: SCHEMA-FROM-TYPES — emit a machine-readable, PROVENANCE-ELIDED description of the formula AST/grammar (the `Value`/`ErrKind`/`Expr`/`UnOp`/`BinOp` shapes, the operator-precedence ladder, and the live function registry) generated from the Rust types themselves, so the published grammar spec cannot silently drift from the parser; exhaustive `match` guards make a new enum variant a COMPILE error until the schema covers it, and a golden test pins the emitted text | Non-concern: parsing/evaluating (lexer/parser/eval own those) and the human design prose (docs/architecture.md §3) — this is the terse machine contract, not the narrative | IO: none at runtime — `emit()` -> a `String`; a test compares it to the committed `ast-grammar.schema`
//! Schema-from-types (ast-standards PART 9): [`emit`] renders the abstract grammar from the actual
//! Rust types, eliding all provenance (no `NodeId`, no spans). The golden file `ast-grammar.schema`
//! is the committed rendering; the test below asserts `emit() == golden`, so the two cannot diverge
//! without turning a build red — "the spec cannot drift from the parser."

use crate::expr::{BinOp, Expr, UnOp};
use crate::func::FUNCS;
use crate::value::{ErrKind, Value};
use std::fmt::Write;

/// The schema format version — bump when the *rendering* changes (not on every grammar change; a
/// grammar change is reflected automatically by the generators below).
const SCHEMA_VERSION: u32 = 1;

/// Emit the machine-readable AST/grammar schema. Deterministic and provenance-free.
pub fn emit() -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "# charlie-ast formula grammar (generated from the Rust types; provenance elided)"
    );
    let _ = writeln!(s, "# schema-version: {SCHEMA_VERSION}");
    s.push('\n');

    // --- value domain ---
    s.push_str("value =\n");
    for line in VALUE_VARIANTS {
        let _ = writeln!(s, "  {line}");
    }
    s.push('\n');

    let _ = writeln!(s, "errkind = {}", ERRKIND_VARIANTS.join(" | "));
    s.push('\n');

    // --- expression grammar ---
    s.push_str("expr =\n");
    for line in EXPR_VARIANTS {
        let _ = writeln!(s, "  {line}");
    }
    s.push('\n');

    let _ = writeln!(s, "unop = {}", UNOP_VARIANTS.join(" | "));
    let _ = writeln!(s, "binop = {}", BINOP_VARIANTS.join(" | "));
    s.push('\n');

    // --- precedence ladder (tightest last) ---
    s.push_str("# operator precedence (binding power; tightest last)\n");
    s.push_str("precedence =\n");
    for (label, bp) in PRECEDENCE {
        let _ = writeln!(s, "  {label} : {bp}");
    }
    s.push('\n');

    // --- function registry (generated straight from FUNCS data) ---
    s.push_str("# function registry (name : arity)\n");
    s.push_str("functions =\n");
    for f in FUNCS {
        let arity = match f.max_args {
            Some(max) if max == f.min_args => format!("{}", f.min_args),
            Some(max) => format!("{}-{max}", f.min_args),
            None => format!("{}+", f.min_args),
        };
        let _ = writeln!(s, "  {} : {arity}", f.name);
    }

    s
}

// The variant lists below are the schema's source of truth for each enum. Each is paired with an
// exhaustive `match` guard (see `variant_guards`) so that adding a variant to the type WITHOUT
// updating the list here fails to compile — the drift-proofing ast-standards PART 9 asks for.

const VALUE_VARIANTS: &[&str] = &[
    "Number(f64)",
    "Text(string)",
    "Bool(bool)",
    "Error(errkind)",
    "Array(shape, value*)",
    "Blank",
];

const ERRKIND_VARIANTS: &[&str] = &[
    "Ref", "Div0", "Value", "Name", "Na", "Null", "Num", "Spill", "Calc",
];

const EXPR_VARIANTS: &[&str] = &[
    "Lit(value)",
    "Ref(refnode)",
    "Range(rangenode)",
    "WholeRange(wholerangenode)  # A:A / 1:1 / Sheet!B:B — bound to used region by the model",
    "Unary(unop, expr)",
    "Binary(binop, expr, expr)",
    "Call(funcid, expr*)",
    "ImplicitIntersect(expr)  # reserved: @",
    "SpillRef(expr)  # reserved: #",
];

const UNOP_VARIANTS: &[&str] = &["Plus", "Neg", "Percent"];

const BINOP_VARIANTS: &[&str] = &[
    "Add", "Sub", "Mul", "Div", "Pow", "Concat", "Eq", "Ne", "Lt", "Le", "Gt", "Ge",
];

/// The precedence ladder, tightest last. The *numbers* are bound to `parser::{infix_bp, postfix_bp,
/// PREFIX_BP, AT_BP}` by `tests::precedence_matches_the_parsers_binding_powers`, so a change to a
/// parser binding power reddens that test until this ladder is updated — the published spec cannot
/// silently drift from the parser it documents. (Only the labels are hand-authored prose.)
const PRECEDENCE: &[(&str, u8)] = &[
    ("comparisons(=,<>,<,<=,>,>=)", 10),
    ("concat(&)", 20),
    ("add-sub(+,-)", 30),
    ("mul-div(*,/)", 40),
    ("pow(^)", 50),
    ("percent(%,postfix)", 60),
    ("unary(-,+,prefix)", 70),
    ("spill(#,postfix)", 80),
    ("implicit-intersect(@,prefix)", 85),
    ("range(:)", 90),
];

/// Exhaustive `match`es whose ONLY purpose is to fail compilation if a variant is added to a type
/// without the schema list above being updated to match. Never called; `#[allow(dead_code)]` is
/// deliberate — it is a compile-time invariant, not runtime logic.
#[allow(dead_code)]
fn variant_guards(v: &Value, e: &Expr, k: &ErrKind, u: &UnOp, b: &BinOp) {
    // If you add a variant, add its schema line to the corresponding `*_VARIANTS` list above, then
    // extend the arm here. The golden test will then require the schema file be regenerated.
    match v {
        Value::Number(_)
        | Value::Text(_)
        | Value::Bool(_)
        | Value::Error(_)
        | Value::Array(..)
        | Value::Blank => {}
    }
    match e {
        Expr::Lit(_)
        | Expr::Ref(_)
        | Expr::Range(_)
        | Expr::WholeRange(_)
        | Expr::Unary(..)
        | Expr::Binary(..)
        | Expr::Call(..)
        | Expr::ImplicitIntersect(_)
        | Expr::SpillRef(_) => {}
    }
    match k {
        ErrKind::Ref
        | ErrKind::Div0
        | ErrKind::Value
        | ErrKind::Name
        | ErrKind::Na
        | ErrKind::Null
        | ErrKind::Num
        | ErrKind::Spill
        | ErrKind::Calc => {}
    }
    match u {
        UnOp::Plus | UnOp::Neg | UnOp::Percent => {}
    }
    match b {
        BinOp::Add
        | BinOp::Sub
        | BinOp::Mul
        | BinOp::Div
        | BinOp::Pow
        | BinOp::Concat
        | BinOp::Eq
        | BinOp::Ne
        | BinOp::Lt
        | BinOp::Le
        | BinOp::Gt
        | BinOp::Ge => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed rendering. Kept in sync BY THIS TEST: on a grammar change, regenerate the file
    /// from `emit()` (the assertion message tells you), and the diff is the reviewable spec delta.
    const GOLDEN: &str = include_str!("../ast-grammar.schema");

    #[test]
    fn emitted_schema_matches_the_committed_golden() {
        let emitted = emit();
        assert_eq!(
            emitted, GOLDEN,
            "\nast-grammar.schema is stale. Regenerate it with the emitted schema:\n\
             ------8<------\n{emitted}------8<------\n"
        );
    }

    #[test]
    fn variant_lists_match_their_enum_counts() {
        // A cheap numeric backstop for the exhaustive guards: the schema list length must equal the
        // number of variants (any added-but-unlisted variant breaks `variant_guards` compilation;
        // any listed-but-removed variant breaks these counts).
        assert_eq!(VALUE_VARIANTS.len(), 6);
        assert_eq!(ERRKIND_VARIANTS.len(), 9);
        assert_eq!(EXPR_VARIANTS.len(), 9);
        assert_eq!(UNOP_VARIANTS.len(), 3);
        assert_eq!(BINOP_VARIANTS.len(), 12);
    }

    #[test]
    fn precedence_matches_the_parsers_binding_powers() {
        // The single most correctness-critical part of the grammar: bind each published ladder rung
        // to the binding power the parser ACTUALLY uses for a representative operator of that rung
        // (left bp for infix/prefix, the postfix bp for %/#). A parser precedence change now fails
        // here until PRECEDENCE is updated — no silent drift, and the doc claim above is now true.
        use crate::lexer::TokenKind;
        use crate::parser::{AT_BP, PREFIX_BP, infix_bp, postfix_bp};
        let bound: &[(&str, u8)] = &[
            (
                "comparisons(=,<>,<,<=,>,>=)",
                infix_bp(&TokenKind::Eq).unwrap().0,
            ),
            ("concat(&)", infix_bp(&TokenKind::Amp).unwrap().0),
            ("add-sub(+,-)", infix_bp(&TokenKind::Plus).unwrap().0),
            ("mul-div(*,/)", infix_bp(&TokenKind::Star).unwrap().0),
            ("pow(^)", infix_bp(&TokenKind::Caret).unwrap().0),
            (
                "percent(%,postfix)",
                postfix_bp(&TokenKind::Percent).unwrap(),
            ),
            ("unary(-,+,prefix)", PREFIX_BP),
            ("spill(#,postfix)", postfix_bp(&TokenKind::Hash).unwrap()),
            ("implicit-intersect(@,prefix)", AT_BP),
            ("range(:)", infix_bp(&TokenKind::Colon).unwrap().0),
        ];
        assert_eq!(
            PRECEDENCE, bound,
            "the schema precedence ladder drifted from the parser's binding powers"
        );
    }

    #[test]
    fn function_section_reflects_the_live_registry() {
        // The functions section is generated straight from FUNCS, so every registered function
        // appears — proving the schema tracks the registry, not a hand-kept copy.
        let s = emit();
        for f in FUNCS {
            assert!(s.contains(f.name), "schema must list {}", f.name);
        }
    }
}
