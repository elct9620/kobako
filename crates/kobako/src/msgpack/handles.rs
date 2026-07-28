//! The Handle position: reaching the object behind an id as your own type.
//!
//! A Handle is an id wherever it travels — the table owns it, and a schema
//! only decides how it is spelled on the wire (`Value::Handle(id)` here, a
//! plain field in another). Reading that spelling is a destructure, so
//! nothing here wraps it; what the overlay owes is the other end, where the
//! wrapper it introduced stands between an id and the type a caller bound.

use std::sync::Arc;

use crate::execution::Execution;
use crate::handles::Handles;
use crate::msgpack::receiver::{IntoReceiver, ValueReceiver};

impl Handles<'_> {
    /// Resolve a Handle id to the receiver bound under it, as the type
    /// that bound it; `None` for an id this invocation never issued, or
    /// one standing for something else.
    ///
    /// The byte-level path recovers a concrete type by downcasting what
    /// `resolve` hands back. A value receiver reaches the table behind
    /// `into_receiver`'s wrapper, so this walks that step for the caller
    /// and answers with the same `Arc<V>` the byte-level path would.
    pub fn resolve_as<V: ValueReceiver>(&self, id: u32) -> Option<Arc<V>> {
        resolve_as(self.resolve(id)?)
    }
}

impl Execution {
    /// Resolve a Handle id this run's result carried, as the type that
    /// bound it — `Handles::resolve_as` against the table the Execution
    /// holds.
    pub fn resolve_as<V: ValueReceiver>(&self, id: u32) -> Option<Arc<V>> {
        resolve_as(self.resolve(id)?)
    }
}

/// Unwrap a resolved receiver back to the value receiver it was bound as.
fn resolve_as<V: ValueReceiver>(receiver: Arc<dyn crate::receiver::Receiver>) -> Option<Arc<V>> {
    let any: Arc<dyn std::any::Any + Send + Sync> = receiver;
    any.downcast::<IntoReceiver<V>>()
        .ok()
        .map(|bound| Arc::clone(bound.shared()))
}

#[cfg(test)]
mod tests {
    use kobako_codec::msgpack::codec::Value;

    use super::*;
    use crate::handles::Detached;
    use crate::receiver::{Fault, Probe, Receiver};
    use crate::yielder::Yielder;

    struct Kv;

    impl ValueReceiver for Kv {
        fn call(
            &self,
            _method: &str,
            _args: &[Value],
            _kwargs: &[(String, Value)],
            _block: Option<&mut Yielder<'_>>,
            _handles: &Handles<'_>,
        ) -> Result<Value, Fault> {
            Ok(Value::Nil)
        }
    }

    #[test]
    fn resolve_as_hands_back_the_type_that_was_bound() {
        let table = Detached::new();
        let handles = table.view();
        let id = handles.alloc(Arc::new(Kv.into_receiver())).unwrap();

        let kv: Option<Arc<Kv>> = handles.resolve_as(id);

        assert!(
            kv.is_some(),
            "an id bound through into_receiver must resolve back to the caller's own type, \
             the same shape the byte-level path's downcast produces"
        );
    }

    #[test]
    fn resolve_as_refuses_an_id_standing_for_another_type() {
        let table = Detached::new();
        let handles = table.view();
        let id = handles.alloc(Arc::new(Probe) as Arc<dyn Receiver>).unwrap();

        let kv: Option<Arc<Kv>> = handles.resolve_as(id);

        assert!(
            kv.is_none(),
            "an id standing for a different receiver must resolve to nothing rather than \
             a mistyped object"
        );
    }

    #[test]
    fn resolve_as_refuses_an_id_the_invocation_never_issued() {
        let table = Detached::new();
        let handles = table.view();
        let id = handles.alloc(Arc::new(Kv.into_receiver())).unwrap();

        let kv: Option<Arc<Kv>> = handles.resolve_as(id + 1);

        assert!(
            kv.is_none(),
            "an unissued id must resolve to nothing, so a corrupted payload reaches no \
             host object"
        );
    }
}
