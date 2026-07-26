//! kobako-runtime — engine-neutral host runtime contract.
//!
//! The surface where a wasm engine implementation and a host frontend
//! meet: the `Runtime` trait, the isolation `Profile` a runtime
//! declares, the neutral per-invocation value types, the dispatch /
//! yield re-entry traits a frontend supplies, and `envelope` — this
//! side's implementation of the core envelope, the byte-oracle peer of
//! the one in `kobako-codec`.
//! Nothing here depends on an engine or a frontend type — each engine
//! hides its own machinery behind `Runtime`, and each frontend maps
//! these shapes onto its own host-language surface at its boundary
//! (for the Ruby ext that is the error mapper in its runtime module),
//! so the engine stays swappable.

pub mod dispatch;
pub mod envelope;
pub mod error;
pub mod profile;
pub mod runtime;
pub mod snapshot;
pub mod yielder;
