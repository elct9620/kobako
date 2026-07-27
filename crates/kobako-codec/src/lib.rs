//! kobako-codec — the payload codecs.
//!
//! What rides inside a core envelope's opaque `payload` field, one
//! namespace per schema: `msgpack` holds the byte codec and the
//! invocation arguments it carries. Naming a namespace after its schema
//! is what makes a second one an addition rather than a rename.
//!
//! A payload codec is the replaceable half of the kobako wire — the fixed
//! half is `kobako-transport`, which this crate does not touch. Two
//! endpoints that agree on another schema carry none of this crate at
//! all. Nothing here is guest-bound: no ABI import, no mruby, no engine.

#[cfg(feature = "msgpack")]
pub mod msgpack;
