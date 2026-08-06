// Concern: declares Value, ErrKind, Shape and the borrowed ArrayView | Non-concern: rendering a value as text, evaluation, which resolver holds the cells | IO: none

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrKind {
    Ref,
    Div0,
    Value,
    Name,
    Na,
    Null,
    Num,
    Spill,
    Calc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Shape {
    pub rows: u32,
    pub cols: u32,
}

#[derive(Clone, Debug)]
pub enum Value {
    Number(f64),
    Text(String),
    Bool(bool),
    Error(ErrKind),
    Array(Shape, Vec<Value>),
    Blank,
}

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
