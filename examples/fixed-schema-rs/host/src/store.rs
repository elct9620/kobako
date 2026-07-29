//! The host-side KV Service.
//!
//! `ProtoKv` implements `Receiver` directly, so it is handed the payload
//! bytes and the method name and picks its own message from the pair. A
//! Service written against the bundled codec would take the decoded
//! shape instead; this one decodes what it agreed to decode, and nothing
//! in the SDK looks inside the payload on its behalf.
//!
//! The store is keyed by bytes rather than `String` for the reason the
//! schema is: a guest String need not be UTF-8, and a store keyed by
//! `String` would have to rewrite one before filing it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use kobako::{Fault, FaultKind, Handles, Receiver, Yielder};
use prost::Message;

use crate::schema::{
    CountRequest, CountResponse, EachResponse, GetRequest, GetResponse, OpenRequest, OpenResponse,
    PutRequest, PutResponse, YieldKey,
};
use crate::session::{self, Session};

/// The key/value state the Service edits.
#[derive(Default)]
pub struct Store(Mutex<HashMap<Vec<u8>, Vec<u8>>>);

impl Store {
    pub fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.0.lock().expect("store lock").get(key).cloned()
    }

    /// Store `value`, answering whether the key already held one.
    pub fn put(&self, key: Vec<u8>, value: Vec<u8>) -> bool {
        self.0
            .lock()
            .expect("store lock")
            .insert(key, value)
            .is_some()
    }

    /// How many keys the store holds — what the host prints to show the
    /// guest's writes landed on this side.
    pub fn count(&self) -> usize {
        self.0.lock().expect("store lock").len()
    }

    /// Every key, in insertion-independent order — what `each_key`
    /// yields.
    pub fn keys(&self) -> Vec<Vec<u8>> {
        let mut keys: Vec<Vec<u8>> = self.0.lock().expect("store lock").keys().cloned().collect();
        keys.sort_unstable();
        keys
    }

    /// How many keys sit under `prefix` — one session's share.
    pub fn count_under(&self, prefix: &[u8]) -> usize {
        self.0
            .lock()
            .expect("store lock")
            .keys()
            .filter(|key| key.starts_with(prefix))
            .count()
    }
}

/// `MyService::KV` — the Service the guest gem's methods dispatch to.
pub struct ProtoKv(pub Arc<Store>);

impl Receiver for ProtoKv {
    fn call(
        &self,
        method: &str,
        payload: &[u8],
        block: Option<&mut Yielder<'_>>,
        handles: &Handles<'_>,
    ) -> Result<Vec<u8>, Fault> {
        match method {
            "get" => {
                let request = GetRequest::decode(payload).map_err(malformed)?;
                let found = self.0.get(&request.key);
                Ok(GetResponse {
                    found: found.is_some(),
                    value: found.unwrap_or_default(),
                }
                .encode_to_vec())
            }
            "put" => {
                let request = PutRequest::decode(payload).map_err(malformed)?;
                let replaced = self.0.put(request.key, request.value);
                Ok(PutResponse { replaced }.encode_to_vec())
            }
            // A Handle leaves the host as the id its table issued, and
            // the schema carries it as the integer it is. Nothing about
            // the object crosses — the guest gets a name for it, and the
            // table is what turns that name back into the object.
            "open" => {
                let request = OpenRequest::decode(payload).map_err(malformed)?;
                let session = Session::new(self.0.clone(), request.prefix);
                let handle = handles.alloc(Arc::new(session))?;
                Ok(OpenResponse { handle }.encode_to_vec())
            }
            // The same Handle arriving the other way: in an argument
            // rather than as the Call's target. One table answers both.
            "count" => {
                let request = CountRequest::decode(payload).map_err(malformed)?;
                let session = session::resolve(handles, request.handle)?;
                Ok(CountResponse {
                    keys: session.count() as u32,
                }
                .encode_to_vec())
            }
            // A yield is a Call the other way: the host writes the
            // block's arguments in the same schema the guest reads its
            // own with, and the answer comes back as bytes the guest's
            // codec wrote.
            "each_key" => {
                let block = block.ok_or_else(|| {
                    Fault::new(FaultKind::Argument, "MyService::KV.each_key needs a block")
                })?;
                let keys = self.0.keys();
                for key in &keys {
                    let args = YieldKey { key: key.clone() }.encode_to_vec();
                    block
                        .call_payload(&args)
                        .map_err(|err| Fault::new(FaultKind::Runtime, format!("yield: {err}")))?;
                }
                Ok(EachResponse {
                    keys: keys.len() as u32,
                }
                .encode_to_vec())
            }
            // A method this schema does not describe is refused, not
            // guessed at. `undefined` is the same refusal an unbound
            // path gets, so probing the surface teaches a guest nothing.
            _ => Err(Fault::new(
                FaultKind::Undefined,
                format!("MyService::KV has no method {method}"),
            )),
        }
    }
}

/// Bytes the KV schema cannot read. A malformed payload is the caller's
/// error to see, so it comes back as a fault rather than a panic.
fn malformed(err: impl std::fmt::Display) -> Fault {
    Fault::new(
        FaultKind::Runtime,
        format!("MyService::KV received a malformed request: {err}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kobako::handles::Detached;

    /// A Receiver is the host's own code, and the guest is not what it is
    /// being tested against — the schema is. A detached Handle table is
    /// what lets the method be called directly, so a schema change shows
    /// up here rather than three layers away in an invocation.
    #[test]
    fn get_answers_a_miss_and_a_hit_under_the_same_schema() {
        let kv = ProtoKv(Arc::new(Store::default()));
        let table = Detached::new();
        let ask = |kv: &ProtoKv, key: &[u8]| {
            let request = GetRequest { key: key.to_vec() }.encode_to_vec();
            let answer = kv
                .call("get", &request, None, &table.as_handles())
                .expect("get always answers");
            GetResponse::decode(&answer[..]).expect("its own schema is readable")
        };

        assert!(
            !ask(&kv, b"absent").found,
            "a key the store never held must answer found=false rather than an empty value, \
             which is the distinction a bare value field could not make"
        );

        kv.call(
            "put",
            &PutRequest {
                key: b"k".to_vec(),
                value: b"v".to_vec(),
            }
            .encode_to_vec(),
            None,
            &table.as_handles(),
        )
        .expect("put always answers");

        let hit = ask(&kv, b"k");
        assert!(
            hit.found && hit.value == b"v",
            "a stored key must answer found=true carrying the bytes it was given"
        );
    }
}
