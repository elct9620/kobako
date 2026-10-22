//! Engine-neutral block-yield re-entry contract, free of `magnus` and of
//! any wasmtime type.
//!
//! During a guest→host dispatch, a Service method may yield to a guest
//! block. The host drives that re-entry through a `Yielder`: it ships the
//! yield-arguments payload into the in-flight guest and returns the
//! decoded Yield Reply. What backs the re-entry — a wasmtime `Caller`, some
//! other engine handle — is the implementer's concern; the dispatch
//! contract sees only this trait.

use kobako_transport::envelope::YieldReply;

use crate::error::Trap;

/// Host-initiated re-entry into the in-flight guest instance to run a
/// yielded block.
///
/// Each open Call has a `frame`, counted from `0` at the outermost.
/// `yield_to_block` ships `args` as a `yield` downcall to the block of the
/// open Call `frame` names, and returns the decoded Yield Reply,
/// `Unsupported` arm included, or a `Trap` — surfaced through the
/// frontend's trap-error mapping — when the re-entry traps, the guest
/// returns an empty or unframeable result, or a payload exceeds the
/// 16 MiB cap.
pub trait Yielder {
    /// The frame of the Call this Yielder was lent for.
    fn frame(&self) -> u32;

    fn yield_to_block(&mut self, frame: u32, args: &[u8]) -> Result<YieldReply, Trap>;
}
