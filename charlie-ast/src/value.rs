// Concern: the first-class spreadsheet VALUE domain — `Value` (Number/Text/Bool/Error/Array/Blank), the `ErrKind` error taxonomy that will propagate through operators, `Shape`, and `ArrayView`, a BORROWED view over a rectangular block of cells; equality compares floats by BIT PATTERN so a round-trip never smooths `-0.0` vs `0.0` or collapses `NaN` | Non-concern: how a value is COMPUTED (the evaluator, later) and where cells physically live (charlie-model) | IO: none — value types
//! Value layer: [`Value`], [`ErrKind`], [`Shape`], [`ArrayView`].

/// A spreadsheet error value.
///
/// Errors are first-class values that propagate through operators. All nine are live in v1:
/// [`ErrKind::Spill`] is the GRID5 array-formula-region shape/orientation mismatch (charlie-model's
/// `fill_array_region`), and [`ErrKind::Calc`] is an empty dynamic-array result (an empty
/// `UNIQUE`/`FILTER`, a zero-dimension `SEQUENCE`) and other calculation-engine errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrKind {
    /// `#REF!` — a reference to a cell that does not exist (e.g. deleted).
    Ref,
    /// `#DIV/0!` — division by zero.
    Div0,
    /// `#VALUE!` — a value of the wrong type for an operator/function.
    Value,
    /// `#NAME?` — an unrecognized name (function or defined name).
    Name,
    /// `#N/A` — a value is not available (e.g. a failed lookup).
    Na,
    /// `#NULL!` — the null intersection of two ranges that do not overlap.
    Null,
    /// `#NUM!` — a numeric value is invalid (out of range / no result).
    Num,
    /// `#SPILL!` — a dynamic-array result did not fit its region: a GRID5 array formula whose value's
    /// shape/orientation does not match its declared range (charlie-model's `fill_array_region`).
    Spill,
    /// `#CALC!` — a calculation-engine error: an empty dynamic-array result (empty `UNIQUE`/`FILTER`,
    /// a zero-dimension `SEQUENCE`).
    Calc,
}

/// The dimensions of an array value: `rows` × `cols`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Shape {
    pub rows: u32,
    pub cols: u32,
}

/// A spreadsheet value.
///
/// The `Number` arm holds an IEEE-754 `f64`; equality compares it by **bit pattern** (see the
/// hand-written [`PartialEq`] below), so `-0.0 != 0.0` and `NaN == NaN` — a round-trip that flips
/// a bit is a real difference, never smoothed over (ast-standards PART 3, "compare values
/// exactly"). `Array` carries its [`Shape`] alongside a row-major cell vector.
#[derive(Clone, Debug)]
pub enum Value {
    Number(f64),
    Text(String),
    Bool(bool),
    Error(ErrKind),
    Array(Shape, Vec<Value>),
    Blank,
}

// Exact-value equality: floats by bit pattern (not by `==`, which would make `NaN != NaN` and
// `-0.0 == 0.0`). Everything else is structural. This is a valid `Eq` — bit-pattern comparison is
// reflexive (`NaN.to_bits() == NaN.to_bits()`), symmetric, and transitive.
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Number(a), Value::Number(b)) => a.to_bits() == b.to_bits(),
            (Value::Text(a), Value::Text(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Error(a), Value::Error(b)) => a == b,
            (Value::Array(sa, va), Value::Array(sb, vb)) => sa == sb && va == vb,
            (Value::Blank, Value::Blank) => true,
            _ => false,
        }
    }
}

impl Eq for Value {}

/// A resolved rectangular block of cells, handed back by [`crate::Resolver::range`].
///
/// This is a **borrowed view** (`docs/architecture.md` §2: "a rectangular block, borrowed"): it
/// pairs a [`Shape`] with a slice `&[Value]` that *borrows* the resolver's backing store for the
/// lifetime `'a` rather than copying it — the evaluator reads range cells in place.
///
/// Being borrowed makes it categorically distinct from the owned [`Value::Array`], so there is no
/// duplicated payload to keep reconciled: `Value::Array` *owns* its `(Shape, Vec<Value>)` and can
/// live in the tree, whereas an `ArrayView` only *names* cells that live in the store — it cannot
/// outlive them. (Materializing a view into an owned `Value::Array`, when eval needs to, is a copy
/// the evaluator makes deliberately, not an invariant these two types must silently mirror.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArrayView<'a> {
    pub shape: Shape,
    pub cells: &'a [Value],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_zero_is_distinct_from_zero() {
        assert_ne!(Value::Number(0.0), Value::Number(-0.0));
    }

    #[test]
    fn nan_is_reflexively_equal_by_bit_pattern() {
        assert_eq!(Value::Number(f64::NAN), Value::Number(f64::NAN));
    }

    #[test]
    fn ordinary_values_compare_structurally() {
        assert_eq!(Value::Number(1.5), Value::Number(1.5));
        assert_eq!(Value::Text("hi".into()), Value::Text("hi".into()));
        assert_ne!(Value::Text("hi".into()), Value::Text("ho".into()));
        assert_eq!(Value::Error(ErrKind::Div0), Value::Error(ErrKind::Div0));
        assert_ne!(Value::Error(ErrKind::Div0), Value::Error(ErrKind::Ref));
        assert_ne!(Value::Bool(true), Value::Blank);
    }

    #[test]
    fn arrays_compare_by_shape_and_cells() {
        let shape = Shape { rows: 1, cols: 2 };
        let a = Value::Array(shape, vec![Value::Number(1.0), Value::Blank]);
        let b = Value::Array(shape, vec![Value::Number(1.0), Value::Blank]);
        let c = Value::Array(shape, vec![Value::Number(1.0), Value::Number(2.0)]);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
