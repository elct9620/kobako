//! The host-object seam: what a Rust embedder binds under a
//! `<Namespace>::<Member>` name.
//!
//! A `Member` answers the guest's dispatches with wire `Value`s or a
//! `Fault` — the three refusal kinds the dispatch contract lets a
//! Service surface. The dispatcher folds everything else (decode
//! failures, unencodable responses) itself, so implementations never
//! need to think about the wire.

use kobako_codec::codec::Value;

use crate::block::Block;
use crate::handles::Handles;

/// The refusal kinds a dispatch can come back with; each maps to the
/// proxy-side error the guest raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    /// No such member / method (Ruby dispatcher's `undefined`).
    Undefined,
    /// The call shape does not fit the method (`argument`).
    Argument,
    /// The member itself failed (`runtime`).
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

/// A host object the guest reaches as `<Namespace>::<Member>`.
///
/// `Send + Sync` because the dispatch handler crosses the engine
/// boundary behind an `Arc`; calls take `&self`, so a stateful member
/// carries its state behind interior mutability (a `Mutex` field).
///
/// Expected refusals return a `Fault`. A panic is a programming
/// error: it unwinds out of the invocation verb instead of folding
/// into a fault envelope — the counterpart of a non-`StandardError`
/// escaping the Ruby dispatcher's rescue.
///
/// `block` is present when the guest call site supplied a block; each
/// `Block::call` is a synchronous yield round-trip into the guest, and
/// its errors propagate with `?`. `handles` is the invocation's
/// capability-Handle view: `Handles::alloc` hands the guest a stateful
/// host object as an opaque token, `Handles::resolve` turns a
/// `Value::Handle` argument back into the live object.
pub trait Member: Send + Sync {
    fn call(
        &self,
        method: &str,
        args: &[Value],
        kwargs: &[(String, Value)],
        block: Option<&mut Block<'_>>,
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
