//! kobako-wasm — Guest Binary crate root.
//!
//! This crate is the source of `kobako.wasm`, the Guest Binary artifact
//! described in SPEC.md "Core Abstractions". It hosts:
//!
//! * `codec` — MessagePack wire codec, a thin glue layer over the `rmp`
//!   crate that adds kobako's two ext types (docs/wire-codec.md).
//! * `rpc` — Per-call RPC layer mirroring the host's `lib/kobako/rpc/`.
//!   Holds `rpc::envelope` (Request / Response value objects and their
//!   encoders/decoders on top of `codec` — docs/wire-contract.md) and
//!   `rpc::client` (the round-trip pipeline used by the guest-side
//!   mruby bridge to dispatch a call through `__kobako_dispatch`).
//! * `outcome` — Per-run Outcome envelope mirroring the host's
//!   `lib/kobako/outcome.rb`. Holds the Panic / Outcome value objects
//!   and the `encode_outcome` / `decode_outcome` / `encode_panic` /
//!   `decode_panic` / `encode_result` / `decode_result` helpers
//!   (docs/wire-contract.md § Outcome Envelope). Shares
//!   [`rpc::envelope::EnvelopeError`] for codec-shape faults.
//! * `abi` — Wire ABI surface: the `__kobako_dispatch` host import and
//!   the `__kobako_eval` / `__kobako_run` / `__kobako_alloc` /
//!   `__kobako_take_outcome` guest exports (docs/wire-codec.md
//!   § ABI Signatures).
//! * `kobako` — domain runtime: owns the `Kobako` value-token that
//!   installs the `Kobako` module / `Kobako::RPC` / `Kobako::RPC::Handle` /
//!   exception classes on an mruby VM and registers the C-bridges in
//!   its `bridges` submodule. No Ruby boot text.
//! * `mruby` — façade for the mruby C API binding. Submodule `mruby::sys`
//!   holds the hand-rolled FFI declarations; `mruby::value` adds the small
//!   ergonomic layer (inherent methods on `mrb_value` + the `cstr!` macro);
//!   `mruby::state` exposes the `Mrb` RAII wrapper around `mrb_state *`.
//!
//! The crate uses `std` on every target. `wasm32-wasip1` (the production
//! target — see SPEC.md "Implementation Standards" Architecture) ships a
//! working `std`, including allocator and panic handler. A `no_std` codec
//! is not required by SPEC; switching adds friction (custom allocator,
//! custom panic handler) without buying anything for the Guest Binary,
//! which already pays for `std` through the embedded mruby interpreter.

/// Width in bytes of the wire-protocol length prefix that precedes each
/// stdin frame and outcome buffer (docs/wire-codec.md § Invocation
/// channels).
pub const FRAME_LEN_SIZE: usize = 4;

pub mod abi;
pub mod codec;
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod kobako;
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod mruby;
pub mod outcome;
pub mod rpc;
