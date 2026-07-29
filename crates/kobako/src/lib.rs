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
//! payload, a yield, an invocation's result, and the host object a
//! Handle stands for — has a byte-level entry, because what those bytes
//! mean is the host's own choice of schema. `Receiver`,
//! `RunPayload::bytes` / `build`, `Yielder::call_payload`,
//! `Execution::payload`, and `resolve` on `Handles` / `Execution` are
//! that surface. The last takes an id, because an id is what the Handle
//! table owns and spelling one is the schema's business.
//!
//! The default `msgpack` feature adds the bundled codec's spelling of
//! each, in the `msgpack` module: `ValueReceiver` and its
//! `into_receiver`, `RunPayload::values`, `Yielder::call_values`,
//! `Execution::value`, `resolve_as`, and the `Value` type they speak in.
//! Every member there is a thin wrapper over the byte-level entry it
//! flavours, so a verb never belongs to one spelling — `run` takes
//! whichever payload it is handed — and turning the feature off removes
//! conveniences, not capabilities. The crate then resolves to no payload
//! codec at all, which is what makes "the codec is replaceable" a
//! property of the dependency graph rather than a claim about it.
//!
//! A schema kobako does not ship takes the same shape from outside: an
//! overlay of extension traits over those same entries, written in the
//! consuming crate rather than added to this one.

mod catalog;
mod dispatch;
pub mod error;
pub mod execution;
pub mod extension;
pub mod handles;
#[cfg(feature = "msgpack")]
pub mod msgpack;
pub mod payload;
pub mod receiver;
pub mod sandbox;
mod snippet;
pub mod yielder;

// The root carries what every build has. The `msgpack` overlay's own
// names stay in `msgpack`, since a name that vanishes from the root when a
// feature goes off was never a root name.
pub use error::{Error, Failure};
pub use execution::{Execution, Usage};
pub use extension::{Backend, Extension, Provider};
pub use handles::Handles;
pub use kobako_runtime::profile::Profile;
pub use payload::RunPayload;
pub use receiver::{Fault, FaultKind, Receiver};
pub use sandbox::{Context, Options, Sandbox};
pub use yielder::{YieldError, Yielder};
