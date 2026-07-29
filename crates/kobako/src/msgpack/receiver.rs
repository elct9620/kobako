//! The dispatch position: a Receiver that speaks the value tree.

use std::any::Any;
use std::sync::Arc;

use kobako_codec::msgpack::codec::{Decode, Encoder, Value};
use kobako_codec::msgpack::payload::Arguments;

use crate::handles::Handles;
use crate::receiver::{Fault, FaultKind, Receiver};
use crate::yielder::Yielder;

/// A Receiver that speaks a value tree: positional and keyword arguments
/// as wire `Value`s, answering with one.
///
/// The shape is a class of codec's, not one codec's — any schema that can
/// carry the whole `Value` set fills it, and the bundled MessagePack one
/// is the instance in this build. A schema that cannot carry some of the
/// set (JSON has no byte string and no ext) does not belong here; it
/// implements `Receiver` directly and owns its own bytes.
///
/// `into_receiver` is how one reaches the byte-level seam every binding
/// site takes.
pub trait ValueReceiver: Any + Send + Sync {
    fn call(
        &self,
        method: &str,
        args: &[Value],
        kwargs: &[(String, Value)],
        block: Option<&mut Yielder<'_>>,
        handles: &Handles<'_>,
    ) -> Result<Value, Fault>;

    /// Same narrowing contract as `Receiver::respond_to_guest`, forwarded
    /// unchanged across the seam.
    fn respond_to_guest(&self, method: &str) -> bool {
        let _ = method;
        true
    }

    /// Present this at the byte-level seam — what `Sandbox::bind`,
    /// `Context::bind`, and `Handles::alloc` all take.
    ///
    /// Hands back an `Arc` rather than the bare wrapper: every binding site
    /// takes one, and the wrapper holds its receiver behind an `Arc`
    /// already, so returning the bare form only ever bought a second layer
    /// at each call site.
    ///
    /// A type implementing two schemas' receiver traits has two of these
    /// in scope, and the call is ambiguous until one is named
    /// (`ValueReceiver::into_receiver(kv)`). That is the right question to
    /// be asked: binding an object is choosing the schema the guest will
    /// reach it through.
    fn into_receiver(self) -> Arc<IntoReceiver<Self>>
    where
        Self: Sized,
    {
        Arc::new(IntoReceiver(Arc::new(self)))
    }
}

/// A `ValueReceiver` standing at the byte-level seam: it decodes each
/// payload into a value tree and encodes the answer back, with the codec
/// this build resolves to.
///
/// A malformed payload and an unencodable answer both surface as a
/// `runtime` fault, matching how the Ruby frontend folds the same two
/// failures.
///
/// The wrapped receiver is held behind its own `Arc`, so `resolve_as`
/// hands back an `Arc<V>` — the same shape the byte-level path's
/// `downcast` produces, rather than one wrapped in this type.
pub struct IntoReceiver<V>(Arc<V>);

impl<V> IntoReceiver<V> {
    /// The shared handle this holds its receiver by, borrowed — what
    /// `resolve_as` clones out when it walks a resolved receiver back to
    /// the type that bound it.
    pub(crate) fn shared(&self) -> &Arc<V> {
        &self.0
    }
}

impl<V: ValueReceiver> Receiver for IntoReceiver<V> {
    fn call(
        &self,
        method: &str,
        payload: &[u8],
        block: Option<&mut Yielder<'_>>,
        handles: &Handles<'_>,
    ) -> Result<Vec<u8>, Fault> {
        let arguments = Arguments::decode(payload).map_err(|err| {
            Fault::new(
                FaultKind::Runtime,
                format!("Sandbox received a malformed request: {err}"),
            )
        })?;
        let value = self
            .0
            .call(method, &arguments.args, &arguments.kwargs, block, handles)?;
        Encoder::encode(&value)
            .map_err(|err| Fault::new(FaultKind::Runtime, format!("response not encodable: {err}")))
    }

    fn respond_to_guest(&self, method: &str) -> bool {
        self.0.respond_to_guest(method)
    }
}

#[cfg(test)]
mod tests {
    use kobako_codec::msgpack::codec::{Encode, Encoder};
    use kobako_codec::msgpack::payload::Arguments;

    use super::*;
    use crate::handles::Detached;

    /// Answers `echo` with its first positional argument, and narrows
    /// every other name away.
    struct Echo;

    impl ValueReceiver for Echo {
        fn call(
            &self,
            _method: &str,
            args: &[Value],
            _kwargs: &[(String, Value)],
            _block: Option<&mut Yielder<'_>>,
            _handles: &Handles<'_>,
        ) -> Result<Value, Fault> {
            Ok(args.first().cloned().unwrap_or(Value::Nil))
        }

        fn respond_to_guest(&self, method: &str) -> bool {
            method == "echo"
        }
    }

    #[test]
    fn the_seam_decodes_the_payload_and_encodes_the_answer() {
        let payload = Arguments::new(vec![Value::Int(42)], Vec::new())
            .encode()
            .unwrap();
        let table = Detached::new();

        let answer = Echo
            .into_receiver()
            .call("echo", &payload, None, &table.as_handles())
            .unwrap();

        assert_eq!(
            answer,
            Encoder::encode(&Value::Int(42)).unwrap(),
            "an encodable payload through into_receiver must reach the receiver as values \
             and come back as this schema's bytes"
        );
    }

    #[test]
    fn a_payload_this_schema_cannot_read_folds_into_a_runtime_fault() {
        let table = Detached::new();

        // A truncated msgpack str header — framed as a payload, unreadable
        // as one.
        let refusal = Echo
            .into_receiver()
            .call("echo", &[0xd9], None, &table.as_handles());

        assert!(
            matches!(refusal, Err(fault) if fault.kind == FaultKind::Runtime),
            "a payload this schema cannot read must refuse as a runtime fault rather than \
             reach the receiver"
        );
    }

    #[test]
    fn the_seam_forwards_the_wrapped_receivers_narrowing() {
        let bound = Echo.into_receiver();

        assert!(
            bound.respond_to_guest("echo") && !bound.respond_to_guest("label"),
            "a narrowed ValueReceiver at the seam must keep its own answer, since the \
             predicate is forwarded unchanged"
        );
    }
}
