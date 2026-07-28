//! kobako host SDK — run untrusted mruby in an in-process Wasm sandbox.
//!
//! The Rust counterpart of the Ruby gem's `Kobako::Sandbox`: one
//! `Sandbox` per guest, Services bound under `MyService::KV` names,
//! `eval` / `run` invocations returning an `Execution` — the record of
//! one run, carrying the guest's answer (or a guest failure's `Error`),
//! the output captures, and usage. Behavior parity with the Ruby
//! frontend is pinned by the differential harness in the repository's
//! `test/parity/` suite; the API shape itself is deliberately idiomatic
//! Rust, not a Ruby mirror.
//!
//! # Payloads and the `msgpack` feature
//!
//! Every payload position — a dispatch's arguments and answer, a `run`
//! payload, a yield, an invocation's result — has a byte-level entry,
//! because what those bytes mean is the host's own choice of schema.
//! `Receiver`, `Sandbox::run_payload`, `Yielder::call_payload`, and
//! `Execution::payload` are that surface.
//!
//! The default `msgpack` feature adds the bundled codec's spelling of
//! each: `ValueReceiver`, `Sandbox::run`, `Yielder::call`,
//! `Execution::value`, and the `Value` type they speak in. Turn it off
//! and the crate resolves to no payload codec at all — which is what
//! makes "the codec is replaceable" a property of the dependency graph
//! rather than a claim about it.

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
#[cfg(feature = "msgpack")]
pub use kobako_codec::msgpack::codec::Value;
pub use kobako_runtime::profile::Profile;
pub use receiver::{Fault, FaultKind, Receiver};
#[cfg(feature = "msgpack")]
pub use receiver::{ValueAdapter, ValueReceiver};
#[cfg(feature = "msgpack")]
pub use sandbox::RunArg;
pub use sandbox::{Context, Options, Sandbox, Usage};
pub use yielder::{YieldError, Yielder};
