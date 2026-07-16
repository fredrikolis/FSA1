// Concern: the broadcast-conformance dimension check (FORMAT §6) — given a declared range shape and a body's result shape, decide the `Placement` (scalar fill / row-vector broadcast-down / col-vector broadcast-across / exact array) or raise a `#SPILL!`-class refusal; the vector's AXIS is read from the body shape itself, so R==C is unambiguous (§6.1) | Non-concern: WHERE the result shape comes from (body.rs gives a literal's shape; a formula's shape needs eval, W3) and the ragged-block check that precedes this (body.rs, §5) | IO: (declared `Shape`, result `Shape`) -> `Result<Placement, Diagnostic>`
//! Broadcast-conformance: [`classify_placement`], [`validate_conformance`], [`Placement`].

use crate::diagnostic::{Code, Diagnostic, Loc};
use charlie_ast::Shape;

/// How a conforming body result is placed into the declared `R x C` region (FORMAT §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Scalar `1x1` -> every one of the `R x C` cells.
    Fill,
    /// Row vector `1xC` -> copied down all `R` rows.
    BroadcastDown,
    /// Col vector `Rx1` -> copied across all `C` cols.
    BroadcastAcross,
    /// Array `RxC` -> placed cell-for-cell.
    Exact,
}

/// The pure §6 dimension rule: does `result` conform to `declared`, and if so, how is it placed?
///
/// The axis of a vector is a property of the vector's own shape (`1xk` row vs `kx1` col), never
/// inferred from the declared range — so when `R == C`, a `1xC` still broadcasts *down* and an
/// `Rx1` still broadcasts *across* (FORMAT §6.1, the disambiguator). `None` means non-conforming.
///
/// **Precedence (a resolved B1 ambiguity — see the crate notes).** The §6 table's rows are NOT
/// mutually exclusive when `R == 1` or `C == 1`: a `1xC` body into a `1xC` range satisfies *both*
/// the row-vector rule (`k == C`) and the exact-array rule (`r==R && c==C`); the placed cells are
/// identical either way, but the *label* is under-determined. We resolve it by strongest match:
/// scalar `Fill` first (a `1x1` is never a degenerate vector), then exact array, then the two
/// vector broadcasts. A single-row range with a single-row body is therefore `Exact`, not a
/// broadcast — the intuitive reading — and the square-range disambiguator (`R==C`, body decides the
/// axis) still holds because a `1xC`/`Cx1` body is not exact against an `RxC` range with `R,C > 1`.
///
/// This particular tie is *not* the §6.1 kill-clause (that one — a square `R==C` range — produces
/// two behaviorally-distinct verdicts and IS oracle-covered by the `square-disambiguator` fixture).
/// It is a strictly weaker, **behaviorally-unobservable** tie: `Exact` and `BroadcastDown` place the
/// *same* cells for a `1xC`-into-`1xC` body, so no independent oracle — which can only observe placed
/// cells — could ever distinguish the two labels. There is therefore no ledger verdict to encode and
/// none to violate; the label is an internal, deterministic, arbitrary-but-stable choice (strongest
/// match). It is nonetheless anchored to a *frozen* corpus input, not only a synthetic shape: the
/// encoding harness asserts a real header-row fixture (a `1xC` `.range` with a one-line literal body)
/// resolves to `Exact`, so the pinned label rides on a provenance-guarded fixture.
pub fn classify_placement(declared: Shape, result: Shape) -> Option<Placement> {
    let (r, c) = (declared.rows, declared.cols);
    let (rr, rc) = (result.rows, result.cols);

    if rr == 1 && rc == 1 {
        // Scalar always conforms (checked first, so a `1x1` is Fill, never a degenerate vector).
        Some(Placement::Fill)
    } else if rr == r && rc == c {
        // Exact array match — the strongest form; wins the R==1/C==1 tie against a vector rule.
        Some(Placement::Exact)
    } else if rr == 1 {
        // Row vector `1xk` conforms iff k == C, and broadcasts down.
        (rc == c).then_some(Placement::BroadcastDown)
    } else if rc == 1 {
        // Col vector `kx1` conforms iff k == R, and broadcasts across.
        (rr == r).then_some(Placement::BroadcastAcross)
    } else {
        None
    }
}

/// Validate a body result shape against a declared shape, producing the [`Placement`] or a located
/// `#SPILL!`-class [`Code::NonConforming`] refusal naming the file, declared shape, and result
/// shape (FORMAT §6 — a *static* check, charlie's advantage over Excel's runtime-only detection).
pub fn validate_conformance(
    file: &str,
    declared: Shape,
    result: Shape,
) -> Result<Placement, Diagnostic> {
    classify_placement(declared, result).ok_or_else(|| {
        Diagnostic::new(
            Code::NonConforming,
            Loc::file(file),
            format!(
                "body shape {}x{} does not conform to declared {}x{} \
                 (not scalar/row-vector/col-vector/exact-array) -- #SPILL!-class",
                result.rows, result.cols, declared.rows, declared.cols,
            ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(rows: u32, cols: u32) -> Shape {
        Shape { rows, cols }
    }

    #[test]
    fn scalar_always_fills() {
        assert_eq!(
            classify_placement(shape(6, 7), shape(1, 1)),
            Some(Placement::Fill)
        );
        assert_eq!(
            classify_placement(shape(1, 1), shape(1, 1)),
            Some(Placement::Fill)
        );
    }

    #[test]
    fn row_vector_broadcasts_down_when_k_equals_cols() {
        // 3x3 declared, 1x3 row vector -> broadcast down (FORMAT §10.1).
        assert_eq!(
            classify_placement(shape(3, 3), shape(1, 3)),
            Some(Placement::BroadcastDown)
        );
        // 1x2 into 3x3 -> k(2) != C(3) -> non-conforming.
        assert_eq!(classify_placement(shape(3, 3), shape(1, 2)), None);
    }

    #[test]
    fn col_vector_broadcasts_across_when_k_equals_rows() {
        assert_eq!(
            classify_placement(shape(3, 3), shape(3, 1)),
            Some(Placement::BroadcastAcross)
        );
        assert_eq!(classify_placement(shape(3, 3), shape(2, 1)), None);
    }

    #[test]
    fn exact_array_must_match_both_axes() {
        assert_eq!(
            classify_placement(shape(2, 3), shape(2, 3)),
            Some(Placement::Exact)
        );
        assert_eq!(classify_placement(shape(2, 3), shape(3, 2)), None);
    }

    #[test]
    fn square_range_disambiguates_by_body_axis() {
        // The §6.1 clause B1 must not find ambiguous: R == C == 3.
        // A 1x3 row vector broadcasts DOWN; a 3x1 col vector broadcasts ACROSS. Distinct verdicts,
        // decided solely by the body's own shape.
        assert_eq!(
            classify_placement(shape(3, 3), shape(1, 3)),
            Some(Placement::BroadcastDown)
        );
        assert_eq!(
            classify_placement(shape(3, 3), shape(3, 1)),
            Some(Placement::BroadcastAcross)
        );
    }

    #[test]
    fn non_conforming_is_spill_class_and_located() {
        let d = validate_conformance("B2:D4.range", shape(3, 3), shape(2, 5)).unwrap_err();
        assert_eq!(d.code, Code::NonConforming);
        assert_eq!(d.code.err_class(), Some(charlie_ast::ErrKind::Spill));
        assert!(d.message.contains("2x5"));
        assert!(d.message.contains("3x3"));
    }

    #[test]
    fn single_row_range_prefers_exact_over_row_vector() {
        // A resolved §6 ambiguity: a 1x4 body into a 1x4 range satisfies BOTH the row-vector rule
        // (k==C) and the exact-array rule; strongest-match wins, so the label is Exact (the placed
        // cells are identical either way). Likewise a single-column range with a col body.
        assert_eq!(
            classify_placement(shape(1, 4), shape(1, 4)),
            Some(Placement::Exact)
        );
        assert_eq!(
            classify_placement(shape(5, 1), shape(5, 1)),
            Some(Placement::Exact)
        );
    }

    #[test]
    fn a_cell_declares_1x1_so_a_multi_value_literal_body_does_not_conform() {
        // A `.cell` (declared 1x1) whose literal body somehow has 3 fields -> non-conforming.
        assert_eq!(classify_placement(shape(1, 1), shape(1, 3)), None);
    }
}
