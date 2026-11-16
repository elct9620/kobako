//! The implicit conversions CRuby's `object.c` makes for every IO: a value
//! stands for a String, an Integer, an IO, or an Array through the method
//! that names the conversion, asked only when the value answers it.

use beni::prelude::*;
use beni::{Error, Integer, IntoValue, Mrb, RString, Symbol, TryConvert, Value};

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

/// The Integer `obj` stands for, as CRuby's `rb_num2long` reads it: an
/// Integer as itself, a Float truncated, NaN, an infinity, or a Float past
/// what an Integer holds raising `RangeError`, and anything else through its
/// `to_int`, as `rb_to_int` asks; one with no `to_int`, or whose `to_int`
/// answers no Integer, raises the `TypeError` mruby's own coercion words.
pub(crate) fn to_int(mrb: &Mrb, obj: Value) -> Result<Integer, Error> {
    if let Some(int) = Integer::from_value(obj) {
        return Ok(int);
    }
    if let Some(float) = f64::from_value(obj) {
        // `i64::MIN as f64` is exactly -2⁶³, and `i64::MAX as f64` rounds up
        // to 2⁶³, the first value past it.
        if float >= i64::MIN as f64 && float < i64::MAX as f64 {
            return Integer::try_convert((float.trunc() as i64).into_value(mrb), mrb);
        }
        let shown = obj
            .funcall::<_, _, Value>(mrb, c"to_s", ())?
            .to_r_string(mrb)?
            .to_string(mrb)?;
        return Err(crate::errno::to_error(
            mrb,
            mrb.exception_range_error(),
            &format!("float {shown} out of range of integer"),
        ));
    }
    if !obj.is_nil() {
        if let Some(int) =
            check_funcall(mrb, obj, c"to_int")?.and_then(Integer::from_value)
        {
            return Ok(int);
        }
    }
    Integer::try_convert(obj, mrb)
}

/// `value` as CRuby's `NUM2INT` reads it: `rb_to_int`'s Integer, past C's
/// `int` raising `RangeError` too big above and too small below. It answers
/// the Integer mruby holds, inside `int`'s range.
pub(crate) fn num2int(mrb: &Mrb, value: Value) -> Result<i64, Error> {
    let number = to_int(mrb, value)?.to_i64(mrb)?;
    ensure_fits(
        mrb,
        number,
        i64::from(i32::MIN)..=i64::from(i32::MAX),
        "int",
    )?;
    Ok(number)
}

/// `value` as CRuby's `NIL_P(value) ? 0 : NUM2INT(value)` reads an optional
/// int, `nil` or none being 0.
pub(crate) fn num2int_or_zero(mrb: &Mrb, value: Option<Value>) -> Result<i64, Error> {
    match value.filter(|value| !value.is_nil()) {
        Some(value) => num2int(mrb, value),
        None => Ok(0),
    }
}

/// Raise `RangeError` for `number` outside what a C `c_type` holds, `range`,
/// as CRuby's `check_int` and `rb_num2uint` word it: too big above, too
/// small below.
pub(crate) fn ensure_fits(
    mrb: &Mrb,
    number: i64,
    range: core::ops::RangeInclusive<i64>,
    c_type: &str,
) -> Result<(), Error> {
    let side = match number {
        n if n > *range.end() => "big",
        n if n < *range.start() => "small",
        _ => return Ok(()),
    };
    Err(errno::to_error(
        mrb,
        mrb.exception_range_error(),
        &format!("integer {number} too {side} to convert to '{c_type}'"),
    ))
}

/// Raise `ArgumentError` for `bytes` holding a NUL, as CRuby's
/// `StringValueCStr` refuses one.
pub(crate) fn ensure_no_nul(mrb: &Mrb, bytes: &[u8]) -> Result<(), Error> {
    match bytes.contains(&0) {
        true => Err(crate::io::argument_error(mrb, "string contains null byte")),
        false => Ok(()),
    }
}
