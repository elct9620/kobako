//! kobako host SDK — run untrusted mruby in an in-process Wasm sandbox.
//!
//! The Rust counterpart of the Ruby gem's `Kobako::Sandbox`: one
//! `Sandbox` per guest, Services bound under `<Namespace>::<Member>`
//! names, `eval` / `run` invocations returning a decoded wire `Value`
//! or a typed `Error`. Behavior parity with the Ruby frontend is
//! pinned by the differential harness in the repository's
//! `test/parity/` suite; the API shape itself is deliberately
//! idiomatic Rust, not a Ruby mirror.

mod catalog;
mod dispatch;
pub mod error;
pub mod handles;
mod outcome;
pub mod receiver;
pub mod sandbox;
mod snippet;
pub mod yielder;

pub use error::{Error, GuestFailure};
pub use handles::Handles;
pub use kobako_codec::codec::Value;
pub use kobako_runtime::profile::Profile;
pub use receiver::{Fault, FaultKind, Receiver};
pub use sandbox::{Options, RunArg, Sandbox, Usage};
pub use yielder::{YieldError, Yielder};
