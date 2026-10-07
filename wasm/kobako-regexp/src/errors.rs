//! Shared builders for the guest exceptions the Regexp / MatchData surface
//! raises, so each call site reads as `Err(errors::index_error(mrb, "..."))`
//! without every module re-defining the same one-line constructors.

use beni::{Error, Mrb};
use core::ffi::CStr;

pub(crate) fn regexp_error(mrb: &Mrb, source: &str, detail: &str) -> Error {
    let message = format!(
        "{source:?} is an invalid regular expression: {}",
        detail.lines().next().unwrap_or(detail)
    );
    exception(mrb, c"RegexpError", &message)
}

pub(crate) fn replace_expression_error(mrb: &Mrb, replacement: &str) -> Error {
    exception(
        mrb,
        c"RegexpError",
        &format!("invalid replace expression: {replacement:?}"),
    )
}

pub(crate) fn argument_error(mrb: &Mrb, message: &str) -> Error {
    exception(mrb, c"ArgumentError", message)
}

pub(crate) fn index_error(mrb: &Mrb, message: &str) -> Error {
    exception(mrb, c"IndexError", message)
}

pub(crate) fn type_error(mrb: &Mrb, message: &str) -> Error {
    exception(mrb, c"TypeError", message)
}

pub(crate) fn no_method_error(mrb: &Mrb, message: &str) -> Error {
    exception(mrb, c"NoMethodError", message)
}

/// A miss surfaces mruby's own lookup error rather than degrading the raise
/// to a different class.
fn exception(mrb: &Mrb, class: &CStr, message: &str) -> Error {
    match mrb.exc_get(class) {
        Ok(cls) => Error::new(mrb, cls, message),
        Err(err) => err,
    }
}
