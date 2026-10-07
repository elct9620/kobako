//! Shared builders for the guest exceptions the Regexp / MatchData surface
//! raises, so each call site reads as `Err(errors::index_error(mrb, "..."))`
//! without every module re-defining the same one-line constructors.

use beni::{Error, ExceptionClass, Mrb};

pub(crate) fn regexp_error(mrb: &Mrb, source: &str, detail: &str) -> Error {
    let message = format!(
        "{source:?} is an invalid regular expression: {}",
        detail.lines().next().unwrap_or(detail)
    );
    exception(mrb, mrb.exception_regexp_error(), &message)
}

pub(crate) fn replace_expression_error(mrb: &Mrb, replacement: &str) -> Error {
    exception(
        mrb,
        mrb.exception_regexp_error(),
        &format!("invalid replace expression: {replacement:?}"),
    )
}

pub(crate) fn argument_error(mrb: &Mrb, message: &str) -> Error {
    exception(mrb, mrb.exception_arg_error(), message)
}

pub(crate) fn index_error(mrb: &Mrb, message: &str) -> Error {
    exception(mrb, mrb.exception_index_error(), message)
}

pub(crate) fn type_error(mrb: &Mrb, message: &str) -> Error {
    exception(mrb, mrb.exception_type_error(), message)
}

pub(crate) fn no_method_error(mrb: &Mrb, message: &str) -> Error {
    exception(mrb, mrb.exception_no_method_error(), message)
}

/// A miss surfaces mruby's own lookup error rather than degrading the raise
/// to a different class.
fn exception(mrb: &Mrb, class: Result<ExceptionClass, Error>, message: &str) -> Error {
    match class {
        Ok(cls) => Error::new(mrb, cls, message),
        Err(err) => err,
    }
}
