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
pub mod member;
mod outcome;
pub mod sandbox;
mod snippet;

pub use error::{Error, GuestFailure};
pub use kobako_codec::codec::Value;
pub use kobako_runtime::profile::Profile;
pub use member::{Fault, FaultKind, Member};
pub use sandbox::{Options, Sandbox, Usage};
