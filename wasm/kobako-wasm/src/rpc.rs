//! Kobako RPC layer — guest-side mirror of the host's `lib/kobako/rpc/`
//! directory. Houses the RPC envelope value objects (`envelope`) and
//! the guest dispatch path (`client`) that drive every host↔guest RPC
//! call.

pub mod client;
pub mod envelope;
