//! The bundled MessagePack schema's overlay on the byte-level surface.
//!
//! Every payload position this crate exposes is bytes; this module adds
//! the one schema kobako ships a spelling for, and nothing beneath it
//! knows the module exists. Each member is a thin wrapper over the
//! byte-level entry it flavours — `Execution::value` over `payload`,
//! `Yielder::call` over `call_payload`, `resolve_as` over `resolve`,
//! `into_receiver` over `Receiver` — so a member that could not be
//! written that way would be marking a gap in that surface rather than a
//! convenience on top of it.
//!
//! That is also the shape a schema kobako does not ship takes: written
//! outside this crate as extension traits over the same entries, needing
//! nothing here to change. The module is one file per position for the
//! same reason the guest tiers carry one namespace per schema — turning
//! the `msgpack` feature off removes this module whole.

mod execution;
mod handles;
mod payload;
mod receiver;
mod yielder;

pub use payload::RunArg;
pub use receiver::{IntoReceiver, ValueReceiver};
