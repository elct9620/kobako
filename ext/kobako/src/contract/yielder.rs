//! Engine-neutral block-yield re-entry contract, free of `magnus` and of
//! any wasmtime type.
//!
//! During a guest→host dispatch, a Service method may yield to a guest
//! block. The host drives that re-entry through a `Yielder`: it ships the
//! yield-arguments payload into the in-flight guest and returns the
//! YieldResponse bytes. What backs the re-entry — a wasmtime `Caller`, some
//! other engine handle — is the implementer's concern; the dispatch
//! contract sees only this trait.

use crate::contract::error::Trap;

/// Host-initiated re-entry into the in-flight guest instance to run a
/// yielded block.
///
/// `yield_block` ships `args` to `__kobako_yield_to_block` and returns the
/// raw YieldResponse bytes, or a `Trap` (boundary → `Kobako::TrapError`)
/// when the re-entry traps, the guest returns an empty result, or a payload
/// exceeds the 16 MiB cap.
pub(crate) trait Yielder {
    fn yield_block(&mut self, args: &[u8]) -> Result<Vec<u8>, Trap>;
}
