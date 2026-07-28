//! Per-invocation capability-Handle table and its receiver-facing
//! view.
//!
//! Guests never hold host objects — they hold opaque `ext 0x01` Handle
//! ids that resolve against this table. Ids are issued by a
//! monotonically increasing counter starting at 1, and each invocation
//! gets a fresh table, so no Handle survives an invocation boundary.
//! The entries are `Arc<dyn Receiver>`, so a Handle used as a dispatch
//! target answers methods the same way a bound Service does.

use std::sync::{Arc, Mutex};

#[cfg(feature = "msgpack")]
use kobako_codec::msgpack::codec::Value;

use crate::receiver::{Fault, FaultKind, Receiver};

/// Maximum legal Capability Handle ID — the wire pins ids to the
/// positive i32 range (docs/wire/payload-msgpack.md § Ext Types → ext 0x01).
const HANDLE_ID_MAX: u32 = 0x7fff_ffff;

/// The Sandbox-owned table: live entries plus the per-invocation
/// monotonic counter. Ids are `1..=entries.len()`, so the entry vector
/// doubles as the id map.
#[derive(Default)]
pub(crate) struct HandleTable {
    entries: Vec<Arc<dyn Receiver>>,
}

impl HandleTable {
    /// Bind `object` and return its fresh id, or refuse at the id cap
    /// with the message the Ruby allocator raises.
    pub(crate) fn alloc(&mut self, object: Arc<dyn Receiver>) -> Result<u32, String> {
        if self.entries.len() as u32 >= HANDLE_ID_MAX {
            return Err(format!(
                "Out of handle allocations: too many host objects were \
                 referenced in a single invocation (limit {HANDLE_ID_MAX})"
            ));
        }
        self.entries.push(object);
        Ok(self.entries.len() as u32)
    }

    /// Resolve a live id to its bound object.
    pub(crate) fn get(&self, id: u32) -> Option<Arc<dyn Receiver>> {
        let index = (id as usize).checked_sub(1)?;
        self.entries.get(index).cloned()
    }
}

/// The receiver-facing view of the invocation's Handle table, handed
/// to every dispatch alongside the call.
///
/// `alloc` is how a receiver hands the guest a stateful host object: the
/// returned `Value::Handle` rides the wire as an opaque token, and the
/// guest routes later calls on it back to the object. `resolve` is the
/// inverse for arguments: a `Value::Handle` the guest passed resolves
/// to the live object it stands for.
pub struct Handles<'a> {
    table: &'a Mutex<HandleTable>,
}

impl<'a> Handles<'a> {
    pub(crate) fn new(table: &'a Mutex<HandleTable>) -> Self {
        Handles { table }
    }

    /// Bind a host object into the invocation's table and return the id
    /// that stands for it on the wire.
    ///
    /// An id rather than a `Value::Handle` because a Handle is something
    /// each schema spells for itself: the id is what the table owns and
    /// what the guest mints one from, so a Receiver encodes it the way
    /// its own payload does. `Value::Handle(id)` is that spelling in the
    /// bundled codec.
    pub fn alloc(&self, object: Arc<dyn Receiver>) -> Result<u32, Fault> {
        self.table
            .lock()
            .expect("the Handle table mutex is never poisoned")
            .alloc(object)
            .map_err(|message| Fault::new(FaultKind::Runtime, message))
    }

    /// Resolve a Handle id to the live host object it stands for; `None`
    /// for an id with no live binding. Upcast the `Arc` to
    /// `Arc<dyn Any + Send + Sync>` and `downcast` to recover the
    /// concrete receiver type.
    pub fn resolve(&self, id: u32) -> Option<Arc<dyn Receiver>> {
        self.table
            .lock()
            .expect("the Handle table mutex is never poisoned")
            .get(id)
    }

    /// The bundled codec's spelling: resolve a `Value::Handle`, and
    /// nothing else, through `resolve`.
    #[cfg(feature = "msgpack")]
    pub fn resolve_value(&self, value: &Value) -> Option<Arc<dyn Receiver>> {
        let Value::Handle(id) = value else {
            return None;
        };
        self.resolve(*id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receiver::{ValueAdapter, ValueReceiver};

    struct Probe;

    impl ValueReceiver for Probe {
        fn call(
            &self,
            _method: &str,
            _args: &[Value],
            _kwargs: &[(String, Value)],
            _block: Option<&mut crate::yielder::Yielder<'_>>,
            _handles: &Handles<'_>,
        ) -> Result<Value, Fault> {
            Ok(Value::Nil)
        }
    }

    #[test]
    fn ids_start_at_one_and_increase_monotonically() {
        let mut table = HandleTable::default();
        assert_eq!(table.alloc(Arc::new(ValueAdapter::new(Probe))), Ok(1));
        assert_eq!(table.alloc(Arc::new(ValueAdapter::new(Probe))), Ok(2));
    }

    #[test]
    fn get_rejects_the_zero_sentinel_and_unissued_ids() {
        let mut table = HandleTable::default();
        table.alloc(Arc::new(ValueAdapter::new(Probe))).unwrap();
        assert!(table.get(0).is_none());
        assert!(table.get(2).is_none());
    }

    #[test]
    fn facade_round_trips_an_object_through_alloc_and_resolve() {
        let table = Mutex::new(HandleTable::default());
        let handles = Handles::new(&table);
        let object: Arc<dyn Receiver> = Arc::new(ValueAdapter::new(Probe));
        let token = handles.alloc(object.clone()).unwrap();
        let resolved = handles.resolve(token).expect("the id is live");
        assert!(
            Arc::ptr_eq(&resolved, &object),
            "resolve must yield the very object alloc bound"
        );
        assert!(handles.resolve_value(&Value::Int(1)).is_none());
    }

    #[test]
    fn resolved_receiver_downcasts_to_its_concrete_type() {
        let table = Mutex::new(HandleTable::default());
        let handles = Handles::new(&table);
        let token = handles.alloc(Arc::new(ValueAdapter::new(Probe))).unwrap();
        let resolved = handles.resolve(token).expect("the id is live");
        let any: Arc<dyn std::any::Any + Send + Sync> = resolved;
        // A Value-based receiver enters the table wrapped, so the Any
        // upcast recovers the adapter; `receiver` reaches the caller's
        // own type from there.
        let adapter = any.downcast::<ValueAdapter<Probe>>().expect(
            "a resolved Handle must recover the concrete receiver type through the Any upcast",
        );
        assert!(
            adapter.receiver().respond_to_guest("label"),
            "the adapter must hand back the wrapped receiver, whose own narrowing predicate answers"
        );
    }
}
