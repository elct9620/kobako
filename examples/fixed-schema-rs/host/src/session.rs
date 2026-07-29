//! The object a capability Handle stands for.
//!
//! A Session is a namespaced view of the same store `ProtoKv` holds. The
//! guest never holds it — it holds the id the invocation's table issued,
//! and the table hands the object back when a Call names that id. So an
//! id the host never issued reaches nothing, and every id it did reaches
//! only what this invocation put there: the table is built fresh for each
//! run and dropped with it.

use std::any::Any;
use std::sync::Arc;

use kobako::{Fault, FaultKind, Handles, Receiver, Yielder};
use prost::Message;

use crate::schema::{GetRequest, GetResponse, PutRequest, PutResponse};
use crate::store::Store;

/// A store view scoped to one prefix.
pub struct Session {
    store: Arc<Store>,
    prefix: Vec<u8>,
}

impl Session {
    pub fn new(store: Arc<Store>, prefix: Vec<u8>) -> Self {
        Session { store, prefix }
    }

    /// How many keys this session has written.
    pub fn count(&self) -> usize {
        self.store.count_under(&self.prefix)
    }

    fn scoped(&self, key: &[u8]) -> Vec<u8> {
        [self.prefix.as_slice(), key].concat()
    }
}

impl Receiver for Session {
    fn call(
        &self,
        method: &str,
        payload: &[u8],
        _block: Option<&mut Yielder<'_>>,
        _handles: &Handles<'_>,
    ) -> Result<Vec<u8>, Fault> {
        match method {
            "get" => {
                let request = GetRequest::decode(payload).map_err(malformed)?;
                let found = self.store.get(&self.scoped(&request.key));
                Ok(GetResponse {
                    found: found.is_some(),
                    value: found.unwrap_or_default(),
                }
                .encode_to_vec())
            }
            "put" => {
                let request = PutRequest::decode(payload).map_err(malformed)?;
                let replaced = self.store.put(self.scoped(&request.key), request.value);
                Ok(PutResponse { replaced }.encode_to_vec())
            }
            _ => Err(Fault::new(
                FaultKind::Undefined,
                format!("a KV session has no method {method}"),
            )),
        }
    }
}

/// Recover the Session an id stands for, refusing an id this invocation
/// never issued and an id that stands for something else.
///
/// The upcast-then-downcast is how a host reaches its own type back
/// through the table, which stores receivers behind a trait object.
pub fn resolve(handles: &Handles<'_>, id: u32) -> Result<Arc<Session>, Fault> {
    let receiver = handles
        .resolve(id)
        .ok_or_else(|| Fault::new(FaultKind::Undefined, format!("no live Handle for id {id}")))?;
    let any: Arc<dyn Any + Send + Sync> = receiver;
    any.downcast::<Session>()
        .map_err(|_| Fault::new(FaultKind::Argument, format!("Handle {id} is not a session")))
}

/// Bytes the schema cannot read, reported as the caller's error.
fn malformed(err: impl std::fmt::Display) -> Fault {
    Fault::new(
        FaultKind::Runtime,
        format!("a KV session received a malformed request: {err}"),
    )
}
