//! The dispatch position: a Receiver that speaks the value tree.

use std::any::Any;

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
/// `ValueAdapter` bridges this onto the opaque `Receiver` the Catalog
/// stores.
pub trait ValueReceiver: Any + Send + Sync {
    fn call(
        &self,
        method: &str,
        args: &[Value],
        kwargs: &[(String, Value)],
        block: Option<&mut Yielder<'_>>,
        handles: &Handles<'_>,
    ) -> Result<Value, Fault>;

    /// Same narrowing contract as `Receiver::respond_to_guest`; the
    /// adapter forwards it unchanged.
    fn respond_to_guest(&self, method: &str) -> bool {
        let _ = method;
        true
    }
}

/// Binds a `ValueReceiver` into a Catalog by decoding the payload into a
/// value tree and encoding the answer back — with the codec this build
/// resolves to, which is the bundled MessagePack one.
///
/// A malformed payload and an unencodable answer both surface as a
/// `runtime` fault, matching how the Ruby frontend folds the same two
/// failures.
pub struct ValueAdapter<V>(V);

impl<V: ValueReceiver> ValueAdapter<V> {
    pub fn new(receiver: V) -> Self {
        ValueAdapter(receiver)
    }

    /// The wrapped receiver. A Handle resolved back to its object
    /// downcasts to `ValueAdapter<V>` rather than to `V`, since the
    /// adapter is what the Catalog stores; this is how a caller reaches
    /// its own type from there.
    pub fn receiver(&self) -> &V {
        &self.0
    }
}

impl<V: ValueReceiver> Receiver for ValueAdapter<V> {
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
    use std::sync::Arc;

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
    fn the_adapter_decodes_the_payload_and_encodes_the_answer() {
        let payload = Arguments::new(vec![Value::Int(42)], Vec::new())
            .encode()
            .unwrap();
        let table = Detached::new();

        let answer = ValueAdapter::new(Echo)
            .call("echo", &payload, None, &table.view())
            .unwrap();

        assert_eq!(
            answer,
            Encoder::encode(&Value::Int(42)).unwrap(),
            "an encodable payload through ValueAdapter must reach the receiver as values \
             and come back as this schema's bytes"
        );
    }

    #[test]
    fn the_adapter_folds_a_payload_this_schema_cannot_read_into_a_runtime_fault() {
        let table = Detached::new();

        // A truncated msgpack str header — framed as a payload, unreadable
        // as one.
        let refusal = ValueAdapter::new(Echo).call("echo", &[0xd9], None, &table.view());

        assert!(
            matches!(refusal, Err(fault) if fault.kind == FaultKind::Runtime),
            "a payload this schema cannot read through ValueAdapter must refuse as a \
             runtime fault rather than reach the receiver"
        );
    }

    #[test]
    fn the_adapter_forwards_the_wrapped_receivers_narrowing() {
        let adapter = ValueAdapter::new(Echo);

        assert!(
            adapter.respond_to_guest("echo") && !adapter.respond_to_guest("label"),
            "a narrowed ValueReceiver bound through ValueAdapter must keep its own \
             answer, since the adapter forwards the predicate unchanged"
        );
    }

    #[test]
    fn a_resolved_adapter_downcasts_back_to_the_receiver_it_wraps() {
        let table = Detached::new();
        let handles = table.view();
        let id = handles.alloc(Arc::new(ValueAdapter::new(Echo))).unwrap();
        let resolved = handles.resolve(id).expect("the id is live");

        let any: Arc<dyn std::any::Any + Send + Sync> = resolved;
        let adapter = any.downcast::<ValueAdapter<Echo>>().expect(
            "a Value receiver enters the table wrapped, so the upcast recovers the adapter",
        );

        assert!(
            adapter.receiver().respond_to_guest("echo"),
            "a Handle standing for a ValueReceiver must reach the caller's own type \
             through the adapter it was bound behind"
        );
    }
}
