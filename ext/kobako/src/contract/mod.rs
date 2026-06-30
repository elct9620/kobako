//! Engine-neutral, magnus-free host contract types.
//!
//! Nothing in this module depends on `magnus` or any Ruby type; the
//! boundary that maps these shapes onto `Kobako::*` exceptions lives in
//! `crate::runtime::errors`. Keeping the contract free of the Ruby surface
//! lets it move to a standalone runtime crate unchanged.

pub(crate) mod error;
