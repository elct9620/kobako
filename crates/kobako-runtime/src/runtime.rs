//! Engine-neutral host runtime contract.
//!
//! The contract a wasm engine must satisfy to drive a kobako guest: take a
//! per-invocation entry plus its Preamble, run one invocation on a
//! fresh instance, and return the observable `Snapshot` — `Ok` iff the
//! guest export ran, `Err` when the invocation never started. Nothing here
//! mentions `magnus` or a Ruby type — a frontend supplies the dispatch
//! handler, the contract only borrows it.

use std::sync::Arc;

use kobako_transport::envelope::Preamble;

use crate::dispatch::DispatchHandler;
use crate::error::InvokeError;
use crate::grant::{Grant, GrantKind};
use crate::profile::Profile;
use crate::snapshot::Snapshot;

/// The per-invocation entry: a one-shot mruby source (`Eval`) or an
/// entrypoint and its payload (`Run`). Both ride alongside the Sandbox's
/// `Preamble`; the runtime frames `Run` and the `Preamble` onto the wire,
/// so a frontend never encodes either. The payload is owned so that framing it needs no
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
    /// `grants` are this Invocation's, each of a kind `supports` declared;
    /// the runtime presents them to the guest and keeps none afterwards.
    fn invoke(
        &self,
        entry: Entry<'_>,
        preamble: &Preamble,
        handler: Option<Arc<dyn DispatchHandler>>,
        grants: &[Grant],
    ) -> Result<Snapshot, InvokeError>;

    /// Whether this runtime presents a Grant of `kind` to the guest. A
    /// frontend refuses to register one it does not, before any guest runs.
    /// The default supports none, so an engine that has not learned Grants
    /// still serves every invocation without one.
    fn supports(&self, _kind: GrantKind) -> bool {
        false
    }

    /// The isolation profile this runtime provides. Deliberately
    /// without a default: every engine states its posture explicitly,
    /// and a frontend refuses construction when the declaration falls
    /// below the floor its host application requested.
    fn profile(&self) -> Profile;

    /// The bytecode of the Preamble's source Snippets, one per snippet in
    /// order, compiled on a fresh instance that runs none of them. A
    /// frontend hands over the Preamble an invocation sends and replays the
    /// answer in its place, so every invocation after skips the compiler.
    /// `None` whenever there is nothing to replay, an answer that would
    /// carry that Preamble past the payload cap included: the frontend
    /// keeps the source. The default compiles nothing, so an engine that
    /// does not offer it still serves every invocation.
    fn compile(&self, _preamble: &Preamble) -> Option<Vec<Vec<u8>>> {
        None
    }
}
