//! Engine-neutral guest→host dispatch contract, free of any `magnus`
//! dependency.
//!
//! The runtime decodes the core envelope and hands a handler the routed
//! `Call` — target, method, block flag, and the payload bytes it never
//! reads — then expects a `Reply` back. What the handler *is* — a Ruby
//! Proc, a Rust closure — is the frontend's concern; the runtime only sees
//! this trait. The Ruby ext conforms by bridging its dispatch Proc behind
//! it.

use crate::envelope::{Call, Reply};
use crate::yielder::Yielder;

/// Host-side handler for a guest→host dispatch.
///
/// `dispatch` receives the decoded Call plus a `Yielder` for re-entering
/// the in-flight guest when a Service method yields to a block, and returns
/// the Reply — or `None` when the handler itself failed, in which case the
/// runtime walks its 0-return wire-fault path. The bound handler is
/// contracted to fold application failures into the Reply's fault arm, so
/// `None` signals a contract violation (the handler raised) rather than a
/// normal dispatch outcome.
///
/// The `Call` borrows the buffer the runtime decoded, so a handler passes
/// the payload onward without copying it first.
pub trait DispatchHandler: Send + Sync {
    fn dispatch(&self, call: Call<'_>, yielder: &mut dyn Yielder) -> Option<Reply>;
}
