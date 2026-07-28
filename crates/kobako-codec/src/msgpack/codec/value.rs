//! The decoded-value enum the codec accepts, restricted to the 11 codec
//! types the kobako wire allows.

/// A decoded msgpack value, restricted to the 11 codec types the kobako
/// codec accepts (docs/wire/payload-msgpack.md § Type Mapping). Anything outside
/// this set is rejected at decode time with `Error::InvalidType`.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    Str(String),
    Bin(Vec<u8>),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
    /// Symbol name carried inside an ext 0x00 frame; the payload is the
    /// symbol's UTF-8 name (zero or more bytes — empty `:""` is wire-legal).
    Sym(String),
    Handle(u32),
}

impl Value {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_variants_cover_the_closed_wire_type_set() {
        let _ = Value::Nil;
        let _ = Value::Bool(true);
        let _ = Value::Int(-1);
        let _ = Value::UInt(u64::MAX);
        let _ = Value::Float(1.5);
        let _ = Value::Str(String::from("x"));
        let _ = Value::Bin(Vec::new());
        let _ = Value::Sym(String::from("x"));
        let _ = Value::Array(Vec::new());
        let _ = Value::Map(Vec::new());
        let _ = Value::Handle(1);
    }
}
