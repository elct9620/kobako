//! The `MrbGuest` provided flows — per-invocation entry bodies over
//! mruby.
//!
//! Each flow implements one `kobako_core::Guest` entry (docs/wire-codec.md
//! § ABI Signatures) for the trait's provided methods: read the stdin
//! invocation frames, boot a fresh mruby VM with the built-in
//! `KobakoBridge` plus the shell-chosen gems, run the entry-specific
//! body, and write the Outcome envelope through `kobako_core::abi`.
//! The `#[no_mangle]` exports themselves are emitted by
//! `kobako_core::export_guest!` in the leaf shell crate.
//!
//! ## Module layout
//!
//! One sibling file per flow alongside the helpers it owns:
//!
//! * `eval` — `__kobako_eval` body.
//! * `run` — `__kobako_run` body + invocation-envelope parser.
//! * `yield_block` — `__kobako_yield_to_block` body (host-initiated
//!   re-entry into a guest block, docs/behavior.md B-24).
//! * `boot` — shared mruby boot / preamble install / snippet replay
//!   / pending-exception extraction helpers used by both entry points.
//! * `snippets` — Frame 3 snippet decoding (mruby source / RITE
//!   bytecode kinds); the channel reader and the Frame 1 preamble
//!   parser live in `kobako_core::frames`.
//! * `mrb_slot` / `block_stack` — per-invocation statics carrying the
//!   live VM and the guest-supplied block stack across the dispatch
//!   re-entry boundary.

pub(crate) mod block_stack;
#[cfg(any(mruby_linked, test))]
mod boot;
#[cfg(mruby_linked)]
mod eval;
#[cfg(mruby_linked)]
mod mrb_slot;
mod run;
#[cfg(any(mruby_linked, test))]
mod snippets;
mod yield_block;

#[cfg(mruby_linked)]
pub(crate) use eval::eval;
#[cfg(mruby_linked)]
pub(crate) use run::run;
pub(crate) use yield_block::yield_to_block;
