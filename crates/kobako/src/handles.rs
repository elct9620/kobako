//! Per-invocation capability-Handle table and its Member-facing view.
//!
//! Guests never hold host objects — they hold opaque `ext 0x01` Handle
//! ids that resolve against this table. Ids are issued by a
//! monotonically increasing counter starting at 1 and the whole table
//! resets at every invocation entry, so no Handle survives an
//! invocation boundary. The entries are `Arc<dyn Member>` — the SDK's
//! dispatchable unit — so a Handle used as a dispatch target answers
//! methods the same way a bound Service does.

use std::sync::{Arc, Mutex};

use kobako_codec::codec::Value;

use crate::member::{Fault, FaultKind, Member};

/// Maximum legal Capability Handle ID — the wire pins ids to the
/// positive i32 range (docs/wire-codec.md § Ext Types → ext 0x01).
const HANDLE_ID_MAX: u32 = 0x7fff_ffff;

/// The Sandbox-owned table: live entries plus the per-invocation
/// monotonic counter. Ids are `1..=entries.len()` between resets, so
/// the entry vector doubles as the id map.
#[derive(Default)]
pub(crate) struct HandleTable {
    entries: Vec<Arc<dyn Member>>,
}

impl HandleTable {
    /// Bind `object` and return its fresh id, or refuse at the id cap
    /// with the message the Ruby allocator raises.
    pub(crate) fn alloc(&mut self, object: Arc<dyn Member>) -> Result<u32, String> {
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
    pub(crate) fn get(&self, id: u32) -> Option<Arc<dyn Member>> {
        let index = (id as usize).checked_sub(1)?;
        self.entries.get(index).cloned()
    }

    /// Clear all entries and restart the counter — the invocation
    /// boundary.
    pub(crate) fn reset(&mut self) {
        self.entries.clear();
    }
}

/// The Member-facing view of the invocation's Handle table, handed to
/// every dispatch alongside the call.
///
/// `alloc` is how a member hands the guest a stateful host object: the
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

    /// Bind a host object into the invocation's table and return the
    /// `Value::Handle` token that reaches the guest in its place.
    pub fn alloc(&self, object: Arc<dyn Member>) -> Result<Value, Fault> {
        self.table
            .lock()
            .expect("the Handle table mutex is never poisoned")
            .alloc(object)
            .map(Value::Handle)
            .map_err(|message| Fault::new(FaultKind::Runtime, message))
    }

    /// Resolve a `Value::Handle` argument to the live host object it
    /// stands for; `None` for a non-Handle value or an id with no live
    /// binding.
    pub fn resolve(&self, value: &Value) -> Option<Arc<dyn Member>> {
        let Value::Handle(id) = value else {
            return None;
        };
        self.table
            .lock()
            .expect("the Handle table mutex is never poisoned")
            .get(*id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Probe;

    impl Member for Probe {
        fn call(
            &self,
            _method: &str,
            _args: &[Value],
            _kwargs: &[(String, Value)],
            _block: Option<&mut crate::block::Block<'_>>,
            _handles: &Handles<'_>,
        ) -> Result<Value, Fault> {
            Ok(Value::Nil)
        }
    }

    #[test]
    fn ids_start_at_one_and_increase_monotonically() {
        let mut table = HandleTable::default();
        assert_eq!(table.alloc(Arc::new(Probe)), Ok(1));
        assert_eq!(table.alloc(Arc::new(Probe)), Ok(2));
    }

    #[test]
    fn reset_invalidates_every_issued_id_and_restarts_the_counter() {
        let mut table = HandleTable::default();
        table.alloc(Arc::new(Probe)).unwrap();
        table.reset();
        assert!(table.get(1).is_none(), "no Handle survives the boundary");
        assert_eq!(table.alloc(Arc::new(Probe)), Ok(1));
    }

    #[test]
    fn get_rejects_the_zero_sentinel_and_unissued_ids() {
        let mut table = HandleTable::default();
        table.alloc(Arc::new(Probe)).unwrap();
        assert!(table.get(0).is_none());
        assert!(table.get(2).is_none());
    }

    #[test]
    fn facade_round_trips_an_object_through_alloc_and_resolve() {
        let table = Mutex::new(HandleTable::default());
        let handles = Handles::new(&table);
        let object: Arc<dyn Member> = Arc::new(Probe);
        let token = handles.alloc(object.clone()).unwrap();
        let resolved = handles.resolve(&token).expect("the id is live");
        assert!(
            Arc::ptr_eq(&resolved, &object),
            "resolve must yield the very object alloc bound"
        );
        assert!(handles.resolve(&Value::Int(1)).is_none());
    }
}
