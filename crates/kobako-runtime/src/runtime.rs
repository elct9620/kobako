//! Engine-neutral host runtime contract.
//!
//! The contract a wasm engine must satisfy to drive a kobako guest: take a
//! per-invocation entry plus its stdin frames, run one invocation on a
//! fresh instance, and return the observable `Snapshot` — `Ok` iff the
//! guest export ran, `Err` when the invocation never started. Nothing here
//! mentions `magnus` or a Ruby type — a frontend supplies the dispatch
//! handler, the contract only borrows it.

use std::sync::Arc;

use crate::dispatch::DispatchHandler;
use crate::error::Error;
use crate::snapshot::Snapshot;

/// The per-invocation entry: a one-shot mruby source (`Eval`) or an
/// entrypoint-dispatch envelope (`Run`). Both ride alongside the stdin
/// `Frames`; `Run` additionally copies its envelope into guest memory.
pub enum Entry<'a> {
    Eval { source: &'a [u8] },
    Run { envelope: &'a [u8] },
}

/// The stdin frames shared by both entries: the Frame 1 preamble (the
/// Sandbox's registrations) and the Frame 3 snippet-replay payload.
pub struct Frames<'a> {
    pub preamble: &'a [u8],
    pub snippets: &'a [u8],
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
        frames: Frames<'_>,
        handler: Option<Arc<dyn DispatchHandler>>,
    ) -> Result<Snapshot, Error>;
}
