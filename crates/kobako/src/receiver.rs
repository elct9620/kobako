//! The Receiver seam: the host object a guest dispatch resolves its
//! target to — a bound Member path or a capability Handle.
//!
//! A `Receiver` answers the guest's dispatches with wire `Value`s
//! or a `Fault` — the three refusal kinds the dispatch contract lets
//! a Service surface. The dispatcher folds everything else (decode
//! failures, unencodable responses) itself, so implementations never
//! need to think about the wire.

use std::any::Any;

use kobako_codec::codec::Value;

use crate::handles::Handles;
use crate::yielder::Yielder;

/// The refusal kinds a dispatch can come back with; each maps to the
/// proxy-side error the guest raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    /// No such member / method (Ruby dispatcher's `undefined`). The
    /// arm of `call` that answers an unrouted method with this kind is
    /// also what stands in for the Ruby dispatcher's reflection floor:
    /// a Rust host object has no ambient `send` / `instance_eval`
    /// surface, so an unrouted name simply does not exist.
    Undefined,
    /// The call shape does not fit the method (`argument`).
    Argument,
    /// The host object itself failed (`runtime`).
    Runtime,
}

impl FaultKind {
    /// The wire spelling of the fault payload's `type` field.
    pub(crate) fn wire_name(self) -> &'static str {
        match self {
            FaultKind::Undefined => "undefined",
            FaultKind::Argument => "argument",
            FaultKind::Runtime => "runtime",
        }
    }
}

/// A Service-level refusal: the guest re-raises it as a rescuable
/// exception, never a wasm trap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    pub kind: FaultKind,
    pub message: String,
}

impl Fault {
    pub fn new(kind: FaultKind, message: impl Into<String>) -> Self {
        Fault {
            kind,
            message: message.into(),
        }
    }
}

/// The host object a dispatch runs the Request's method on, reached
/// as `<Namespace>::<Member>` or through a capability Handle.
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
/// `Yielder::call` is a synchronous yield round-trip into the guest
/// whose errors propagate with `?`. `handles` is the invocation's
/// capability-Handle view: `Handles::alloc` hands the guest a stateful
/// host object as an opaque token, `Handles::resolve` turns a
/// `Value::Handle` argument back into the live object.
///
/// `Any` is a supertrait so a resolved host object recovers its
/// concrete type: upcast the `Arc` to `Arc<dyn Any + Send + Sync>`
/// and `downcast` — the Rust spelling of the Ruby frontend's
/// restore-to-original-object.
pub trait Receiver: Any + Send + Sync {
    fn call(
        &self,
        method: &str,
        args: &[Value],
        kwargs: &[(String, Value)],
        block: Option<&mut Yielder<'_>>,
        handles: &Handles<'_>,
    ) -> Result<Value, Fault>;

    /// Opt-in least-privilege narrowing of the guest-reachable method
    /// surface: a `false` answer rejects the dispatch as `undefined`
    /// before `call` runs, and the guest cannot reach the predicate
    /// itself. The default leaves the surface unchanged.
    fn respond_to_guest(&self, method: &str) -> bool {
        let _ = method;
        true
    }
}
