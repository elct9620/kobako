//! Kobako transport — the guest dispatch path. `proxy` drives the
//! `__kobako_dispatch` ABI import over the core envelope, which lives on
//! the wire tier (`kobako_codec::envelope`). This module keeps the
//! host-matching `transport::` path for the one guest-bound constant.

pub mod proxy;
