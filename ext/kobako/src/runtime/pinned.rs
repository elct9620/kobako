//! A Ruby object Rust holds across calls into the guest, registered with
//! Ruby's collector so it is neither freed nor moved by compaction while
//! the holder lives.

use magnus::value::Opaque;
use magnus::{Ruby, Value};

use super::gvl;

pub(crate) struct Pinned(Box<Opaque<Value>>);

impl Pinned {
    pub(crate) fn new(ruby: &Ruby, value: Value) -> Self {
        let value = Box::new(Opaque::from(value));
        ruby.gc_register_address(&*value);
        Self(value)
    }

    pub(crate) fn get(&self) -> Opaque<Value> {
        *self.0
    }
}

impl Drop for Pinned {
    fn drop(&mut self) {
        gvl::reenter(|| {
            let ruby = Ruby::get().expect("Ruby handle unavailable releasing a pinned object");
            ruby.gc_unregister_address(&*self.0);
        });
    }
}
