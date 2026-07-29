//! The MessagePack payload codec — kobako's default, and the only one
//! this crate ships.
//!
//! A payload codec is replaceable: the core envelope carries payload
//! bytes without reading them, so a host and guest that agree on another
//! schema build this crate with `--no-default-features` and reach for
//! their own. Naming the namespace after the schema is what makes a
//! second one an addition rather than a rename.

pub mod codec;
pub mod payload;

// The schema's own names, one level up: `msgpack::codec::Value` repeats
// this crate's name in the middle of the path, and a caller naming the
// schema has already said which codec they mean. The modules stay, so a
// second schema is still an addition rather than a reshuffle.
pub use codec::{Decode, Decoder, Encode, Encoder, Error, Value};
pub use payload::Arguments;
