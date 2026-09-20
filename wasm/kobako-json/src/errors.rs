//! The gem-owned `JSON` exception tree and the builders the parse /
//! generate paths raise through, so each call site reads as
//! `Err(errors::parser_error(mrb, "..."))` without re-deriving the class
//! lookup.

use beni::prelude::*;
use beni::{Error, Mrb, TryConvert};
use core::ffi::CStr;

/// Define the `JSON` error tree: `JSON::JSONError < StandardError`, with
/// `JSON::ParserError` and `JSON::GeneratorError` refining it. The `JSON`
/// module is fetched or created here so the tree exists independently of
/// `json::init`'s ordering.
pub(crate) fn init(mrb: &Mrb) -> Result<(), Error> {
    let json = mrb.define_module(c"JSON")?;
    let json_error = json.define_error(mrb, c"JSONError", mrb.exc_get(c"StandardError")?)?;
    json.define_error(mrb, c"ParserError", json_error)?;
    json.define_error(mrb, c"GeneratorError", json_error)?;
    Ok(())
}

/// `JSON::ParserError` carrying `message` — malformed input, or a number
/// the guest cannot hold without precision loss.
pub(crate) fn parser_error(mrb: &Mrb, message: &str) -> Error {
    json_exception(mrb, c"ParserError", message)
}

/// `JSON::GeneratorError` carrying `message` — a value with no JSON
/// rendering: a non-opted object, an unusable object key, `NaN` /
/// `Infinity`, or a structure past the nesting bound.
pub(crate) fn generator_error(mrb: &Mrb, message: &str) -> Error {
    json_exception(mrb, c"GeneratorError", message)
}

/// `TypeError` carrying `message` — a `parse` argument that is not a
/// `String`.
pub(crate) fn type_error(mrb: &Mrb, message: &str) -> Error {
    match mrb.exc_get(c"TypeError") {
        Ok(cls) => Error::new(mrb, cls, message),
        Err(err) => err,
    }
}

/// Build an exception of the class named `member` under `JSON`. `init` defines
/// each member, but the constant holding it is the guest's to reassign, so a
/// miss surfaces mruby's own lookup error rather than degrading the raise to a
/// different class — the same rule the Regexp surface's builders follow.
fn json_exception(mrb: &Mrb, member: &CStr, message: &str) -> Error {
    let resolved = mrb
        .define_module(c"JSON")
        .and_then(|json| json.class_get(mrb, member))
        .and_then(|cls| beni::ExceptionClass::try_convert(cls.as_value(), mrb));
    match resolved {
        Ok(cls) => Error::new(mrb, cls, message),
        Err(err) => err,
    }
}
