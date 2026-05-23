//! Kobako transport layer — guest-side mirror of the host's
//! `lib/kobako/transport/` directory. Houses the envelope value objects
//! (`envelope`) and the guest dispatch path (`client`) that drive every
//! host↔guest transport call. The crate-internal module name keeps the
//! shorter `rpc` form for now; renaming the Rust submodule is a later
//! cleanup.

pub mod client;
pub mod envelope;
