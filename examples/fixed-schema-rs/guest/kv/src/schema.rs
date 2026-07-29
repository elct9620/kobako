//! The KV schema — one message per request and response position.
//!
//! Keys and values are `bytes`, not `string`. A Ruby String is a byte
//! string, and a `string` field would have to replace any byte that is
//! not UTF-8 — silently changing the key it stands for. Matching a
//! field's type to the language's own is an obligation a replacement
//! schema carries alone: the wire cannot check it, and the bundled codec
//! does not set the bar, since it renders an outbound String lossily and
//! so loses exactly the keys this schema keeps.
//!
//! Tag numbers are the schema. With no `.proto` as their source of
//! truth, renumbering one here without renumbering its twin in the host
//! breaks the wire silently — the two definitions agree by the bytes,
//! not by sharing a type.

/// `KV.get(key)` — the key to look up.
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct GetRequest {
    #[prost(bytes = "vec", tag = "1")]
    pub key: Vec<u8>,
}

/// `KV.get` answer. `found` is what distinguishes a stored empty value
/// from a miss, which a bare `value` cannot.
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct GetResponse {
    #[prost(bytes = "vec", tag = "1")]
    pub value: Vec<u8>,
    #[prost(bool, tag = "2")]
    pub found: bool,
}

/// `KV.put(key, value)` — the pair to store.
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct PutRequest {
    #[prost(bytes = "vec", tag = "1")]
    pub key: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    pub value: Vec<u8>,
}

/// `KV.put` answer. `replaced` reports whether the key already held a
/// value, so the round trip carries something the caller could not have
/// computed itself.
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct PutResponse {
    #[prost(bool, tag = "1")]
    pub replaced: bool,
}

/// `KV.open(prefix)` — the namespace the session works under.
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct OpenRequest {
    #[prost(bytes = "vec", tag = "1")]
    pub prefix: Vec<u8>,
}

/// `KV.open` answer: the Handle id the host's table issued.
///
/// A Handle is a plain integer in this schema because that is all one
/// is — the host's table owns the object, and the id is the whole of
/// what crosses. A schema needs no representation of its own for it.
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct OpenResponse {
    #[prost(uint32, tag = "1")]
    pub handle: u32,
}

/// `KV.count(session)` — a Handle in an argument position rather than in
/// the envelope's target, which is the other way a schema carries one.
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct CountRequest {
    #[prost(uint32, tag = "1")]
    pub handle: u32,
}

/// `KV.count` answer — how many keys the session has written.
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct CountResponse {
    #[prost(uint32, tag = "1")]
    pub keys: u32,
}

/// One yield of `KV.each_key` — the key the block receives.
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct YieldKey {
    #[prost(bytes = "vec", tag = "1")]
    pub key: Vec<u8>,
}

/// `KV.each_key` answer — how many keys were yielded.
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct EachResponse {
    #[prost(uint32, tag = "1")]
    pub keys: u32,
}

/// What an invocation entered through `#run` carries — the request body
/// and the capability the host issued for this one run.
///
/// A Handle in a payload position, which the wire calls optional: a
/// codec carries one only if it has a representation, and this schema's
/// is the same integer field `open` answers with. Without it an
/// entrypoint would have to reach its capabilities through a bound
/// constant, which is an ambient name rather than something handed to
/// this request.
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct EntryRequest {
    #[prost(bytes = "vec", tag = "1")]
    pub body: Vec<u8>,
    #[prost(uint32, tag = "2")]
    pub env: u32,
}
