//! kobako-wasmtime — the wasmtime implementation of the kobako runtime
//! contract.
//!
//! `Driver` implements `kobako_runtime::runtime::Runtime` over wasmtime:
//! every invocation instantiates a fresh instance from a pre-linked
//! template and discards the whole Store afterwards — the
//! per-invocation instance discipline. Everything engine-bound
//! lives behind the contract surface, so a frontend shell (the Ruby
//! ext's `Kobako::Runtime`) sees no wasmtime type.
//!
//! Each module holds one responsibility and opens with its own doc.

mod abi;
mod ambient;
mod cache;
mod capture;
mod config;
mod dispatch;
mod driver;
mod exports;
mod frames;
mod guest_mem;
mod instance_pre;
mod invocation;
mod limiter;
mod trap;

pub use config::Config;
pub use driver::Driver;
