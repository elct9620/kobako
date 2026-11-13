//! What a Host App's callback raises that is not a `StandardError`:
//! `Interrupt`, `exit`, `throw`, `Timeout`, and any interrupt delivered to the
//! Thread while a run gives up the GVL. These belong to the host, so one is held
//! here instead of answering the guest with it, the invocation ends at its next
//! host call, and the run raises it again once control is back in Ruby.
//!
//! The slot is per thread, as each invocation runs on its caller's
//! thread. A nested invocation takes what its own run held before the
//! outer callback sees it, so the outer one holds it next.

use std::cell::RefCell;

use magnus::value::BoxValue;
use magnus::{Error as MagnusError, Ruby, Value};

thread_local! {
    static ESCAPE: RefCell<Option<Escape>> = const { RefCell::new(None) };
}

/// The error and, when it carries one, its exception kept rooted while
/// the guest finishes unwinding.
struct Escape {
    error: MagnusError,
    _rooted: Option<BoxValue<Value>>,
}

/// Hold `error` unless it is a `StandardError`, which comes back for the
/// callback to answer the guest with.
pub(super) fn hold(ruby: &Ruby, error: MagnusError) -> Result<(), MagnusError> {
    if error.is_kind_of(ruby.exception_standard_error()) {
        return Err(error);
    }
    keep(error);
    Ok(())
}

/// Run what is pending on this Thread, holding what it raises, whatever its
/// class: no callback asked for it, so no guest is answered with it.
pub(super) fn hold_interrupts() {
    let ruby = Ruby::get().expect("a Thread that was not given up holds the GVL");
    if let Err(error) = ruby.thread_check_ints() {
        keep(error);
    }
}

/// Keep `error` for the host; the first one kept wins.
fn keep(error: MagnusError) {
    ESCAPE.with(|held| {
        held.borrow_mut().get_or_insert_with(|| Escape {
            _rooted: error.value().map(BoxValue::new),
            error,
        });
    });
}

/// Whether this thread holds an error for the host.
pub(super) fn pending() -> bool {
    ESCAPE.with(|held| held.borrow().is_some())
}

/// Raise what was held, once the run is back in Ruby.
pub(super) fn resume() -> Result<(), MagnusError> {
    match ESCAPE.with(|held| held.borrow_mut().take()) {
        Some(held) => Err(held.error),
        None => Ok(()),
    }
}
