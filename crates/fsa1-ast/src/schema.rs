// Concern: renders the abstract grammar from the Rust types | Non-concern: parsing, evaluating, the golden file's content (ast-grammar.schema holds it) | IO: () -> schema text

use crate::expr::{BinOp, Expr, UnOp};
use crate::func::FUNCS;
use crate::value::{ErrKind, Value};
use std::fmt::Write;

/// Bump when the RENDERING changes; a grammar change is reflected automatically.
const SCHEMA_VERSION: u32 = 1;

pub fn emit() -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "# fsa1-ast formula grammar (generated from the Rust types; provenance elided)"
    );
    let _ = writeln!(s, "# schema-version: {SCHEMA_VERSION}");
    s.push('\n');

    s.push_str("value =\n");
    for line in VALUE_VARIANTS {
        let _ = writeln!(s, "  {line}");
    }
    s.push('\n');

    let _ = writeln!(s, "errkind = {}", ERRKIND_VARIANTS.join(" | "));
    s.push('\n');

    s.push_str("expr =\n");
    for line in EXPR_VARIANTS {
        let _ = writeln!(s, "  {line}");
    }
    s.push('\n');

    let _ = writeln!(s, "unop = {}", UNOP_VARIANTS.join(" | "));
    let _ = writeln!(s, "binop = {}", BINOP_VARIANTS.join(" | "));
    s.push('\n');

    s.push_str("# operator precedence (binding power; tightest last)\n");
    s.push_str("precedence =\n");
    for (label, bp) in PRECEDENCE {
        let _ = writeln!(s, "  {label} : {bp}");
    }
    s.push('\n');

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

/// Never called: exhaustive matches that fail compilation when a variant is added without its line
/// in the `*_VARIANTS` list above. `dead_code` is deliberate — a compile-time invariant.
#[allow(dead_code)]
fn variant_guards(v: &Value, e: &Expr, k: &ErrKind, u: &UnOp, b: &BinOp) {
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
        assert_eq!(VALUE_VARIANTS.len(), 6, "a Value variant was removed");
        assert_eq!(ERRKIND_VARIANTS.len(), 9, "an ErrKind variant was removed");
        assert_eq!(EXPR_VARIANTS.len(), 8, "an Expr variant was removed");
        assert_eq!(UNOP_VARIANTS.len(), 3, "a UnOp variant was removed");
        assert_eq!(BINOP_VARIANTS.len(), 12, "a BinOp variant was removed");
    }

    #[test]
    fn precedence_matches_the_parsers_binding_powers() {
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
        let s = emit();
        for f in FUNCS {
            assert!(s.contains(f.name), "schema must list {}", f.name);
        }
    }
}
