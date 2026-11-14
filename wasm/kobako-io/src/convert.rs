//! The implicit conversions CRuby's `object.c` makes for every IO: a value
//! stands for a String, an Integer, an IO, or an Array through the method
//! that names the conversion, asked only when the value answers it.

use beni::prelude::*;
use beni::{Error, Mrb, RString, Symbol, Value};

use crate::errno;

/// Whether `value` answers `method`, as `respond_to?` does.
pub(crate) fn responds_to(mrb: &Mrb, value: Value, method: &str) -> Result<bool, Error> {
    let name = Symbol::new(mrb, method)?.as_value();
    value.funcall(mrb, c"respond_to?", [name])
}

/// What `value.method` answers when `value` answers `method`, `None` when it
/// does not, as CRuby's `rb_check_funcall` reads it.
pub(crate) fn check_funcall(
    mrb: &Mrb,
    value: Value,
    method: &core::ffi::CStr,
) -> Result<Option<Value>, Error> {
    let name = method.to_str().expect("a method name is ASCII");
    if !responds_to(mrb, value, name)? {
        return Ok(None);
    }
    value.funcall(mrb, method, ()).map(Some)
}

/// `value` as CRuby's `rb_check_string_type` takes it: a String, or what its
/// `to_str` answers, `None` for a value with no `to_str`; a `to_str` answering
/// no String raises `TypeError`.
pub(crate) fn check_string_type(mrb: &Mrb, value: Value) -> Result<Option<RString>, Error> {
    if let Some(string) = RString::from_value(value) {
        return Ok(Some(string));
    }
    match check_funcall(mrb, value, c"to_str")? {
        None => Ok(None),
        Some(string) => RString::from_value(string)
            .map(Some)
            .ok_or_else(|| errno::type_error(mrb, value)),
    }
}

/// `value` as CRuby's `StringValue` takes it: a String, or what its `to_str`
/// answers; anything else raises `TypeError`.
pub(crate) fn string_value_of(mrb: &Mrb, value: Value) -> Result<RString, Error> {
    check_string_type(mrb, value)?.ok_or_else(|| errno::type_error(mrb, value))
}

/// The bytes of `value` as CRuby's `StringValue` takes it.
pub(crate) fn string_bytes_of(mrb: &Mrb, value: Value) -> Result<Vec<u8>, Error> {
    let string = string_value_of(mrb, value)?;
    // SAFETY: the bytes are copied out before any later mruby call.
    Ok(unsafe { string.as_slice(mrb) }.to_vec())
}
