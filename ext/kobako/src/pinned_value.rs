//! A Ruby object Rust holds across calls into the guest, registered with
//! Ruby's collector so it is neither freed nor moved by compaction while
//! the holder lives.

use std::mem::ManuallyDrop;

use magnus::value::Opaque;
use magnus::{Ruby, Value};

use super::gvl;

pub(crate) struct PinnedValue(ManuallyDrop<Box<Opaque<Value>>>);

impl PinnedValue {
    pub(crate) fn new(ruby: &Ruby, value: Value) -> Self {
        let value = Box::new(Opaque::from(value));
        ruby.gc_register_address(&*value);
        Self(ManuallyDrop::new(value))
    }

    pub(crate) fn get(&self) -> Opaque<Value> {
        **self.0
    }
}

impl Drop for PinnedValue {
    fn drop(&mut self) {
        let released = gvl::reenter(|| {
            let ruby = Ruby::get().expect("Ruby handle unavailable releasing a pinned object");
            ruby.gc_unregister_address(&**self.0);
        });
        // With the run's caller gone, the collector still reads the address,
        // so it stays, keeping the object.
        if released.is_some() {
            // SAFETY: the address is unregistered, and nothing reads it again.
            unsafe { ManuallyDrop::drop(&mut self.0) };
        }
    }
}
