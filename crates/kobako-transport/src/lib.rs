//! kobako-transport — the fixed tier of the kobako wire.
//!
//! A host, a payload codec, and a guest are chosen independently only
//! because two things are the same in every assembly, and this crate is
//! both of them:
//!
//! - `envelope` — the outer frame every host↔guest message rides in,
//!   carrying routing and outcome attribution and handing the payload
//!   through as bytes it never reads
//! - `abi` — the version, the packed-return layout, the frame prefix, and
//!   the size cap the two sides must already agree on to exchange a byte
//!
//! Every value here is spelled once. Nothing depends on an engine, an
//! interpreter, or a schema — and nothing may come to: a tier every other
//! tier composes against cannot carry a choice any of them might want to
//! make differently.
//!
//! [core envelope]: ../../../docs/wire/envelope.md

pub mod abi;
pub mod envelope;
