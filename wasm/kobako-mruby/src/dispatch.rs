//! Reaching the host from a method this guest defines.
//!
//! The tier below routes a Call without knowing mruby, so it takes the
//! wire's `block_given` bit and nothing else. A block is an mruby value,
//! and the host's yield re-enters through a separate export while this
//! dispatch frame is still parked on the wasm stack — so something has to
//! hold it there. Both halves live here, in one call, because they are one
//! fact: a caller that passed the bit without parking the block would have
//! the host yield into nothing, and one that parked it without the bit
//! would have a block silently ignored.
//!
//! The payload is bytes this crate never reads. A gem encodes it before
//! calling and decodes the answer after, which is also what keeps every
//! raise it might do outside the parked block's lifetime.

use beni::Value;
use kobako_core::proxy;

pub use kobako_core::DispatchError;
pub use kobako_transport::envelope::Target;

use crate::runtime::block_stack::BlockFrame;

/// Round one Call through the host and hand back the body its Reply
/// tagged, with `block` reachable by the host's yields for as long as the
/// call is out.
///
/// `block` is the value an mruby method received — `Value::nil()` when the
/// caller passed none, which is what an any-arity method reads off its
/// call frame. A nil block parks nothing and tells the host there is
/// nothing to yield to.
///
/// A refusal arrives typed on the envelope rather than as payload bytes,
/// so a gem reads the host's reason for saying no without owning a
/// representation for it.
pub fn dispatch(
    target: Target<'_>,
    method: &str,
    block: Value,
    payload: &[u8],
) -> Result<Vec<u8>, DispatchError> {
    let frame = BlockFrame::push_if_block(block);
    let answer = proxy::dispatch(target, method, frame.block_given(), payload);
    drop(frame);
    answer
}
