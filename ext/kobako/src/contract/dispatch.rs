//! Engine-neutral guest→host dispatch contract, free of any `magnus`
//! dependency.
//!
//! The wasm runtime hands a handler the raw Request bytes a guest produced
//! and expects raw Response bytes back. What the handler *is* — a Ruby Proc,
//! a Rust closure — is the frontend's concern; the runtime only sees this
//! trait. The concrete Ruby-Proc bridge lives in `crate::runtime::dispatch`.

/// Host-side handler for a guest→host dispatch.
///
/// `dispatch` returns the raw Response bytes, or `None` when the handler
/// itself failed — the runtime then walks its 0-return wire-fault path. The
/// bound handler is contracted to fold application failures into a
/// `Response.err` envelope, so `None` signals a contract violation (the
/// handler raised) rather than a normal dispatch outcome.
pub(crate) trait DispatchHandler: Send + Sync {
    fn dispatch(&self, request: &[u8]) -> Option<Vec<u8>>;
}
