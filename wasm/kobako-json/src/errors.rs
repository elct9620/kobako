//! The gem-owned `JSON` exception tree and the builders the parse /
//! generate paths raise through, so each call site reads as
//! `Err(errors::parser_error(mrb, "..."))` without re-deriving the class
//! lookup.

use beni::{Error, Module, Mrb, RClass};
use core::ffi::CStr;

/// Define the `JSON` error tree: `JSON::JSONError < StandardError`, with
/// `JSON::ParserError` and `JSON::GeneratorError` refining it. The `JSON`
/// module is fetched or created here so the tree exists independently of
/// `json::init`'s ordering.
pub(crate) fn init(mrb: &Mrb) -> Result<(), Error> {
    let json = mrb.define_module(c"JSON")?;
    let json_error = json.define_class(mrb, c"JSONError", mrb.class_get(c"StandardError")?)?;
    json.define_class(mrb, c"ParserError", json_error)?;
    json.define_class(mrb, c"GeneratorError", json_error)?;
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
    let cls = mrb
        .class_get(c"TypeError")
        .expect("TypeError is an mruby core class");
    exception(mrb, cls, message)
}

/// Resolve the class named `member` nested under `JSON`. `init` defines
/// each member before any path raises one, so the lookup cannot miss.
fn json_exception(mrb: &Mrb, member: &CStr, message: &str) -> Error {
    let json = mrb
        .define_module(c"JSON")
        .expect("JSON module is defined at gem init");
    let cls = json
        .class_get(mrb, member)
        .expect("JSON error class is defined at gem init");
    exception(mrb, cls, message)
}

/// Wrap `cls` and `message` into an `Error::Exception` a handler returns,
/// so the bridge frame raises it only after the Rust frame unwinds —
/// unlike a direct `mrb_raise` long-jump.
fn exception(mrb: &Mrb, cls: RClass, message: &str) -> Error {
    Error::Exception(cls.exc_new(mrb, message))
}
