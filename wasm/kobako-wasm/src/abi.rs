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
//! The `__kobako_dispatch` import declaration and the packed-u64
//! helpers live in `kobako_core::abi`; this façade owns the WASI
//! reactor `_initialize` shim. Each guest export body lives in its
//! own sibling file alongside the helpers it owns:
//!
//! * `eval` — `__kobako_eval` body.
//! * `run` — `__kobako_run` body + invocation-envelope parser.
//! * `boot` — shared mruby boot / preamble install / snippet replay
//!   / pending-exception extraction helpers used by both entry points.
//! * `frames` — stdin frame reader and Frame 1 / Frame 3 decoders.
//! * `outcome_buffer` — `OUTCOME_BUFFER` plus `__kobako_alloc` /
//!   `__kobako_take_outcome` and the Panic / outcome write helpers.

#[cfg(target_arch = "wasm32")]
pub(crate) mod block_stack;
#[cfg(any(target_arch = "wasm32", test))]
mod boot;
mod eval;
#[cfg(any(target_arch = "wasm32", test))]
mod frames;
#[cfg(target_arch = "wasm32")]
mod mrb_slot;
mod outcome_buffer;
mod run;
mod yield_block;

pub use eval::__kobako_eval;
pub use outcome_buffer::{__kobako_alloc, __kobako_take_outcome};
pub use run::__kobako_run;
pub use yield_block::__kobako_yield_to_block;

// ---------------------------------------------------------------------------
// WASI Reactor `_initialize` entry-point.
// ---------------------------------------------------------------------------

/// WASI Reactor `_initialize` entry-point.
///
/// When compiling as a WASI reactor (`cdylib` targeting
/// `wasm32-wasip1`), the rust-lld linker looks for an `_initialize`
/// export to satisfy the reactor CRT model. Without it the link step
/// fails with:
///
///   rust-lld: error: entry symbol not defined: _initialize
///
/// We export a no-op here because wasi-libc reactor init
/// (`crt1-reactor.o` static ctors) is not required for kobako's boot
/// path — kobako creates and destroys an `mrb_state` inside
/// `__kobako_eval` / `__kobako_run` for every invocation; there are no
/// static C++ constructors or WASI preopen operations that need to run
/// before the first call. Approach (a) from the two known fixes —
/// smaller and sufficient for the kobako use case.
///
/// Per docs/wire-codec.md § ABI Signatures, the five kobako exports
/// counted by the host are `__kobako_eval`, `__kobako_run`,
/// `__kobako_alloc`, `__kobako_take_outcome`, and
/// `__kobako_yield_to_block`. `_initialize` is WASI reactor bookkeeping
/// and is explicitly excluded from the kobako export count.
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn _initialize() {
    // No-op: wasi-libc reactor static ctors are not needed for
    // kobako's reactor model. See item-level doc above.
}
