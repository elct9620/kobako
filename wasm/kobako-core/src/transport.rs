//! Kobako transport — the guest dispatch path. `proxy` drives the
//! `__kobako_dispatch` ABI import over the envelope value objects,
//! which live on the wire tier (`kobako_codec::transport`) together
//! with the `Encode` / `Decode` traits they carry their byte form
//! through. This module keeps the host-matching `transport::` path for
//! the one guest-bound constant.

pub mod proxy;
