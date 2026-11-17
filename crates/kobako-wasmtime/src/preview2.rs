//! The WASI 0.2 imports a Guest Binary links as a core module, encoded onto
//! `kobako-runtime` as preview1's are. `build.rs` generates each import's
//! glue from the vendored WIT; the Wasm Driver answers one host trait per WIT
//! package.

pub(crate) mod abi;
mod error;
pub(crate) mod filesystem;
mod host;
mod http;
pub(crate) mod io;
mod sockets;
pub(crate) mod table;

pub(crate) use bindings::add_to_linker;
#[cfg(test)]
pub(crate) use error::IntoAnswer;
pub(crate) use io::Pollable;
pub(crate) use table::Table;

/// The most bytes one read hands the guest, one `check-write` offers a file,
/// and about the most one datagram `receive` answers, as wasmtime-wasi's
/// one read allocation limit bounds each.
const MAX_READ_SIZE_ALLOC: usize = 64 * 1024;

/// `duration` as the nanoseconds a WASI `duration`, `instant`, or preview1
/// `timestamp` counts, the largest one for a time past what it holds.
pub(crate) fn to_nanos(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

#[allow(clippy::all, unused_parens)]
pub(crate) mod bindings {
    include!(concat!(env!("OUT_DIR"), "/preview2_bindings.rs"));
}

impl bindings::Host for crate::invocation::Invocation {
    fn abandonment(&self) -> Option<wasmtime::Error> {
        crate::invocation::Invocation::abandonment(self)
    }

    fn lift_handle<H: table::Handle>(&mut self, handle: H) -> wasmtime::Result<H> {
        self.table_mut().get(handle).map(|_| handle)
    }

    fn take_handle<H: table::Handle>(&mut self, handle: H) -> wasmtime::Result<H::Host> {
        self.table_mut().delete(handle)
    }
}
