//! Guest ABI surface — host import + guest exports.
//!
//! This module is the façade for the wasm import/export contract pinned
//! by docs/wire-codec.md § ABI Signatures. The contract is:
//!
//! * **Exactly 1 host import**: `__kobako_dispatch` — the transport bridge the
//!   guest uses to dispatch a Service call to the host. Lives in the
//!   `env` wasm namespace (`(import "env" "__kobako_dispatch" ...)`).
//! * **Exactly 5 guest exports**:
//!   - `__kobako_eval()`                          — reactor entry; runs one-shot user source
//!   - `__kobako_run(env_ptr, env_len)`           — reactor entry; entrypoint dispatch
//!   - `__kobako_alloc(size)`                     — bump/malloc allocator for buffers
//!   - `__kobako_take_outcome()`                  — returns packed (ptr, len) of OUTCOME_BUFFER
//!   - `__kobako_yield_to_block(req_ptr, req_len)` — host-initiated re-entry into a guest block (B-24)
//!
//! The import / export name set is enforced at link time: a guest
//! import the host does not provide traps inside wasmtime, and a
//! missing export fails the `link_func_wrap` lookup on the host side
//! or the `Caller::get_export` lookup inside dispatch. E2E journeys
//! (`test/test_e2e_journeys.rb` + `test/test_sandbox_run.rb`) drive a
//! full host↔guest round-trip against the real `data/kobako.wasm`, so
//! any name drift surfaces before any other test runs.
//!
//! ## Module layout
//!
//! The `#[no_mangle]` exports themselves are emitted by
//! `kobako_core::export_guest!` in `crate::guest`; the ABI primitives
//! (`__kobako_dispatch` import declaration, packed-u64 helpers,
//! outcome buffer) live in `kobako_core::abi`. This façade groups the
//! per-entry bodies, one sibling file each alongside the helpers it
//! owns:
//!
//! * `eval` — `__kobako_eval` body.
//! * `run` — `__kobako_run` body + invocation-envelope parser.
//! * `boot` — shared mruby boot / preamble install / snippet replay
//!   / pending-exception extraction helpers used by both entry points.
//! * `snippets` — Frame 3 snippet decoding (mruby source / RITE
//!   bytecode kinds); the channel reader and the Frame 1 preamble
//!   parser live in `kobako_core::frames`.

#[cfg(target_arch = "wasm32")]
pub(crate) mod block_stack;
#[cfg(any(target_arch = "wasm32", test))]
mod boot;
mod eval;
#[cfg(target_arch = "wasm32")]
mod mrb_slot;
mod run;
#[cfg(any(target_arch = "wasm32", test))]
mod snippets;
mod yield_block;

pub(crate) use eval::eval;
pub(crate) use run::run;
pub(crate) use yield_block::yield_to_block;
