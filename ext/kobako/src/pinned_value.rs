//! A Ruby object Rust holds across calls into the guest, kept for the run it
//! is made in: the run's caller marks it, so the collector neither frees nor
//! moves it while the caller stands, and frees it with a caller that never
//! came back. A root of its own would keep that caller's Fiber alive through a
//! block that closes over it.

use std::sync::Arc;

use magnus::rb_sys::AsRawValue;
use magnus::value::Opaque;
use magnus::Value;

use super::escape::{self, RunState};

pub(crate) struct PinnedValue {
    value: Opaque<Value>,
    /// The value's identity in the run's pins, as letting it go needs no GVL.
    identity: rb_sys::VALUE,
    run: Arc<RunState>,
}

impl PinnedValue {
    /// Keep `value` for the run the calling Fiber is in.
    pub(crate) fn new(value: Value) -> Self {
        let run = escape::current_run().expect("a Ruby object is pinned inside a run");
        let identity = value.as_raw();
        run.pin(identity);
        Self {
            value: Opaque::from(value),
            identity,
            run,
        }
    }

    pub(crate) fn get(&self) -> Opaque<Value> {
        self.value
    }
}

impl Drop for PinnedValue {
    fn drop(&mut self) {
        self.run.unpin(self.identity);
    }
}
