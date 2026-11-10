//! The WASI 0.2 imports a Guest Binary links as a core module, encoded onto
//! `kobako-runtime` as preview1's are. `build.rs` generates each import's
//! glue from the vendored WIT; the Wasm Driver answers one host trait per WIT
//! package.

pub(crate) mod abi;
mod error;
mod host;
mod http;
mod sockets;
mod table;

pub(crate) use bindings::add_to_linker;
#[cfg(test)]
pub(crate) use error::IntoAnswer;
pub(crate) use table::{Pollable, Table};

#[allow(clippy::all, unused_parens)]
pub(crate) mod bindings {
    include!(concat!(env!("OUT_DIR"), "/preview2_bindings.rs"));
}

impl bindings::Host for crate::invocation::Invocation {
    fn abandonment(&self) -> Option<wasmtime::Error> {
        crate::invocation::Invocation::abandonment(self)
    }

    fn lift_handle<H: table::Handle>(&mut self, handle: H) -> wasmtime::Result<H> {
        self.table().get(handle).map(|_| handle)
    }
}
