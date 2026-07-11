//! Shared builders for the guest exceptions the Regexp / MatchData surface
//! raises, so each call site reads as `Err(errors::index_error(mrb, "..."))`
//! without every module re-defining the same one-line constructors.

use beni::{Error, Mrb};
use core::ffi::CStr;

/// `RegexpError` naming the offending pattern `source` and the engine `detail`.
pub(crate) fn regexp_error(mrb: &Mrb, source: &str, detail: &str) -> Error {
    let message = format!(
        "{source:?} is an invalid regular expression: {}",
        detail.lines().next().unwrap_or(detail)
    );
    exception(mrb, c"RegexpError", &message)
}

/// `RegexpError` naming a malformed replacement expression (a `\k` not
/// followed by `<name>`).
pub(crate) fn replace_expression_error(mrb: &Mrb, replacement: &str) -> Error {
    exception(
        mrb,
        c"RegexpError",
        &format!("invalid replace expression: {replacement:?}"),
    )
}

/// `ArgumentError` carrying `message`.
pub(crate) fn argument_error(mrb: &Mrb, message: &str) -> Error {
    exception(mrb, c"ArgumentError", message)
}

/// `IndexError` carrying `message`.
pub(crate) fn index_error(mrb: &Mrb, message: &str) -> Error {
    exception(mrb, c"IndexError", message)
}

/// `TypeError` carrying `message`.
pub(crate) fn type_error(mrb: &Mrb, message: &str) -> Error {
    exception(mrb, c"TypeError", message)
}

/// Build an exception of the named class. On the impossible miss (a broken
/// build), surface mruby's own lookup error rather than degrading the raise
/// to a different class.
fn exception(mrb: &Mrb, class: &CStr, message: &str) -> Error {
    match mrb.exc_get(class) {
        Ok(cls) => Error::new(mrb, cls, message),
        Err(err) => err,
    }
}
