//! The gem-owned `JSON` exception tree and the builders the parse /
//! generate paths raise through, so each call site reads as
//! `Err(errors::parser_error(mrb, "..."))` without re-deriving the class
//! lookup.

use beni::prelude::*;
use beni::{Error, Mrb, TryConvert};
use core::ffi::CStr;

/// The `JSON` module is fetched or created here, so the tree exists
/// independently of `json::init`'s ordering.
pub(crate) fn init(mrb: &Mrb) -> Result<(), Error> {
    let json = mrb.define_module(c"JSON")?;
    let json_error = json.define_error(mrb, c"JSONError", mrb.exception_standard_error())?;
    json.define_error(mrb, c"ParserError", json_error)?;
    json.define_error(mrb, c"GeneratorError", json_error)?;
    Ok(())
}

pub(crate) fn parser_error(mrb: &Mrb, message: &str) -> Error {
    json_exception(mrb, c"ParserError", message)
}

pub(crate) fn generator_error(mrb: &Mrb, message: &str) -> Error {
    json_exception(mrb, c"GeneratorError", message)
}

/// CRuby's generator refuses a Hash that grows while it is written, in
/// these words.
pub(crate) fn key_added_error(mrb: &Mrb) -> Error {
    match mrb.exception_runtime_error() {
        Ok(cls) => Error::new(mrb, cls, "can't add a new key into hash during iteration"),
        Err(err) => err,
    }
}

/// The constant is the guest's to reassign, so a miss surfaces mruby's own
/// lookup error rather than degrading the raise to a different class.
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
