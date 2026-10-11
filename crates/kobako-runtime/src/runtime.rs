//! Engine-neutral host runtime contract.
//!
//! The contract a wasm engine must satisfy to drive a kobako guest: take a
//! per-invocation entry plus its stdin frames, run one invocation on a
//! fresh instance, and return the observable `Snapshot` — `Ok` iff the
//! guest export ran, `Err` when the invocation never started. Nothing here
//! mentions `magnus` or a Ruby type — a frontend supplies the dispatch
//! handler, the contract only borrows it.

use std::sync::Arc;

use kobako_transport::envelope::Frames;

use crate::dispatch::DispatchHandler;
use crate::error::InvokeError;
use crate::profile::Profile;
use crate::snapshot::Snapshot;

/// The per-invocation entry: a one-shot mruby source (`Eval`) or an
/// entrypoint and its payload (`Run`). Both ride alongside the Sandbox's
/// `Frames`; the runtime frames `Run` and the `Frames` onto the wire, so
/// a frontend never encodes either. The payload is owned so that framing it needs no
/// second copy.
///
/// Closed rather than `#[non_exhaustive]`: each arm is an invocation the
/// downcall carries under its own tag, and a runtime that has not
/// learned a new one should fail to compile rather than fall into a
/// wildcard.
pub enum Entry<'a> {
    Eval {
        source: &'a [u8],
    },
    Run {
        entrypoint: &'a str,
        payload: Vec<u8>,
    },
}

/// Engine-neutral runtime: drives one guest invocation on a fresh instance
/// and returns its observable `Snapshot`. `Ok` means the guest export ran
/// — the `Snapshot` carries the completion (outcome or trap), captures,
/// and usage uniformly; `Err` means the invocation never started.
///
/// Safety contract for `handler`: the runtime only *borrows* the handler
/// for the duration of `invoke` and never roots it. A frontend whose
/// handler references a GC-managed object (e.g. a Ruby `Proc`) must keep
/// that object alive — and, under a moving GC, pinned — for the whole call.
/// The Ruby ext does this by holding the `Proc` on its long-lived Runtime
/// wrapper and GC-marking it; the runtime itself touches no frontend
/// value.
pub trait Runtime {
    fn invoke(
        &self,
        entry: Entry<'_>,
        frames: &Frames,
        handler: Option<Arc<dyn DispatchHandler>>,
    ) -> Result<Snapshot, InvokeError>;

    /// The isolation profile this runtime provides. Deliberately
    /// without a default: every engine states its posture explicitly,
    /// and a frontend refuses construction when the declaration falls
    /// below the floor its host application requested.
    fn profile(&self) -> Profile;
}
