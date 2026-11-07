//! The WASI 0.2 imports a Guest Binary links as a core module, encoded onto
//! `kobako-runtime` as preview1's are. `build.rs` generates each import's
//! glue from the vendored WIT; the Wasm Driver answers its `Host` trait.

pub(crate) mod abi;
mod host;
mod sockets;
mod table;

pub(crate) use bindings::add_to_linker;
pub(crate) use table::Table;

#[allow(clippy::all, unused_parens)]
pub(crate) mod bindings {
    include!(concat!(env!("OUT_DIR"), "/preview2_bindings.rs"));
}
