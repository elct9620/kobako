//! kobako host SDK — run untrusted mruby in an in-process Wasm sandbox.
//!
//! The Rust counterpart of the Ruby gem's `Kobako::Sandbox`: one
//! `Sandbox` per guest, Services bound under `MyService::KV` names,
//! `eval` / `run` invocations returning an `Execution` — the record of
//! one run, carrying the decoded value (or a guest failure's `Error`),
//! the output captures, and usage. Behavior parity with the Ruby
//! frontend is pinned by the differential harness in the repository's
//! `test/parity/` suite; the API shape itself is deliberately idiomatic
//! Rust, not a Ruby mirror.

mod catalog;
mod dispatch;
pub mod error;
pub mod execution;
pub mod extension;
pub mod handles;
mod outcome;
pub mod receiver;
pub mod sandbox;
mod snippet;
pub mod yielder;

pub use error::{Error, Failure};
pub use execution::Execution;
pub use extension::{Backend, Extension, Provider};
pub use handles::Handles;
pub use kobako_codec::msgpack::codec::Value;
pub use kobako_runtime::profile::Profile;
pub use receiver::{Fault, FaultKind, Receiver, ValueAdapter, ValueReceiver};
pub use sandbox::{Context, Options, RunArg, Sandbox, Usage};
pub use yielder::{YieldError, Yielder};
