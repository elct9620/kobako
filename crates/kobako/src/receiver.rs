//! The Receiver seam: the host object a guest dispatch resolves its
//! target to — a bound constant's path or a capability Handle.
//!
//! A `Receiver` answers the guest's dispatches with payload bytes or a
//! `Fault` — the three refusal kinds the dispatch contract lets a Service
//! surface. What those bytes mean is the Receiver's own choice of schema;
//! `msgpack::ValueReceiver` is the path for the default MessagePack one,
//! reaching this seam through its own `into_receiver`.

use std::any::Any;

use crate::handles::Handles;
use crate::yielder::Yielder;

/// The refusal a dispatch can come back with, and which of the three
/// categories it reports.
///
/// Both come from the fixed tier rather than being restated here: a
/// Fault is the whole of a Reply's fault arm and every field of one is
/// kobako's, so the envelope owns the type and this frontend hands the
/// same value on. `FaultKind::Undefined` is also what stands in for the
/// Ruby dispatcher's reflection floor — a Rust host object has no
/// ambient `send` / `instance_eval` surface, so an unrouted name simply
/// does not exist.
pub use kobako_transport::envelope::{Fault, FaultKind};

/// The host object a dispatch runs the Call's method on, reached
/// as `MyService::KV` or through a capability Handle.
///
/// `Send + Sync` because the dispatch handler crosses the engine
/// boundary behind an `Arc`; calls take `&self`, so a stateful
/// receiver carries its state behind interior mutability (a `Mutex`
/// field).
///
/// Expected refusals return a `Fault`. A panic is a programming
/// error: it unwinds out of the invocation verb instead of folding
/// into a fault envelope — the counterpart of a non-`StandardError`
/// escaping the Ruby dispatcher's rescue.
///
/// `block` is present when the guest call site supplied a block; the
/// `Yielder` riding it is the block's host-side stand-in, and each
/// each yield is a synchronous round-trip into the guest
/// whose errors propagate with `?`. `handles` is the invocation's
/// capability-Handle view: `Handles::alloc` hands the guest a stateful
/// host object as an opaque token, `Handles::resolve` turns a
/// `Value::Handle` argument back into the live object.
///
/// `Any` is a supertrait so a resolved host object recovers its
/// concrete type: upcast the `Arc` to `Arc<dyn Any + Send + Sync>`
/// and `downcast` — the Rust spelling of the Ruby frontend's
/// restore-to-original-object.
/// The payload is bytes, not a decoded shape, because a `Catalog` holds
/// heterogeneous receivers behind `Arc<dyn Receiver>`: a trait object has
/// one signature, so a payload type on the trait would force every
/// Service in one Sandbox onto a single schema. Decoding belongs inside
/// each implementation, which is what lets one Service speak protobuf
/// while another speaks MessagePack.
///
/// Implement `msgpack::ValueReceiver` instead when the payload is
/// kobako's default MessagePack shape, and bind what its `into_receiver`
/// hands back.
pub trait Receiver: Any + Send + Sync {
    fn call(
        &self,
        method: &str,
        payload: &[u8],
        block: Option<&mut Yielder<'_>>,
        handles: &Handles<'_>,
    ) -> Result<Vec<u8>, Fault>;

    /// Opt-in least-privilege narrowing of the guest-reachable method
    /// surface: a `false` answer rejects the dispatch as `undefined`
    /// before `call` runs, and the guest cannot reach the predicate
    /// itself. The default leaves the surface unchanged.
    fn respond_to_guest(&self, method: &str) -> bool {
        let _ = method;
        true
    }
}

/// A receiver that answers nothing — for the tests that need an object
/// standing at a path or in a Handle table rather than a Service with
/// behaviour. Byte-level, so those tests stand without a payload codec.
#[cfg(test)]
pub(crate) struct Probe;

#[cfg(test)]
impl Receiver for Probe {
    fn call(
        &self,
        _method: &str,
        _payload: &[u8],
        _block: Option<&mut Yielder<'_>>,
        _handles: &Handles<'_>,
    ) -> Result<Vec<u8>, Fault> {
        Ok(Vec::new())
    }
}
