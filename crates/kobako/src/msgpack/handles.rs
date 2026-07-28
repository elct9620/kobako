//! The Handle position: an argument's value-tree spelling of an id.

use std::sync::Arc;

use kobako_codec::msgpack::codec::Value;

use crate::handles::Handles;
use crate::receiver::Receiver;

impl Handles<'_> {
    /// The bundled codec's spelling: resolve a `Value::Handle`, and
    /// nothing else, through `resolve`.
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
    use crate::handles::Detached;
    use crate::receiver::Probe;

    #[test]
    fn resolve_value_reaches_the_object_a_handle_value_stands_for() {
        let table = Detached::new();
        let handles = table.view();
        let object: Arc<dyn Receiver> = Arc::new(Probe);
        let id = handles.alloc(object.clone()).unwrap();

        let resolved = handles
            .resolve_value(&Value::Handle(id))
            .expect("the id is live");

        assert!(
            Arc::ptr_eq(&resolved, &object),
            "a Value::Handle through resolve_value must reach the same object its id does"
        );
    }

    #[test]
    fn resolve_value_refuses_a_value_that_is_not_a_handle() {
        let table = Detached::new();

        assert!(
            table.view().resolve_value(&Value::Int(1)).is_none(),
            "a non-Handle value through resolve_value must reach nothing, since only \
             the Handle spelling names a host object"
        );
    }
}
