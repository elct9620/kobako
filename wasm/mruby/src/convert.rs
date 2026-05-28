//! Rust ↔ mruby `Value` conversion traits — the typed layer over the
//! raw boxing / unboxing primitives in `value.rs`.
//!
//! This is mruby-sys's small slice of the magnus conversion
//! contract: `IntoValue` mirrors magnus's `IntoValue` (Rust → value,
//! infallible boxing), `FromValue` mirrors magnus's `TryConvert`
//! (value → Rust, fallible downcast). Both sit ON TOP of the unsafe tag
//! primitives in `value.rs` (`Value::from_int` / `is_integer` +
//! `unbox_integer` / …): those primitives are the C-bind floor, these
//! traits are the safe typed seam consumers call. Keeping the two layers
//! in separate files makes a future `mruby` (typed) / `mruby-sys` (FFI)
//! crate split a move rather than an untangling — the traits travel to
//! `mruby`, the primitives stay here.
//!
//! Scope is deliberately the scalar leaf types the kobako codec
//! round-trips (`i32` / `f64` / `bool`). This is NOT the full magnus
//! hierarchy — no typed-value (`RArray` / `RString`) conversion family
//! and no owned/borrowed split; the `Array` / `Hash` newtypes keep their
//! own `as_value` / `from_value_unchecked` ladder.

use crate::{Mrb, Value};

/// Box a Rust value into an mruby `Value`. Infallible — every
/// implementor has a total mapping into the value domain. Mirrors
/// magnus's `IntoValue`; the call shape is `n.into_value(mrb)`.
pub trait IntoValue {
    fn into_value(self, mrb: &Mrb) -> Value;
}

/// Downcast an mruby `Value` to a Rust type, returning `None` when
/// the value is not tagged as the target type. Safe: the tag check is
/// folded in, so callers no longer pair a predicate with an `unsafe`
/// unbox. Mirrors magnus's `TryConvert`; named `FromValue` here for the
/// `T::from_value(v)` call shape.
pub trait FromValue: Sized {
    fn from_value(value: Value) -> Option<Self>;
}

impl IntoValue for i32 {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        Value::from_int(mrb, self)
    }
}

impl IntoValue for f64 {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        Value::from_float(mrb, self)
    }
}

impl IntoValue for bool {
    #[inline]
    fn into_value(self, _mrb: &Mrb) -> Value {
        if self {
            Value::true_()
        } else {
            Value::false_()
        }
    }
}

impl FromValue for i32 {
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        // SAFETY: the unbox precondition (MRB_TT_INTEGER tagging) is
        // established by the `is_integer` guard immediately before it.
        value.is_integer().then(|| unsafe { value.unbox_integer() })
    }
}

impl FromValue for f64 {
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        // SAFETY: the unbox precondition (MRB_TT_FLOAT tagging) is
        // established by the `is_float` guard immediately before it.
        value.is_float().then(|| unsafe { value.unbox_float() })
    }
}
