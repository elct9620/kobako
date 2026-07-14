//! The decoded-value enum the codec accepts, restricted to the 12 codec
//! types the kobako wire allows.

/// A decoded msgpack value, restricted to the 12 codec types the kobako
/// codec accepts (docs/wire-codec.md § Type Mapping). Anything outside
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
    /// Raw bytes of the embedded msgpack map carried inside an ext 0x02
    /// envelope. Re-decoding the inner map is the boot script's job; the
    /// codec only validates it parses as a single msgpack map.
    ErrEnv(Vec<u8>),
}

impl Value {
    /// Whether this tree carries an `ErrEnv` leaf anywhere. The Fault
    /// envelope's sole legal wire position is the Response fault field,
    /// so the host-side envelope decoders reject a payload-position
    /// tree this answers `true` for.
    pub fn contains_errenv(&self) -> bool {
        match self {
            Value::ErrEnv(_) => true,
            Value::Array(items) => items.iter().any(Value::contains_errenv),
            Value::Map(pairs) => pairs
                .iter()
                .any(|(k, v)| k.contains_errenv() || v.contains_errenv()),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_variants_cover_twelve_codec_types() {
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
        let _ = Value::ErrEnv(Vec::new());
    }
}
