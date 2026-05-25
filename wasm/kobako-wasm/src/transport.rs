//! Kobako transport layer — guest-side mirror of the host's
//! `lib/kobako/transport/` directory. Houses the envelope value objects
//! (`envelope`) and the guest dispatch path (`proxy`) that drive every
//! host↔guest transport call.

pub mod envelope;
pub mod proxy;
