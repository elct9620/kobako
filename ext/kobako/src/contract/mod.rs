//! Engine-neutral, frontend-free host contract types.
//!
//! Nothing in this module depends on `magnus` or any Ruby type. Each
//! frontend maps these shapes onto its own host-language surface at its
//! boundary — for the Ruby ext that is the error mapper in its runtime
//! module. Keeping the contract free of frontend types lets it move to a
//! standalone runtime crate unchanged.

pub(crate) mod dispatch;
pub(crate) mod error;
pub(crate) mod runtime;
pub(crate) mod snapshot;
pub(crate) mod yielder;
