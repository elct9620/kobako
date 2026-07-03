//! Shared frontend plumbing for the example bins — the pieces every
//! kobako frontend implements the same way, kept out of the bins so
//! each `main` reads as pure host assembly.

pub mod report;

use kobako_codec::codec::{Encoder, Value};

/// The empty msgpack array a mandatory-presence stdin frame carries
/// when a host registers nothing.
pub fn empty_frame() -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.write_value(&Value::Array(Vec::new()))
        .expect("an empty msgpack array always encodes");
    enc.into_bytes()
}
