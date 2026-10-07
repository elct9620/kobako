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
/// The value tree is this schema's, not a neutral one — four of its
/// variants come straight from MessagePack's type mapping. A schema shaped
/// the same way can reuse it; one that is not (JSON has no byte string and
/// no ext) does not belong here, and its host implements `Receiver`
/// directly and owns its own bytes.
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
/// A malformed payload surfaces as an `internal` fault and an unencodable
/// answer as a `runtime` one, matching how the Ruby frontend folds the
/// same two failures.
///
/// The wrapped receiver is held behind its own `Arc`, so `resolve_as`
/// hands back an `Arc<V>` — the same shape the byte-level path's
/// `downcast` produces, rather than one wrapped in this type.
pub struct IntoReceiver<V>(Arc<V>);

impl<V> IntoReceiver<V> {
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
                FaultKind::Internal,
                format!("Sandbox could not read the request: {err}"),
            )
        })?;
        let value = self
            .0
            .call(method, &arguments.args, &arguments.kwargs, block, handles)?;
        Encoder::encode(&value).map_err(|err| {
            Fault::new(
                FaultKind::Runtime,
                format!("Sandbox could not write the Service's answer: {err}"),
            )
        })
    }

    fn respond_to_guest(&self, method: &str) -> bool {
        self.0.respond_to_guest(method)
    }
}

#[cfg(test)]
mod tests {
    use kobako_codec::msgpack::codec::{Encode, Encoder, MAX_NESTING_DEPTH};
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

    // @behavior T-171
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

    // @behavior T-163
    #[test]
    fn a_payload_this_schema_cannot_read_folds_into_an_internal_fault() {
        let table = Detached::new();

        // A truncated msgpack str header — framed as a payload, unreadable
        // as one.
        let refusal = Echo
            .into_receiver()
            .call("echo", &[0xd9], None, &table.as_handles());

        assert!(
            matches!(refusal, Err(fault) if fault.kind == FaultKind::Internal),
            "a payload this schema cannot read must refuse as an internal fault — the \
             receiver never ran, so nothing about it failed"
        );
    }

    /// Answers every name with an array nested one level past the deepest
    /// the schema writes.
    struct TooDeep;

    impl ValueReceiver for TooDeep {
        fn call(
            &self,
            _method: &str,
            _args: &[Value],
            _kwargs: &[(String, Value)],
            _block: Option<&mut Yielder<'_>>,
            _handles: &Handles<'_>,
        ) -> Result<Value, Fault> {
            Ok((0..=MAX_NESTING_DEPTH).fold(Value::Nil, |inner, _| Value::Array(vec![inner])))
        }
    }

    #[test]
    fn an_answer_this_schema_cannot_write_is_the_services_failure() {
        let payload = Arguments::new(Vec::new(), Vec::new()).encode().unwrap();
        let table = Detached::new();

        let refusal = TooDeep
            .into_receiver()
            .call("answer", &payload, None, &table.as_handles());

        assert!(
            matches!(&refusal, Err(fault) if fault.kind == FaultKind::Runtime
                && fault.message.contains("could not write the Service's answer")),
            "an answer nested past the schema's bound through into_receiver must refuse as \
             the Service's runtime failure, worded as the Ruby frontend words it, got {refusal:?}"
        );
    }

    // @behavior T-126
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
