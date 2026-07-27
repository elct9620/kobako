//! kobako-transport — the fixed tier of the kobako wire.
//!
//! A host, a payload codec, and a guest are chosen independently only
//! because one layer is the same in every assembly. That layer is the
//! core envelope, and it is all this crate holds: the outer frame every
//! host↔guest message rides in, carrying routing and outcome attribution
//! and handing the payload through as bytes it never reads.
//!
//! Nothing here depends on an engine, an interpreter, or a schema — and
//! nothing may come to. A tier every other tier composes against cannot
//! carry a choice any of them might want to make differently.
//!
//! [core envelope]: ../../../docs/wire/envelope.md

pub mod envelope;
