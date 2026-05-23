//! kobako-wasm — Guest Binary crate root.
//!
//! This crate is the source of `kobako.wasm`, the Guest Binary artifact
//! described in SPEC.md "Core Abstractions". It hosts:
//!
//! * `codec` — MessagePack codec, a thin glue layer over the `rmp`
//!   crate that adds kobako's two ext types (docs/wire-codec.md).
//! * `rpc` — Per-call transport layer mirroring the host's
//!   `lib/kobako/transport/`. Holds `rpc::envelope` (Request / Response
//!   value objects and their encoders/decoders on top of `codec` —
//!   docs/wire-contract.md) and `rpc::client` (the round-trip pipeline
//!   used by the guest-side mruby bridge to dispatch a call through
//!   `__kobako_dispatch`). The crate-internal module name keeps the
//!   shorter `rpc` form for now; renaming the Rust submodule is a
//!   later cleanup.
//! * `outcome` — Per-run Outcome envelope mirroring the host's
//!   `lib/kobako/outcome.rb`. Holds the Panic / Outcome value objects
//!   and the `encode_outcome` / `decode_outcome` / `encode_panic` /
//!   `decode_panic` / `encode_result` / `decode_result` helpers
//!   (docs/wire-contract.md § Outcome Envelope). Shares
//!   [`rpc::envelope::EnvelopeError`] for codec-shape faults.
//! * `abi` — Guest ABI surface: the `__kobako_dispatch` host import and
//!   the `__kobako_eval` / `__kobako_run` / `__kobako_alloc` /
//!   `__kobako_take_outcome` guest exports (docs/wire-codec.md
//!   § ABI Signatures).
//! * `kobako` — domain runtime: owns the `Kobako` value-token that
//!   installs the `Kobako` module / `Kobako::Transport` / `Kobako::Handle` /
//!   exception classes on an mruby VM and registers the C-bridges in
//!   its `bridges` submodule. No Ruby boot text.
//! * `mruby` — thin façade re-exporting the mruby C-API binding from
//!   the sibling `kobako-mruby-sys` crate. Both the raw FFI surface
//!   (`mruby::sys`) and every safe wrapper (`Mrb`, `Ccontext`, the
//!   typed `Value` / `Class` newtypes, `cstr_ptr`) now live in that
//!   crate; this module forwards the existing `use crate::mruby::*`
//!   call-site shape until the consumer code adopts the longer
//!   `crate::mruby::sys::*` paths everywhere, at which point the
//!   façade collapses.
//!
//! The crate uses `std` on every target. `wasm32-wasip1` (the production
//! target — see SPEC.md "Implementation Standards" Architecture) ships a
//! working `std`, including allocator and panic handler. A `no_std` codec
//! is not required by SPEC; switching adds friction (custom allocator,
//! custom panic handler) without buying anything for the Guest Binary,
//! which already pays for `std` through the embedded mruby interpreter.

/// Width in bytes of the length prefix that precedes each stdin frame
/// and outcome buffer (docs/wire-codec.md § Invocation channels).
pub const FRAME_LEN_SIZE: usize = 4;

pub mod abi;
pub mod codec;
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod kobako;
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod mruby;
pub mod outcome;
pub mod rpc;
pub mod yield_response;

// Re-export the `cstr!` macro at the crate root so the consumer-side
// `use crate::cstr;` pattern continues to resolve after the macro
// migrated to `kobako-mruby-sys` (`#[macro_export]` exports a macro
// from its defining crate's root, so re-anchoring it here is the
// minimum-diff bridge).
#[cfg(any(target_arch = "wasm32", test))]
pub use kobako_mruby_sys::cstr;
