//! The host's half of the KV schema.
//!
//! Deliberately a second definition of what the guest gem declares. The
//! payload layer has two implementations on purpose — one per endpoint —
//! and they are held together by the bytes, so a tag renumbered on one
//! side and not the other is a wire break, not a type error.
//!
//! Keys and values are `bytes` for the reason the guest's copy gives: a
//! Ruby String is a byte string, and a `string` field would replace any
//! byte that is not UTF-8 and change the key it stands for.

/// `KV.get(key)` — the key to look up.
#[derive(Clone, PartialEq, prost::Message)]
pub struct GetRequest {
    #[prost(bytes = "vec", tag = "1")]
    pub key: Vec<u8>,
}

/// `KV.get` answer. `found` separates a stored empty value from a miss.
#[derive(Clone, PartialEq, prost::Message)]
pub struct GetResponse {
    #[prost(bytes = "vec", tag = "1")]
    pub value: Vec<u8>,
    #[prost(bool, tag = "2")]
    pub found: bool,
}

/// `KV.put(key, value)` — the pair to store.
#[derive(Clone, PartialEq, prost::Message)]
pub struct PutRequest {
    #[prost(bytes = "vec", tag = "1")]
    pub key: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    pub value: Vec<u8>,
}

/// `KV.put` answer. `replaced` reports whether the key already held a
/// value.
#[derive(Clone, PartialEq, prost::Message)]
pub struct PutResponse {
    #[prost(bool, tag = "1")]
    pub replaced: bool,
}

/// `KV.open(prefix)` — the namespace the session works under.
#[derive(Clone, PartialEq, prost::Message)]
pub struct OpenRequest {
    #[prost(bytes = "vec", tag = "1")]
    pub prefix: Vec<u8>,
}

/// `KV.open` answer: the Handle id this invocation's table issued.
///
/// A Handle is a plain integer here because that is all one is on the
/// wire — the table owns the object, and the id is the whole of what
/// crosses. A schema needs no representation of its own for it.
#[derive(Clone, PartialEq, prost::Message)]
pub struct OpenResponse {
    #[prost(uint32, tag = "1")]
    pub handle: u32,
}

/// `KV.count(session)` — a Handle in an argument position rather than in
/// the envelope's target.
#[derive(Clone, PartialEq, prost::Message)]
pub struct CountRequest {
    #[prost(uint32, tag = "1")]
    pub handle: u32,
}

/// `KV.count` answer — how many keys the session has written.
#[derive(Clone, PartialEq, prost::Message)]
pub struct CountResponse {
    #[prost(uint32, tag = "1")]
    pub keys: u32,
}

/// One yield of `KV.each_key` — the key the guest block receives.
#[derive(Clone, PartialEq, prost::Message)]
pub struct YieldKey {
    #[prost(bytes = "vec", tag = "1")]
    pub key: Vec<u8>,
}

/// `KV.each_key` answer — how many keys were yielded.
#[derive(Clone, PartialEq, prost::Message)]
pub struct EachResponse {
    #[prost(uint32, tag = "1")]
    pub keys: u32,
}

/// What an invocation entered through `#run` carries — the request body
/// and the capability this host issued for this one run.
///
/// The Handle is a payload position rather than the envelope's target,
/// which is what lets an entrypoint be *handed* its capabilities instead
/// of reaching for an ambient constant. The id has to come from the
/// invocation's own table, which is why the byte-level `run` takes a
/// closure over `Handles` rather than finished bytes.
#[derive(Clone, PartialEq, prost::Message)]
pub struct EntryRequest {
    #[prost(bytes = "vec", tag = "1")]
    pub body: Vec<u8>,
    #[prost(uint32, tag = "2")]
    pub env: u32,
}
