//! kobako-wasm — Guest Binary crate root.
//!
//! This crate is the source of `kobako.wasm`, the Guest Binary artifact
//! described in SPEC.md "Core Abstractions". It hosts:
//!
//! * `codec` — MessagePack wire codec, a thin glue layer over the `rmp`
//!   crate that adds kobako's two ext types (SPEC.md "Wire Codec").
//! * `envelope` — Request / Response / Result / Panic / Outcome
//!   envelope encoders and decoders on top of `codec` (SPEC.md
//!   "Wire Contract").
//! * `abi` — Wire ABI surface: the `__kobako_rpc_call` host import and
//!   the `__kobako_run` / `__kobako_alloc` / `__kobako_take_outcome`
//!   guest exports (SPEC.md "ABI Signatures").
//! * `rpc_client` — RPC round-trip pipeline used by the guest-side
//!   mruby bridge to dispatch a call through `__kobako_rpc_call`.
//! * `kobako` — domain runtime: owns the `Kobako` value-token that
//!   installs the `Kobako` module / `Kobako::RPC` / `Kobako::Handle` /
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
/// stdin frame and outcome buffer (per SPEC.md frame protocol).
pub const FRAME_LEN_SIZE: usize = 4;

pub mod abi;
pub mod codec;
pub mod envelope;
pub mod kobako;
pub mod mruby;
pub mod rpc_client;

pub use abi::{pack_u64, unpack_u64, EXPORT_NAMES, IMPORT_MODULE, IMPORT_NAME};
pub use codec::{CodecError, Decoder, Encoder, Value};
pub use envelope::{
    decode_outcome, decode_panic, decode_request, decode_response, decode_result, encode_outcome,
    encode_panic, encode_request, encode_response, encode_result, EnvelopeError, Outcome, Panic,
    Request, Response, ResultEnv, Target,
};
pub use rpc_client::{build_request_bytes, invoke_rpc, ExceptionPayload, InvokeError};
